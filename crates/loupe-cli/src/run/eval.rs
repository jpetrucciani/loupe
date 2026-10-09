use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use loupe_core::backend::{Backend, Discovery, with_deadline};
use loupe_core::budget::{Budget, Usage};
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::encoder::{self, ClefEncoder};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::pack::{Mode as PackMode, SourceKind, Window};
use loupe_core::policy::{Policy, PolicyFlag, Probability, QuestionKind};
use loupe_core::preset_store::{Origin, Preset, Store};
use loupe_core::suites::{
    Case, Label, Manifest, Metric, Pin, Provenance, Split, SplitConfig, Splits, Suite, Task,
};
use loupe_core::task_metrics::{self, Comparison, Observation};
use loupe_core::wire::{Answer, Question, Request};
use loupe_core::{presets, split, templates, truncate};

use crate::args::{Cli, Eval, EvalSplit};
use crate::run::focus_jobs::FocusSettings;
use crate::run::{
    Completed, Execution, SplitSettings, TruncateSettings, duration, evaluate_job,
    interrupt_signal, json_error, plan_jobs, write_json,
};

pub(crate) mod calibrate;
mod ensemble;
mod gates;
mod join;
mod output;
mod runtime;
mod selection;
mod variants;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyData {
    threshold: f64,
    margin: f64,
    min_confidence: f64,
    min_gap: f64,
}
impl From<Policy> for PolicyData {
    fn from(policy: Policy) -> Self {
        Self {
            threshold: policy.threshold.get(),
            margin: policy.margin,
            min_confidence: policy.min_confidence.get(),
            min_gap: policy.min_gap,
        }
    }
}
impl PolicyData {
    fn policy(&self) -> Result<Policy> {
        let policy = Policy {
            threshold: Probability::new(self.threshold)?,
            margin: self.margin,
            min_confidence: Probability::new(self.min_confidence)?,
            min_gap: self.min_gap,
        };
        policy.validate()?;
        Ok(policy)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    pub(crate) verified: bool,
    pub(crate) model: Option<loupe_core::identity::Model>,
    pub(crate) requested_model: String,
    pub(crate) local_encoder: String,
    pub(crate) server_encoder: Option<String>,
    pub(crate) reason: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunReport {
    pub(crate) schema_version: u32,
    pub(crate) generated_at: String,
    pub(crate) cli_version: String,
    pub(crate) status: String,
    pub(crate) identity: Identity,
    pub(crate) endpoint: String,
    pub(crate) environment: Value,
    pub(crate) server: Value,
    pub(crate) token_usage: Usage,
    pub(crate) pricing: Option<loupe_core::cost::Pricing>,
    pub(crate) cost_usd_estimate: Option<f64>,
    pub(crate) cost_unknown_output: bool,
    pub(crate) suites: Vec<SuiteReport>,
    pub(crate) error: Option<Failure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) acceptance: Option<gates::Report>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Failure {
    pub(crate) code: u8,
    pub(crate) message: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionReport {
    pub(crate) field: String,
    pub(crate) rubric_signature: String,
    pub(crate) question: Question,
    pub(crate) policy: PolicyData,
    pub(crate) case_ids: Vec<String>,
    pub(crate) metrics: task_metrics::Report,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RequestRole {
    OriginalSplitInput,
    OriginalFocusInput,
    FocusFinal,
    Evaluated,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensemble: Option<ensemble::Saved>,
    pub(crate) case_id: String,
    pub(crate) index: u64,
    pub(crate) label: Label,
    pub(crate) source: String,
    pub(crate) reviewed: bool,
    pub(crate) sampling: Option<loupe_core::suites::Sampling>,
    pub(crate) preset: Value,
    pub(crate) effective_document: Value,
    pub(crate) policies: BTreeMap<String, PolicyData>,
    pub(crate) questions: IndexMap<String, Question>,
    pub(crate) base_question_order: Vec<String>,
    pub(crate) binding: BTreeMap<String, String>,
    pub(crate) request: Request,
    pub(crate) request_role: RequestRole,
    pub(crate) request_question_order: Vec<String>,
    pub(crate) counts: loupe_core::encoder::Counts,
    pub(crate) evaluation: Option<loupe_core::backend::Evaluation>,
    pub(crate) packing: Value,
    pub(crate) provenance: Value,
    pub(crate) reference: Option<loupe_core::suites::Reference>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Phase {
    pub(crate) name: String,
    pub(crate) pass: usize,
    pub(crate) cache_requested: bool,
    pub(crate) cache_hits: u64,
    pub(crate) logical_requests: u64,
    pub(crate) cache_hit_rate: Option<f64>,
    pub(crate) usage: Usage,
    pub(crate) timing: task_metrics::Timing,
    pub(crate) groups: Vec<Value>,
    pub(crate) error: Option<Failure>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuiteReport {
    pub(crate) manifest: Manifest,
    pub(crate) splits: Splits,
    pub(crate) partition: String,
    pub(crate) selected_case_ids: Vec<String>,
    pub(crate) records: Vec<Record>,
    pub(crate) questions: Vec<QuestionReport>,
    pub(crate) parity: Option<task_metrics::Parity>,
    pub(crate) phases: Vec<Phase>,
    pub(crate) warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) join: Option<join::Report>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selection: Option<selection::Report>,
}

struct PreparedCase {
    selection: Option<selection::Prepared>,
    ensemble: Option<ensemble::Settings>,
    case: Case,
    unit: Unit,
    preset: Value,
    document: Value,
    questions: IndexMap<String, Question>,
    policies: BTreeMap<String, PolicyData>,
    settings: ViewSettings,
    signature: String,
}
struct ViewSettings {
    pack: PackMode,
    source: SourceKind,
    split: bool,
    modes: IndexMap<String, loupe_core::combine::Mode>,
    overlap: usize,
    max_chunks: usize,
    focus: Option<std::num::NonZeroUsize>,
    truncate: Option<truncate::Mode>,
    path: String,
}

fn packing(settings: &ViewSettings) -> Value {
    let mut value = json!({"declared":format!("{:?}",settings.pack),"combine":settings.modes,"split":settings.split,"focus":settings.focus,"truncate":settings.truncate,"path":settings.path});
    if settings.split || settings.focus.is_some() {
        value["overlap"] = json!(settings.overlap);
        value["max_chunks"] = json!(settings.max_chunks);
    }
    if matches!(settings.pack, PackMode::Window(_)) {
        value["question_id_source"] = json!(format!("{:?}", settings.source));
    }
    value
}
struct Group {
    cases: Vec<Arc<PreparedCase>>,
}
struct PreparedSuite {
    suite: Suite,
    splits: Splits,
    selected: Vec<String>,
    cases: Vec<Arc<PreparedCase>>,
}
struct GroupResult {
    results: Vec<crate::run::failure::WorkResult<Completed>>,
    elapsed: Duration,
}

struct RunScope<'a> {
    budget: &'a Budget,
    deadline: Option<Instant>,
    initialize_splits: bool,
    recalibrate: bool,
    baseline: Option<&'a gates::Baseline>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Eval,
    store: &Store,
    profile: Profile,
    config: &loupe_core::config::Config,
) -> Result<u8> {
    let budget = Budget::new(options.budget);
    let deadline = deadline(options)?;
    if options.baseline.is_some() {
        return gates::execute(cli, options, store, profile, &budget, deadline).await;
    }
    if !options.compare.is_empty()
        || !options.sweep_pack.is_empty()
        || !options.sweep_combine.is_empty()
        || !options.sweep_focus.is_empty()
    {
        return variants::execute(cli, options, store, profile, config, &budget, deadline).await;
    }
    let report = run(
        cli,
        options,
        store,
        profile,
        RunScope {
            budget: &budget,
            deadline,
            initialize_splits: true,
            recalibrate: false,
            baseline: None,
        },
    )
    .await?;
    let code = report.error.as_ref().map_or(0, |error| error.code);
    if let Some(root) = &options.report {
        let mut destination = output::Destination::open(root)?;
        let path = destination.publish(&report)?;
        if !cli.quiet {
            eprintln!("loupe: reports published under {}", path.display());
        }
    }
    if options.json {
        write_json(&mut io::stdout().lock(), &report)?;
    } else {
        io::stdout()
            .lock()
            .write_all(output::markdown(&report).as_bytes())
            .map_err(|error| Error::io("writing eval report", error))?;
    }
    Ok(code)
}

fn deadline(options: &Eval) -> Result<Option<Instant>> {
    let started = Instant::now();
    options
        .deadline
        .as_deref()
        .map(duration)
        .transpose()?
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| usage("deadline is outside the clock range"))
        })
        .transpose()
}

async fn run(
    cli: &Cli,
    options: &Eval,
    store: &Store,
    mut profile: Profile,
    scope: RunScope<'_>,
) -> Result<RunReport> {
    let budget = scope.budget;
    let deadline = scope.deadline;
    let usage_before = budget.usage()?;
    let encoder = encoder::load_profile(&profile)?.map(Arc::new);
    if options.budget.is_some() && encoder.is_none() {
        return Err(usage("hard --budget requires verified clef-v1 counts"));
    }
    if options.allow_unverified && options.dry_run {
        return Err(usage(
            "offline plans do not verify model identity; omit --allow-unverified",
        ));
    }
    let suites = load_suites(options, store, &profile)?;
    if suites.iter().any(|suite| suite.manifest.task == Task::Join) {
        return join::run(cli, options, store, profile, scope, encoder, suites).await;
    }
    let partition = match options.partition.unwrap_or(EvalSplit::Test) {
        EvalSplit::Test => Split::Test,
        EvalSplit::Calibrate => Split::Calibrate,
        EvalSplit::Captured => Split::Captured,
    };
    let mut prepared = Vec::new();
    for suite in suites {
        let splits = if options.init_splits {
            suite.planned_splits()?
        } else {
            suite.load_splits()?
        };
        let selected = suite
            .cases_for(&splits, partition)?
            .iter()
            .map(|case| case.id.clone())
            .collect::<BTreeSet<_>>();
        if selected.is_empty() {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "suite {} has no cases in selected partition",
                    suite.manifest.id
                ),
            ));
        }
        let mut cases = Vec::new();
        for (position, case) in suite.cases.iter().enumerate() {
            if selected.contains(&case.id) {
                let prepared_case = prepare_case(
                    &suite,
                    position,
                    options,
                    store,
                    &profile,
                    encoder.as_deref(),
                    scope.recalibrate,
                )?;
                if !scope.recalibrate
                    && let Some(pin) =
                        loupe_core::calibration_pin::validate(&prepared_case.preset["document"])?
                {
                    pin.check_ensemble(
                        options.ensemble,
                        loupe_core::ensemble::minimum(
                            &prepared_case.document,
                            options.min_agreement,
                        )?,
                    )?;
                }
                cases.push(Arc::new(prepared_case));
            }
        }
        validate_metrics(&suite.manifest, &cases)?;
        prepared.push(PreparedSuite {
            selected: cases.iter().map(|case| case.case.id.clone()).collect(),
            cases,
            splits,
            suite,
        });
    }
    if let Some(baseline) = scope.baseline {
        baseline.check_prepared(&prepared)?;
    }
    if options.init_splits && scope.initialize_splits {
        for suite in &prepared {
            if suite.suite.ensure_splits()? != suite.splits {
                return Err(Error::new(
                    ErrorKind::Input,
                    "split IDs changed during preparation",
                ));
            }
        }
    }
    let (accuracy, uncached, discovery, cache_warning) = if options.dry_run {
        (None, None, None, None)
    } else {
        let backend = with_deadline(deadline, Backend::new(&profile))
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery = with_deadline(deadline, backend.discover(&profile.limits)).await?;
        profile.limits = discovery.limits.clone();
        if let Some(baseline) = scope.baseline {
            baseline.check_identity(
                &identity(&profile, encoder.as_deref(), Some(&discovery), false)?,
                &serde_json::to_value(&profile.limits).map_err(json_error)?,
            )?;
        }
        if discovery.identity.is_none() && !options.allow_unverified {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "eval requires fresh full model identity; --allow-unverified emits provisional diagnostics and keeps cache disabled",
            ));
        }
        for suite in &prepared {
            for case in &suite.cases {
                if !scope.recalibrate {
                    Preset {
                        name: "effective".to_owned(),
                        document: case.document.clone(),
                        origin: Origin::Builtin,
                        trusted: true,
                        content_hash: String::new(),
                    }
                    .check_policy_identity(discovery.full_model_identity.as_deref())?;
                    if let Some(pin) =
                        loupe_core::calibration_pin::validate(&case.preset["document"])?
                    {
                        let question = case
                            .questions
                            .get(&pin.field)
                            .ok_or_else(|| usage("calibrated field is absent"))?;
                        pin.check_policy(
                            &case.document,
                            case.policies
                                .get(&pin.field)
                                .ok_or_else(|| usage("calibrated policy is absent"))?
                                .policy()?,
                            question.kind(),
                        )?;
                        let view = loupe_core::calibration_pin::view(
                            &packing(&case.settings),
                            &serde_json::to_value(&profile.limits).map_err(json_error)?,
                        )?;
                        pin.check(
                            discovery.full_model_identity.as_deref(),
                            encoder
                                .as_ref()
                                .map_or("estimate-v1", |encoder| encoder.identity.as_str()),
                            discovery
                                .identity
                                .as_ref()
                                .map(|identity| identity.encoder_identity.as_str()),
                            &view,
                        )?;
                    }
                }
                profile.limits.validate_schema(&Request {
                    state: Value::Null,
                    model: profile.model.clone(),
                    questions: case.questions.clone(),
                })?;
                if case.selection.is_some() {
                    selection::preflight(case, encoder.as_deref(), &profile.limits)?;
                }
            }
        }
        let mode = if options.no_cache {
            CacheMode::Disabled
        } else if options.refresh {
            CacheMode::Refresh
        } else {
            CacheMode::Normal
        };
        let (accuracy, warning) = with_deadline(
            deadline,
            backend
                .clone()
                .with_cache(&discovery, encoder.clone(), &profile.cache_dir, mode),
        )
        .await?;
        let (uncached, _) = with_deadline(
            deadline,
            backend.with_cache(
                &discovery,
                encoder.clone(),
                &profile.cache_dir,
                CacheMode::Disabled,
            ),
        )
        .await?;
        if let Some(warning) = &warning
            && !cli.quiet
        {
            eprintln!("loupe: {warning}");
        }
        (Some(accuracy), Some(uncached), Some(discovery), warning)
    };
    let identity = identity(
        &profile,
        encoder.as_deref(),
        discovery.as_ref(),
        options.dry_run,
    )?;
    let mut provisional_reference = false;
    for suite in &prepared {
        for case in &suite.cases {
            if let Some(reference) = &case.case.reference {
                match loupe_core::identity::Model::parse(&reference.response.model) {
                    Ok(reference_model) => {
                        if identity
                            .model
                            .as_ref()
                            .is_some_and(|model| model != &reference_model)
                            && options.compare.is_empty()
                        {
                            return Err(Error::new(
                                ErrorKind::Unavailable,
                                "reference model identity differs; cross-model/quantization comparison requires explicit --compare",
                            ));
                        }
                    }
                    Err(_) => {
                        if !options.dry_run && !options.allow_unverified {
                            return Err(Error::new(
                                ErrorKind::Unavailable,
                                "parity reference lacks a complete model identity; --allow-unverified permits provisional diagnostics only",
                            ));
                        }
                        provisional_reference = true;
                    }
                }
            }
        }
    }
    let mut report = RunReport {
        schema_version: 1,
        generated_at: loupe_core::report_store::Stamp::now().generated_at,
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        status: if options.dry_run {
            "plan"
        } else if identity.verified && !provisional_reference {
            "complete"
        } else {
            "provisional"
        }
        .to_owned(),
        identity,
        endpoint: format!("{}{}", profile.base_url, profile.path),
        environment: json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"client_concurrency":profile.concurrency,"latency_scope":"packing-group pipeline including encoding, planning, admission, retries and focus/split phases","latency_sampling":"one exact duration per completed packing group; original-unit throughput","warmup_passes":options.warmup,"phase_order":"accuracy, uncached warmup passes, uncached measured latency","gpu_work":"unavailable; global server counters are not attributed to this run"}),
        server: discovery
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(json_error)?
            .unwrap_or_else(
                || json!({"discovery":"not attempted in offline plan","limits":profile.limits}),
            ),
        token_usage: Usage::default(),
        pricing: profile.pricing.clone(),
        cost_usd_estimate: None,
        cost_unknown_output: false,
        suites: Vec::new(),
        error: None,
        acceptance: None,
    };
    let mut failure = None;
    let context = PhaseContext {
        encoder,
        profile: &profile,
        budget,
        deadline,
        options,
        cli,
        response_model: std::sync::Mutex::new(None),
    };
    let selection_context =
        runtime::Context::new(options, &profile, context.encoder.as_deref(), &scope);
    if prepared
        .iter()
        .any(|suite| suite.suite.manifest.task == Task::Retrieval)
    {
        report.environment["selection_latency_scope"] = json!(
            "complete document tournament/independent pipeline, including planning, admission, retries and normalization"
        );
        report.environment["selection_client_concurrency"] = json!(1);
    }
    for suite in prepared {
        if suite.suite.manifest.task == Task::Retrieval {
            let (item, error) = selection::run_suite(
                suite,
                accuracy.as_ref(),
                uncached.as_ref(),
                cache_warning.as_deref(),
                &selection_context,
            )
            .await?;
            report.suites.push(item);
            if let Some(error) = error {
                failure = Some(error);
                break;
            }
            continue;
        }
        let groups = groups(
            &suite.cases,
            &profile,
            context.encoder.as_deref(),
            discovery.as_ref(),
        )?;
        let mut item = SuiteReport {
            manifest: suite.suite.manifest.clone(),
            splits: suite.splits,
            partition: format!("{partition:?}").to_lowercase(),
            selected_case_ids: suite.selected,
            records: Vec::new(),
            questions: Vec::new(),
            parity: None,
            phases: Vec::new(),
            warnings: cache_warning.iter().cloned().collect(),
            join: None,
            selection: None,
        };
        if item.manifest.task == Task::Parity && provisional_reference {
            item.warnings.push("saved reference has an alias only; parity is provisional and cannot establish a precision/quantization baseline".to_owned());
        }
        if !item.manifest.gates.is_empty() && scope.baseline.is_none() {
            item.warnings.push("suite gates were not assessed; --baseline FILE supplies human-ratified floors for held-out acceptance".to_owned());
        }
        let (records, phase, error) =
            run_phase("accuracy", 0, &groups, accuracy.clone(), &context).await?;
        item.records = records;
        item.phases.push(phase);
        failure = error;
        if failure.is_none() && !options.dry_run {
            for pass in 0..options.warmup {
                let (_, phase, error) =
                    run_phase("warmup", pass + 1, &groups, uncached.clone(), &context).await?;
                item.phases.push(phase);
                failure = error;
                if failure.is_some() {
                    break;
                }
            }
            if failure.is_none() {
                let (_, phase, error) =
                    run_phase("latency", 1, &groups, uncached.clone(), &context).await?;
                item.phases.push(phase);
                failure = error;
            }
        }
        if !options.dry_run {
            item.questions = question_reports(&item.records)?;
            item.parity = parity_report(&item.records, item.manifest.task)?;
        }
        report.suites.push(item);
        if failure.is_some() {
            break;
        }
    }
    report.token_usage = usage_delta(budget.usage()?, usage_before)?;
    if let Some(pricing) = &profile.pricing {
        let estimate = pricing.estimate(report.token_usage)?;
        report.cost_usd_estimate = Some(estimate.usd);
        report.cost_unknown_output = estimate.unknown_output;
    }
    if let Some(error) = failure {
        report.status = "incomplete".to_owned();
        report.error = Some(error);
    }
    Ok(report)
}

fn identity(
    profile: &Profile,
    encoder: Option<&ClefEncoder>,
    discovery: Option<&Discovery>,
    offline: bool,
) -> Result<Identity> {
    let local = encoder
        .map(|encoder| encoder.identity.clone())
        .unwrap_or_else(|| "estimate-v1".to_owned());
    let model = discovery
        .and_then(|value| value.identity.as_ref())
        .map(|value| value.model.clone());
    let server = discovery
        .and_then(|value| value.identity.as_ref())
        .map(|value| value.encoder_identity.clone());
    if encoder.is_some() && server.as_ref().is_some_and(|server| server != &local) {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "discovered encoder identity differs from local encoder",
        ));
    }
    let verified = !offline && model.is_some();
    Ok(Identity {
        verified,
        model,
        requested_model: profile.model.clone(),
        local_encoder: local,
        server_encoder: server,
        reason: if verified {
            None
        } else {
            Some(
                if offline {
                    "offline plan; discovery was not attempted"
                } else {
                    "fresh authoritative model identity is unavailable"
                }
                .to_owned(),
            )
        },
    })
}
fn load_suites(options: &Eval, store: &Store, profile: &Profile) -> Result<Vec<Suite>> {
    if let Some(dataset) = options.dataset.as_ref().or(options.labels.as_ref()) {
        let preset = options
            .preset
            .as_deref()
            .ok_or_else(|| usage("labeled JSONL requires --preset"))?;
        let revision = options
            .dataset_revision
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| usage("labeled JSONL requires a nonempty --dataset-revision"))?;
        let loaded = store.lookup(preset)?;
        loaded.require_trusted()?;
        loaded.validate(&profile.limits, &profile.model)?;
        let path = dataset
            .canonicalize()
            .map_err(|error| Error::io("resolving labeled dataset", error))?;
        let root = path
            .parent()
            .ok_or_else(|| usage("dataset has no parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| usage("dataset has no filename"))?;
        let pin = Pin::capture(root, Path::new(name), revision)?;
        let task = if loaded.document.get("questions").is_some() {
            Task::Mixed
        } else {
            match presets::question(&loaded.document)?.kind() {
                QuestionKind::Noul => Task::Noul,
                QuestionKind::Choice => Task::Choice,
                QuestionKind::Score => Task::Score,
            }
        };
        let split_file = options
            .split_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}.splits.json", name.to_string_lossy())));
        let split_file = if split_file.is_absolute() {
            split_file
                .strip_prefix(root)
                .map_err(|_| usage("ad hoc split file must be beneath the dataset directory"))?
                .to_owned()
        } else {
            split_file
        };
        return Ok(vec![Suite::open(
            Manifest {
                schema_version: 1,
                id: format!("adhoc/{preset}/{}", name.to_string_lossy()),
                revision: revision.clone(),
                preset: preset.to_owned(),
                task,
                cases: pin,
                splits: split_file,
                split: SplitConfig {
                    seed: 42,
                    calibrate_percent: 50,
                },
                provenance: Provenance {
                    source: "explicit labeled JSONL; per-case source/review status retained"
                        .to_owned(),
                    license: "user-supplied; publication permission is not inferred".to_owned(),
                },
                metrics: Vec::new(),
                gates: Vec::new(),
                fixtures: Vec::new(),
                join: None,
                selection: None,
            },
            root,
        )?]);
    }
    if options.suite.is_empty() {
        return Err(usage(
            "provide --suite NAME/PATH or labeled JSONL with --preset",
        ));
    }
    let mut paths = Vec::new();
    for name in &options.suite {
        if name == "all" {
            discover_suites(&options.suites_dir, &mut paths)?;
        } else {
            let explicit = PathBuf::from(name);
            let path = if explicit.exists() {
                explicit
            } else {
                options.suites_dir.join(name)
            };
            paths.push(if path.is_dir() {
                path.join("suite.toml")
            } else {
                path
            });
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(Error::new(
            ErrorKind::Input,
            "no suite manifests were found",
        ));
    }
    let suites = paths
        .iter()
        .map(|path| Suite::load(path))
        .collect::<Result<Vec<_>>>()?;
    let mut ids = BTreeSet::new();
    if suites.iter().any(|suite| !ids.insert(&suite.manifest.id)) {
        return Err(Error::new(
            ErrorKind::Input,
            "suite IDs must be unique in one eval run",
        ));
    }
    Ok(suites)
}
fn discover_suites(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root).map_err(|error| Error::io("listing suites", error))? {
        let entry = entry.map_err(|error| Error::io("reading suite entry", error))?;
        let kind = entry
            .file_type()
            .map_err(|error| Error::io("checking suite entry", error))?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            discover_suites(&entry.path(), paths)?;
        } else if entry.file_name() == "suite.toml" {
            paths.push(entry.path());
        }
    }
    Ok(())
}
fn usage(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn prepare_case(
    suite: &Suite,
    position: usize,
    options: &Eval,
    store: &Store,
    profile: &Profile,
    encoder: Option<&ClefEncoder>,
    recalibrate: bool,
) -> Result<PreparedCase> {
    if suite.manifest.task == Task::Retrieval {
        return selection::prepare(
            suite,
            position,
            options,
            store,
            profile,
            encoder,
            recalibrate,
        );
    }
    let case = suite
        .cases
        .get(position)
        .ok_or_else(|| usage("case position missing"))?;
    let state = suite.state(case)?;
    let mut args = options
        .arg
        .iter()
        .map(|value| {
            value
                .split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .ok_or_else(|| usage("--arg requires NAME=VALUE"))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    for (name, value) in [
        ("query", case.query.as_ref()),
        ("candidates", case.candidates.as_ref()),
    ] {
        if let Some(value) = value {
            if args.get(name).is_some_and(|declared| declared != value) {
                return Err(usage(
                    "CLI arguments cannot replace a labeled case's query/candidates",
                ));
            }
            args.insert(name.to_owned(), value.clone());
        }
    }
    let preset = if suite.manifest.task == Task::Parity {
        let reference = case
            .reference
            .as_ref()
            .ok_or_else(|| usage("parity reference missing"))?;
        if encoder.is_none_or(|encoder| encoder.identity != reference.encoder_identity) {
            return Err(Error::new(
                ErrorKind::Input,
                "parity reference encoder identity does not match local encoder",
            ));
        }
        if reference.request.state != state
            || options.preset.is_some()
            || options.preset_override.is_some()
            || reference.request.model != profile.model && options.compare.is_empty()
            || options.pack.is_some_and(|pack| pack != PackMode::Unit)
            || options.split_text
            || options.focus.is_some()
            || options.truncate.is_some()
            || options.truncate_field.is_some()
            || options.ensemble
            || case.context.is_some()
            || !options.arg.is_empty()
            || case.query.is_some()
            || case.candidates.is_some()
        {
            return Err(usage(
                "server parity requires the reference's exact state/questions/model alias and unit packing",
            ));
        }
        let document = json!({"version":1,"questions":reference.request.questions});
        Preset {
            name: "reference".to_owned(),
            content_hash: blake3::hash(&serde_json::to_vec(&document).map_err(json_error)?)
                .to_hex()
                .to_string(),
            document,
            origin: Origin::Builtin,
            trusted: true,
        }
    } else {
        store.lookup(
            options
                .preset_override
                .as_deref()
                .or(options.preset.as_deref())
                .unwrap_or(&suite.manifest.preset),
        )?
    };
    preset.require_trusted()?;
    preset.validate(&profile.limits, &profile.model)?;
    let document = if suite.manifest.task == Task::Parity {
        preset.document.clone()
    } else {
        templates::expand(&preset, store, &args)?
    };
    let effective_options = if recalibrate {
        options.clone()
    } else {
        crate::run::calibrated::evaluation(options, &document)?
    };
    let options = &effective_options;
    let mut questions = if let Some(reference) = &case.reference {
        reference.request.questions.clone()
    } else if let Some(questions) = document.get("questions") {
        serde_json::from_value::<IndexMap<String, Question>>(questions.clone())
            .map_err(json_error)?
    } else {
        IndexMap::from([(preset.name.clone(), presets::question(&document)?)])
    };
    let signals = loupe_core::signals::Set::from_document(&document)?;
    let (ensemble, ensembles) = ensemble::prepare(options, &document, &mut questions)?;
    if !loupe_core::factors::Set::from_document(&document)?.is_empty() {
        return Err(Error::new(
            ErrorKind::Usage,
            "weighted composite presets need factor-specific labeled measurements; ordinary classification eval does not score them",
        ));
    }
    if case.reference.is_none() {
        signals.append(&mut questions)?;
    }
    if let Some(context) = document.get("context") {
        presets::add_context(&mut questions, context.clone());
    }
    if let Some(context) = &case.context {
        presets::add_context(&mut questions, context.clone());
    }
    let expected = labels(case, &ensembles.primary(&questions), &signals)?;
    let mut policies = BTreeMap::new();
    for (id, question) in &questions {
        let mut policy = if let Some(signal) = signals.get(id) {
            signal.policy()
        } else {
            Policy::from_field(
                &document,
                ensembles.base_for(id).unwrap_or(id),
                question.kind(),
            )?
        };
        if signals.get(id).is_none() && question.kind() == QuestionKind::Noul {
            if let Some(value) = options.threshold {
                policy.threshold = Probability::new(value)?;
            }
            if let Some(value) = options.margin {
                policy.margin = value;
            }
        } else if signals.get(id).is_none() {
            if let Some(value) = options.min_confidence {
                policy.min_confidence = Probability::new(value)?;
            }
            if let Some(value) = options.min_gap {
                policy.min_gap = value;
            }
        }
        policy.validate()?;
        policies.insert(id.clone(), policy.into());
        if let Some(label) = expected.get(id) {
            task_metrics::validate_label(question, label)?;
        }
    }
    for (flag, provided) in [
        (PolicyFlag::Threshold, options.threshold.is_some()),
        (PolicyFlag::Margin, options.margin.is_some()),
        (PolicyFlag::MinConfidence, options.min_confidence.is_some()),
        (PolicyFlag::MinGap, options.min_gap.is_some()),
    ] {
        if provided
            && !questions.iter().any(|(id, question)| {
                signals.get(id).is_none() && flag.validate(question.kind()).is_ok()
            })
        {
            return Err(usage("decision-policy flag is inapplicable to this suite"));
        }
    }
    let declared_pack = document
        .get("policy")
        .and_then(|policy| policy.get("pack"))
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| usage("preset pack must be a string"))?
                .parse()
        })
        .transpose()?;
    let pack = options.pack.or(declared_pack).unwrap_or_default();
    let pack = if pack == PackMode::Auto {
        declared_pack
            .filter(|pack| *pack != PackMode::Auto)
            .unwrap_or(PackMode::Unit)
    } else {
        pack
    };
    let structured = !state.is_string();
    let splitting = options.split_text
        || options.focus.is_none()
            && options.truncate.is_none()
            && options.truncate_field.is_none()
            && document
                .get("policy")
                .and_then(|policy| policy.get("splittable"))
                == Some(&Value::Bool(true));
    if splitting && structured
        || options.focus.is_some() && structured
        || options.truncate.is_some() && structured
        || options.truncate_field.is_some() && !structured
    {
        return Err(usage(
            "structured cases never split/focus; --truncate-field requires structured state and --truncate requires text",
        ));
    }
    if !splitting
        && options.focus.is_none()
        && (options.combine.is_some() || options.overlap.is_some() || options.max_chunks.is_some())
    {
        return Err(usage(
            "combine/overlap/max-chunks require splitting or focus",
        ));
    }
    if (splitting
        || options.focus.is_some()
        || options.truncate.is_some()
        || options.truncate_field.is_some())
        && encoder.is_none()
    {
        return Err(usage("explicit views require a verified clef-v1 tokenizer"));
    }
    let modes = if splitting {
        let mut modes = loupe_core::combine::resolve(
            &ensembles.primary(&questions),
            document
                .get("policy")
                .and_then(|policy| policy.get("combine")),
            options.combine,
        )?;
        ensembles.extend_combine(&mut modes)?;
        modes
    } else {
        IndexMap::new()
    };
    let path = options
        .truncate_field
        .as_deref()
        .map(loupe_core::records::field_path)
        .transpose()?
        .map(|parts| {
            parts
                .iter()
                .map(|part| format!("/{}", part.replace('~', "~0").replace('/', "~1")))
                .collect::<String>()
        })
        .unwrap_or_default();
    let settings = ViewSettings {
        pack,
        source: options.question_id_source.unwrap_or(SourceKind::Records),
        split: splitting,
        modes,
        overlap: options.overlap.unwrap_or(256),
        max_chunks: options.max_chunks.map(|value| value.get()).unwrap_or(64),
        focus: options.focus,
        truncate: options.truncate.or(options
            .truncate_field
            .as_ref()
            .map(|_| truncate::Mode::Head)),
        path,
    };
    let raw = if let Some(value) = state.as_str() {
        value.as_bytes().to_vec()
    } else {
        serde_json::to_vec(&state).map_err(json_error)?
    };
    let unit = Unit {
        index: *suite
            .positions
            .get(position)
            .ok_or_else(|| usage("case source index missing"))?,
        text: String::from_utf8(raw.clone()).map_err(|error| usage(error.to_string()))?,
        raw,
        subject: state.clone(),
        state,
        header: None,
        location: suite
            .locations
            .get(position)
            .cloned()
            .ok_or_else(|| usage("case source location missing"))?,
    };
    let signature=blake3::hash(&serde_json::to_vec(&json!({"questions":questions.iter().collect::<Vec<_>>(),"policies":policies,"document":document,"pack":format!("{:?}",settings.pack),"split":settings.split,"modes":settings.modes,"overlap":settings.overlap,"max_chunks":settings.max_chunks,"focus":settings.focus,"truncate":settings.truncate,"path":settings.path})).map_err(json_error)?).to_hex().to_string();
    Ok(PreparedCase {
        selection: None,
        ensemble,
        case: case.clone(),
        unit,
        preset: serde_json::to_value(&preset).map_err(json_error)?,
        document,
        questions,
        policies,
        settings,
        signature,
    })
}
fn labels(
    case: &Case,
    questions: &IndexMap<String, Question>,
    signals: &loupe_core::signals::Set,
) -> Result<BTreeMap<String, Label>> {
    let primary = signals.primary(questions);
    if let Label::Fields(labels) = &case.label {
        if primary.keys().any(|id| !labels.contains_key(id))
            || labels.keys().any(|id| !questions.contains_key(id))
        {
            return Err(Error::new(
                ErrorKind::Input,
                "case labels must cover every primary question and no unknown fields; declared signal labels are optional",
            ));
        }
        Ok(labels.clone())
    } else if primary.len() == 1 {
        let id = primary
            .keys()
            .next()
            .ok_or_else(|| usage("question missing"))?;
        Ok(BTreeMap::from([(id.clone(), case.label.clone())]))
    } else {
        Err(Error::new(
            ErrorKind::Input,
            "multi-question cases require a label map",
        ))
    }
}
fn validate_metrics(manifest: &Manifest, cases: &[Arc<PreparedCase>]) -> Result<()> {
    if manifest.task == Task::Retrieval {
        return selection::validate_metrics(manifest);
    }
    for metric in manifest
        .metrics
        .iter()
        .chain(manifest.gates.iter().map(|gate| &gate.metric))
    {
        if matches!(
            metric,
            Metric::LatencyP50Ms | Metric::LatencyP95Ms | Metric::UnitsPerSecond
        ) {
            continue;
        }
        if matches!(
            metric,
            Metric::ProbabilityMaxError | Metric::ProbabilityMeanError | Metric::ArgmaxAgreement
        ) && manifest.task == Task::Parity
        {
            continue;
        }
        let mut supported = false;
        for case in cases {
            let signals = loupe_core::signals::Set::from_document(&case.document)?;
            let ensembles = case
                .ensemble
                .as_ref()
                .map(|settings| settings.set(&case.document, &case.questions))
                .transpose()?
                .unwrap_or_default();
            for (id, question) in &case.questions {
                if ensembles.contains(id)
                    || signals.get(id).is_some()
                        && !matches!(&case.case.label, Label::Fields(labels) if labels.contains_key(id))
                {
                    continue;
                }
                let empty = task_metrics::report(
                    question,
                    &case
                        .policies
                        .get(id)
                        .ok_or_else(|| usage("policy missing"))?
                        .policy()?,
                    &[],
                    &[],
                )?;
                supported |= empty.metric(*metric).is_ok();
            }
        }
        if !supported {
            return Err(usage(format!(
                "metric {metric:?} is inapplicable to suite {}",
                manifest.id
            )));
        }
    }
    Ok(())
}
fn groups(
    cases: &[Arc<PreparedCase>],
    profile: &Profile,
    encoder: Option<&ClefEncoder>,
    discovery: Option<&Discovery>,
) -> Result<Vec<Arc<Group>>> {
    let mut groups = Vec::new();
    let mut members = Vec::new();
    for case in cases {
        let cap = match case.settings.pack {
            PackMode::Window(size) => Window {
                encoder,
                limits: &profile.limits,
                questions: &case.questions,
                model: &profile.model,
                contextual: false,
                source: case.settings.source,
            }
            .max_units(size)?,
            PackMode::Unit
                if !case.settings.split
                    && case.settings.focus.is_none()
                    && case.settings.truncate.is_none() =>
            {
                discovery
                    .filter(|value| value.batch.supported)
                    .map_or(1, |value| value.batch.max_records.min(profile.concurrency))
            }
            _ => 1,
        };
        if members
            .first()
            .is_some_and(|first: &Arc<PreparedCase>| first.signature != case.signature)
            || members.len() >= cap
        {
            groups.push(Arc::new(Group {
                cases: std::mem::take(&mut members),
            }));
        }
        members.push(case.clone());
    }
    if !members.is_empty() {
        groups.push(Arc::new(Group { cases: members }));
    }
    Ok(groups)
}
fn execution(
    group: &Group,
    backend: Option<Backend>,
    encoder: Option<Arc<ClefEncoder>>,
    profile: &Profile,
    budget: &Budget,
    deadline: Option<Instant>,
    strict: bool,
) -> Result<Arc<Execution>> {
    let first = group
        .cases
        .first()
        .ok_or_else(|| usage("empty eval group"))?;
    let view = &first.settings;
    Ok(Arc::new(Execution {
        backend,
        encoder,
        questions: first.questions.clone(),
        model: profile.model.clone(),
        limits: profile.limits.clone(),
        budget: budget.clone(),
        strict,
        deadline,
        use_batch: group.cases.len() > 1 && view.pack == PackMode::Unit,
        concurrency: profile.concurrency,
        pack: view.pack,
        contextual: false,
        source: view.source,
        split: view.split.then(|| SplitSettings {
            options: split::Options {
                overlap: view.overlap,
                max_chunks: view.max_chunks,
            },
            modes: view.modes.clone(),
            path: view.path.clone(),
        }),
        truncate: view.truncate.map(|mode| TruncateSettings {
            mode,
            path: view.path.clone(),
        }),
        focus: view.focus.map(|k| FocusSettings {
            k,
            options: split::Options {
                overlap: view.overlap,
                max_chunks: view.max_chunks,
            },
            path: view.path.clone(),
        }),
        keep_going: false,
        recovery: None,
    }))
}
async fn run_group(group: Arc<Group>, execution: Arc<Execution>) -> GroupResult {
    let started = Instant::now();
    let offline = execution.backend.is_none();
    let records = group
        .cases
        .iter()
        .map(|case| Ok(case.unit.clone()))
        .collect();
    let jobs = if execution.focus.is_some() {
        crate::run::focus_jobs::plan_focus_jobs(records, &execution).await
    } else {
        plan_jobs(records, &execution)
    };
    let results: Vec<_> = futures::stream::iter(jobs)
        .map(|job| evaluate_job(job, execution.clone()))
        .buffered(execution.concurrency)
        .flat_map(futures::stream::iter)
        .collect()
        .await;
    let results = if offline {
        let mut expanded = Vec::new();
        for result in results {
            match result {
                Ok(completed) if completed.packed_indices.is_some() => {
                    for index in completed.packed_indices.as_deref().unwrap_or_default() {
                        if let Some(case) =
                            group.cases.iter().find(|case| case.unit.index == *index)
                        {
                            expanded.push(Ok(Completed {
                                unit: case.unit.clone(),
                                request: completed.request.clone(),
                                counts: completed.counts.clone(),
                                evaluation: None,
                                answer_id: completed.answer_id.clone(),
                                packed_indices: completed.packed_indices.clone(),
                                split_plan: completed.split_plan.clone(),
                                view: completed.view.clone(),
                                focus_plan: completed.focus_plan.clone(),
                                fallback: None,
                                ensemble: None,
                                review: None,
                            }));
                        } else {
                            expanded.push(Err(crate::run::failure::WorkError::at(
                                usage("planned source case missing"),
                                &completed.unit,
                            )));
                            break;
                        }
                    }
                }
                result => expanded.push(result),
            }
        }
        expanded
    } else {
        results
    };
    GroupResult {
        results,
        elapsed: started.elapsed(),
    }
}
struct PhaseContext<'a> {
    encoder: Option<Arc<ClefEncoder>>,
    profile: &'a Profile,
    budget: &'a Budget,
    deadline: Option<Instant>,
    options: &'a Eval,
    cli: &'a Cli,
    response_model: std::sync::Mutex<Option<String>>,
}
async fn run_phase(
    name: &str,
    pass: usize,
    groups: &[Arc<Group>],
    backend: Option<Backend>,
    context: &PhaseContext<'_>,
) -> Result<(Vec<Record>, Phase, Option<Failure>)> {
    let PhaseContext {
        encoder,
        profile,
        budget,
        deadline,
        options,
        cli,
        ..
    } = context;
    let deadline = *deadline;
    let before = budget.usage()?;
    let logical_before = budget.metrics.snapshot()?.logical_records;
    let started = Instant::now();
    let contexts = groups
        .iter()
        .map(|group| {
            execution(
                group,
                backend.clone(),
                encoder.clone(),
                profile,
                budget,
                deadline,
                options.strict,
            )
            .map(|execution| (group.clone(), execution))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut pending = futures::stream::iter(contexts)
        .map(|(group, execution)| run_group(group, execution))
        .buffered(profile.concurrency);
    let interrupt = interrupt_signal();
    tokio::pin!(interrupt);
    let mut records = Vec::new();
    let mut latencies = Vec::new();
    let mut details = Vec::new();
    let mut failure = None;
    let mut complete_units = 0usize;
    let lookup = groups
        .iter()
        .flat_map(|group| group.cases.iter())
        .map(|case| (case.unit.index, case))
        .collect::<BTreeMap<_, _>>();
    loop {
        let result = tokio::select! {
            result=pending.next()=>result,
            signal=&mut interrupt=>{failure=Some(match signal {Ok(code)=>Failure{code,message:"evaluation interrupted".to_owned()},Err(error)=>Failure{code:error.exit_code(),message:error.to_string()}});break;},
            _=async{if let Some(deadline)=deadline{tokio::time::sleep_until(deadline).await;}else{std::future::pending::<()>().await;}}=>{failure=Some(Failure{code:69,message:"overall deadline expired".to_owned()});break;}
        };
        let Some(result) = result else {
            break;
        };
        let mut case_ids = Vec::new();
        for result in result.results {
            let completed = match result {
                Ok(completed) => completed,
                Err(error) => {
                    failure = Some(Failure {
                        code: error.error.exit_code(),
                        message: error.error.to_string(),
                    });
                    break;
                }
            };
            let case = lookup
                .get(&completed.unit.index)
                .ok_or_else(|| usage("result belongs to an unselected case"))?;
            if let Some(evaluation) = &completed.evaluation {
                let reported = loupe_core::identity::Model::parse(&evaluation.response.model)
                    .map_or_else(
                        |_| evaluation.response.model.clone(),
                        |model| model.as_str().to_owned(),
                    );
                let mut first = context.response_model.lock().map_err(|_| {
                    Error::new(
                        ErrorKind::Unavailable,
                        "response model tracking lock poisoned",
                    )
                })?;
                if first.as_ref().is_some_and(|model| model != &reported) {
                    failure = Some(Failure {
                        code: 69,
                        message: "response model identity changed during evaluation".to_owned(),
                    });
                    break;
                }
                if first.is_none() {
                    *first = Some(reported);
                }
            }
            if let Some(warning) = completed
                .evaluation
                .as_ref()
                .and_then(|evaluation| evaluation.warning.as_ref())
                && !cli.quiet
            {
                eprintln!("loupe: {}: {warning}", case.case.id);
            }
            if name != "accuracy"
                && completed
                    .evaluation
                    .as_ref()
                    .is_some_and(|evaluation| evaluation.cached)
            {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "latency/warmup must never use a cached result",
                ));
            }
            case_ids.push(case.case.id.clone());
            complete_units = complete_units
                .checked_add(1)
                .ok_or_else(|| usage("eval unit count overflow"))?;
            if name == "accuracy" {
                records.push(record(case, &completed)?);
            }
        }
        details.push(json!({"case_ids":case_ids,"seconds":(!options.dry_run).then_some(result.elapsed.as_secs_f64()),"completed":failure.is_none()}));
        if failure.is_some() {
            break;
        }
        latencies.push(result.elapsed);
    }
    drop(pending);
    let after = budget.usage()?;
    let logical = budget
        .metrics
        .snapshot()?
        .logical_records
        .checked_sub(logical_before)
        .ok_or_else(|| usage("logical phase counters moved backwards"))?;
    let usage = usage_delta(after, before)?;
    let phase = Phase {
        name: name.to_owned(),
        pass,
        cache_requested: name == "accuracy" && !options.no_cache && !options.dry_run,
        cache_hits: usage.cache_hits,
        logical_requests: logical,
        cache_hit_rate: if logical == 0 {
            None
        } else {
            Some(usage.cache_hits as f64 / logical as f64)
        },
        usage,
        timing: if options.dry_run {
            task_metrics::timing(&[], 0, Duration::ZERO)
        } else {
            task_metrics::timing(&latencies, complete_units, started.elapsed())
        },
        groups: details,
        error: failure.clone(),
    };
    Ok((records, phase, failure))
}
fn usage_delta(after: Usage, before: Usage) -> Result<Usage> {
    let sub = |after: u64, before: u64| {
        after
            .checked_sub(before)
            .ok_or_else(|| usage("phase token counters moved backwards"))
    };
    Ok(Usage {
        spent_input: sub(after.spent_input, before.spent_input)?,
        reserved_input: after.reserved_input,
        attempts: sub(after.attempts, before.attempts)?,
        http_attempts: sub(after.http_attempts, before.http_attempts)?,
        unknown_attempts: sub(after.unknown_attempts, before.unknown_attempts)?,
        unknown_input: sub(after.unknown_input, before.unknown_input)?,
        reported_input: sub(after.reported_input, before.reported_input)?,
        reported_output: sub(after.reported_output, before.reported_output)?,
        cache_hits: sub(after.cache_hits, before.cache_hits)?,
    })
}
fn record(case: &PreparedCase, completed: &Completed) -> Result<Record> {
    let mut binding = BTreeMap::new();
    for id in case.questions.keys() {
        binding.insert(
            id.clone(),
            if completed.packed_indices.is_some() {
                let source = case.settings.source.id(completed.unit.index);
                if case.questions.len() == 1 {
                    source
                } else {
                    format!("{source}/{id}")
                }
            } else {
                id.clone()
            },
        );
    }
    let focus = completed.focus_plan.clone().or_else(|| {
        completed
            .evaluation
            .as_ref()
            .and_then(|evaluation| {
                binding
                    .values()
                    .find_map(|id| evaluation.response.answers.get(id))
            })
            .and_then(|answer| answer.get("provenance"))
            .filter(|provenance| {
                provenance.get("operation").and_then(Value::as_str) == Some("focus")
            })
            .cloned()
    });
    let mut record = Record {
        ensemble: case.ensemble.as_ref().map(ensemble::Settings::saved),
        case_id: case.case.id.clone(),
        index: case.unit.index,
        label: case.case.label.clone(),
        source: case.case.source.clone(),
        reviewed: case.case.reviewed,
        sampling: case.case.sampling,
        preset: case.preset.clone(),
        effective_document: case.document.clone(),
        policies: case.policies.clone(),
        questions: case.questions.clone(),
        base_question_order: case.questions.keys().cloned().collect(),
        binding,
        request: completed.request.as_ref().clone(),
        request_role: if completed.split_plan.is_some() {
            RequestRole::OriginalSplitInput
        } else if focus.is_some() && completed.evaluation.is_none() {
            RequestRole::OriginalFocusInput
        } else if focus.is_some() {
            RequestRole::FocusFinal
        } else {
            RequestRole::Evaluated
        },
        request_question_order: completed.request.questions.keys().cloned().collect(),
        counts: completed.counts.as_ref().clone(),
        evaluation: completed.evaluation.as_deref().cloned(),
        packing: {
            let mut value = packing(&case.settings);
            value["source_indices"] = json!(completed.packed_indices);
            value
        },
        provenance: json!({"source":completed.unit.location,"split":completed.split_plan,"truncate":completed.view,"focus":focus}),
        reference: case.case.reference.clone(),
    };
    ensemble::capture(&mut record)?;
    Ok(record)
}
fn parsed(record: &Record) -> Result<IndexMap<String, Answer>> {
    ensemble::answers(record)
}
fn question_reports(records: &[Record]) -> Result<Vec<QuestionReport>> {
    let answers = records.iter().map(parsed).collect::<Result<Vec<_>>>()?;
    let mut buckets = BTreeMap::<(String, String), Vec<(usize, Label)>>::new();
    for (index, record) in records.iter().enumerate() {
        let signals = loupe_core::signals::Set::from_document(&record.effective_document)?;
        let ensembles = ensemble::set(record)?;
        let expected = if let Label::Fields(labels) = &record.label {
            labels.clone()
        } else {
            BTreeMap::from([(
                record
                    .questions
                    .keys()
                    .next()
                    .ok_or_else(|| usage("record question missing"))?
                    .clone(),
                record.label.clone(),
            )])
        };
        for (id, question) in &record.questions {
            let Some(label) = expected.get(id) else {
                if signals.get(id).is_some() || ensembles.contains(id) {
                    continue;
                }
                return Err(usage("record label missing"));
            };
            let mut rubric = json!({"type":question.type_name(),"options":question.options(),"policy":record.policies.get(id)});
            if let Some(saved) = &record.ensemble {
                rubric["ensemble"] = serde_json::to_value(saved).map_err(json_error)?;
                if let Some(saved) = rubric["ensemble"].as_object_mut() {
                    saved.remove("reports");
                }
                rubric["variants"] = record.effective_document["variants"].clone();
                rubric["joint_fields"] = json!(record.questions.iter().map(|(id, question)| json!({"id":id,"type":question.type_name(),"options":question.options()})).collect::<Vec<_>>());
            }
            let signature = blake3::hash(&serde_json::to_vec(&rubric).map_err(json_error)?)
                .to_hex()
                .to_string();
            buckets
                .entry((id.clone(), signature))
                .or_default()
                .push((index, label.clone()));
        }
    }
    let mut output = Vec::new();
    for ((id, signature), members) in buckets {
        let first = records
            .get(
                members
                    .first()
                    .ok_or_else(|| usage("empty metric bucket"))?
                    .0,
            )
            .ok_or_else(|| usage("metric record missing"))?;
        let question = first
            .questions
            .get(&id)
            .ok_or_else(|| usage("metric question missing"))?;
        let policy = first
            .policies
            .get(&id)
            .ok_or_else(|| usage("metric policy missing"))?;
        let observations = members
            .iter()
            .map(|(index, label)| {
                let record = records
                    .get(*index)
                    .ok_or_else(|| usage("metric case missing"))?;
                let answer_id = record
                    .binding
                    .get(&id)
                    .ok_or_else(|| usage("metric binding missing"))?;
                Ok(Observation {
                    id: &record.case_id,
                    label,
                    answer: answers
                        .get(*index)
                        .and_then(|answers| answers.get(answer_id))
                        .ok_or_else(|| usage("metric answer missing"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let uncertainty = ensemble::uncertainty_cases(
            records,
            &id,
            &members.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        )?;
        output.push(QuestionReport {
            field: id,
            rubric_signature: signature,
            question: question.clone(),
            policy: policy.clone(),
            case_ids: observations
                .iter()
                .map(|observation| observation.id.to_owned())
                .collect(),
            metrics: task_metrics::report_with_uncertainty(
                question,
                &policy.policy()?,
                &observations,
                &[0.0, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99, 1.0],
                &uncertainty,
            )?,
        });
    }
    Ok(output)
}
fn parity_report(records: &[Record], task: Task) -> Result<Option<task_metrics::Parity>> {
    if task != Task::Parity {
        return Ok(None);
    }
    let actual = records.iter().map(parsed).collect::<Result<Vec<_>>>()?;
    let reference = records
        .iter()
        .map(|record| {
            let reference = record
                .reference
                .as_ref()
                .ok_or_else(|| usage("reference missing"))?;
            reference.response.validate(&reference.request)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut comparisons = Vec::new();
    for (index, record) in records.iter().enumerate() {
        for (id, question) in &record.questions {
            comparisons.push(Comparison {
                case_id: &record.case_id,
                question_id: id,
                question,
                actual: actual
                    .get(index)
                    .and_then(|answers| answers.get(id))
                    .ok_or_else(|| usage("parity actual answer missing"))?,
                reference: reference
                    .get(index)
                    .and_then(|answers| answers.get(id))
                    .ok_or_else(|| usage("parity reference answer missing"))?,
            });
        }
    }
    Ok(Some(task_metrics::parity(&comparisons)?))
}
