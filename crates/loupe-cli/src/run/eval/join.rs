//! Catalog-level measurements using the same rubric, blocking and assignment as join.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Cursor;
use std::sync::Arc;

use loupe_core::backend::{Backend, Evaluation, with_deadline};
use loupe_core::blocking::{Fields, Index};
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::encoder::{ClefEncoder, Counts};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::matching::{Assignment, Cardinality, Decision, GATE_ID, Template};
use loupe_core::policy::UncertainAction;
use loupe_core::preset_store::Store;
use loupe_core::records::{Format, Options, Source};
use loupe_core::suites::{Case, Input, Label, Metric, Split, Splits, Suite, Task};
use loupe_core::task_metrics::{self, Observation, Undefined, Value as MetricValue};
use loupe_core::wire::{Answer, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::args::{Cli, Eval, EvalSplit};
use crate::run::eval::runtime::{Context, Measurement};
use crate::run::eval::{
    Failure, Phase, RunReport, RunScope, SuiteReport, identity, usage, usage_delta,
};
use crate::run::json_error;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    cases: Vec<Catalog>,
    blocking_recall: MetricValue,
    all_pair_quality: Option<task_metrics::Report>,
    blocked_pair_quality: Option<task_metrics::Report>,
    ranking_auroc: MetricValue,
    assignments: Vec<AssignmentMetrics>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    case_id: String,
    source: String,
    reviewed: bool,
    source_location: loupe_core::input::Location,
    left: Vec<Unit>,
    right: Vec<Unit>,
    effective_document: Value,
    policies: BTreeMap<String, crate::run::eval::PolicyData>,
    question_order: Vec<String>,
    truth: BTreeMap<u64, BTreeSet<u64>>,
    blocked: BTreeMap<u64, Vec<u64>>,
    pairs: Vec<Pair>,
    assignments: Vec<Assigned>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    id: String,
    left: u64,
    right: u64,
    label: Label,
    request: Request,
    counts: Counts,
    evaluation: Option<Evaluation>,
    decision: Option<Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assigned {
    cardinality: Cardinality,
    rows: Vec<Row>,
    metrics: AssignmentMetrics,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    left: u64,
    candidates: Vec<u64>,
    selected: Vec<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentMetrics {
    cardinality: Cardinality,
    correct_rows: usize,
    rows: usize,
    true_edges: usize,
    output_edges: usize,
    feasible_edges: usize,
    assignment_accuracy: MetricValue,
    edge_precision: MetricValue,
    feasible_edge_recall: MetricValue,
}
struct Prepared {
    suite: Suite,
    splits: Splits,
    catalogs: Vec<PreparedCatalog>,
}
struct PreparedCatalog {
    catalog: Catalog,
    index: Index,
    template: Template,
    uncertain: UncertainAction,
}

fn ratio(numerator: usize, denominator: usize, reason: Undefined) -> MetricValue {
    MetricValue {
        value: (denominator != 0).then(|| numerator as f64 / denominator as f64),
        support: denominator,
        undefined: (denominator == 0).then_some(reason),
    }
}
fn input(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}
fn records(values: &Value) -> Result<Vec<Unit>> {
    let array = values
        .as_array()
        .ok_or_else(|| input("join catalog sides must be arrays of records"))?;
    if array.is_empty() || array.iter().any(|value| !value.is_object()) {
        return Err(input("join catalog sides require nonempty object records"));
    }
    let mut bytes = Vec::new();
    for value in array {
        serde_json::to_writer(&mut bytes, value).map_err(json_error)?;
        bytes.push(b'\n');
    }
    let mut reader = Source::new(Cursor::new(bytes), Options::new(Format::Jsonl), None)?;
    let mut units = Vec::new();
    while let Some(unit) = reader.next_unit()? {
        units.push(unit);
    }
    Ok(units)
}
fn right_index(units: &[Unit], fields: Fields) -> Result<Index> {
    let bytes = units
        .iter()
        .flat_map(|unit| unit.raw.iter().copied())
        .collect::<Vec<_>>();
    Index::load(
        Cursor::new(bytes),
        Options::new(Format::Jsonl),
        None,
        fields,
    )
}
fn truth(case: &Case, left: &[Unit], right: &[Unit]) -> Result<BTreeMap<u64, BTreeSet<u64>>> {
    let Label::Fields(labels) = &case.label else {
        return Err(input("join labels require L<n> to R<n> ID lists"));
    };
    let mut result = BTreeMap::new();
    let right_ids = right
        .iter()
        .map(|unit| format!("R{}", unit.index))
        .collect::<BTreeSet<_>>();
    for unit in left {
        let Some(Label::Ids(ids)) = labels.get(&format!("L{}", unit.index)) else {
            return Err(input(
                "join labels must cover every left record with an ID list",
            ));
        };
        let mut indices = BTreeSet::new();
        for id in ids {
            if !right_ids.contains(id) {
                return Err(input("join label names an unknown right record"));
            }
            let index = id
                .strip_prefix('R')
                .and_then(|id| id.parse::<u64>().ok())
                .ok_or_else(|| input("invalid join right ID"))?;
            indices.insert(index);
        }
        result.insert(unit.index, indices);
    }
    if labels.len() != left.len() {
        return Err(input("join labels contain an unknown left ID"));
    }
    Ok(result)
}
fn prepare(
    suite: Suite,
    options: &Eval,
    store: &Store,
    profile: &Profile,
    encoder: Option<&ClefEncoder>,
    partition: Split,
) -> Result<Prepared> {
    let settings = suite
        .manifest
        .join
        .as_ref()
        .ok_or_else(|| usage("join suite needs [join] settings"))?;
    let splits = if options.init_splits {
        suite.planned_splits()?
    } else {
        suite.load_splits()?
    };
    let selected = suite
        .cases_for(&splits, partition)?
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        return Err(input("join suite has no selected scenarios"));
    }
    for metric in suite
        .manifest
        .metrics
        .iter()
        .chain(suite.manifest.gates.iter().map(|gate| &gate.metric))
    {
        if !matches!(
            metric,
            Metric::Accuracy
                | Metric::MacroF1
                | Metric::F1
                | Metric::Auroc
                | Metric::Brier
                | Metric::Ece
                | Metric::Coverage
                | Metric::SelectiveAccuracy
                | Metric::BlockingRecall
                | Metric::AssignmentAccuracy
                | Metric::LatencyP50Ms
                | Metric::LatencyP95Ms
                | Metric::UnitsPerSecond
        ) {
            return Err(usage("metric is inapplicable to join catalog evaluation"));
        }
    }
    let mut catalogs = Vec::new();
    let mut pair_count = 0usize;
    for (position, case) in suite.cases.iter().enumerate() {
        if !selected.contains(case.id.as_str()) {
            continue;
        }
        if case.query.is_some() || case.candidates.is_some() {
            return Err(input(
                "join scenarios use catalog labels/context, not retrieval queries or candidate parsers",
            ));
        }
        let Input::Unit { unit } = &case.input else {
            return Err(input(
                "join scenarios require unit: {left: [...], right: [...]}",
            ));
        };
        let object = unit
            .as_object()
            .ok_or_else(|| input("join scenario unit must be an object"))?;
        if object.len() != 2 || !object.contains_key("left") || !object.contains_key("right") {
            return Err(input(
                "join scenario unit contains only left and right catalogs",
            ));
        }
        let left = records(&unit["left"])?;
        let right = records(&unit["right"])?;
        pair_count = left
            .len()
            .checked_mul(right.len())
            .and_then(|n| pair_count.checked_add(n))
            .ok_or_else(|| input("join pair universe overflows"))?;
        if pair_count > settings.max_pairs.get() {
            return Err(input("selected join pair universe exceeds join.max_pairs"));
        }
        let truth = truth(case, &left, &right)?;
        let preset = options
            .preset_override
            .as_deref()
            .or(options.preset.as_deref())
            .unwrap_or(&suite.manifest.preset);
        let (mut template, mut document) = crate::run::matching::rubric::prepare_eval(
            options,
            preset,
            store,
            profile,
            &settings.fields,
        )?;
        if let Some(context) = &case.context {
            template.add_context(context.clone())?;
        }
        document["compiled_questions"] =
            serde_json::to_value(template.questions().iter().collect::<Vec<_>>())
                .map_err(json_error)?;
        let uncertain = match document
            .get("policy")
            .and_then(|p| p.get("on_uncertain"))
            .and_then(Value::as_str)
        {
            Some("keep") => UncertainAction::Keep,
            Some("fail") => UncertainAction::Fail,
            Some("stderr") => UncertainAction::Stderr,
            Some("drop") | None => UncertainAction::Drop,
            _ => {
                return Err(usage(
                    "join evaluation cannot execute fallback or unknown uncertainty actions",
                ));
            }
        };
        let policies = template
            .policies()
            .iter()
            .map(|(id, policy)| (id.clone(), (*policy).into()))
            .collect();
        let mut index = right_index(&right, Fields::parse(&settings.fields.join(","))?)?;
        let mut blocked = BTreeMap::new();
        let mut pairs = Vec::new();
        for l in &left {
            blocked.insert(
                l.index,
                index
                    .select(l, settings.max_candidates, settings.blocking)?
                    .candidates
                    .iter()
                    .map(|c| c.right_index)
                    .collect(),
            );
            for r in &right {
                let plan = template.plan(l, r, encoder, &profile.limits)?;
                pairs.push(Pair {
                    id: format!("{}:L{}:R{}", case.id, l.index, r.index),
                    left: l.index,
                    right: r.index,
                    label: Label::Noul(
                        truth
                            .get(&l.index)
                            .is_some_and(|set| set.contains(&r.index)),
                    ),
                    request: plan.request().clone(),
                    counts: plan.counts().clone(),
                    evaluation: None,
                    decision: None,
                });
            }
        }
        catalogs.push(PreparedCatalog {
            index,
            uncertain,
            template: template.clone(),
            catalog: Catalog {
                case_id: case.id.clone(),
                source: case.source.clone(),
                reviewed: case.reviewed,
                source_location: suite
                    .locations
                    .get(position)
                    .cloned()
                    .ok_or_else(|| input("case location missing"))?,
                left,
                right,
                effective_document: document,
                policies,
                question_order: template.questions().keys().cloned().collect(),
                truth,
                blocked,
                pairs,
                assignments: Vec::new(),
            },
        });
    }
    Ok(Prepared {
        suite,
        splits,
        catalogs,
    })
}

fn supported(options: &Eval, scope: &RunScope<'_>, suites: &[Suite]) -> Result<()> {
    if suites.iter().any(|suite| suite.manifest.task != Task::Join) {
        return Err(usage(
            "run join catalog suites separately from classification/parity suites",
        ));
    }
    if scope.recalibrate
        || scope.baseline.is_some()
        || !options.compare.is_empty()
        || !options.sweep_pack.is_empty()
        || !options.sweep_combine.is_empty()
        || !options.sweep_focus.is_empty()
        || options.pack.is_some()
        || options.question_id_source.is_some()
        || options.split_text
        || options.combine.is_some()
        || options.overlap.is_some()
        || options.max_chunks.is_some()
        || options.focus.is_some()
        || options.truncate.is_some()
        || options.truncate_field.is_some()
        || options.ensemble
    {
        return Err(usage(
            "join catalog evaluation uses pair-local unit requests; classification views, fitting, baselines, comparisons and ensembles are inapplicable",
        ));
    }
    Ok(())
}

pub(in crate::run::eval) async fn run(
    cli: &Cli,
    options: &Eval,
    store: &Store,
    mut profile: Profile,
    scope: RunScope<'_>,
    encoder: Option<Arc<ClefEncoder>>,
    suites: Vec<Suite>,
) -> Result<RunReport> {
    supported(options, &scope, &suites)?;
    let before = scope.budget.usage()?;
    let partition = match options.partition.unwrap_or(EvalSplit::Test) {
        EvalSplit::Test => Split::Test,
        EvalSplit::Calibrate => Split::Calibrate,
        EvalSplit::Captured => Split::Captured,
    };
    let mut prepared = suites
        .into_iter()
        .map(|suite| {
            prepare(
                suite,
                options,
                store,
                &profile,
                encoder.as_deref(),
                partition,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    // Plans, labels, pins and policies are complete before credentials or split writes.
    if options.init_splits && scope.initialize_splits {
        for item in &prepared {
            if item.suite.ensure_splits()? != item.splits {
                return Err(input("join split assignments changed during preparation"));
            }
        }
    }
    let (accuracy, uncached, discovery, warning) = if options.dry_run {
        (None, None, None, None)
    } else {
        let backend = with_deadline(scope.deadline, Backend::new(&profile))
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery = with_deadline(scope.deadline, backend.discover(&profile.limits)).await?;
        profile.limits = discovery.limits.clone();
        if discovery.identity.is_none() && !options.allow_unverified {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "eval requires fresh full model identity; --allow-unverified emits provisional diagnostics and keeps cache disabled",
            ));
        }
        identity(&profile, encoder.as_deref(), Some(&discovery), false)?;
        // Revalidate all plans against discovered limits before cache/inference admission.
        for item in &mut prepared {
            for catalog in &mut item.catalogs {
                loupe_core::preset_store::Preset {
                    name: "join-effective".into(),
                    document: catalog.catalog.effective_document.clone(),
                    origin: loupe_core::preset_store::Origin::Builtin,
                    trusted: true,
                    content_hash: String::new(),
                }
                .check_policy_identity(discovery.full_model_identity.as_deref())?;
                for pair in &mut catalog.catalog.pairs {
                    let l = catalog
                        .catalog
                        .left
                        .iter()
                        .find(|unit| unit.index == pair.left)
                        .ok_or_else(|| input("left record missing"))?;
                    let r = catalog
                        .catalog
                        .right
                        .iter()
                        .find(|unit| unit.index == pair.right)
                        .ok_or_else(|| input("right record missing"))?;
                    let plan = catalog
                        .template
                        .plan(l, r, encoder.as_deref(), &profile.limits)?;
                    pair.counts = plan.counts().clone();
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
            scope.deadline,
            backend
                .clone()
                .with_cache(&discovery, encoder.clone(), &profile.cache_dir, mode),
        )
        .await?;
        let (uncached, _) = with_deadline(
            scope.deadline,
            backend.with_cache(
                &discovery,
                encoder.clone(),
                &profile.cache_dir,
                CacheMode::Disabled,
            ),
        )
        .await?;
        (Some(accuracy), Some(uncached), Some(discovery), warning)
    };
    if let Some(warning) = &warning
        && !cli.quiet
    {
        eprintln!("loupe: {warning}");
    }
    let identity = identity(
        &profile,
        encoder.as_deref(),
        discovery.as_ref(),
        options.dry_run,
    )?;
    let mut report = RunReport {
        schema_version: 1,
        generated_at: loupe_core::report_store::Stamp::now().generated_at,
        cli_version: env!("CARGO_PKG_VERSION").into(),
        status: if options.dry_run {
            "plan"
        } else if identity.verified {
            "complete"
        } else {
            "provisional"
        }
        .into(),
        identity,
        endpoint: format!("{}{}", profile.base_url, profile.path),
        environment: json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"client_concurrency":1,"phase_order":"all-pair accuracy, uncached complete join warmups, uncached complete join latency","latency_scope":"scenario/cardinality join pipeline; blocking, pair inference, assignment","latency_units":"left records","warmup_passes":options.warmup,"gpu_work":"unavailable"}),
        server: discovery
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(json_error)?
            .unwrap_or_else(
                || json!({"discovery":"not attempted in offline plan","limits":profile.limits}),
            ),
        token_usage: Default::default(),
        pricing: profile.pricing.clone(),
        cost_usd_estimate: None,
        cost_unknown_output: false,
        suites: Vec::new(),
        error: None,
        acceptance: None,
    };
    let context = Context::new(options, &profile, encoder.as_deref(), &scope);
    for mut item in prepared {
        let settings = item
            .suite
            .manifest
            .join
            .as_ref()
            .ok_or_else(|| usage("join settings missing"))?;
        let (phase, error) =
            accuracy_phase(&mut item.catalogs, accuracy.as_ref(), settings, &context).await?;
        let mut phases = vec![phase];
        let mut failure = error;
        if failure.is_none() && !options.dry_run {
            for pass in 1..=options.warmup {
                let (phase, error) = join_phase(
                    "warmup",
                    pass,
                    &mut item.catalogs,
                    uncached.as_ref(),
                    settings,
                    &context,
                )
                .await?;
                phases.push(phase);
                failure = error;
                if failure.is_some() {
                    break;
                }
            }
            if failure.is_none() {
                let (phase, error) = join_phase(
                    "latency",
                    1,
                    &mut item.catalogs,
                    uncached.as_ref(),
                    settings,
                    &context,
                )
                .await?;
                phases.push(phase);
                failure = error;
            }
        }
        let selected_case_ids = item
            .catalogs
            .iter()
            .map(|c| c.catalog.case_id.clone())
            .collect();
        let summary = summarize(item.catalogs, settings, options.dry_run)?;
        let mut warnings = warning.iter().cloned().collect::<Vec<_>>();
        if !item.suite.manifest.gates.is_empty() {
            warnings.push("join gate declarations are unassessed; human-ratified join baseline support is pending".into());
        }
        report.suites.push(SuiteReport {
            manifest: item.suite.manifest,
            splits: item.splits,
            partition: format!("{partition:?}").to_lowercase(),
            selected_case_ids,
            records: Vec::new(),
            questions: Vec::new(),
            parity: None,
            phases,
            warnings,
            join: Some(summary),
            selection: None,
        });
        if let Some(failure) = failure {
            report.error = Some(failure);
            report.status = "incomplete".into();
            break;
        }
    }
    report.token_usage = usage_delta(scope.budget.usage()?, before)?;
    if let Some(pricing) = &profile.pricing {
        let estimate = pricing.estimate(report.token_usage)?;
        report.cost_usd_estimate = Some(estimate.usd);
        report.cost_unknown_output = estimate.unknown_output;
    }
    Ok(report)
}

async fn accuracy_phase(
    catalogs: &mut [PreparedCatalog],
    backend: Option<&Backend>,
    settings: &loupe_core::suites::Join,
    context: &Context<'_, '_>,
) -> Result<(Phase, Option<Failure>)> {
    let before = context.scope.budget.usage()?;
    let started = Instant::now();
    let mut error = None;
    let mut durations = Vec::new();
    let mut completed = 0;
    let mut logical_requests = 0u64;
    for prepared in catalogs.iter_mut() {
        let begin = Instant::now();
        let mut decisions = BTreeMap::new();
        for pair in &mut prepared.catalog.pairs {
            let Some(backend) = backend else {
                continue;
            };
            let result = async {
                let evaluation = context
                    .evaluate(backend, &pair.request, &pair.counts)
                    .await?;
                logical_requests += 1;
                pair.evaluation = Some(evaluation.clone());
                let l = prepared
                    .catalog
                    .left
                    .iter()
                    .find(|unit| unit.index == pair.left)
                    .ok_or_else(|| input("left missing"))?;
                let r = prepared
                    .catalog
                    .right
                    .iter()
                    .find(|unit| unit.index == pair.right)
                    .ok_or_else(|| input("right missing"))?;
                let plan =
                    prepared
                        .template
                        .plan(l, r, context.encoder, &context.profile.limits)?;
                let decision =
                    prepared
                        .template
                        .assess(&plan, &evaluation.response, prepared.uncertain)?;
                pair.decision = Some(serde_json::to_value(&decision).map_err(json_error)?);
                decisions.insert((pair.left, pair.right), decision);
                Ok::<_, Error>(())
            }
            .await;
            if let Err(failure) = result {
                error = Some(context.failure(failure));
                break;
            }
        }
        if error.is_some() {
            break;
        }
        if backend.is_some() {
            for cardinality in &settings.cardinalities {
                let assigned = assign(prepared, &decisions, *cardinality, settings)?;
                prepared.catalog.assignments.push(assigned);
            }
            completed += prepared.catalog.pairs.len();
            durations.push(begin.elapsed());
        }
    }
    // Raw accuracy pairs live in join.cases, avoiding another full response copy.
    let mut result = Measurement {
        name: "accuracy",
        pass: 0,
        cache: !context.options.no_cache,
        groups: Vec::new(),
        latencies: durations,
        units: completed,
        started,
        before,
    }
    .finish(context, error.clone())?;
    result.logical_requests = logical_requests;
    result.cache_hit_rate =
        (logical_requests != 0).then(|| result.cache_hits as f64 / logical_requests as f64);
    Ok((result, error))
}

fn assign(
    prepared: &mut PreparedCatalog,
    decisions: &BTreeMap<(u64, u64), Decision>,
    cardinality: Cardinality,
    settings: &loupe_core::suites::Join,
) -> Result<Assigned> {
    let mut assignment = Assignment::new(
        cardinality,
        prepared.catalog.right.iter().map(|unit| unit.index),
    )?;
    let mut rows = Vec::new();
    for l in &prepared.catalog.left {
        let candidates = prepared
            .index
            .select_available(
                l,
                settings.max_candidates,
                settings.blocking,
                assignment.excluded(),
            )?
            .candidates;
        let candidates = candidates
            .iter()
            .map(|candidate| candidate.right_index)
            .collect::<Vec<_>>();
        let evaluated = candidates
            .iter()
            .map(|r| {
                decisions
                    .get(&(l.index, *r))
                    .cloned()
                    .ok_or_else(|| input("complete pair universe lacks candidate response"))
            })
            .collect::<Result<Vec<_>>>()?;
        let selected = assignment
            .assign(l.index, &evaluated)?
            .iter()
            .map(|index| {
                evaluated
                    .get(*index)
                    .map(Decision::right_index)
                    .ok_or_else(|| input("assignment selection outside candidates"))
            })
            .collect::<Result<Vec<_>>>()?;
        rows.push(Row {
            left: l.index,
            candidates,
            selected,
        });
    }
    Ok(Assigned {
        metrics: assignment_metrics(cardinality, &prepared.catalog.truth, &rows)?,
        cardinality,
        rows,
    })
}

/// Maximum true-edge capacity, independent of scores and candidate blocking.
fn capacity(cardinality: Cardinality, truth: &BTreeMap<u64, BTreeSet<u64>>) -> usize {
    if cardinality == Cardinality::ManyToMany {
        return truth.values().map(BTreeSet::len).sum();
    }
    if cardinality == Cardinality::OneToMany {
        return truth.values().flatten().collect::<BTreeSet<_>>().len();
    }
    // Iterative augmenting paths avoid recursion on long catalogs.
    let mut matched = BTreeMap::<u64, u64>::new();
    for start in truth.keys() {
        let mut queue = VecDeque::from([*start]);
        let mut visited = BTreeSet::from([*start]);
        let mut parents = BTreeMap::<u64, u64>::new();
        let mut free = None;
        while let Some(left) = queue.pop_front() {
            if let Some(rights) = truth.get(&left) {
                for right in rights {
                    if parents.contains_key(right) {
                        continue;
                    }
                    parents.insert(*right, left);
                    if let Some(other) = matched.get(right) {
                        if visited.insert(*other) {
                            queue.push_back(*other);
                        }
                    } else {
                        free = Some(*right);
                        break;
                    }
                }
            }
            if free.is_some() {
                break;
            }
        }
        while let Some(right) = free {
            let Some(left) = parents.get(&right).copied() else {
                break;
            };
            let prior = matched
                .iter()
                .find_map(|(right, owner)| (*owner == left).then_some(*right));
            matched.insert(right, left);
            free = prior;
        }
    }
    matched.len()
}
fn assignment_metrics(
    cardinality: Cardinality,
    truth: &BTreeMap<u64, BTreeSet<u64>>,
    rows: &[Row],
) -> Result<AssignmentMetrics> {
    let mut claimed = BTreeSet::new();
    let mut correct_rows = 0;
    let mut true_edges = 0;
    let mut output_edges = 0;
    for row in rows {
        let gold = truth
            .get(&row.left)
            .ok_or_else(|| input("assignment row lacks truth"))?;
        let available = gold
            .iter()
            .filter(|r| cardinality == Cardinality::ManyToMany || !claimed.contains(*r))
            .count();
        let expected = if cardinality == Cardinality::OneToOne {
            available.min(1)
        } else {
            available
        };
        let selected = row.selected.iter().collect::<BTreeSet<_>>();
        if selected.len() != row.selected.len()
            || cardinality == Cardinality::OneToOne && selected.len() > 1
            || cardinality != Cardinality::ManyToMany
                && row.selected.iter().any(|right| claimed.contains(right))
        {
            return Err(input("assignment violates its cardinality"));
        }
        let correct = row
            .selected
            .iter()
            .filter(|right| gold.contains(*right))
            .count();
        correct_rows += usize::from(correct == row.selected.len() && correct == expected);
        true_edges += correct;
        output_edges += row.selected.len();
        claimed.extend(row.selected.iter().copied());
    }
    let feasible_edges = capacity(cardinality, truth);
    Ok(assignment_summary(
        cardinality,
        correct_rows,
        rows.len(),
        true_edges,
        output_edges,
        feasible_edges,
    ))
}
fn assignment_summary(
    cardinality: Cardinality,
    correct_rows: usize,
    rows: usize,
    true_edges: usize,
    output_edges: usize,
    feasible_edges: usize,
) -> AssignmentMetrics {
    AssignmentMetrics {
        cardinality,
        correct_rows,
        rows,
        true_edges,
        output_edges,
        feasible_edges,
        assignment_accuracy: ratio(correct_rows, rows, Undefined::NoSamples),
        edge_precision: ratio(true_edges, output_edges, Undefined::NoPredictedLabels),
        feasible_edge_recall: ratio(true_edges, feasible_edges, Undefined::NoPositiveLabels),
    }
}

async fn join_phase(
    name: &str,
    pass: usize,
    catalogs: &mut [PreparedCatalog],
    backend: Option<&Backend>,
    settings: &loupe_core::suites::Join,
    context: &Context<'_, '_>,
) -> Result<(Phase, Option<Failure>)> {
    let backend = backend.ok_or_else(|| usage("join latency requires a backend"))?;
    let before = context.scope.budget.usage()?;
    let started = Instant::now();
    let mut durations = Vec::new();
    let mut completed = 0;
    let mut groups = Vec::new();
    let mut failure = None;
    'catalogs: for prepared in catalogs {
        for cardinality in &settings.cardinalities {
            let begin = Instant::now();
            let mut pairs = Vec::new();
            let mut rows = Vec::new();
            let mut assignment = Assignment::new(
                *cardinality,
                prepared.catalog.right.iter().map(|unit| unit.index),
            )?;
            let result = async {
                for l in &prepared.catalog.left {
                    let candidates = prepared
                        .index
                        .select_available(
                            l,
                            settings.max_candidates,
                            settings.blocking,
                            assignment.excluded(),
                        )?
                        .candidates;
                    let candidate_ids = candidates
                        .iter()
                        .map(|candidate| candidate.right_index)
                        .collect::<Vec<_>>();
                    let mut decisions = Vec::new();
                    for candidate in candidates {
                        let r = prepared.index.unit(&candidate)?;
                        let plan = prepared.template.plan(
                            l,
                            r,
                            context.encoder,
                            &context.profile.limits,
                        )?;
                        let mut pair = Pair {
                            id: format!("{}:L{}:R{}", prepared.catalog.case_id, l.index, r.index),
                            left: l.index,
                            right: r.index,
                            label: Label::Noul(
                                prepared
                                    .catalog
                                    .truth
                                    .get(&l.index)
                                    .is_some_and(|set| set.contains(&r.index)),
                            ),
                            request: plan.request().clone(),
                            counts: plan.counts().clone(),
                            evaluation: None,
                            decision: None,
                        };
                        // Preserve submitted plans, including a failed attempt with no response.
                        let result = context
                            .evaluate(backend, plan.request(), plan.counts())
                            .await;
                        if let Ok(evaluation) = &result {
                            pair.evaluation = Some(evaluation.clone());
                        }
                        let value = serde_json::to_value(&pair).map_err(json_error)?;
                        pairs.push(value);
                        let evaluation = result?;
                        let decision = prepared.template.assess(
                            &plan,
                            &evaluation.response,
                            prepared.uncertain,
                        )?;
                        if let Some(pair) = pairs.last_mut() {
                            pair["decision"] =
                                serde_json::to_value(&decision).map_err(json_error)?;
                        }
                        decisions.push(decision);
                    }
                    let selected = assignment
                        .assign(l.index, &decisions)?
                        .iter()
                        .map(|index| {
                            decisions
                                .get(*index)
                                .map(Decision::right_index)
                                .ok_or_else(|| input("assignment selection outside candidates"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    rows.push(Row {
                        left: l.index,
                        candidates: candidate_ids,
                        selected,
                    });
                }
                Ok::<_, Error>(())
            }
            .await;
            groups.push(json!({"case_id":prepared.catalog.case_id,"cardinality":cardinality,"pairs":pairs,"rows":rows,"complete":result.is_ok()}));
            if let Err(error) = result {
                failure = Some(context.failure(error));
                break 'catalogs;
            }
            durations.push(begin.elapsed());
            completed += prepared.catalog.left.len();
        }
    }
    let phase = Measurement {
        name,
        pass,
        cache: false,
        groups,
        latencies: durations,
        units: completed,
        started,
        before,
    }
    .finish(context, failure.clone())?;
    Ok((phase, failure))
}

fn summarize(
    catalogs: Vec<PreparedCatalog>,
    settings: &loupe_core::suites::Join,
    offline: bool,
) -> Result<Report> {
    let mut positive = 0;
    let mut retained = 0;
    let mut samples = Vec::new();
    let mut rank_answers = Vec::new();
    let mut rank_labels = Vec::new();
    let mut rank_ids = Vec::new();
    for prepared in &catalogs {
        for (left, gold) in &prepared.catalog.truth {
            positive += gold.len();
            retained += gold
                .iter()
                .filter(|right| {
                    prepared
                        .catalog
                        .blocked
                        .get(left)
                        .is_some_and(|ids| ids.contains(right))
                })
                .count();
        }
        for pair in &prepared.catalog.pairs {
            let Some(evaluation) = &pair.evaluation else {
                continue;
            };
            let answers = evaluation.response.validate(&pair.request)?;
            let answer = answers
                .get(GATE_ID)
                .ok_or_else(|| input("same_entity response missing"))?;
            let is_blocked = prepared
                .catalog
                .blocked
                .get(&pair.left)
                .is_some_and(|ids| ids.contains(&pair.right));
            samples.push((
                pair.id.clone(),
                pair.label.clone(),
                answer.clone(),
                is_blocked,
            ));
            if let Some(score) = pair
                .decision
                .as_ref()
                .and_then(|decision| decision["ranking_score"].as_f64())
            {
                rank_answers.push(Answer::Noul { noul: score });
                rank_labels.push(pair.label.clone());
                rank_ids.push(pair.id.clone());
            }
        }
    }
    let template = &catalogs
        .first()
        .ok_or_else(|| input("no prepared join catalogs"))?
        .template;
    let all = samples
        .iter()
        .map(|(id, label, answer, _)| Observation { id, label, answer })
        .collect::<Vec<_>>();
    let blocked = samples
        .iter()
        .filter(|(_, _, _, blocked)| *blocked)
        .map(|(id, label, answer, _)| Observation { id, label, answer })
        .collect::<Vec<_>>();
    let question = template
        .questions()
        .get(GATE_ID)
        .ok_or_else(|| input("same_entity question missing"))?;
    let policy = template
        .policies()
        .get(GATE_ID)
        .ok_or_else(|| input("same_entity policy missing"))?;
    let ranking = rank_ids
        .iter()
        .zip(&rank_labels)
        .zip(&rank_answers)
        .map(|((id, label), answer)| Observation { id, label, answer })
        .collect::<Vec<_>>();
    let all_pair_quality = (!offline)
        .then(|| task_metrics::report(question, policy, &all, &[0.5, 0.75, 0.9, 0.95]))
        .transpose()?;
    let blocked_pair_quality = (!offline)
        .then(|| task_metrics::report(question, policy, &blocked, &[0.5, 0.75, 0.9, 0.95]))
        .transpose()?;
    let ranking_auroc = task_metrics::report(question, policy, &ranking, &[])?.auroc;
    let assignments = settings
        .cardinalities
        .iter()
        .map(|cardinality| {
            let metrics = catalogs
                .iter()
                .flat_map(|c| &c.catalog.assignments)
                .filter(|a| a.cardinality == *cardinality)
                .map(|a| &a.metrics)
                .collect::<Vec<_>>();
            assignment_summary(
                *cardinality,
                metrics.iter().map(|m| m.correct_rows).sum(),
                metrics.iter().map(|m| m.rows).sum(),
                metrics.iter().map(|m| m.true_edges).sum(),
                metrics.iter().map(|m| m.output_edges).sum(),
                metrics.iter().map(|m| m.feasible_edges).sum(),
            )
        })
        .collect();
    Ok(Report {
        blocking_recall: ratio(retained, positive, Undefined::NoPositiveLabels),
        all_pair_quality,
        blocked_pair_quality,
        ranking_auroc,
        assignments,
        cases: catalogs.into_iter().map(|c| c.catalog).collect(),
    })
}

pub(crate) fn completed_catalogs(report: &Report) -> usize {
    report
        .cases
        .iter()
        .filter(|catalog| catalog.pairs.iter().all(|pair| pair.decision.is_some()))
        .count()
}
pub(crate) fn markdown(report: &Report) -> String {
    let number = |metric: &MetricValue| {
        metric.value.map_or_else(
            || format!("undefined ({:?}; n={})", metric.undefined, metric.support),
            |value| format!("{value:.6} (n={})", metric.support),
        )
    };
    let mut text = format!(
        "### Join stages\n\nBlocking recall before assignment: {}. Pair universe: {} planned, {} answered. Ranking AUROC: {}.\n\n",
        number(&report.blocking_recall),
        report.cases.iter().map(|c| c.pairs.len()).sum::<usize>(),
        report
            .cases
            .iter()
            .flat_map(|c| &c.pairs)
            .filter(|p| p.evaluation.is_some())
            .count(),
        number(&report.ranking_auroc)
    );
    if let Some(pair) = &report.all_pair_quality {
        text.push_str(&format!(
            "All-pair gate accuracy: {}. F1: {}. AUROC: {}. Brier: {}. ECE: {}. Coverage: {}.\n\n",
            number(&pair.accuracy),
            number(&pair.f1),
            number(&pair.auroc),
            number(&pair.brier),
            number(&pair.ece),
            number(&pair.coverage)
        ));
    }
    if let Some(pair) = &report.blocked_pair_quality {
        text.push_str(&format!(
            "Retained-pair gate accuracy: {}. F1: {}.\n\n",
            number(&pair.accuracy),
            number(&pair.f1)
        ));
    }
    text.push_str("| Cardinality | Row accuracy | Edge precision | Feasible edge recall |\n| --- | --- | --- | --- |\n");
    for assignment in &report.assignments {
        text.push_str(&format!(
            "| {:?} | {} | {} | {} |\n",
            assignment.cardinality,
            number(&assignment.assignment_accuracy),
            number(&assignment.edge_precision),
            number(&assignment.feasible_edge_recall)
        ));
    }
    text.push('\n');
    text
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::run::eval::join::{Row, assignment_metrics, capacity, ratio};
    use loupe_core::matching::Cardinality;
    use loupe_core::task_metrics::Undefined;
    use std::collections::{BTreeMap, BTreeSet};
    #[test]
    fn capacity_finds_augmenting_paths_and_each_cardinality_denominator() {
        let truth = BTreeMap::from([
            (1, BTreeSet::from([1, 2])),
            (2, BTreeSet::from([1])),
            (3, BTreeSet::from([2, 3])),
        ]);
        assert_eq!(capacity(Cardinality::OneToOne, &truth), 3);
        assert_eq!(capacity(Cardinality::OneToMany, &truth), 3);
        assert_eq!(capacity(Cardinality::ManyToMany, &truth), 5);
        let rows = vec![
            Row {
                left: 1,
                candidates: vec![1, 2],
                selected: vec![2],
            },
            Row {
                left: 2,
                candidates: vec![1],
                selected: vec![1],
            },
            Row {
                left: 3,
                candidates: vec![3],
                selected: vec![3],
            },
        ];
        let metrics = assignment_metrics(Cardinality::OneToOne, &truth, &rows).unwrap();
        assert_eq!(metrics.assignment_accuracy.value, Some(1.0));
        assert_eq!(metrics.feasible_edge_recall.value, Some(1.0));
    }
    #[test]
    fn wrong_claims_missing_matches_and_undefined_denominators_stay_visible() {
        let truth = BTreeMap::from([(1, BTreeSet::from([1])), (2, BTreeSet::from([2]))]);
        let rows = vec![
            Row {
                left: 1,
                candidates: vec![2],
                selected: vec![2],
            },
            Row {
                left: 2,
                candidates: vec![],
                selected: vec![],
            },
        ];
        let metrics = assignment_metrics(Cardinality::OneToOne, &truth, &rows).unwrap();
        assert_eq!(metrics.assignment_accuracy.value, Some(0.5));
        assert_eq!(metrics.edge_precision.value, Some(0.0));
        assert_eq!(metrics.feasible_edge_recall.value, Some(0.0));
        assert_eq!(
            ratio(0, 0, Undefined::NoPositiveLabels).undefined,
            Some(Undefined::NoPositiveLabels)
        );
        let duplicate = vec![Row {
            left: 1,
            candidates: vec![1],
            selected: vec![1, 1],
        }];
        assert!(assignment_metrics(Cardinality::ManyToMany, &truth, &duplicate).is_err());
    }
    #[test]
    fn every_three_by_three_truth_graph_matches_exhaustive_capacity() {
        fn exhaustive(
            left: u64,
            truth: &BTreeMap<u64, BTreeSet<u64>>,
            used: &BTreeSet<u64>,
        ) -> usize {
            if left > 3 {
                return 0;
            }
            let mut best = exhaustive(left + 1, truth, used);
            for right in truth.get(&left).unwrap() {
                if !used.contains(right) {
                    let mut next = used.clone();
                    next.insert(*right);
                    best = best.max(1 + exhaustive(left + 1, truth, &next));
                }
            }
            best
        }
        for mask in 0..512 {
            let truth = (1..=3)
                .map(|left| {
                    (
                        left,
                        (1..=3)
                            .filter(|right| mask & (1 << ((left - 1) * 3 + right - 1)) != 0)
                            .collect(),
                    )
                })
                .collect();
            assert_eq!(
                capacity(Cardinality::OneToOne, &truth),
                exhaustive(1, &truth, &BTreeSet::new()),
                "graph mask {mask}"
            );
        }
    }
}
