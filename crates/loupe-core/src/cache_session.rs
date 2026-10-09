use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::cache::{self, Entry, Store};
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::identity::{Discovery, Model};
use crate::wire::{Request, Response};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Refresh,
    Disabled,
    ReadOnly,
}
struct State {
    store: Option<Store>,
    enabled: bool,
}
pub struct Session {
    identity: Discovery,
    encoder: Option<Arc<ClefEncoder>>,
    mode: Mode,
    state: Mutex<State>,
}
pub struct Lookup {
    pub responses: Vec<Option<Response>>,
    pub warning: Option<String>,
}
pub struct Finished {
    pub valid: Vec<bool>,
    pub warning: Option<String>,
}

impl Session {
    pub fn open(
        identity: Option<Discovery>,
        encoder: Option<Arc<ClefEncoder>>,
        directory: &Path,
        mode: Mode,
    ) -> (Option<Arc<Self>>, Option<String>) {
        let Some(identity) = identity else {
            return (
                None,
                (mode != Mode::Disabled).then(|| {
                    "cache disabled: fresh authoritative model/encoder identity is unavailable"
                        .to_owned()
                }),
            );
        };
        let mut warning = None;
        let store = if mode == Mode::Disabled {
            None
        } else if encoder
            .as_ref()
            .is_some_and(|encoder| encoder.identity != identity.encoder_identity)
        {
            warning = Some(
                "cache disabled: encoder/tokenizer identity differs from fresh discovery"
                    .to_owned(),
            );
            None
        } else {
            match Store::open(directory, mode == Mode::ReadOnly) {
                Ok(store) => store,
                Err(error) => {
                    warning = Some(format!("cache disabled: {error}"));
                    None
                }
            }
        };
        let enabled = store.is_some();
        (
            Some(Arc::new(Self {
                identity,
                encoder,
                mode,
                state: Mutex::new(State { store, enabled }),
            })),
            warning,
        )
    }
    pub fn lookup(&self, requests: &[Request], counts: &[Counts]) -> Result<Lookup> {
        let empty = || {
            requests
                .iter()
                .map(|_| None)
                .collect::<Vec<Option<Response>>>()
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Io, "cache session lock poisoned"))?;
        if !state.enabled || matches!(self.mode, Mode::Disabled | Mode::Refresh) {
            return Ok(Lookup {
                responses: empty(),
                warning: None,
            });
        }
        let read = || -> Result<Vec<Option<Response>>> {
            if requests.len() != counts.len() {
                return Err(Error::new(
                    ErrorKind::Io,
                    "cache request/count cardinality mismatch",
                ));
            }
            let keys = requests
                .iter()
                .map(|request| cache::key(&self.identity, self.encoder.as_deref(), request))
                .collect::<Result<Vec<_>>>()?;
            let entries = state
                .store
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::Io, "cache store missing"))?
                .get_many(&keys)?;
            entries
                .into_iter()
                .zip(requests)
                .zip(counts)
                .map(|((entry, request), counts)| {
                    let Some(entry) = entry else {
                        return Ok(None);
                    };
                    if entry.model != self.identity.model
                        || entry.encoder_identity != self.identity.encoder_identity
                        || Model::parse(&entry.response.model)? != self.identity.model
                    {
                        return Err(Error::new(
                            ErrorKind::Io,
                            "cache entry identity does not match the request",
                        ));
                    }
                    entry.response.validate(request)?;
                    if counts.exact
                        && entry.response.usage.input_tokens != counts.input_tokens as u64
                    {
                        return Err(Error::new(
                            ErrorKind::Io,
                            "cached input-token count differs from the encoded request",
                        ));
                    }
                    Ok(Some(entry.response))
                })
                .collect()
        };
        match read() {
            Ok(responses) => Ok(Lookup {
                responses,
                warning: None,
            }),
            Err(error) => {
                state.enabled = false;
                state.store = None;
                Ok(Lookup {
                    responses: empty(),
                    warning: Some(format!("cache disabled: {error}")),
                })
            }
        }
    }
    pub fn finish(
        &self,
        requests: &[Request],
        counts: &[Counts],
        responses: &[Option<Response>],
    ) -> Result<Finished> {
        if requests.len() != counts.len() || requests.len() != responses.len() {
            return Err(Error::new(
                ErrorKind::Io,
                "cache completion cardinality mismatch",
            ));
        }
        let valid = responses
            .iter()
            .map(|response| {
                response.as_ref().is_none_or(|response| {
                    Model::parse(&response.model).is_ok_and(|model| model == self.identity.model)
                })
            })
            .collect::<Vec<_>>();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Io, "cache session lock poisoned"))?;
        if valid.iter().any(|valid| !*valid) {
            state.enabled = false;
            state.store = None;
            return Ok(Finished {
                valid,
                warning: None,
            });
        }
        if !state.enabled || matches!(self.mode, Mode::Disabled | Mode::ReadOnly) {
            return Ok(Finished {
                valid,
                warning: None,
            });
        }
        let write = || -> Result<()> {
            let mut entries = Vec::new();
            for ((request, counts), response) in requests.iter().zip(counts).zip(responses) {
                let Some(response) = response else {
                    continue;
                };
                response.validate(request)?;
                if counts.exact && response.usage.input_tokens != counts.input_tokens as u64 {
                    continue;
                }
                entries.push((
                    cache::key(&self.identity, self.encoder.as_deref(), request)?,
                    Entry::new(&self.identity, response.clone())?,
                ));
            }
            if entries.is_empty() {
                return Ok(());
            }
            state
                .store
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::Io, "cache store missing"))?
                .put_many(&entries)
        };
        match write() {
            Ok(()) => Ok(Finished {
                valid,
                warning: None,
            }),
            Err(error) => {
                state.enabled = false;
                state.store = None;
                Ok(Finished {
                    valid,
                    warning: Some(format!("cache disabled: {error}")),
                })
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::cache::{self, Store};
    use crate::cache_session::{Mode, Session};
    use crate::encoder::Counts;
    use crate::identity::{Discovery, Model};
    use crate::wire::{Request, Response};
    use serde_json::json;
    use std::sync::Arc;

    fn fixtures() -> (Discovery, Request, Response, Counts) {
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
        let counts = Counts {
            input_tokens: 298,
            state_tokens: 10,
            question_tokens: 288,
            exact: false,
        };
        (identity, request, response, counts)
    }
    fn open(identity: &Discovery, directory: &std::path::Path, mode: Mode) -> Arc<Session> {
        let (session, warning) = Session::open(Some(identity.clone()), None, directory, mode);
        assert!(warning.is_none(), "{warning:?}");
        session.unwrap()
    }
    #[test]
    fn verified_encoder_checks_discovery_before_open_and_usage_before_reusing_a_hit() {
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER").ok(),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let encoder = Arc::new(crate::encoder::load_profile(&profile).unwrap().unwrap());
        let (mut identity, request, response, _) = fixtures();
        identity.encoder_identity = encoder.identity.clone();
        let counts = encoder.encode(&request).unwrap().1;
        assert_eq!(counts.input_tokens as u64, response.usage.input_tokens);
        let dir = tempfile::tempdir().unwrap();
        let (session, warning) = Session::open(
            Some(identity.clone()),
            Some(encoder.clone()),
            dir.path(),
            Mode::Normal,
        );
        assert!(warning.is_none());
        let session = session.unwrap();
        session
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(response)],
            )
            .unwrap();
        drop(session);
        let (reader, warning) = Session::open(
            Some(identity.clone()),
            Some(encoder.clone()),
            dir.path(),
            Mode::ReadOnly,
        );
        assert!(warning.is_none());
        let reader = reader.unwrap();
        assert!(
            reader
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .is_some()
        );
        let mut wrong_count = counts.clone();
        wrong_count.input_tokens += 1;
        let lookup = reader
            .lookup(std::slice::from_ref(&request), &[wrong_count])
            .unwrap();
        assert!(lookup.responses[0].is_none());
        assert!(
            lookup
                .warning
                .unwrap()
                .contains("input-token count differs")
        );
        drop(reader);
        let path = dir.path().join(cache::FILE_NAME);
        std::fs::write(&path, b"must not open mismatched encoder cache").unwrap();
        identity.encoder_identity.push_str("-mismatch");
        let (session, warning) =
            Session::open(Some(identity), Some(encoder), dir.path(), Mode::Normal);
        assert!(
            warning
                .unwrap()
                .contains("encoder/tokenizer identity differs")
        );
        drop(session);
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"must not open mismatched encoder cache"
        );
    }
    #[test]
    fn unavailable_identity_and_disabled_mode_leave_preexisting_or_missing_cache_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(cache::FILE_NAME);
        std::fs::write(&path, b"invalid database that must not be opened").unwrap();
        let (identity, request, response, counts) = fixtures();
        for mode in [Mode::Normal, Mode::Refresh, Mode::ReadOnly, Mode::Disabled] {
            let (session, warning) = Session::open(None, None, dir.path(), mode);
            assert!(session.is_none());
            assert_eq!(warning.is_none(), mode == Mode::Disabled);
            let session = open(&identity, dir.path(), Mode::Disabled);
            assert!(
                session
                    .lookup(
                        std::slice::from_ref(&request),
                        std::slice::from_ref(&counts)
                    )
                    .unwrap()
                    .responses[0]
                    .is_none()
            );
            assert_eq!(
                session
                    .finish(
                        std::slice::from_ref(&request),
                        std::slice::from_ref(&counts),
                        &[Some(response.clone())]
                    )
                    .unwrap()
                    .valid,
                vec![true]
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"invalid database that must not be opened"
            );
        }
        let missing = dir.path().join("missing");
        drop(open(&identity, &missing, Mode::Disabled));
        let (session, warning) = Session::open(Some(identity), None, &missing, Mode::ReadOnly);
        assert!(warning.is_none());
        drop(session);
        assert!(!missing.exists());
    }
    #[test]
    fn raw_mixed_hits_refresh_and_read_only_modes_preserve_order_without_local_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (identity, request, response, counts) = fixtures();
        let session = open(&identity, dir.path(), Mode::Normal);
        session
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(response.clone())],
            )
            .unwrap();
        drop(session);
        let mut miss = request.clone();
        miss.state = json!("another unit");
        let before = std::fs::read(dir.path().join(cache::FILE_NAME)).unwrap();
        let reader = open(&identity, dir.path(), Mode::ReadOnly);
        let lookup = reader
            .lookup(
                &[miss.clone(), request.clone(), miss.clone(), request.clone()],
                &[
                    counts.clone(),
                    counts.clone(),
                    counts.clone(),
                    counts.clone(),
                ],
            )
            .unwrap();
        assert!(lookup.warning.is_none());
        assert!(lookup.responses[0].is_none() && lookup.responses[2].is_none());
        assert_eq!(
            serde_json::to_value(lookup.responses[1].as_ref().unwrap()).unwrap(),
            serde_json::to_value(&response).unwrap()
        );
        reader
            .finish(
                std::slice::from_ref(&miss),
                std::slice::from_ref(&counts),
                &[Some(response.clone())],
            )
            .unwrap();
        drop(reader);
        assert_eq!(
            std::fs::read(dir.path().join(cache::FILE_NAME)).unwrap(),
            before
        );
        let refresh = open(&identity, dir.path(), Mode::Refresh);
        assert!(
            refresh
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .is_none()
        );
        let mut updated = response.clone();
        updated.answers["outage"]["noul"] = json!(0.25);
        refresh
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(updated.clone())],
            )
            .unwrap();
        drop(refresh);
        let reader = open(&identity, dir.path(), Mode::ReadOnly);
        assert_eq!(
            reader
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .as_ref()
                .unwrap()
                .answers["outage"]["noul"],
            json!(0.25)
        );
    }
    #[test]
    fn fresh_model_changes_cannot_reuse_old_entries_and_live_changes_disable_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let (identity, request, response, counts) = fixtures();
        let session = open(&identity, dir.path(), Mode::Normal);
        session
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(response.clone())],
            )
            .unwrap();
        drop(session);
        let mut changed = identity.clone();
        changed.model = Model::parse(&identity.model.as_str().replace("nf4", "none")).unwrap();
        let session = open(&changed, dir.path(), Mode::Normal);
        assert!(
            session
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .is_none()
        );
        assert_eq!(
            session
                .finish(
                    &[request.clone(), request.clone()],
                    &[counts.clone(), counts.clone()],
                    &[None, Some(response.clone())]
                )
                .unwrap()
                .valid,
            vec![true, false]
        );
        assert!(
            session
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .is_none()
        );
        drop(session);
        assert_eq!(
            Store::open(dir.path(), true)
                .unwrap()
                .unwrap()
                .stats()
                .unwrap()
                .entries,
            1
        );
        // --no-cache still checks the fresh response identity.
        let disabled = open(&changed, dir.path(), Mode::Disabled);
        assert_eq!(
            disabled
                .finish(&[request], &[counts], &[Some(response)])
                .unwrap()
                .valid,
            vec![false]
        );
    }
    #[test]
    fn exact_usage_mismatch_is_never_inserted_and_corrupt_entries_disable_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let (identity, request, response, mut counts) = fixtures();
        counts.exact = true;
        counts.input_tokens += 1;
        let session = open(&identity, dir.path(), Mode::Normal);
        session
            .finish(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
                &[Some(response.clone())],
            )
            .unwrap();
        drop(session);
        assert_eq!(
            Store::open(dir.path(), true)
                .unwrap()
                .unwrap()
                .stats()
                .unwrap()
                .entries,
            0
        );
        counts.exact = false;
        let store = Store::open(dir.path(), false).unwrap().unwrap();
        let key = cache::key(&identity, None, &request).unwrap();
        let mut invalid = cache::Entry::new(&identity, response).unwrap();
        invalid.response.answers.shift_remove("outage");
        store.put_many(&[(key, invalid)]).unwrap();
        drop(store);
        let session = open(&identity, dir.path(), Mode::ReadOnly);
        let lookup = session
            .lookup(
                std::slice::from_ref(&request),
                std::slice::from_ref(&counts),
            )
            .unwrap();
        assert!(lookup.warning.unwrap().contains("cache disabled"));
        assert!(lookup.responses[0].is_none());
        assert!(
            session
                .lookup(&[request], &[counts])
                .unwrap()
                .warning
                .is_none()
        );
    }
    #[test]
    fn busy_or_corrupt_cache_warns_and_falls_back_without_losing_identity_guard() {
        let dir = tempfile::tempdir().unwrap();
        let (identity, request, mut response, counts) = fixtures();
        let writer = Store::open(dir.path(), false).unwrap().unwrap();
        let (session, warning) =
            Session::open(Some(identity.clone()), None, dir.path(), Mode::Normal);
        assert!(warning.unwrap().contains("cache disabled"));
        let session = session.unwrap();
        assert!(
            session
                .lookup(
                    std::slice::from_ref(&request),
                    std::slice::from_ref(&counts)
                )
                .unwrap()
                .responses[0]
                .is_none()
        );
        response.model = "clef-flash".to_owned();
        assert_eq!(
            session
                .finish(&[request], &[counts], &[Some(response)])
                .unwrap()
                .valid,
            vec![false]
        );
        drop(writer);
    }
}
