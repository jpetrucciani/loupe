use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::wire::{ApiFormat, Request};

pub const CLEF_REVISION: &str = "17f0b0ad64efb65d273590632833508766b2aae6";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    #[default]
    Clef,
    Jev,
}
impl BackendKind {
    fn is_clef(&self) -> bool {
        *self == Self::Clef
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub backend: Option<BackendKind>,
    pub api_format: Option<ApiFormat>,
    pub base_url: Option<String>,
    pub path: Option<String>,
    pub model: Option<String>,
    pub encoder: Option<String>,
    pub tokenizer: Option<String>,
    pub tokenizer_revision: Option<String>,
    pub concurrency: Option<usize>,
    pub max_length: Option<usize>,
    pub max_state_tokens: Option<usize>,
    pub max_questions: Option<usize>,
    pub max_options: Option<usize>,
    pub max_total_options: Option<usize>,
    pub max_batch: Option<usize>,
    pub api_key_env: Option<String>,
    pub api_key_cmd: Option<String>,
    pub pricing: Option<crate::cost::Pricing>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub defaults: Settings,
    pub profile: IndexMap<String, Settings>,
    pub mcp: crate::mcp_access::Settings,
    pub fallback: crate::fallback::Settings,
    pub hooks: crate::hooks::Settings,
}

#[derive(Clone, Debug, Serialize)]
pub struct Limits {
    #[serde(skip_serializing_if = "BackendKind::is_clef")]
    pub backend: BackendKind,
    pub max_length: usize,
    pub max_state_tokens: usize,
    pub max_questions: usize,
    pub max_options: usize,
    pub max_total_options: usize,
    pub max_batch: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            backend: BackendKind::Clef,
            max_length: 16384,
            max_state_tokens: 16384,
            max_questions: 128,
            max_options: 512,
            max_total_options: 2048,
            max_batch: 4,
        }
    }
}

impl Limits {
    pub fn constrain(&mut self, advertised: &serde_json::Value) -> Result<()> {
        for (names, current) in [
            (
                &["max_length", "max_input_tokens"][..],
                &mut self.max_length,
            ),
            (&["max_state_tokens"][..], &mut self.max_state_tokens),
            (&["max_questions"][..], &mut self.max_questions),
            (&["max_options"][..], &mut self.max_options),
            (&["max_total_options"][..], &mut self.max_total_options),
            (&["max_batch"][..], &mut self.max_batch),
        ] {
            for name in names {
                if let Some(value) = advertised.get(name) {
                    let limit = value
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| *n > 0)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorKind::Unavailable,
                                format!("invalid server limit {name}"),
                            )
                        })?;
                    *current = (*current).min(limit);
                }
            }
        }
        Ok(())
    }

    pub fn validate_request(&self, request: &Request) -> Result<()> {
        self.validate_schema(request)?;
        if self.backend == BackendKind::Jev
            && !matches!(
                request.state,
                serde_json::Value::String(_)
                    | serde_json::Value::Object(_)
                    | serde_json::Value::Array(_)
            )
        {
            return Err(Error::new(
                ErrorKind::Input,
                "Jev state must be a string, object or array",
            ));
        }
        Ok(())
    }

    /// Check model/questions without requiring an input state during schema preparation.
    pub fn validate_schema(&self, request: &Request) -> Result<()> {
        request.validate()?;
        if request.questions.len() > self.max_questions {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "{} questions exceed limit {}",
                    request.questions.len(),
                    self.max_questions
                ),
            ));
        }
        let mut total = 0usize;
        for (id, question) in &request.questions {
            if self.backend == BackendKind::Jev {
                if !question.instructions().is_some_and(|value| {
                    matches!(
                        value,
                        serde_json::Value::String(_)
                            | serde_json::Value::Object(_)
                            | serde_json::Value::Array(_)
                    )
                }) {
                    return Err(Error::new(
                        ErrorKind::Input,
                        format!("Jev question {id} requires string, object or array instructions"),
                    ));
                }
                if let crate::wire::Question::Score { criteria, .. } = question
                    && !(2..=10).contains(&criteria.len())
                {
                    return Err(Error::new(
                        ErrorKind::Input,
                        format!("Jev Score {id} requires 2 to 10 levels"),
                    ));
                }
                for (_, description) in question.options() {
                    let valid = matches!(
                        description,
                        serde_json::Value::String(_)
                            | serde_json::Value::Object(_)
                            | serde_json::Value::Array(_)
                    ) || matches!(question, crate::wire::Question::Choice { .. })
                        && description.is_null();
                    if !valid {
                        return Err(Error::new(
                            ErrorKind::Input,
                            format!("Jev question {id} has an unsupported option description"),
                        ));
                    }
                }
            }
            let count = question.options().len();
            let maximum = if self.backend == BackendKind::Jev {
                self.max_options.min(255)
            } else {
                self.max_options
            };
            if count > maximum {
                return Err(Error::new(
                    ErrorKind::Input,
                    format!("question {id} has {count} options, limit {}", maximum),
                ));
            }
            total = total
                .checked_add(count)
                .ok_or_else(|| Error::new(ErrorKind::Input, "option count overflow"))?;
        }
        if total > self.max_total_options {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "{total} options exceed total limit {}",
                    self.max_total_options
                ),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Profile {
    pub name: String,
    pub base_url: String,
    pub path: String,
    pub api_format: ApiFormat,
    pub model: String,
    pub encoder: String,
    pub tokenizer: String,
    pub tokenizer_revision: String,
    pub concurrency: usize,
    pub limits: Limits,
    pub cache_dir: PathBuf,
    pub api_key: Option<String>,
    pub api_key_required: bool,
    pub api_key_cmd: Option<String>,
    pub request_timeout: Duration,
    pub pricing: Option<crate::cost::Pricing>,
}

pub fn xdg_path(env: &HashMap<String, String>, variable: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = env.get(variable) {
        return Ok(PathBuf::from(path).join("loupe"));
    }
    let home = env
        .get("HOME")
        .ok_or_else(|| Error::new(ErrorKind::Config, format!("set {variable} or HOME")))?;
    Ok(Path::new(home).join(fallback).join("loupe"))
}

impl Config {
    pub fn load(path: &Path, required: bool) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(|error| {
                Error::new(ErrorKind::Config, format!("{}: {error}", path.display()))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
                Ok(Self::default())
            }
            Err(error) => Err(Error::io(&format!("reading {}", path.display()), error)),
        }
    }

    pub fn resolve(
        &self,
        selected: Option<&str>,
        cli: &Settings,
        env: &HashMap<String, String>,
    ) -> Result<Profile> {
        let selected = selected
            .or_else(|| env.get("LOUPE_PROFILE").map(String::as_str))
            .or_else(|| self.profile.first().map(|(name, _)| name.as_str()))
            .unwrap_or("local");
        let empty = Settings::default();
        let profile = match self.profile.get(selected) {
            Some(profile) => profile,
            None if self.profile.is_empty() && selected == "local" => &empty,
            None => {
                return Err(Error::new(
                    ErrorKind::Config,
                    format!("unknown profile {selected}"),
                ));
            }
        };
        let get = |cli: &Option<String>,
                   variable: &str,
                   profile: &Option<String>,
                   default: &Option<String>,
                   builtin: &str| {
            cli.as_ref()
                .or_else(|| env.get(variable))
                .or(profile.as_ref())
                .or(default.as_ref())
                .cloned()
                .unwrap_or_else(|| builtin.to_owned())
        };
        let base_url = get(
            &cli.base_url,
            "LOUPE_URL",
            &profile.base_url,
            &self.defaults.base_url,
            "http://127.0.0.1:8015",
        );
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| {
            Error::new(
                ErrorKind::Config,
                "base URL must be an absolute HTTP(S) URL",
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::new(
                ErrorKind::Config,
                "base URL must use HTTP(S) without embedded credentials, query or fragment",
            ));
        }
        let api_format = if let Some(format) = cli.api_format {
            format
        } else if let Some(value) = env.get("LOUPE_API_FORMAT") {
            value.parse()?
        } else {
            profile
                .api_format
                .or(self.defaults.api_format)
                .unwrap_or_default()
        };
        let path = get(
            &cli.path,
            "LOUPE_PATH",
            &profile.path,
            &self.defaults.path,
            api_format.default_path(),
        );
        if !path.starts_with('/') || path.contains('?') || path.contains('#') {
            return Err(Error::new(
                ErrorKind::Config,
                "endpoint path must start with / and contain no query or fragment",
            ));
        }
        let number = |cli: Option<usize>,
                      profile: Option<usize>,
                      default: Option<usize>,
                      builtin| cli.or(profile).or(default).unwrap_or(builtin);
        let backend = cli
            .backend
            .or(profile.backend)
            .or(self.defaults.backend)
            .unwrap_or(if api_format == ApiFormat::Decisions {
                BackendKind::Jev
            } else {
                BackendKind::Clef
            });
        let encoder = get(
            &cli.encoder,
            "LOUPE_ENCODER",
            &profile.encoder,
            &self.defaults.encoder,
            if backend == BackendKind::Jev || api_format == ApiFormat::Decisions {
                "estimate"
            } else {
                "clef-v1"
            },
        );
        if backend == BackendKind::Jev && encoder != "estimate" {
            return Err(Error::new(
                ErrorKind::Config,
                "Jev profiles require encoder = estimate; clef-v1 exact counts do not apply",
            ));
        }
        if api_format == ApiFormat::Decisions && encoder != "estimate" {
            return Err(Error::new(
                ErrorKind::Config,
                "Decisions requires encoder = estimate; clef-v1 exact counts do not apply",
            ));
        }
        let concurrency = if let Some(cli) = cli.concurrency {
            cli
        } else if let Some(value) = env.get("LOUPE_CONCURRENCY") {
            value.parse().map_err(|_| {
                Error::new(
                    ErrorKind::Config,
                    "LOUPE_CONCURRENCY must be a positive integer",
                )
            })?
        } else {
            number(
                None,
                profile.concurrency,
                self.defaults.concurrency,
                if encoder == "clef-v1" { 32 } else { 16 },
            )
        };
        let limits = Limits {
            backend,
            max_length: number(
                cli.max_length,
                profile.max_length,
                self.defaults.max_length,
                16384,
            ),
            max_state_tokens: number(
                cli.max_state_tokens,
                profile.max_state_tokens,
                self.defaults.max_state_tokens,
                16384,
            ),
            max_questions: number(
                cli.max_questions,
                profile.max_questions,
                self.defaults.max_questions,
                128,
            ),
            max_options: number(
                cli.max_options,
                profile.max_options,
                self.defaults.max_options,
                if backend == BackendKind::Jev {
                    255
                } else {
                    512
                },
            )
            .min(if backend == BackendKind::Jev {
                255
            } else {
                usize::MAX
            }),
            max_total_options: number(
                cli.max_total_options,
                profile.max_total_options,
                self.defaults.max_total_options,
                2048,
            ),
            max_batch: number(
                cli.max_batch,
                profile.max_batch,
                self.defaults.max_batch,
                if backend == BackendKind::Jev { 1 } else { 4 },
            ),
        };
        if [
            concurrency,
            limits.max_length,
            limits.max_state_tokens,
            limits.max_questions,
            limits.max_options,
            limits.max_total_options,
            limits.max_batch,
        ]
        .contains(&0)
        {
            return Err(Error::new(
                ErrorKind::Config,
                "concurrency and server limits must be positive",
            ));
        }
        let api_key_env = profile
            .api_key_env
            .as_ref()
            .or(self.defaults.api_key_env.as_ref());
        let api_key = env
            .get("LOUPE_API_KEY")
            .cloned()
            .or_else(|| api_key_env.and_then(|name| env.get(name).cloned()));
        let pricing = cli
            .pricing
            .as_ref()
            .or(profile.pricing.as_ref())
            .or(self.defaults.pricing.as_ref())
            .cloned();
        if let Some(pricing) = &pricing {
            pricing.validate()?;
        }
        Ok(Profile {
            name: selected.to_owned(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            path,
            api_format,
            model: get(
                &cli.model,
                "LOUPE_MODEL",
                &profile.model,
                &self.defaults.model,
                if backend == BackendKind::Jev {
                    "jev-latest"
                } else {
                    "clef-flash"
                },
            ),
            encoder,
            tokenizer: get(
                &cli.tokenizer,
                "LOUPE_TOKENIZER",
                &profile.tokenizer,
                &self.defaults.tokenizer,
                "hf:Cloudflare/clef-flash",
            ),
            tokenizer_revision: get(
                &cli.tokenizer_revision,
                "LOUPE_TOKENIZER_REVISION",
                &profile.tokenizer_revision,
                &self.defaults.tokenizer_revision,
                CLEF_REVISION,
            ),
            concurrency,
            limits,
            cache_dir: if let Some(path) = env.get("LOUPE_CACHE_DIR") {
                PathBuf::from(path)
            } else {
                xdg_path(env, "XDG_CACHE_HOME", ".cache")?
            },
            api_key,
            api_key_required: api_key_env.is_some(),
            api_key_cmd: profile
                .api_key_cmd
                .as_ref()
                .or(self.defaults.api_key_cmd.as_ref())
                .cloned(),
            request_timeout: Duration::from_secs(30),
            pricing,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn jev_capabilities_are_checked_before_submission_without_changing_clef() {
        let config: crate::config::Config = toml::from_str(
            "[profile.jev]\nbackend='jev'\nbase_url='https://api.typesafe.ai'\nmax_options=999\n",
        )
        .unwrap();
        let env = std::collections::HashMap::from([("HOME".into(), "/tmp".into())]);
        let profile = config
            .resolve(Some("jev"), &crate::config::Settings::default(), &env)
            .unwrap();
        assert_eq!(profile.encoder, "estimate");
        assert_eq!(profile.model, "jev-latest");
        assert_eq!(profile.limits.max_options, 255);
        assert_eq!(
            serde_json::to_value(&profile.limits).unwrap()["backend"],
            "jev"
        );
        assert!(
            serde_json::to_value(crate::config::Limits::default())
                .unwrap()
                .get("backend")
                .is_none()
        );
        let schema = crate::wire::Request {
            model: profile.model.clone(),
            state: serde_json::Value::Null,
            questions: indexmap::IndexMap::from([(
                "question".into(),
                crate::wire::Question::Noul {
                    instructions: Some(serde_json::json!("Is ALPHA present?")),
                    criteria: None,
                },
            )]),
        };
        assert!(profile.limits.validate_schema(&schema).is_ok());
        assert!(profile.limits.validate_request(&schema).is_err());
        for (levels, valid) in [(1, false), (2, true), (10, true), (11, false)] {
            let request = crate::wire::Request {
                model: profile.model.clone(),
                state: serde_json::json!("record"),
                questions: indexmap::IndexMap::from([(
                    "score".into(),
                    crate::wire::Question::Score {
                        instructions: Some(serde_json::json!("Rate this")),
                        criteria: (0..levels)
                            .map(|n| serde_json::json!(n.to_string()))
                            .collect(),
                    },
                )]),
            };
            assert_eq!(profile.limits.validate_request(&request).is_ok(), valid);
            assert!(
                crate::config::Limits::default()
                    .validate_request(&request)
                    .is_ok()
            );
        }
        for instructions in [
            None,
            Some(serde_json::json!(true)),
            Some(serde_json::json!(7)),
        ] {
            let request = crate::wire::Request {
                model: profile.model.clone(),
                state: serde_json::json!("record"),
                questions: indexmap::IndexMap::from([(
                    "question".into(),
                    crate::wire::Question::Noul {
                        instructions,
                        criteria: None,
                    },
                )]),
            };
            assert!(profile.limits.validate_request(&request).is_err());
        }
        let invalid: crate::config::Config =
            toml::from_str("[profile.jev]\nbackend='jev'\nencoder='clef-v1'\n").unwrap();
        assert_eq!(
            invalid
                .resolve(Some("jev"), &crate::config::Settings::default(), &env)
                .err()
                .unwrap()
                .exit_code(),
            78
        );
    }
    #[test]
    #[allow(clippy::unwrap_used)]
    fn configured_pricing_is_explicit_validated_and_obeys_profile_precedence() {
        let config:crate::config::Config=toml::from_str("[defaults.pricing]\ninput_usd_per_million=2.0\noutput_usd_per_million=4.0\n[profile.local.pricing]\ninput_usd_per_million=3.0\noutput_usd_per_million=5.0\n").unwrap();
        let env = std::collections::HashMap::from([("HOME".to_owned(), "/tmp".to_owned())]);
        let profile = config
            .resolve(Some("local"), &crate::config::Settings::default(), &env)
            .unwrap();
        assert_eq!(profile.pricing.unwrap().input_usd_per_million, 3.0);
        let settings = crate::config::Settings {
            pricing: Some(crate::cost::Pricing {
                input_usd_per_million: 0.0,
                output_usd_per_million: 0.0,
            }),
            ..crate::config::Settings::default()
        };
        assert_eq!(
            config
                .resolve(Some("local"), &settings, &env)
                .unwrap()
                .pricing
                .unwrap()
                .input_usd_per_million,
            0.0
        );
        let invalid: crate::config::Config = toml::from_str(
            "[profile.local.pricing]\ninput_usd_per_million=nan\noutput_usd_per_million=0.0\n",
        )
        .unwrap();
        assert_eq!(
            invalid
                .resolve(Some("local"), &crate::config::Settings::default(), &env)
                .err()
                .unwrap()
                .exit_code(),
            78
        );
    }
    use crate::config::{Config, Settings};
    use std::collections::HashMap;
    #[test]
    fn resolution_preserves_profile_order_and_cli_env_precedence() {
        let config: Config = toml::from_str("[defaults]\nmodel='default'\n[profile.z]\nmodel='profile'\n[profile.a]\nmodel='second'\n").unwrap();
        let mut env = HashMap::from([
            ("HOME".to_owned(), "/tmp".to_owned()),
            ("LOUPE_MODEL".to_owned(), "env".to_owned()),
            ("LOUPE_API_FORMAT".to_owned(), "decisions".to_owned()),
        ]);
        let cli = Settings {
            model: Some("cli".to_owned()),
            ..Settings::default()
        };
        assert_eq!(config.resolve(None, &cli, &env).unwrap().model, "cli");
        let resolved = config.resolve(None, &cli, &env).unwrap();
        assert_eq!(resolved.api_format, crate::wire::ApiFormat::Decisions);
        assert_eq!(resolved.path, "/v1/decisions");
        assert_eq!(resolved.encoder, "estimate");
        let systemone = config
            .resolve(
                None,
                &Settings {
                    api_format: Some(crate::wire::ApiFormat::SystemOne),
                    ..Settings::default()
                },
                &env,
            )
            .unwrap();
        assert_eq!(systemone.api_format, crate::wire::ApiFormat::SystemOne);
        assert_eq!(systemone.path, "/v1/systemone");
        env.remove("LOUPE_API_FORMAT");
        assert_eq!(
            config
                .resolve(None, &Settings::default(), &env)
                .unwrap()
                .name,
            "z"
        );
        assert_eq!(
            config
                .resolve(None, &Settings::default(), &env)
                .unwrap()
                .model,
            "env"
        );
        env.remove("LOUPE_MODEL");
        let mut profile = config.resolve(None, &Settings::default(), &env).unwrap();
        profile
            .limits
            .constrain(&serde_json::json!({"max_input_tokens":1024,"max_queued_requests":2}))
            .unwrap();
        assert_eq!(profile.limits.max_length, 1024);
        assert_eq!(profile.limits.max_batch, 4);
    }

    #[test]
    fn malformed_config_urls_and_zero_limits_fail() {
        let config = Config::default();
        let env = HashMap::from([("HOME".to_owned(), "/tmp".to_owned())]);
        for cli in [
            Settings {
                base_url: Some("http://key:secret@server".to_owned()),
                ..Settings::default()
            },
            Settings {
                concurrency: Some(0),
                ..Settings::default()
            },
        ] {
            assert_eq!(
                config.resolve(None, &cli, &env).err().unwrap().exit_code(),
                78
            );
        }
        assert!(toml::from_str::<Config>("[defaults]\nunknown=1").is_err());
        assert_eq!(
            config
                .resolve(None, &Settings::default(), &env)
                .unwrap()
                .concurrency,
            32
        );
        assert_eq!(
            config
                .resolve(
                    None,
                    &Settings {
                        encoder: Some("estimate".to_owned()),
                        ..Settings::default()
                    },
                    &env
                )
                .unwrap()
                .concurrency,
            16
        );
    }
}
