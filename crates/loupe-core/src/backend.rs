use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ALLOW, AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER};
use serde::Serialize;
use serde_json::Value;
use tokio::time::Instant;

use crate::admission::Admission;
use crate::budget::Budget;
use crate::cache_session::{Mode as CacheMode, Session};
use crate::config::{Limits, Profile};
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::wire::{ApiFormat, Request, Response, decisions};

#[derive(Clone)]
pub struct Backend {
    client: reqwest::Client,
    base_url: String,
    path: String,
    api_format: ApiFormat,
    admission: std::sync::Arc<Admission>,
    model: String,
    cache: Option<std::sync::Arc<Session>>,
    trace_requests: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Discovery {
    pub health: Option<Value>,
    pub models: Option<Value>,
    pub limits: Limits,
    pub unsupported: Vec<String>,
    pub unavailable: Vec<String>,
    pub full_model_identity: Option<String>,
    pub identity: Option<crate::identity::Discovery>,
    pub batch: BatchCapability,
}

#[derive(Clone, Debug, Serialize)]
pub struct BatchCapability {
    pub supported: bool,
    pub path: String,
    pub max_records: usize,
    pub queue_records: Option<usize>,
    pub gpu_records: Option<usize>,
    pub gpu_padded_tokens: Option<usize>,
}

impl BatchCapability {
    fn constrain(&mut self, advertised: &Value) -> Result<()> {
        for (name, target) in [
            ("max_queued_requests", &mut self.queue_records),
            ("max_batch_size", &mut self.gpu_records),
            ("max_batch_tokens", &mut self.gpu_padded_tokens),
        ] {
            if let Some(value) = advertised.get(name) {
                let value = value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::Unavailable,
                            format!("invalid server limit {name}"),
                        )
                    })?;
                *target = Some(target.map_or(value, |current| current.min(value)));
            }
        }
        if let Some(queue) = self.queue_records {
            self.max_records = self.max_records.min(queue);
        }
        if let Some(maximum) = advertised.get("max_batch") {
            let maximum = maximum
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    Error::new(ErrorKind::Unavailable, "invalid server limit max_batch")
                })?;
            self.max_records = self.max_records.min(maximum);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    pub response: Response,
    pub warning: Option<String>,
    pub cached: bool,
}

struct Payload<'a> {
    requests: &'a [Request],
    counts: &'a [Counts],
    batch: bool,
}

impl Backend {
    async fn batch_supported(&self, path: &str) -> Result<bool> {
        let response = self
            .client
            .get(format!("{}{path}", self.base_url))
            .send()
            .await
            .map_err(|_| {
                Error::new(ErrorKind::Unavailable, format!("{path}: transport failure"))
            })?;
        match response.status().as_u16() {
            404 | 501 => Ok(false),
            401 | 403 => Err(Error::new(
                ErrorKind::Auth,
                format!("{path}: authentication rejected"),
            )),
            405 => Ok(response
                .headers()
                .get(ALLOW)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.split(',').any(|method| method.trim() == "POST"))),
            status => Err(Error::new(
                ErrorKind::Unavailable,
                format!("{path}: capability probe HTTP {status}"),
            )),
        }
    }

    pub async fn new(profile: &Profile) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let key = if profile.api_key.is_some() {
            profile.api_key.clone()
        } else if let Some(command) = &profile.api_key_cmd {
            let expires = Instant::now()
                .checked_add(Duration::from_secs(5))
                .ok_or_else(|| {
                    Error::new(ErrorKind::Auth, "credential timeout outside clock range")
                })?;
            let mut lookup = tokio::process::Command::new("sh");
            lookup.args(["-c", command]);
            let result = crate::process::execute(lookup, b"", expires, 8192)
                .await
                .map_err(|_| Error::new(ErrorKind::Auth, "credential lookup failed"))?;
            if result.timed_out {
                return Err(Error::new(ErrorKind::Auth, "credential lookup timed out"));
            }
            if result.code != Some(0) {
                return Err(Error::new(
                    ErrorKind::Auth,
                    "credential lookup exited unsuccessfully",
                ));
            }
            if result.stdout.truncated {
                return Err(Error::new(
                    ErrorKind::Auth,
                    "credential lookup exceeded 8192 bytes",
                ));
            }
            Some(
                String::from_utf8(result.stdout.bytes)
                    .map_err(|_| {
                        Error::new(ErrorKind::Auth, "credential lookup did not return UTF-8")
                    })?
                    .trim()
                    .to_owned(),
            )
        } else if profile.api_key_required {
            return Err(Error::new(
                ErrorKind::Config,
                "configured API credential environment variable is unset",
            ));
        } else {
            None
        };
        if let Some(key) = key {
            if key.is_empty() {
                return Err(Error::new(ErrorKind::Auth, "configured API key is empty"));
            }
            let mut value = HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| Error::new(ErrorKind::Auth, "invalid API credential"))?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(profile.request_timeout)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| Error::new(ErrorKind::Config, "could not construct HTTP client"))?;
        Ok(Self {
            client,
            base_url: profile.base_url.clone(),
            path: profile.path.clone(),
            api_format: profile.api_format,
            admission: Admission::new(profile.concurrency),
            model: profile.model.clone(),
            cache: None,
            trace_requests: false,
        })
    }

    pub fn with_request_trace(mut self, enabled: bool) -> Self {
        self.trace_requests = enabled;
        self
    }

    async fn optional(&self, path: &str) -> Result<Option<Value>> {
        let mut request = self.client.get(format!("{}{path}", self.base_url));
        if path == "/v1/metadata" {
            request = request.query(&[("model", &self.model)]);
        }
        let response = request.send().await.map_err(|_| {
            Error::new(ErrorKind::Unavailable, format!("{path}: transport failure"))
        })?;
        match response.status().as_u16() {
            404 | 405 | 501 => Ok(None),
            401 | 403 => Err(Error::new(
                ErrorKind::Auth,
                format!("{path}: authentication rejected"),
            )),
            200 => {
                response.json().await.map(Some).map_err(|_| {
                    Error::new(ErrorKind::Unavailable, format!("{path}: invalid JSON"))
                })
            }
            status => Err(Error::new(
                ErrorKind::Unavailable,
                format!("{path}: HTTP {status}"),
            )),
        }
    }

    pub async fn discover(&self, configured: &Limits) -> Result<Discovery> {
        let batch_path = format!("{}/batch", self.path.trim_end_matches('/'));
        let (health, models, limits, batch, metadata) = tokio::join!(
            self.optional("/health"),
            self.optional("/v1/models"),
            self.optional("/v1/limits"),
            async {
                if self.api_format == ApiFormat::SystemOne {
                    self.batch_supported(&batch_path).await
                } else {
                    Ok(false)
                }
            },
            self.optional("/v1/metadata")
        );
        let mut result = Discovery {
            health: None,
            models: None,
            limits: configured.clone(),
            unsupported: Vec::new(),
            unavailable: Vec::new(),
            full_model_identity: None,
            identity: None,
            batch: BatchCapability {
                supported: false,
                path: batch_path.clone(),
                max_records: configured.max_batch,
                queue_records: None,
                gpu_records: None,
                gpu_padded_tokens: None,
            },
        };
        for (path, response) in [
            ("/health", health),
            ("/v1/models", models),
            ("/v1/limits", limits),
        ] {
            match response {
                Ok(Some(value)) => match path {
                    "/health" => {
                        result.limits.constrain(&value)?;
                        result.batch.constrain(&value)?;
                        result.health = Some(value);
                    }
                    "/v1/limits" => {
                        result.limits.constrain(&value)?;
                        result.batch.constrain(&value)?;
                    }
                    _ => result.models = Some(value),
                },
                Ok(None) => result.unsupported.push(path.to_owned()),
                Err(error) if error.kind == ErrorKind::Auth => return Err(error),
                Err(error) => result.unavailable.push(error.to_string()),
            }
        }
        match batch {
            Ok(true) => result.batch.supported = true,
            Ok(false) => result.unsupported.push(batch_path),
            Err(error) if error.kind == ErrorKind::Auth => return Err(error),
            Err(error) => result.unavailable.push(error.to_string()),
        }
        match metadata {
            Ok(Some(value)) => match crate::identity::Discovery::parse(value, &self.model) {
                Ok(identity) => {
                    result.full_model_identity = Some(identity.model.as_str().to_owned());
                    result.identity = Some(identity);
                }
                Err(error) => result.unavailable.push(error.to_string()),
            },
            Ok(None) => result.unsupported.push("/v1/metadata".to_owned()),
            Err(error) if error.kind == ErrorKind::Auth => return Err(error),
            Err(error) => result.unavailable.push(error.to_string()),
        }
        Ok(result)
    }

    pub async fn with_cache(
        mut self,
        discovery: &Discovery,
        encoder: Option<std::sync::Arc<ClefEncoder>>,
        directory: &std::path::Path,
        mode: CacheMode,
    ) -> Result<(Self, Option<String>)> {
        let identity = discovery.identity.clone();
        let directory = directory.to_owned();
        let (session, warning) =
            tokio::task::spawn_blocking(move || Session::open(identity, encoder, &directory, mode))
                .await
                .map_err(|_| Error::new(ErrorKind::Io, "cache preparation task failed"))?;
        self.cache = session;
        Ok((self, warning))
    }

    pub async fn models(&self) -> Result<Value> {
        self.optional("/v1/models").await?.ok_or_else(|| {
            Error::new(
                ErrorKind::Unavailable,
                "server does not support GET /v1/models",
            )
        })
    }

    pub async fn evaluate(
        &self,
        request: &Request,
        counts: &Counts,
        budget: &Budget,
        strict: bool,
        deadline: Option<Instant>,
    ) -> Result<Evaluation> {
        let payload = Payload {
            requests: std::slice::from_ref(request),
            counts: std::slice::from_ref(counts),
            batch: false,
        };
        let mut records =
            with_deadline(deadline, self.evaluate_inner(payload, budget, strict)).await?;
        records.pop().ok_or_else(|| {
            Error::new(ErrorKind::Unavailable, "single request returned no result")
        })?
    }

    pub async fn evaluate_batch(
        &self,
        requests: &[Request],
        counts: &[Counts],
        budget: &Budget,
        strict: bool,
        deadline: Option<Instant>,
    ) -> Result<Vec<Result<Evaluation>>> {
        with_deadline(
            deadline,
            self.evaluate_inner(
                Payload {
                    requests,
                    counts,
                    batch: true,
                },
                budget,
                strict,
            ),
        )
        .await
    }

    async fn evaluate_inner(
        &self,
        payload: Payload<'_>,
        budget: &Budget,
        strict: bool,
    ) -> Result<Vec<Result<Evaluation>>> {
        if payload.requests.is_empty() || payload.requests.len() != payload.counts.len() {
            return Err(Error::new(
                ErrorKind::Usage,
                "requests and token counts must have matching nonempty lengths",
            ));
        }
        for request in payload.requests {
            request.validate()?;
        }
        let mut metrics = budget.metrics.begin(payload.requests.len())?;
        let result = self
            .evaluate_cached(payload, budget, strict, &mut metrics)
            .await;
        let succeeded = result.as_ref().map_or(0, |results| {
            results.iter().filter(|result| result.is_ok()).count()
        });
        metrics.finish(succeeded)?;
        result
    }

    async fn evaluate_cached(
        &self,
        payload: Payload<'_>,
        budget: &Budget,
        strict: bool,
        metrics: &mut crate::metrics::Evaluation,
    ) -> Result<Vec<Result<Evaluation>>> {
        let Some(cache) = &self.cache else {
            return self
                .evaluate_uncached(payload, budget, strict, metrics)
                .await;
        };
        let session = cache.clone();
        let requests = payload.requests.to_vec();
        let counts = payload.counts.to_vec();
        let lookup = tokio::task::spawn_blocking(move || session.lookup(&requests, &counts))
            .await
            .map_err(|_| Error::new(ErrorKind::Io, "cache lookup task failed"))??;
        let hits = lookup
            .responses
            .iter()
            .filter(|response| response.is_some())
            .count();
        budget.record_cache_hits(hits as u64)?;
        metrics.cached(hits)?;
        let indices = lookup
            .responses
            .iter()
            .enumerate()
            .filter_map(|(index, response)| response.is_none().then_some(index))
            .collect::<Vec<_>>();
        let missing_requests = indices
            .iter()
            .filter_map(|index| payload.requests.get(*index).cloned())
            .collect::<Vec<_>>();
        let missing_counts = indices
            .iter()
            .filter_map(|index| payload.counts.get(*index).cloned())
            .collect::<Vec<_>>();
        let mut live = if indices.is_empty() {
            Vec::new()
        } else {
            match self
                .evaluate_uncached(
                    Payload {
                        requests: &missing_requests,
                        counts: &missing_counts,
                        batch: payload.batch,
                    },
                    budget,
                    strict,
                    metrics,
                )
                .await
            {
                Ok(results) => results,
                Err(error) => indices
                    .iter()
                    .map(|_| Err(Error::new(error.kind, error.to_string())))
                    .collect(),
            }
        };
        let session = cache.clone();
        let requests = missing_requests;
        let counts = missing_counts;
        let responses = live
            .iter()
            .map(|result| {
                result
                    .as_ref()
                    .ok()
                    .map(|evaluation| evaluation.response.clone())
            })
            .collect::<Vec<_>>();
        let finished =
            tokio::task::spawn_blocking(move || session.finish(&requests, &counts, &responses))
                .await
                .map_err(|_| Error::new(ErrorKind::Io, "cache completion task failed"))??;
        for (result, valid) in live.iter_mut().zip(finished.valid) {
            if !valid {
                *result = Err(Error::new(
                    ErrorKind::Unavailable,
                    "response model identity changed or is incomplete; cache disabled, restart the run",
                ));
            }
        }
        let warning = lookup.warning.or(finished.warning);
        let mut misses = live.into_iter();
        Ok(lookup
            .responses
            .into_iter()
            .map(|response| {
                let mut result = match response {
                    Some(response) => Ok(Evaluation {
                        response,
                        warning: None,
                        cached: true,
                    }),
                    None => misses.next().ok_or_else(|| {
                        Error::new(ErrorKind::Unavailable, "cache/miss result missing")
                    })?,
                };
                if let (Ok(evaluation), Some(warning)) = (&mut result, &warning) {
                    evaluation.warning = Some(evaluation.warning.take().map_or_else(
                        || warning.clone(),
                        |previous| format!("{previous}; {warning}"),
                    ));
                }
                result
            })
            .collect())
    }

    async fn evaluate_uncached(
        &self,
        payload: Payload<'_>,
        budget: &Budget,
        strict: bool,
        metrics: &mut crate::metrics::Evaluation,
    ) -> Result<Vec<Result<Evaluation>>> {
        if payload.batch && self.api_format == ApiFormat::Decisions {
            return Err(Error::new(
                ErrorKind::Usage,
                "Decisions does not support the System One batch endpoint",
            ));
        }
        if payload.requests.is_empty() || payload.requests.len() != payload.counts.len() {
            return Err(Error::new(
                ErrorKind::Usage,
                "requests and token counts must have matching nonempty lengths",
            ));
        }
        for request in payload.requests {
            request.validate()?;
        }
        let tokens = payload
            .counts
            .iter()
            .map(|counts| {
                u64::try_from(counts.input_tokens)
                    .map_err(|_| Error::new(ErrorKind::Budget, "input-token count overflow"))
            })
            .collect::<Result<Vec<_>>>()?;
        let path = if payload.batch {
            format!("{}/batch", self.path.trim_end_matches('/'))
        } else {
            self.path.clone()
        };
        let url = format!("{}{path}", self.base_url);
        for attempt in 0..5 {
            let permit = self.admission.acquire_many(payload.requests.len()).await?;
            let builder = self.client.post(&url);
            let builder = if payload.batch {
                builder.json(payload.requests)
            } else {
                let request = payload
                    .requests
                    .first()
                    .ok_or_else(|| Error::new(ErrorKind::Usage, "single request missing"))?;
                match self.api_format {
                    ApiFormat::SystemOne => builder.json(request),
                    ApiFormat::Decisions => {
                        builder.json(&decisions::Request::from_systemone(request)?)
                    }
                }
            };
            let http_request = builder.build().map_err(|_| {
                Error::new(ErrorKind::Usage, "could not serialize decision request")
            })?;
            let submitted = budget.reserve_batch(&tokens)?.submit()?;
            metrics.submitted();
            if self.trace_requests {
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "loupe: submitted attempt={} records={} input_tokens={}",
                    attempt + 1,
                    payload.requests.len(),
                    submitted.input_tokens()
                );
            }
            let response = match self.client.execute(http_request).await {
                Ok(response) => response,
                Err(error) => {
                    drop(permit);
                    if attempt == 4 {
                        return Err(Error::new(
                            ErrorKind::Unavailable,
                            if error.is_timeout() {
                                "request timed out after five attempts"
                            } else {
                                "transport failed after five attempts"
                            },
                        ));
                    }
                    tokio::time::sleep(backoff(attempt, jitter_seed())).await;
                    continue;
                }
            };
            let status = response.status().as_u16();
            if matches!(status, 429 | 503 | 529) {
                self.admission.congestion()?;
            }
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(retry_after);
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .filter(|value| {
                    value
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                })
                .unwrap_or("unavailable")
                .to_owned();
            if status == 200 {
                let responses: Vec<Response> = if payload.batch {
                    response.json().await.map_err(|_| {
                        Error::new(
                            ErrorKind::Unavailable,
                            "invalid System One batch JSON response",
                        )
                    })?
                } else if self.api_format == ApiFormat::Decisions {
                    let decoded: decisions::Response = response.json().await.map_err(|_| {
                        Error::new(ErrorKind::Unavailable, "invalid Decisions JSON response")
                    })?;
                    let request = payload
                        .requests
                        .first()
                        .ok_or_else(|| Error::new(ErrorKind::Usage, "single request missing"))?;
                    vec![decoded.into_systemone(request)?]
                } else {
                    vec![response.json().await.map_err(|_| {
                        Error::new(ErrorKind::Unavailable, "invalid System One JSON response")
                    })?]
                };
                if responses.len() != payload.requests.len() {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        format!(
                            "batch response cardinality mismatch: {} requests, {} responses",
                            payload.requests.len(),
                            responses.len()
                        ),
                    ));
                }
                let (input, output) =
                    responses
                        .iter()
                        .try_fold((0u64, 0u64), |(input, output), response| {
                            Ok::<_, Error>((
                                input.checked_add(response.usage.input_tokens).ok_or_else(
                                    || {
                                        Error::new(
                                            ErrorKind::Unavailable,
                                            "batch input usage overflow",
                                        )
                                    },
                                )?,
                                output
                                    .checked_add(response.usage.output_tokens)
                                    .ok_or_else(|| {
                                        Error::new(
                                            ErrorKind::Unavailable,
                                            "batch output usage overflow",
                                        )
                                    })?,
                            ))
                        })?;
                submitted.finish(input, output)?;
                return Ok(responses
                    .into_iter()
                    .zip(payload.requests)
                    .zip(payload.counts)
                    .map(|((response, request), counts)| {
                        response.validate(request)?;
                        let expected = u64::try_from(counts.input_tokens).map_err(|_| {
                            Error::new(ErrorKind::Budget, "input-token count overflow")
                        })?;
                        let warning = if counts.exact && response.usage.input_tokens != expected {
                            let message = format!(
                                "input-token mismatch: encoded {expected}, server reported {}",
                                response.usage.input_tokens
                            );
                            if strict {
                                return Err(Error::new(ErrorKind::Unavailable, message));
                            }
                            Some(message)
                        } else {
                            None
                        };
                        Ok(Evaluation {
                            response,
                            warning,
                            cached: false,
                        })
                    })
                    .collect());
            }

            if retryable(status) && attempt < 4 {
                drop(permit);
                wait_retry(retry_after.unwrap_or_else(|| backoff(attempt, jitter_seed()))).await?;
                continue;
            }
            let kind = match status {
                401 | 403 => ErrorKind::Auth,
                413 | 422 => ErrorKind::Input,
                _ => ErrorKind::Unavailable,
            };
            let detail = if status == 422 {
                let value: Value = response.json().await.unwrap_or(Value::Null);
                let fields: Vec<_> = value
                    .get("detail")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|field| field.get("loc"))
                    .map(Value::to_string)
                    .collect();
                format!("; fields: {}", fields.join(", "))
            } else {
                String::new()
            };
            return Err(Error::new(
                kind,
                format!(
                    "{} HTTP {status}; request id {request_id}{detail}",
                    match self.api_format {
                        ApiFormat::SystemOne => "System One",
                        ApiFormat::Decisions => "Decisions",
                    }
                ),
            ));
        }
        Err(Error::new(
            ErrorKind::Unavailable,
            "request attempts exhausted",
        ))
    }
}

pub fn retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504 | 529)
}

pub async fn with_deadline<T>(
    deadline: Option<Instant>,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if let Some(deadline) = deadline {
        if Instant::now() >= deadline {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "overall deadline expired before admission",
            ));
        }
        tokio::time::timeout_at(deadline, operation)
            .await
            .map_err(|_| Error::new(ErrorKind::Unavailable, "overall deadline expired"))?
    } else {
        operation.await
    }
}

fn retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    (date.with_timezone(&chrono::Utc) - chrono::Utc::now())
        .to_std()
        .ok()
        .or(Some(Duration::ZERO))
}

async fn wait_retry(delay: Duration) -> Result<()> {
    let wake = Instant::now().checked_add(delay).ok_or_else(|| {
        Error::new(
            ErrorKind::Unavailable,
            "server retry delay is outside the clock range",
        )
    })?;
    tokio::time::sleep_until(wake).await;
    Ok(())
}

fn jitter_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| u64::from(time.subsec_nanos()))
        .unwrap_or(0)
}

pub fn backoff(attempt: u32, seed: u64) -> Duration {
    let cap = 250u64
        .saturating_mul(2u64.saturating_pow(attempt))
        .min(10_000);
    Duration::from_millis(cap / 2 + seed % (cap / 2 + 1))
}

#[cfg(test)]
mod tests {
    use crate::backend::{backoff, retryable};

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn credentials_are_bounded_private_and_explicit_keys_skip_commands() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings::default(),
                &std::collections::HashMap::from([(
                    "HOME".to_owned(),
                    directory.path().to_string_lossy().into_owned(),
                )]),
            )
            .unwrap();
        for command in ["printf key", "cat >/dev/null; printf key"] {
            profile.api_key_cmd = Some(command.into());
            assert!(crate::backend::Backend::new(&profile).await.is_ok());
        }
        for (command, expected) in [
            (
                "printf 'private-detail' >&2; exit 7",
                "exited unsuccessfully",
            ),
            ("printf ''", "empty"),
            (r"printf '\377'", "UTF-8"),
            (
                "head -c 8193 /dev/zero | tr '\\000' x",
                "exceeded 8192 bytes",
            ),
            (
                "printf 'private-detail\\nsecond-line'",
                "invalid API credential",
            ),
        ] {
            profile.api_key_cmd = Some(command.into());
            let error = crate::backend::Backend::new(&profile).await.err().unwrap();
            assert_eq!(error.exit_code(), 77);
            assert!(error.to_string().contains(expected), "{error}");
            assert!(!error.to_string().contains("private-detail"));
        }
        let marker = directory.path().join("command-ran");
        profile.api_key_cmd = Some(format!(
            "touch {}; printf key",
            crate::hooks::quote(marker.to_str().unwrap()).unwrap()
        ));
        profile.api_key = Some("explicit-key".into());
        assert!(crate::backend::Backend::new(&profile).await.is_ok());
        assert!(!marker.exists());
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn credential_success_and_cancelled_lookup_stop_background_descendants() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings::default(),
                &std::collections::HashMap::from([(
                    "HOME".to_owned(),
                    directory.path().to_string_lossy().into_owned(),
                )]),
            )
            .unwrap();
        let marker = directory.path().join("orphan-ran");
        let quoted = crate::hooks::quote(marker.to_str().unwrap()).unwrap();
        profile.api_key_cmd = Some(format!("(sleep 0.25; touch {quoted}) & printf key"));
        assert!(crate::backend::Backend::new(&profile).await.is_ok());
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(!marker.exists());
        profile.api_key_cmd = Some(format!("(sleep 0.25; touch {quoted}) & sleep 30"));
        let error = crate::backend::with_deadline(
            Some(tokio::time::Instant::now() + std::time::Duration::from_millis(40)),
            crate::backend::Backend::new(&profile),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.exit_code(), 69);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(!marker.exists());
    }

    #[tokio::test(start_paused = true)]
    #[allow(clippy::unwrap_used)]
    async fn retry_headers_preserve_provider_delays_and_reject_clock_overflow() {
        assert_eq!(
            crate::backend::retry_after("120"),
            Some(std::time::Duration::from_secs(120))
        );
        assert!(crate::backend::retry_after("invalid").is_none());
        assert!(
            crate::backend::wait_retry(std::time::Duration::from_secs(u64::MAX))
                .await
                .is_err()
        );
        let before = tokio::time::Instant::now();
        crate::backend::wait_retry(std::time::Duration::from_secs(120))
            .await
            .unwrap();
        assert!(before.elapsed() >= std::time::Duration::from_secs(120));
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn http_batch_admission_is_independent_of_gpu_batch_shape() {
        let mut batch = crate::backend::BatchCapability {
            supported: true,
            path: "/v1/systemone/batch".to_owned(),
            max_records: 16,
            queue_records: None,
            gpu_records: None,
            gpu_padded_tokens: None,
        };
        batch.constrain(&serde_json::json!({"max_queued_requests":8,"max_batch_size":2,"max_batch_tokens":1024})).unwrap();
        assert_eq!(batch.max_records, 8);
        assert_eq!(batch.gpu_records, Some(2));
        assert_eq!(batch.gpu_padded_tokens, Some(1024));
        batch
            .constrain(&serde_json::json!({"max_batch":6,"max_queued_requests":12}))
            .unwrap();
        assert_eq!(batch.max_records, 6);
        assert_eq!(batch.queue_records, Some(8));
        assert!(
            batch
                .constrain(&serde_json::json!({"max_queued_requests":0}))
                .is_err()
        );
    }
    #[test]
    fn retry_statuses_and_backoff_stay_bounded() {
        assert!(!retryable(422));
        assert!(!retryable(401));
        assert!(retryable(529));
        for attempt in 0..20 {
            assert!(backoff(attempt, u64::MAX).as_millis() <= 10_000);
        }
        assert_eq!(backoff(0, 0).as_millis(), 125);
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn cached_batches_preserve_mixed_results_and_do_not_charge_hit_usage() {
        use crate::cache_session::{Mode, Session};
        use crate::identity::{Discovery, Model};
        use crate::wire::{Request, Response};
        let captured: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let request: Request = serde_json::from_value(captured["request"].clone()).unwrap();
        let mut response: Response = serde_json::from_value(captured["response"].clone()).unwrap();
        let identity = Discovery {
            model: Model::parse(&format!(
                "clef-flash@{};compute=bf16;quantization=nf4;server={}",
                "a".repeat(40),
                "b".repeat(64)
            ))
            .unwrap(),
            encoder_identity: "fixture-encoder".to_owned(),
        };
        response.model = identity.model.as_str().to_owned();
        let counts = crate::encoder::Counts {
            input_tokens: 298,
            state_tokens: 10,
            question_tokens: 288,
            exact: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let (session, warning) = Session::open(Some(identity), None, dir.path(), Mode::Normal);
        assert!(warning.is_none());
        let session = session.unwrap();
        session
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(response.clone())],
            )
            .unwrap();
        // The fixture identity tests the cache pipeline; the miss uses a genuinely refused TCP connection.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let env = std::collections::HashMap::from([("HOME".to_owned(), "/tmp".to_owned())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    base_url: Some(format!("http://{address}")),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let mut backend = crate::backend::Backend::new(&profile).await.unwrap();
        backend.cache = Some(session);
        let budget = crate::budget::Budget::new(Some(0));
        let hit = backend
            .evaluate(&request, &counts, &budget, true, None)
            .await
            .unwrap();
        assert!(hit.cached);
        assert_eq!(
            serde_json::to_value(hit.response).unwrap(),
            serde_json::to_value(&response).unwrap()
        );
        let all = backend
            .evaluate_batch(
                &[request.clone(), request.clone()],
                &[counts.clone(), counts.clone()],
                &budget,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(all.into_iter().all(|result| result.unwrap().cached));
        let usage = budget.usage().unwrap();
        assert_eq!(usage.cache_hits, 3);
        assert_eq!(usage.spent_input, 0);
        assert_eq!(usage.reported_input, 0);
        assert_eq!(usage.attempts, 0);
        let metrics = budget.metrics.snapshot().unwrap();
        assert_eq!(metrics.logical_records, 3);
        assert_eq!(metrics.succeeded_records, 3);
        assert_eq!(metrics.latency_samples, 0);
        assert!(metrics.p50.is_none());
        let mut miss = request.clone();
        miss.state = serde_json::json!("not in the cache");
        let budget = crate::budget::Budget::new(Some(298));
        let mut mixed = backend
            .evaluate_batch(
                &[request.clone(), miss, request],
                &[counts.clone(), counts.clone(), counts],
                &budget,
                true,
                None,
            )
            .await
            .unwrap()
            .into_iter();
        assert!(mixed.next().unwrap().unwrap().cached);
        assert_eq!(mixed.next().unwrap().err().unwrap().exit_code(), 4);
        assert!(mixed.next().unwrap().unwrap().cached);
        let usage = budget.usage().unwrap();
        assert_eq!(usage.cache_hits, 2);
        assert_eq!(usage.spent_input, 298);
        assert_eq!(usage.unknown_attempts, 1);
        assert_eq!(usage.attempts, 1);
        assert_eq!(usage.http_attempts, 1);
        let metrics = budget.metrics.snapshot().unwrap();
        assert_eq!(metrics.logical_records, 3);
        assert_eq!(metrics.succeeded_records, 2);
        assert_eq!(metrics.failed_records, 1);
        assert_eq!(metrics.latency_samples, 1);
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn missing_credentials_allow_offline_resolution_but_reject_backend_start() {
        let config: crate::config::Config =
            toml::from_str("[profile.hosted]\napi_key_env='UNSET'\n").unwrap();
        let env = std::collections::HashMap::from([("HOME".to_owned(), "/tmp".to_owned())]);
        let profile = config
            .resolve(Some("hosted"), &crate::config::Settings::default(), &env)
            .unwrap();
        assert!(profile.api_key_required);
        assert_eq!(
            crate::backend::Backend::new(&profile)
                .await
                .err()
                .unwrap()
                .exit_code(),
            78
        );
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn genuine_connection_failure_charges_every_attempt_and_deadline_bounds_retries() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let env = std::collections::HashMap::from([("HOME".to_owned(), "/tmp".to_owned())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    base_url: Some(format!("http://{address}")),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let backend = crate::backend::Backend::new(&profile).await.unwrap();
        let request: crate::wire::Request = serde_json::from_value(
            serde_json::json!({"state":"x","model":"clef-flash","questions":{"q":{"type":"noul"}}}),
        )
        .unwrap();
        let counts = crate::encoder::Counts {
            input_tokens: 100,
            state_tokens: 1,
            question_tokens: 99,
            exact: true,
        };
        let budget = crate::budget::Budget::new(Some(500));
        let error = backend
            .evaluate(&request, &counts, &budget, false, None)
            .await
            .err()
            .unwrap();
        assert_eq!(error.exit_code(), 69);
        assert_eq!(budget.usage().unwrap().attempts, 5);
        assert_eq!(budget.usage().unwrap().spent_input, 500);
        assert_eq!(budget.usage().unwrap().unknown_attempts, 5);
        let budget = crate::budget::Budget::new(Some(500));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(20);
        assert_eq!(
            backend
                .evaluate(&request, &counts, &budget, false, Some(deadline))
                .await
                .err()
                .unwrap()
                .exit_code(),
            69
        );
        assert_eq!(budget.usage().unwrap().attempts, 1);
        assert_eq!(budget.usage().unwrap().spent_input, 100);
        let budget = crate::budget::Budget::new(Some(1000));
        assert_eq!(
            backend
                .evaluate_batch(
                    &[request.clone(), request],
                    &[counts.clone(), counts],
                    &budget,
                    false,
                    None
                )
                .await
                .err()
                .unwrap()
                .exit_code(),
            69
        );
        let usage = budget.usage().unwrap();
        assert_eq!(usage.attempts, 10);
        assert_eq!(usage.http_attempts, 5);
        assert_eq!(usage.spent_input, 1000);
        assert_eq!(usage.unknown_attempts, 10);
    }

    #[tokio::test]
    #[ignore = "requires real LOUPE_TEST_URL System One service"]
    #[allow(clippy::unwrap_used)]
    async fn actual_batch_discovery_order_usage_and_nonretryable_admission_errors() {
        let url = std::env::var("LOUPE_TEST_URL").unwrap();
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    base_url: Some(url),
                    concurrency: Some(8),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = crate::backend::Backend::new(&profile).await.unwrap();
        let discovery = backend.discover(&profile.limits).await.unwrap();
        assert!(discovery.batch.supported);
        assert!(
            discovery
                .batch
                .queue_records
                .is_some_and(|maximum| maximum > 0)
        );
        assert!(
            discovery
                .batch
                .gpu_records
                .is_some_and(|maximum| maximum > 0)
        );
        assert_eq!(
            discovery.full_model_identity.as_deref(),
            discovery
                .identity
                .as_ref()
                .map(|identity| identity.model.as_str())
        );
        let mut requests = Vec::new();
        for (index, state) in [
            "Checkout is down.",
            "Checkout is healthy.",
            "Checkout remains unavailable.",
            "Checkout works normally.",
        ]
        .iter()
        .enumerate()
        {
            requests.push(serde_json::from_value::<crate::wire::Request>(serde_json::json!({"model":"clef-flash","state":state,"questions":{format!("record{index}"):{"type":"noul","instructions":"Is checkout currently unavailable?"}}})).unwrap());
        }
        let counts = requests
            .iter()
            .map(|request| encoder.encode(request).unwrap().1)
            .collect::<Vec<_>>();
        let budget = crate::budget::Budget::new(None);
        let responses = backend
            .evaluate_batch(&requests, &counts, &budget, true, None)
            .await
            .unwrap();
        assert_eq!(responses.len(), 4);
        for (index, response) in responses.into_iter().enumerate() {
            let response = response.unwrap();
            assert!(response.warning.is_none());
            if let Some(identity) = &discovery.identity {
                assert_eq!(response.response.model, identity.model.as_str());
            }
            assert_eq!(
                response.response.answers.keys().next().unwrap(),
                &format!("record{index}")
            );
            assert_eq!(
                response.response.usage.input_tokens,
                counts[index].input_tokens as u64
            );
        }
        let usage = budget.usage().unwrap();
        assert_eq!(usage.attempts, 4);
        assert_eq!(usage.http_attempts, 1);
        assert_eq!(usage.unknown_attempts, 0);
        assert_eq!(usage.spent_input, usage.reported_input);
        let too_small = crate::budget::Budget::new(Some(1));
        assert_eq!(
            backend
                .evaluate_batch(&requests, &counts, &too_small, true, None)
                .await
                .err()
                .unwrap()
                .exit_code(),
            4
        );
        assert_eq!(too_small.usage().unwrap().attempts, 0);
        requests.push(requests[0].clone());
        let mut counts = counts;
        counts.push(counts[0].clone());
        let rejected = crate::budget::Budget::new(None);
        assert_eq!(
            backend
                .evaluate_batch(&requests, &counts, &rejected, true, None)
                .await
                .err()
                .unwrap()
                .exit_code(),
            65
        );
        assert_eq!(rejected.usage().unwrap().http_attempts, 1);
        assert_eq!(rejected.usage().unwrap().unknown_attempts, 5);
    }

    #[tokio::test]
    #[ignore = "requires real LOUPE_TEST_URL System One service"]
    #[allow(clippy::unwrap_used)]
    async fn actual_http_decoding_preserves_python_goldens_and_exact_usage() {
        let url = std::env::var("LOUPE_TEST_URL").unwrap();
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    base_url: Some(url),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = crate::backend::Backend::new(&profile).await.unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../evals/golden/clef-v1/records.json"))
                .unwrap();
        let budget = crate::budget::Budget::new(None);
        for case in cases.into_iter().take(12) {
            let request: crate::wire::Request =
                serde_json::from_value(case["request"].clone()).unwrap();
            let (_, counts) = encoder.encode(&request).unwrap();
            let evaluation = backend
                .evaluate(&request, &counts, &budget, true, None)
                .await
                .unwrap();
            assert_eq!(
                evaluation.response.usage.input_tokens,
                counts.input_tokens as u64
            );
            assert!(evaluation.warning.is_none());
        }
        assert_eq!(budget.usage().unwrap().attempts, 12);
        assert_eq!(budget.usage().unwrap().unknown_attempts, 0);
    }
}
