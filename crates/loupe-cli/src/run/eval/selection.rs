//! Real document selection measurements; labels never enter tournament requests.
use crate::args::Eval;
use crate::run::eval::runtime::{Context, Measurement};
use crate::run::eval::{Failure, PreparedCase, PreparedSuite, SuiteReport, ViewSettings, usage};
use crate::run::json_error;
use indexmap::IndexMap;
use loupe_core::backend::{Backend, Evaluation};
use loupe_core::candidates::{Candidate, Document, Id};
use loupe_core::config::{Limits, Profile};
use loupe_core::encoder::{ClefEncoder, Counts};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::independent;
use loupe_core::input::Unit;
use loupe_core::normalize::Normalizer;
use loupe_core::pack::{Mode as PackMode, SourceKind};
use loupe_core::policy::{Policy, Probability, QuestionKind, UncertainAction};
use loupe_core::preset_store::{Origin, Preset, Store};
use loupe_core::selection::{Policy as SelectionPolicy, Session, Status, Step};
use loupe_core::suites::{Input, Label, Manifest, Metric, SelectionMode, Suite};
use loupe_core::task_metrics::{self, Undefined, Value as MetricValue};
use loupe_core::tournament::{Options, Planner, Template};
use loupe_core::wire::Answer;
use loupe_core::wire::{Question, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::time::Instant;

pub(in crate::run::eval) struct Prepared {
    template: Template,
    policy: SelectionPolicy,
    mode: SelectionMode,
    options: Options,
    parser: Option<loupe_core::extract::Parser>,
    normalizer: Option<Normalizer>,
    source: Option<std::path::PathBuf>,
    keep_blank: bool,
    context_lines: usize,
    score_criteria: Vec<Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    cases: Vec<CaseReport>,
    retrieval: task_metrics::Retrieval,
    presence_accuracy: MetricValue,
    absence_accuracy: MetricValue,
    coverage: MetricValue,
    offset_accuracy: MetricValue,
    normalization_accuracy: MetricValue,
    bracket_presence_quality: Vec<PresenceQuality>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresenceQuality {
    policy: crate::run::eval::PolicyData,
    metrics: task_metrics::Report,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseReport {
    case_id: String,
    index: u64,
    source: String,
    reviewed: bool,
    sampling: Option<loupe_core::suites::Sampling>,
    source_location: loupe_core::input::Location,
    text: String,
    mode: SelectionMode,
    preset: Value,
    effective_document: Value,
    template: TemplateData,
    policy: Value,
    candidate_parser: Option<Value>,
    normalization: Option<Value>,
    expected: Vec<String>,
    expected_spans: Vec<loupe_core::suites::Span>,
    catalog: Vec<CandidateData>,
    plan: Value,
    requests: Vec<Saved>,
    outcome: Option<Value>,
    selected: Vec<Selected>,
    complete: bool,
    uncertain: bool,
    error: Option<Failure>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateData {
    value: Value,
    companion_order: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateData {
    id: Id,
    verbatim: String,
    location: loupe_core::input::Location,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    round: usize,
    bracket: usize,
    request: Request,
    question_order: Vec<String>,
    counts: Counts,
    evaluation: Option<Evaluation>,
    decision: Option<Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selected {
    id: Id,
    unit: String,
    location: loupe_core::input::Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ranking_score: Option<f64>,
    normalized: Option<Value>,
    normalization_error: Option<Failure>,
    offset_correct: Option<bool>,
    normalization_correct: Option<bool>,
}
fn input(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}
fn ratio(numerator: usize, denominator: usize, reason: Undefined) -> MetricValue {
    MetricValue {
        value: (denominator != 0).then(|| numerator as f64 / denominator as f64),
        support: denominator,
        undefined: (denominator == 0).then_some(reason),
    }
}
fn supported(options: &Eval, recalibrate: bool) -> Result<()> {
    if recalibrate
        || options.baseline.is_some()
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
            "selection suites use document tournaments; classification views, fitting, baselines, comparisons and ensembles are inapplicable",
        ));
    }
    Ok(())
}
pub(in crate::run::eval) fn prepare(
    suite: &Suite,
    position: usize,
    options: &Eval,
    store: &Store,
    profile: &Profile,
    encoder: Option<&ClefEncoder>,
    recalibrate: bool,
) -> Result<PreparedCase> {
    supported(options, recalibrate)?;
    let case = suite
        .cases
        .get(position)
        .ok_or_else(|| input("selection case missing"))?;
    let state = suite.state(case)?;
    let text = state
        .as_str()
        .ok_or_else(|| input("selection case input must be a UTF-8 document string"))?;
    let mut args = options
        .arg
        .iter()
        .map(|arg| {
            arg.split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .ok_or_else(|| usage("--arg requires NAME=VALUE"))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    for (key, value) in [
        ("query", case.query.as_ref()),
        ("candidates", case.candidates.as_ref()),
    ] {
        if let Some(value) = value {
            if args.get(key).is_some_and(|old| old != value) {
                return Err(usage(
                    "CLI arguments cannot replace a labeled selection query/candidates",
                ));
            }
            args.insert(key.into(), value.clone());
        }
    }
    let name = options
        .preset_override
        .as_deref()
        .or(options.preset.as_deref())
        .unwrap_or(&suite.manifest.preset);
    let inline = name == "selection-query";
    let preset = if inline {
        let query = case
            .query
            .as_deref()
            .filter(|query| !query.trim().is_empty())
            .ok_or_else(|| input("selection-query requires a nonempty case query"))?;
        Preset {
            name: name.into(),
            document: json!({"version":1,"type":"noul","instructions":query}),
            origin: Origin::Builtin,
            trusted: true,
            content_hash: String::new(),
        }
    } else {
        store.lookup(name)?
    };
    preset.require_trusted()?;
    preset.validate(&profile.limits, &profile.model)?;
    let document = if inline {
        preset.document.clone()
    } else {
        loupe_core::templates::expand(&preset, store, &args)?
    };
    if loupe_core::calibration_pin::validate(&document)?.is_some()
        || !loupe_core::factors::Set::from_document(&document)?.is_empty()
    {
        return Err(usage(
            "selection suites require unpinned query rubrics without weighted factors",
        ));
    }
    if let Some(policy) = document.get("policy")
        && (policy.get("splittable") == Some(&Value::Bool(true))
            || policy.get("combine").is_some()
            || policy
                .get("pack")
                .is_some_and(|p| !matches!(p.as_str(), Some("auto" | "whole"))))
    {
        return Err(usage(
            "selection rubric classification packing/split/combine defaults are inapplicable",
        ));
    }
    let mut questions: IndexMap<String, Question> =
        if let Some(questions) = document.get("questions") {
            serde_json::from_value(questions.clone()).map_err(json_error)?
        } else {
            IndexMap::from([(name.to_owned(), loupe_core::presets::question(&document)?)])
        };
    let primary = questions
        .first()
        .map(|(id, q)| (id.clone(), q.kind()))
        .ok_or_else(|| usage("selection query missing"))?;
    let signals = loupe_core::signals::Set::from_document(&document)?;
    signals.append(&mut questions)?;
    if let Some(context) = document.get("context") {
        loupe_core::presets::add_context(&mut questions, context.clone());
    }
    if let Some(context) = &case.context {
        loupe_core::presets::add_context(&mut questions, context.clone());
    }
    let question = questions
        .get(&primary.0)
        .ok_or_else(|| usage("primary selection query missing"))?;
    let settings = suite
        .manifest
        .selection
        .clone()
        .unwrap_or(loupe_core::suites::Selection {
            mode: SelectionMode::Find,
            take: NonZeroUsize::new(1).ok_or_else(|| usage("invalid take"))?,
            max_rounds: None,
            context_lines: 0,
            keep_blank: false,
            normalize: None,
        });
    let kind = case
        .candidates
        .as_deref()
        .map(str::parse::<loupe_core::extract::Kind>)
        .transpose()?;
    if settings.mode == SelectionMode::Independent && settings.max_rounds.is_some() {
        return Err(usage(
            "max_rounds is inapplicable to independently scored selection",
        ));
    }
    let reader =
        loupe_core::records::Options::from_document(&document, loupe_core::records::Format::Whole)?;
    reader.validate()?;
    if reader.format != loupe_core::records::Format::Whole
        || reader.records.is_some()
        || reader.field.is_some()
        || !reader.select.is_empty()
    {
        return Err(usage(
            "selection suites require whole documents, without structured/projection input defaults",
        ));
    }
    let keep_blank = settings.keep_blank || reader.keep_blank;
    let context_lines = if suite.manifest.selection.is_some() {
        settings.context_lines
    } else {
        reader.with_context
    };
    if (settings.mode == SelectionMode::Extract) != kind.is_some()
        || settings.mode != SelectionMode::Extract && settings.normalize.is_some()
        || settings.mode == SelectionMode::Extract && keep_blank
    {
        return Err(usage(
            "only extract suites require candidate kinds and allow normalization; parsed spans cannot keep blank lines",
        ));
    }
    let (query, presence_criteria, score_criteria) = match question {
        Question::Noul {
            instructions,
            criteria,
        } => (
            instructions.clone().unwrap_or_else(|| json!(primary.0)),
            criteria.clone(),
            independent::default_criteria(),
        ),
        Question::Score {
            instructions,
            criteria,
        } if matches!(
            settings.mode,
            SelectionMode::Pick | SelectionMode::Independent
        ) =>
        {
            (
                instructions.clone().unwrap_or_else(|| json!(primary.0)),
                None,
                criteria.clone(),
            )
        }
        _ => {
            return Err(usage(
                "find/extract query rubrics must be Noul; pick also accepts a Score relevance rubric",
            ));
        }
    };
    let mut presence = if primary.1 == QuestionKind::Noul {
        Policy::from_field(&document, &primary.0, QuestionKind::Noul)?
    } else {
        Policy::default()
    };
    let mut choice = if primary.1 == QuestionKind::Score {
        Policy::from_field(&document, &primary.0, QuestionKind::Score)?
    } else {
        Policy::default()
    };
    if let Some(value) = options.threshold {
        presence.threshold = Probability::new(value)?;
    }
    if let Some(value) = options.margin {
        presence.margin = value;
    }
    if let Some(value) = options.min_confidence {
        choice.min_confidence = Probability::new(value)?;
    }
    if let Some(value) = options.min_gap {
        choice.min_gap = value;
    }
    presence.validate()?;
    choice.validate()?;
    let on_uncertain = match document
        .get("policy")
        .and_then(|p| p.get("on_uncertain"))
        .and_then(Value::as_str)
    {
        None | Some("drop") => UncertainAction::Drop,
        Some("keep") => UncertainAction::Keep,
        Some("stderr") => UncertainAction::Stderr,
        Some("fail") => UncertainAction::Fail,
        _ => {
            return Err(usage(
                "selection evaluation cannot execute fallback or unknown uncertainty actions",
            ));
        }
    };
    let source = match &case.input {
        Input::Document { doc } => Some(
            suite.root_path.join(
                doc.strip_prefix('@')
                    .ok_or_else(|| input("invalid fixture reference"))?,
            ),
        ),
        _ => None,
    };
    let prepared = Prepared {
        template: Template {
            model: profile.model.clone(),
            state: Value::Null,
            question: query,
            presence_criteria,
            descriptions: BTreeMap::new(),
            companions: questions
                .iter()
                .skip(1)
                .map(|(id, q)| (id.clone(), q.clone()))
                .collect(),
            ensemble: None,
        },
        policy: SelectionPolicy {
            presence,
            choice,
            on_uncertain,
        },
        mode: settings.mode,
        options: Options {
            take: settings.take,
            max_rounds: settings
                .max_rounds
                .unwrap_or(NonZeroUsize::new(64).ok_or_else(|| usage("invalid round limit"))?),
        },
        parser: kind.map(loupe_core::extract::Parser::new).transpose()?,
        normalizer: kind
            .zip(settings.normalize)
            .map(|(kind, options)| Normalizer::for_kind(kind, options))
            .transpose()?,
        source,
        keep_blank,
        context_lines,
        score_criteria,
    };
    let Label::Ids(expected) = &case.label else {
        return Err(input("selection labels require candidate ID lists"));
    };
    let doc = Document::new(text, prepared.source.clone());
    let catalog = catalog(&prepared, &doc)?;
    let ids = catalog
        .iter()
        .map(|candidate| candidate.id.to_string())
        .collect::<BTreeSet<_>>();
    if expected.iter().any(|id| !ids.contains(id)) {
        return Err(input("selection label names a missing candidate"));
    }
    let mut spans = BTreeSet::new();
    for span in &case.expected_spans {
        let start = usize::try_from(span.byte_start)
            .map_err(|_| input("span offset outside platform range"))?;
        let end = usize::try_from(span.byte_end)
            .map_err(|_| input("span offset outside platform range"))?;
        if !expected.contains(&span.id.to_string())
            || !spans.insert(span.id)
            || span.text.is_empty()
            || doc.slice(start..end)? != span.text
            || span.line_start == 0
            || span.line_end < span.line_start
            || span.normalized.is_some() && prepared.normalizer.is_none()
        {
            return Err(input(
                "selection span labels require unique gold IDs, verbatim UTF-8 bounds, positive lines and enabled normalization",
            ));
        }
    }
    let raw = text.as_bytes().to_vec();
    let unit = Unit {
        index: *suite
            .positions
            .get(position)
            .ok_or_else(|| input("case source index missing"))?,
        text: text.into(),
        raw,
        subject: state.clone(),
        state: state.clone(),
        header: None,
        location: suite
            .locations
            .get(position)
            .cloned()
            .ok_or_else(|| input("case location missing"))?,
    };
    let policies = BTreeMap::from([(primary.0.clone(), presence.into())]);
    let signature=blake3::hash(&serde_json::to_vec(&json!({"template":prepared.template,"policy":policy(&prepared),"settings":suite.manifest.selection,"parser":kind,"normalization":normalization(&prepared)})).map_err(json_error)?).to_hex().to_string();
    let case = PreparedCase {
        selection: Some(prepared),
        ensemble: None,
        case: case.clone(),
        unit,
        preset: serde_json::to_value(preset).map_err(json_error)?,
        document,
        questions,
        policies,
        settings: ViewSettings {
            pack: PackMode::Unit,
            source: SourceKind::Records,
            split: false,
            modes: IndexMap::new(),
            overlap: 256,
            max_chunks: 64,
            focus: None,
            truncate: None,
            path: String::new(),
        },
        signature,
    };
    preflight(&case, encoder, &profile.limits)?;
    Ok(case)
}
fn catalog<'a>(prepared: &Prepared, document: &Document<'a>) -> Result<Vec<Candidate<'a>>> {
    if let Some(parser) = &prepared.parser {
        parser.parse(document)
    } else {
        document.lines(prepared.keep_blank)
    }
}
fn template(
    prepared: &Prepared,
    document: &Document<'_>,
    catalog: &[Candidate<'_>],
) -> Result<Template> {
    let mut template = prepared.template.clone();
    template.descriptions = loupe_core::tournament::descriptions(
        document,
        catalog,
        prepared.parser.is_some(),
        prepared.context_lines,
    )?;
    Ok(template)
}
fn policy(prepared: &Prepared) -> Value {
    json!({"presence_threshold":prepared.policy.presence.threshold.get(),"presence_margin":prepared.policy.presence.margin,"min_confidence":prepared.policy.choice.min_confidence.get(),"min_gap":prepared.policy.choice.min_gap,"on_uncertain":format!("{:?}",prepared.policy.on_uncertain)})
}
fn normalization(prepared: &Prepared) -> Option<Value> {
    prepared.normalizer.as_ref().map(|n|json!({"version":"normalize-v1","options":n.options,"timezone_data":loupe_core::normalize::TIMEZONE_DATA_VERSION}))
}
pub(in crate::run::eval) fn preflight(
    case: &PreparedCase,
    encoder: Option<&ClefEncoder>,
    limits: &Limits,
) -> Result<()> {
    let prepared = case
        .selection
        .as_ref()
        .ok_or_else(|| usage("selection preparation missing"))?;
    let doc = Document::new(&case.unit.text, prepared.source.clone());
    let catalog = catalog(prepared, &doc)?;
    let template = template(prepared, &doc, &catalog)?;
    if prepared.mode == SelectionMode::Independent {
        independent::Plan::new(
            template,
            &catalog,
            prepared.score_criteria.clone(),
            encoder,
            limits,
        )?;
    } else {
        Planner::new(template, &catalog, encoder, limits, prepared.options)?.first()?;
    }
    Ok(())
}
pub(in crate::run::eval) fn validate_metrics(manifest: &Manifest) -> Result<()> {
    for metric in manifest
        .metrics
        .iter()
        .chain(manifest.gates.iter().map(|g| &g.metric))
    {
        if !matches!(
            metric,
            Metric::Accuracy
                | Metric::F1
                | Metric::MacroF1
                | Metric::Auroc
                | Metric::Brier
                | Metric::Ece
                | Metric::SelectiveAccuracy
                | Metric::Coverage
                | Metric::LatencyP50Ms
                | Metric::LatencyP95Ms
                | Metric::UnitsPerSecond
        ) {
            return Err(usage(
                "metric is inapplicable to document selection evaluation",
            ));
        }
    }
    Ok(())
}

fn select_value(
    prepared: &Prepared,
    candidate: &Candidate<'_>,
    score: f64,
    expected: &[loupe_core::suites::Span],
) -> Result<Selected> {
    let label = expected.iter().find(|span| span.id == candidate.id);
    let offset_correct = label.map(|span| {
        span.text == candidate.verbatim
            && span.byte_start == candidate.location.byte_start
            && span.byte_end == candidate.location.byte_end
            && span.line_start == candidate.location.line_start
            && span.line_end == candidate.location.line_end
    });
    let normalized = prepared.normalizer.as_ref().map(|normalizer| {
        prepared
            .parser
            .as_ref()
            .ok_or_else(|| usage("normalization parser missing"))
            .and_then(|parser| normalizer.normalize(parser.kind(), candidate.verbatim))
    });
    let (normalized, normalization_error) = match normalized {
        Some(Ok(value)) => (Some(serde_json::to_value(value).map_err(json_error)?), None),
        Some(Err(error)) => (
            None,
            Some(Failure {
                code: error.exit_code(),
                message: error.to_string(),
            }),
        ),
        None => (None, None),
    };
    let normalization_correct = label
        .and_then(|label| label.normalized.as_ref())
        .map(|expected| normalized.as_ref().and_then(|value| value.get("value")) == Some(expected));
    Ok(Selected {
        id: candidate.id,
        unit: candidate.verbatim.into(),
        location: candidate.location.clone(),
        probability: (prepared.mode != SelectionMode::Independent).then_some(score),
        ranking_score: (prepared.mode == SelectionMode::Independent).then_some(score),
        normalized,
        normalization_error,
        offset_correct,
        normalization_correct,
    })
}
async fn run_case(
    case: &PreparedCase,
    backend: Option<&Backend>,
    context: &Context<'_, '_>,
) -> Result<CaseReport> {
    let prepared = case
        .selection
        .as_ref()
        .ok_or_else(|| usage("selection preparation missing"))?;
    let document = Document::new(&case.unit.text, prepared.source.clone());
    let catalog = catalog(prepared, &document)?;
    let template = template(prepared, &document, &catalog)?;
    let Label::Ids(expected) = &case.case.label else {
        return Err(input("selection labels missing"));
    };
    let mut report = CaseReport {
        case_id: case.case.id.clone(),
        index: case.unit.index,
        source: case.case.source.clone(),
        reviewed: case.case.reviewed,
        sampling: case.case.sampling,
        source_location: case.unit.location.clone(),
        text: case.unit.text.clone(),
        mode: prepared.mode,
        preset: case.preset.clone(),
        effective_document: case.document.clone(),
        template: TemplateData {
            value: serde_json::to_value(&template).map_err(json_error)?,
            companion_order: template.companions.keys().cloned().collect(),
        },
        policy: policy(prepared),
        candidate_parser: prepared
            .parser
            .as_ref()
            .map(|parser| json!({"version":"extract-v1","kind":parser.kind()})),
        normalization: normalization(prepared),
        expected: expected.clone(),
        expected_spans: case.case.expected_spans.clone(),
        catalog: catalog
            .iter()
            .map(|candidate| CandidateData {
                id: candidate.id,
                verbatim: candidate.verbatim.into(),
                location: candidate.location.clone(),
            })
            .collect(),
        plan: Value::Null,
        requests: Vec::new(),
        outcome: None,
        selected: Vec::new(),
        complete: false,
        uncertain: false,
        error: None,
    };
    let result = if prepared.mode == SelectionMode::Independent {
        let plan = independent::Plan::new(
            template,
            &catalog,
            prepared.score_criteria.clone(),
            context.encoder,
            &context.profile.limits,
        )?;
        report.plan = serde_json::to_value(&plan).map_err(json_error)?;
        if let Some(backend) = backend {
            async {
                let mut decisions = Vec::new();
                for (position, item) in plan.items().iter().enumerate() {
                    let mut saved = Saved {
                        round: 1,
                        bracket: position,
                        request: item.request().clone(),
                        question_order: item.request().questions.keys().cloned().collect(),
                        counts: item.counts().clone(),
                        evaluation: None,
                        decision: None,
                    };
                    let evaluation = context
                        .evaluate(backend, item.request(), item.counts())
                        .await;
                    if let Ok(value) = &evaluation {
                        saved.evaluation = Some(value.clone());
                    }
                    report.requests.push(saved);
                    let evaluation = evaluation?;
                    let decision =
                        plan.assess_response(position, &evaluation.response, prepared.policy)?;
                    if let Some(saved) = report.requests.last_mut() {
                        saved.decision = Some(serde_json::to_value(&decision).map_err(json_error)?);
                    }
                    decisions.push(decision);
                }
                let outcome = plan.reduce(&decisions, prepared.policy, prepared.options.take)?;
                report.uncertain = outcome.uncertain_candidates > 0;
                for selected in &outcome.selected {
                    let candidate = catalog
                        .iter()
                        .find(|candidate| candidate.id == selected.id)
                        .ok_or_else(|| input("selected candidate missing"))?;
                    report.selected.push(select_value(
                        prepared,
                        candidate,
                        selected.ranking_score,
                        &case.case.expected_spans,
                    )?);
                }
                report.outcome = Some(serde_json::to_value(outcome).map_err(json_error)?);
                report.complete = true;
                Ok::<_, Error>(())
            }
            .await
        } else {
            Ok(())
        }
    } else {
        let planner = Planner::new(
            template,
            &catalog,
            context.encoder,
            &context.profile.limits,
            prepared.options,
        )?;
        let mut session = Session::new(planner, prepared.policy)?;
        let mut round = session.first()?;
        report.plan = serde_json::to_value(&round).map_err(json_error)?;
        if let Some(backend) = backend {
            async {
                loop {
                    let mut decisions = Vec::new();
                    for bracket in round.brackets() {
                        let mut saved = Saved {
                            round: round.number(),
                            bracket: bracket.index(),
                            request: bracket.request().clone(),
                            question_order: bracket.request().questions.keys().cloned().collect(),
                            counts: bracket.counts().clone(),
                            evaluation: None,
                            decision: None,
                        };
                        let evaluation = context
                            .evaluate(backend, bracket.request(), bracket.counts())
                            .await;
                        if let Ok(value) = &evaluation {
                            saved.evaluation = Some(value.clone());
                        }
                        report.requests.push(saved);
                        let evaluation = evaluation?;
                        let decision = session.assess_response(bracket, &evaluation.response)?;
                        if let Some(saved) = report.requests.last_mut() {
                            saved.decision =
                                Some(serde_json::to_value(&decision).map_err(json_error)?);
                        }
                        decisions.push(decision);
                    }
                    match session.advance(&round, &decisions)? {
                        Step::Next { round: next } => round = next,
                        Step::Final { outcome } => {
                            report.uncertain = outcome.status == Status::Uncertain;
                            for selected in &outcome.tournament.selected {
                                let candidate = catalog
                                    .iter()
                                    .find(|candidate| candidate.id == selected.id)
                                    .ok_or_else(|| input("selected candidate missing"))?;
                                report.selected.push(select_value(
                                    prepared,
                                    candidate,
                                    selected.probability,
                                    &case.case.expected_spans,
                                )?);
                            }
                            report.outcome =
                                Some(serde_json::to_value(outcome).map_err(json_error)?);
                            report.complete = true;
                            break;
                        }
                    }
                }
                Ok::<_, Error>(())
            }
            .await
        } else {
            Ok(())
        }
    };
    if let Err(error) = result {
        report.error = Some(context.failure(error));
    }
    if report.error.is_none() {
        report.error = report
            .selected
            .iter()
            .find_map(|selected| selected.normalization_error.clone());
    }
    Ok(report)
}

async fn run_phase(
    name: &str,
    pass: usize,
    cases: &[Arc<PreparedCase>],
    backend: Option<&Backend>,
    context: &Context<'_, '_>,
) -> Result<(Vec<CaseReport>, crate::run::eval::Phase, Option<Failure>)> {
    let before = context.scope.budget.usage()?;
    let started = Instant::now();
    let mut latencies = Vec::new();
    let mut groups = Vec::new();
    let mut reports = Vec::new();
    let mut complete = 0;
    let mut failure = None;
    for case in cases {
        let begin = Instant::now();
        let report = run_case(case, backend, context).await?;
        if report.complete {
            complete += 1;
            latencies.push(begin.elapsed());
        }
        if name != "accuracy" {
            groups.push(json!({"case_id":report.case_id,"requests":report.requests,"outcome":report.outcome,"selected":report.selected,"complete":report.complete,"error":report.error}));
        }
        failure = report.error.clone();
        reports.push(report);
        if failure.is_some() {
            break;
        }
    }
    let logical_requests = reports
        .iter()
        .flat_map(|report| &report.requests)
        .filter(|saved| saved.evaluation.is_some())
        .count() as u64;
    let mut phase = Measurement {
        name,
        pass,
        cache: name == "accuracy" && !context.options.no_cache,
        groups,
        latencies,
        units: complete,
        started,
        before,
    }
    .finish(context, failure.clone())?;
    phase.logical_requests = logical_requests;
    phase.cache_hit_rate =
        (logical_requests != 0).then(|| phase.cache_hits as f64 / logical_requests as f64);
    Ok((reports, phase, failure))
}
fn summarize(cases: Vec<CaseReport>) -> Result<Report> {
    let actual = cases
        .iter()
        .map(|case| {
            case.selected
                .iter()
                .map(|selected| selected.id.to_string())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let observations = cases
        .iter()
        .zip(&actual)
        .filter(|(case, _)| case.complete)
        .map(|(case, actual)| task_metrics::Selection {
            id: &case.case_id,
            expected: &case.expected,
            actual,
        })
        .collect::<Vec<_>>();
    let retrieval = task_metrics::retrieval(&observations)?;
    let complete = cases
        .iter()
        .filter(|case| case.complete)
        .collect::<Vec<_>>();
    let present = complete
        .iter()
        .filter(|case| !case.expected.is_empty())
        .collect::<Vec<_>>();
    let absent = complete
        .iter()
        .filter(|case| case.expected.is_empty())
        .collect::<Vec<_>>();
    let offsets = complete
        .iter()
        .flat_map(|case| &case.selected)
        .filter_map(|selected| selected.offset_correct)
        .collect::<Vec<_>>();
    let normalized = complete
        .iter()
        .flat_map(|case| &case.selected)
        .filter_map(|selected| selected.normalization_correct)
        .collect::<Vec<_>>();
    let bracket_presence_quality = presence_quality(&cases)?;
    Ok(Report {
        retrieval,
        presence_accuracy: ratio(
            present
                .iter()
                .filter(|case| !case.uncertain && !case.selected.is_empty())
                .count(),
            present.len(),
            Undefined::NoPositiveLabels,
        ),
        absence_accuracy: ratio(
            absent
                .iter()
                .filter(|case| !case.uncertain && case.selected.is_empty())
                .count(),
            absent.len(),
            Undefined::NoNegativeLabels,
        ),
        coverage: ratio(
            complete.iter().filter(|case| !case.uncertain).count(),
            complete.len(),
            Undefined::NoSamples,
        ),
        offset_accuracy: ratio(
            offsets.iter().filter(|correct| **correct).count(),
            offsets.len(),
            Undefined::NoSamples,
        ),
        normalization_accuracy: ratio(
            normalized.iter().filter(|correct| **correct).count(),
            normalized.len(),
            Undefined::NoSamples,
        ),
        bracket_presence_quality,
        cases,
    })
}

fn presence_quality(cases: &[CaseReport]) -> Result<Vec<PresenceQuality>> {
    struct Sample {
        id: String,
        label: Label,
        answer: Answer,
    }
    let mut groups = BTreeMap::<String, (Policy, Vec<Sample>)>::new();
    for case in cases {
        let threshold = case.policy["presence_threshold"]
            .as_f64()
            .ok_or_else(|| input("presence threshold missing"))?;
        let margin = case.policy["presence_margin"]
            .as_f64()
            .ok_or_else(|| input("presence margin missing"))?;
        let policy = Policy {
            threshold: Probability::new(threshold)?,
            margin,
            ..Policy::default()
        };
        policy.validate()?;
        let key = serde_json::to_string(&(threshold, margin)).map_err(json_error)?;
        let entry = groups.entry(key).or_insert((policy, Vec::new()));
        for saved in &case.requests {
            let Some(evaluation) = &saved.evaluation else {
                continue;
            };
            let Some(Question::Choice { criteria, .. }) = saved
                .request
                .questions
                .get(loupe_core::tournament::SELECTION_ID)
            else {
                return Err(input("selection Choice missing"));
            };
            let positive = case.expected.iter().any(|id| criteria.contains_key(id));
            let answers = evaluation.response.validate(&saved.request)?;
            let answer = answers
                .get(loupe_core::tournament::PRESENCE_ID)
                .ok_or_else(|| input("presence answer missing"))?
                .clone();
            entry.1.push(Sample {
                id: format!("{}:{}:{}", case.case_id, saved.round, saved.bracket),
                label: Label::Noul(positive),
                answer,
            });
        }
    }
    groups
        .into_values()
        .filter(|(_, samples)| !samples.is_empty())
        .map(|(policy, samples)| {
            let observations = samples
                .iter()
                .map(|sample| task_metrics::Observation {
                    id: &sample.id,
                    label: &sample.label,
                    answer: &sample.answer,
                })
                .collect::<Vec<_>>();
            let metrics = task_metrics::report(
                &Question::Noul {
                    instructions: None,
                    criteria: None,
                },
                &policy,
                &observations,
                &[0.5, 0.75, 0.9, 0.95],
            )?;
            Ok(PresenceQuality {
                policy: policy.into(),
                metrics,
            })
        })
        .collect()
}
pub(in crate::run::eval) async fn run_suite(
    suite: PreparedSuite,
    accuracy: Option<&Backend>,
    uncached: Option<&Backend>,
    warning: Option<&str>,
    context: &Context<'_, '_>,
) -> Result<(SuiteReport, Option<Failure>)> {
    let (cases, phase, mut failure) =
        run_phase("accuracy", 0, &suite.cases, accuracy, context).await?;
    let mut phases = vec![phase];
    if failure.is_none() && !context.options.dry_run {
        for pass in 1..=context.options.warmup {
            let (_, phase, error) =
                run_phase("warmup", pass, &suite.cases, uncached, context).await?;
            phases.push(phase);
            failure = error;
            if failure.is_some() {
                break;
            }
        }
        if failure.is_none() {
            let (_, phase, error) =
                run_phase("latency", 1, &suite.cases, uncached, context).await?;
            phases.push(phase);
            failure = error;
        }
    }
    let selection = summarize(cases)?;
    let mut warnings = warning.into_iter().map(str::to_owned).collect::<Vec<_>>();
    if !suite.suite.manifest.gates.is_empty() {
        warnings.push("selection gate declarations are unassessed; stage-specific human-ratified baseline support is pending".into());
    }
    let report = SuiteReport {
        manifest: suite.suite.manifest,
        splits: suite.splits,
        partition: match context
            .options
            .partition
            .unwrap_or(crate::args::EvalSplit::Test)
        {
            crate::args::EvalSplit::Test => "test",
            crate::args::EvalSplit::Calibrate => "calibrate",
            crate::args::EvalSplit::Captured => "captured",
        }
        .into(),
        selected_case_ids: suite.selected,
        records: Vec::new(),
        questions: Vec::new(),
        parity: None,
        phases,
        warnings,
        join: None,
        selection: Some(selection),
    };
    Ok((report, failure))
}
pub(crate) fn completed_cases(report: &Report) -> usize {
    report.cases.iter().filter(|case| case.complete).count()
}
pub(crate) fn markdown(report: &Report) -> String {
    let number = |value: &MetricValue| {
        value.value.map_or_else(
            || format!("undefined ({:?}; n={})", value.undefined, value.support),
            |number| format!("{number:.6} (n={})", value.support),
        )
    };
    let mut text = String::from("### Document selection\n\n| Metric | Value |\n| --- | --- |\n");
    for (name, value) in [
        ("Exact match", &report.retrieval.exact_match),
        ("Precision", &report.retrieval.precision),
        ("Recall", &report.retrieval.recall),
        ("F1", &report.retrieval.f1),
        ("Presence accuracy", &report.presence_accuracy),
        ("Absence accuracy", &report.absence_accuracy),
        ("Certain coverage", &report.coverage),
        ("Original span/offset accuracy", &report.offset_accuracy),
        ("Normalization accuracy", &report.normalization_accuracy),
    ] {
        text.push_str(&format!("| {name} | {} |\n", number(value)));
    }
    for group in &report.bracket_presence_quality {
        text.push_str(&format!("\nBracket presence (threshold {}, margin {}): accuracy {}, F1 {}, AUROC {}, Brier {}, ECE {}, coverage {}.\n",
            group.policy.threshold,group.policy.margin,number(&group.metrics.accuracy),number(&group.metrics.f1),number(&group.metrics.auroc),number(&group.metrics.brier),number(&group.metrics.ece),number(&group.metrics.coverage)));
    }
    text.push('\n');
    text
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::run::eval::selection::{CaseReport, summarize};
    use serde_json::Value;
    fn captured() -> CaseReport {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../evals/golden/selection/actual-email-role-false-positive.json"
        ))
        .unwrap();
        serde_json::from_value(fixture["case"].clone()).unwrap()
    }
    #[test]
    fn real_false_positive_has_wrong_retrieval_and_binary_calibration_cost() {
        let case = captured();
        let p = case.requests[0]
            .evaluation
            .as_ref()
            .unwrap()
            .response
            .answers["selection-present"]["noul"]
            .as_f64()
            .unwrap();
        let report = summarize(vec![case]).unwrap();
        assert_eq!(report.retrieval.exact_match.value, Some(0.0));
        assert_eq!(report.absence_accuracy.value, Some(0.0));
        assert_eq!(report.bracket_presence_quality.len(), 1);
        assert!(
            (report.bracket_presence_quality[0]
                .metrics
                .brier
                .value
                .unwrap()
                - p * p)
                .abs()
                < 1e-12
        );
        assert_eq!(report.normalization_accuracy.support, 0);
    }
    #[test]
    fn uncertainty_is_separate_from_confirmed_absence_and_undefined_support() {
        use loupe_core::candidates::Document;
        use loupe_core::extract::{Kind, Parser};
        use loupe_core::selection::{Policy as SelectionPolicy, Session, Status, Step};
        use loupe_core::tournament::{Options, Planner, Template};
        let mut case = captured();
        let document = Document::new(&case.text, None);
        let catalog = Parser::new(Kind::Emails).unwrap().parse(&document).unwrap();
        let request = &case.requests[0].request;
        let template = Template {
            model: request.model.clone(),
            state: Value::Null,
            question: request.questions["selection"]
                .instructions()
                .unwrap()
                .clone(),
            presence_criteria: None,
            descriptions: loupe_core::tournament::descriptions(&document, &catalog, true, 0)
                .unwrap(),
            companions: indexmap::IndexMap::new(),
            ensemble: None,
        };
        let mut policy = SelectionPolicy::default();
        policy.presence.margin = 1.0;
        let options = Options {
            take: std::num::NonZeroUsize::new(1).unwrap(),
            max_rounds: std::num::NonZeroUsize::new(64).unwrap(),
        };
        let limits = loupe_core::config::Limits::default();
        let planner = Planner::new(template, &catalog, None, &limits, options).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let round = session.first().unwrap();
        assert_eq!(
            serde_json::to_value(round.brackets()[0].request()).unwrap(),
            serde_json::to_value(request).unwrap()
        );
        let response = &case.requests[0].evaluation.as_ref().unwrap().response;
        let decision = session
            .assess_response(&round.brackets()[0], response)
            .unwrap();
        let Step::Final { outcome } = session.advance(&round, &[decision]).unwrap() else {
            panic!("expected one complete bracket");
        };
        assert_eq!(outcome.status, Status::Uncertain);
        assert!(outcome.tournament.selected.is_empty());
        case.selected.clear();
        case.uncertain = outcome.status == Status::Uncertain;
        case.outcome = Some(serde_json::to_value(outcome).unwrap());
        let report = summarize(vec![case]).unwrap();
        assert_eq!(report.retrieval.exact_match.value, Some(1.0));
        assert_eq!(report.absence_accuracy.value, Some(0.0));
        assert_eq!(report.coverage.value, Some(0.0));
        assert_eq!(report.offset_accuracy.support, 0);
        assert!(report.offset_accuracy.value.is_none());
    }
}
