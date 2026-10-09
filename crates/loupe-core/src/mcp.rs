//! Read-only classifier operations shared by the MCP protocol adapter.

use crate::backend::{Backend, Discovery, Evaluation, with_deadline};
use crate::budget::Budget;
use crate::cache_session::Mode;
use crate::candidates::Document;
use crate::config::Profile;
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::extract::{Kind, Parser};
use crate::mcp_access::{Scope, Settings};
use crate::policy::{Policy, Probability, UncertainAction};
use crate::preset_store::{Preset, Store};
use crate::selection::{Session, Step};
use crate::tournament::{Options, Planner, Template};
use crate::wire::{Question, Request};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classify {
    #[serde(default, deserialize_with = "present_state")]
    pub state: Option<Value>,
    pub path: Option<PathBuf>,
    pub question: Option<Question>,
    pub questions: Option<IndexMap<String, Question>>,
    pub preset: Option<String>,
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub ensemble: bool,
    pub min_agreement: Option<f64>,
}

fn present_state<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Select {
    pub state: Option<String>,
    pub path: Option<PathBuf>,
    pub query: String,
    pub take: Option<usize>,
    pub candidates: Option<Kind>,
    pub normalize: Option<crate::normalize::Options>,
}

pub struct Prepared {
    pub request: Request,
    pub counts: Counts,
    policies: IndexMap<String, Policy>,
    preset: Option<Preset>,
    ensembles: crate::ensemble::Set,
    minimum: Probability,
}

pub struct Engine {
    scope: Scope,
    store: Store,
    presets: IndexMap<String, Preset>,
    profile: Profile,
    encoder: Option<Arc<ClefEncoder>>,
    byte_limit: usize,
    deadline: Duration,
    budget: Option<u64>,
}

impl Engine {
    pub fn new(
        settings: &Settings,
        store: Store,
        profile: Profile,
        deadline: Duration,
        budget: Option<u64>,
    ) -> Result<Self> {
        settings.validate()?;
        if deadline.is_zero() {
            return Err(usage("MCP deadline must be positive"));
        }
        if budget.is_some() && profile.encoder != "clef-v1" {
            return Err(usage("hard MCP input-token budgets require clef-v1"));
        }
        let scope = Scope::new(
            settings,
            &std::env::current_dir().map_err(|error| Error::io("reading MCP directory", error))?,
        )?;
        let mut presets = IndexMap::new();
        for name in &settings.presets {
            let preset = store.lookup(name)?;
            preset.require_trusted()?;
            preset.validate(&profile.limits, &profile.model)?;
            crate::templates::lint(&preset)?;
            presets.insert(name.clone(), preset);
        }
        let encoder = crate::encoder::load_profile(&profile)?.map(Arc::new);
        Ok(Self {
            scope,
            store,
            presets,
            profile,
            encoder,
            byte_limit: settings.byte_limit(),
            deadline,
            budget,
        })
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn presets(&self) -> Value {
        json!({"presets":self.presets.values().map(|preset| json!({"name":preset.name,"content_hash":preset.content_hash,"document":preset.document})).collect::<Vec<_>>()})
    }

    fn input(&self, state: Option<Value>, path: Option<PathBuf>) -> Result<Value> {
        match (state, path) {
            (Some(state), None) => {
                self.bound(&state)?;
                Ok(state)
            }
            (None, Some(path)) => Ok(Value::String(self.scope.read_text(&path)?)),
            _ => Err(usage("provide exactly one of state or path")),
        }
    }

    fn bound(&self, value: &impl serde::Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|error| usage(&format!("invalid MCP data: {error}")))?;
        if bytes.len() > self.byte_limit {
            return Err(usage("MCP input exceeds max_input_bytes"));
        }
        Ok(())
    }

    pub fn prepare(&self, arguments: Classify) -> Result<Prepared> {
        let sources = usize::from(arguments.question.is_some())
            + usize::from(arguments.questions.is_some())
            + usize::from(arguments.preset.is_some());
        if sources != 1 || !arguments.args.is_empty() && arguments.preset.is_none() {
            return Err(usage(
                "provide exactly one of question, questions or an allowlisted preset; args require a preset",
            ));
        }
        if arguments.min_agreement.is_some() && !arguments.ensemble {
            return Err(usage("min_agreement requires ensemble=true"));
        }
        let state = self.input(arguments.state, arguments.path)?;
        let mut preset = None;
        let mut document = json!({});
        let mut questions = if let Some(name) = arguments.preset {
            let pinned = self
                .presets
                .get(&name)
                .ok_or_else(|| usage("preset is not in the MCP allowlist"))?;
            let current = self.store.lookup(&name)?;
            current.require_trusted()?;
            if current.content_hash != pinned.content_hash
                || current.origin.path() != pinned.origin.path()
            {
                return Err(usage(
                    "MCP preset changed; inspect its trust and restart the server",
                ));
            }
            document = crate::templates::expand_with_reader(
                pinned,
                &self.store,
                &arguments.args,
                &|path| self.scope.read_text(path),
            )?;
            if !crate::factors::Set::from_document(&document)?.is_empty() {
                return Err(usage(
                    "weighted factor presets require CLI explain/ask/tokens; MCP accepts classification rubrics",
                ));
            }
            // A scalar calibration belongs to its precise CLI view, not an MCP
            // caller-selected structured state or jointly expanded question set.
            if crate::calibration_pin::validate(&document)?.is_some() {
                return Err(usage(
                    "calibrated presets require their fitted execution view; MCP accepts unpinned rubrics",
                ));
            }
            let fields = if let Some(fields) = document.get("questions") {
                serde_json::from_value(fields.clone())
                    .map_err(|error| usage(&format!("preset questions: {error}")))?
            } else {
                IndexMap::from([(name, crate::presets::question(&document)?)])
            };
            preset = Some(pinned.clone());
            fields
        } else if let Some(question) = arguments.question {
            IndexMap::from([("decision".into(), question)])
        } else {
            arguments
                .questions
                .ok_or_else(|| usage("questions missing"))?
        };
        let policies = questions
            .iter()
            .map(|(id, question)| {
                Ok((
                    id.clone(),
                    Policy::from_field(&document, id, question.kind())?,
                ))
            })
            .collect::<Result<IndexMap<_, _>>>()?;
        let ensembles = if arguments.ensemble {
            let set = crate::ensemble::Set::from_document(&document, &questions)?;
            if set.is_empty() {
                return Err(usage("ensemble requires named preset variants"));
            }
            set
        } else {
            crate::ensemble::Set::default()
        };
        let minimum = crate::ensemble::minimum(&document, arguments.min_agreement)?;
        ensembles.append(&mut questions)?;
        crate::signals::Set::from_document(&document)?.append(&mut questions)?;
        if let Some(context) = document.get("context") {
            crate::presets::add_context(&mut questions, context.clone());
        }
        let request = Request {
            model: self.profile.model.clone(),
            state,
            questions,
        };
        self.bound(&request)?;
        self.profile.limits.validate_request(&request)?;
        let counts = crate::pack::count(self.encoder.as_deref(), &request)?;
        counts.preflight(&self.profile.limits)?;
        Ok(Prepared {
            request,
            counts,
            policies,
            preset,
            ensembles,
            minimum,
        })
    }

    pub fn plan(&self, arguments: Classify) -> Result<Value> {
        let prepared = self.prepare(arguments)?;
        Ok(json!({"request":prepared.request,"counts":prepared.counts}))
    }

    fn expires(&self) -> Result<Instant> {
        Instant::now()
            .checked_add(self.deadline)
            .ok_or_else(|| usage("MCP deadline outside clock range"))
    }

    async fn backend(
        &self,
        backend: &Backend,
        expires: Instant,
    ) -> Result<(Backend, Discovery, Option<String>)> {
        let discovery =
            with_deadline(Some(expires), backend.discover(&self.profile.limits)).await?;
        let (backend, warning) = with_deadline(
            Some(expires),
            backend.clone().with_cache(
                &discovery,
                self.encoder.clone(),
                &self.profile.cache_dir,
                Mode::ReadOnly,
            ),
        )
        .await?;
        Ok((backend, discovery, warning))
    }

    pub async fn classify(
        &self,
        backend: &Backend,
        arguments: Classify,
        raw: bool,
    ) -> Result<Value> {
        let expires = self.expires()?;
        let prepared = self.prepare(arguments)?;
        let (backend, discovery, warning) = self.backend(backend, expires).await?;
        discovery.limits.validate_request(&prepared.request)?;
        prepared.counts.preflight(&discovery.limits)?;
        if let Some(preset) = &prepared.preset {
            preset.check_policy_identity(discovery.full_model_identity.as_deref())?;
        }
        let budget = Budget::new(self.budget);
        let evaluation = backend
            .evaluate(
                &prepared.request,
                &prepared.counts,
                &budget,
                true,
                Some(expires),
            )
            .await?;
        if raw {
            return serde_json::to_value(evaluation.response)
                .map_err(|error| usage(&error.to_string()));
        }
        let answers = evaluation.response.validate(&prepared.request)?;
        let mut decisions = IndexMap::new();
        let mut ensembles = IndexMap::new();
        for (id, policy) in &prepared.policies {
            if prepared.ensembles.has_primary(id) {
                let report = prepared.ensembles.report(
                    id,
                    id,
                    &prepared.request,
                    &evaluation.response,
                    *policy,
                    prepared.minimum,
                )?;
                decisions.insert(id.clone(), report.assessment.clone());
                ensembles.insert(
                    id.clone(),
                    serde_json::to_value(report).map_err(|error| usage(&error.to_string()))?,
                );
            } else {
                decisions.insert(
                    id.clone(),
                    answers
                        .get(id)
                        .ok_or_else(|| usage("answer missing"))?
                        .assessment(policy)?,
                );
            }
        }
        Ok(
            json!({"response":evaluation.response,"decisions":decisions,"ensembles":ensembles,"cached":evaluation.cached,"counts":prepared.counts,"usage":budget.usage()?,"warnings":([warning,evaluation.warning].into_iter().flatten().collect::<Vec<_>>())}),
        )
    }

    pub async fn select(
        &self,
        backend: &Backend,
        arguments: Select,
        extract: bool,
    ) -> Result<Value> {
        let expires = self.expires()?;
        if arguments.query.trim().is_empty() || extract != arguments.candidates.is_some() {
            return Err(usage(
                "query must be nonempty; only extract requires candidates",
            ));
        }
        let normalizer = match (arguments.candidates, arguments.normalize) {
            (Some(kind), Some(options)) => {
                Some(crate::normalize::Normalizer::for_kind(kind, options)?)
            }
            (None, Some(_)) => return Err(usage("normalization requires extract")),
            _ => None,
        };
        let take = NonZeroUsize::new(arguments.take.unwrap_or(1))
            .ok_or_else(|| usage("take must be positive"))?;
        let source = arguments.path.clone();
        let state = self.input(arguments.state.map(Value::String), arguments.path)?;
        let text = state
            .as_str()
            .ok_or_else(|| usage("selection state must be text"))?;
        let lines = text.lines().take(10001).collect::<Vec<_>>();
        if lines.len() > 10000 {
            return Err(usage("MCP selection exceeds the 10000-line limit"));
        }
        let document = Document::new(text, source);
        let catalog = if let Some(kind) = arguments.candidates {
            Parser::new(kind)?.parse(&document)?
        } else {
            document.lines(false)?
        };
        if catalog.len() > 10000 {
            return Err(usage("MCP selection exceeds the 10000-candidate limit"));
        }
        let descriptions = crate::tournament::descriptions(&document, &catalog, extract, 0)?;
        let template = Template {
            model: self.profile.model.clone(),
            state: Value::Null,
            question: json!(arguments.query),
            presence_criteria: None,
            descriptions,
            companions: IndexMap::new(),
            ensemble: None,
        };
        let options = Options {
            take,
            max_rounds: NonZeroUsize::new(64).ok_or_else(|| usage("invalid round limit"))?,
        };
        // Preflight local limits before opening a response cache or contacting inference.
        let _ = Planner::new(
            template.clone(),
            &catalog,
            self.encoder.as_deref(),
            &self.profile.limits,
            options,
        )?;
        let (backend, discovery, warning) = self.backend(backend, expires).await?;
        let planner = Planner::new(
            template,
            &catalog,
            self.encoder.as_deref(),
            &discovery.limits,
            options,
        )?;
        let mut session = Session::new(
            planner,
            crate::selection::Policy {
                presence: Policy::default(),
                choice: Policy::default(),
                on_uncertain: UncertainAction::Drop,
            },
        )?;
        let mut round = session.first()?;
        let budget = Budget::new(self.budget);
        let mut history = Vec::new();
        loop {
            let mut decisions = Vec::new();
            for bracket in round.brackets() {
                let evaluation: Evaluation = backend
                    .evaluate(
                        bracket.request(),
                        bracket.counts(),
                        &budget,
                        true,
                        Some(expires),
                    )
                    .await?;
                decisions.push(session.assess_response(bracket, &evaluation.response)?);
                history.push(json!({"round":round.number(),"bracket":bracket.index(),"request":bracket.request(),"counts":bracket.counts(),"evaluation":evaluation}));
            }
            match session.advance(&round, &decisions)? {
                Step::Next { round: next } => round = next,
                Step::Final { outcome } => {
                    let selected = outcome.tournament.selected.iter().map(|scored| {
                        let candidate = catalog.iter().find(|candidate| candidate.id == scored.id).ok_or_else(|| usage("selected source candidate missing"))?;
                        let mut value = json!({"id":candidate.id,"unit":candidate.verbatim,"location":candidate.location,"probability":scored.probability});
                        if let Some(normalizer) = &normalizer {
                            let kind = arguments.candidates.ok_or_else(||usage("normalization candidate kind missing"))?;
                            value["normalized"] = serde_json::to_value(normalizer.normalize(kind,candidate.verbatim)?).map_err(|error|usage(&error.to_string()))?;
                        }
                        Ok(value)
                    }).collect::<Result<Vec<_>>>()?;
                    return Ok(
                        json!({"selected":selected,"outcome":outcome,"provenance":history,"usage":budget.usage()?,"warning":warning}),
                    );
                }
            }
        }
    }
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::mcp::{Classify, Engine};
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

    fn engine(root: &Path, allow_paths: Vec<std::path::PathBuf>, names: &[&str]) -> Engine {
        let environment = HashMap::from([
            ("HOME".into(), std::env::var("HOME").unwrap()),
            (
                "XDG_CONFIG_HOME".into(),
                root.join("xdg").to_str().unwrap().into(),
            ),
        ]);
        let store = crate::preset_store::Store::new(root, &environment).unwrap();
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER")
                        .ok()
                        .or_else(|| std::env::var("LOUPE_TOKENIZER").ok()),
                    ..crate::config::Settings::default()
                },
                &environment,
            )
            .unwrap();
        Engine::new(
            &crate::mcp_access::Settings {
                presets: names.iter().map(|name| name.to_string()).collect(),
                allow_paths,
                ..crate::mcp_access::Settings::default()
            },
            store,
            profile,
            Duration::from_secs(30),
            Some(10000),
        )
        .unwrap()
    }

    fn arguments(value: serde_json::Value) -> Classify {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn weighted_preset_calls_fail_before_silently_omitting_factor_questions() {
        let root = tempfile::tempdir().unwrap();
        let presets = root.path().join("xdg/loupe/presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::write(presets.join("weighted.toml"),"type='noul'\ninstructions='Outage?'\n[[factors]]\nid='impact'\nweight=1\ninstructions='Customer impact?'\n[[variants]]\nid='direct'\nfield='factor:impact'\ninstructions='Are customers affected?'\n").unwrap();
        let engine = engine(root.path(), Vec::new(), &["weighted"]);
        for ensemble in [false, true] {
            let error = engine
                .prepare(arguments(
                    json!({"state":"ALPHA","preset":"weighted","ensemble":ensemble}),
                ))
                .err()
                .unwrap();
            assert_eq!(error.exit_code(), 64);
            assert!(error.to_string().contains("weighted factor presets"));
        }
    }

    #[test]
    fn typed_requests_preserve_null_and_raw_instructions_and_count_real_tokens() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let engine = engine(root.path(), Vec::new(), &[]);
        let prepared = engine.prepare(arguments(json!({"state":null,"question":{"type":"noul","instructions":"Literal {{file:private}} $(false)"}}))).unwrap();
        assert!(prepared.request.state.is_null());
        assert_eq!(
            prepared.request.questions["decision"]
                .instructions()
                .unwrap(),
            "Literal {{file:private}} $(false)"
        );
        assert!(prepared.counts.exact);
        let (_, counts) = engine
            .encoder
            .as_ref()
            .unwrap()
            .encode(&prepared.request)
            .unwrap();
        assert_eq!(prepared.counts.input_tokens, counts.input_tokens);
        assert!(
            engine
                .prepare(arguments(
                    json!({"state":"one","path":"two","question":{"type":"noul"}})
                ))
                .is_err()
        );
        assert!(
            serde_json::from_value::<Classify>(
                json!({"state":"one","question":{"type":"noul"},"exec":"touch marker"})
            )
            .is_err()
        );
        assert!(!root.path().join("xdg/loupe/responses.redb").exists());
    }

    #[test]
    fn preset_allowlists_pinned_content_and_file_templates_share_read_capabilities() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let presets = root.path().join("xdg/loupe/presets");
        std::fs::create_dir_all(&presets).unwrap();
        let rubric = presets.join("reference.toml");
        let reference = root.path().join("reference.txt");
        std::fs::write(&reference, "ALPHA").unwrap();
        let context = engine(root.path(), Vec::new(), &[]);
        let resolved = reference.canonicalize().unwrap();
        let relative = resolved
            .strip_prefix(&context.store.project_root)
            .unwrap()
            .to_str()
            .unwrap();
        let instructions = format!("Does this match {{{{file:{relative}}}}} and {{{{arg.term}}}}?");
        std::fs::write(
            &rubric,
            crate::presets::to_toml(&json!({"type":"noul","instructions":instructions})).unwrap(),
        )
        .unwrap();
        let denied = engine(root.path(), Vec::new(), &["reference"]);
        assert!(
            denied
                .prepare(arguments(
                    json!({"state":"ALPHA","preset":"reference","args":{"term":"BETA"}})
                ))
                .err()
                .unwrap()
                .to_string()
                .contains("allow_paths")
        );
        let allowed = engine(root.path(), vec![reference], &["reference"]);
        let request = allowed
            .prepare(arguments(
                json!({"state":"ALPHA","preset":"reference","args":{"term":"BETA"}}),
            ))
            .unwrap();
        assert_eq!(
            request.request.questions["reference"]
                .instructions()
                .unwrap(),
            "Does this match ALPHA and BETA?"
        );
        assert!(
            allowed
                .prepare(arguments(json!({"state":"ALPHA","preset":"secret"})))
                .is_err()
        );
        std::fs::write(&rubric, "type='noul'\ninstructions='Changed rubric'\n").unwrap();
        assert!(
            allowed
                .prepare(arguments(
                    json!({"state":"ALPHA","preset":"reference","args":{"term":"BETA"}})
                ))
                .is_err()
        );
    }

    #[test]
    fn allowlisted_paths_and_joint_ensemble_plans_keep_source_and_schema_identity() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let presets = root.path().join("xdg/loupe/presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::write(presets.join("alpha.toml"), "type='noul'\ninstructions='Is ALPHA present?'\n[[variants]]\nid='rephrase'\ninstructions='Does this explicitly mention ALPHA?'\n").unwrap();
        let source = root.path().join("source.txt");
        std::fs::write(&source, "ALPHA\r\n").unwrap();
        let engine = engine(root.path(), vec![source.clone()], &["alpha"]);
        let request = engine
            .prepare(arguments(
                json!({"path":source,"preset":"alpha","ensemble":true}),
            ))
            .unwrap();
        assert_eq!(request.request.state, "ALPHA\r\n");
        assert_eq!(request.request.questions.len(), 2);
        assert_eq!(request.minimum.get(), 1.0);
        assert!(
            engine
                .prepare(arguments(
                    json!({"path":root.path().join("other"),"preset":"alpha"})
                ))
                .is_err()
        );
        assert!(
            engine
                .prepare(arguments(
                    json!({"state":"ALPHA","preset":"alpha","min_agreement":0.5})
                ))
                .is_err()
        );
    }
}
