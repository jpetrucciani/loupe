use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::Arc;

use futures::StreamExt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use loupe_core::backend::{Backend, Evaluation, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::candidates::{Candidate, Document};
use loupe_core::config::Profile;
use loupe_core::encoder::{self, ClefEncoder};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::extract;
use loupe_core::independent;
use loupe_core::input::Unit;
use loupe_core::pack::Mode as PackMode;
use loupe_core::policy::{Policy, Probability, QuestionKind};
use loupe_core::preset_store::{Origin, Preset, Store};
use loupe_core::records::{Format, Options as InputOptions};
use loupe_core::selection::{
    Outcome as ChoiceOutcome, Policy as SelectionPolicy, Session, Status, Step,
};
use loupe_core::tournament::{Options, PRESENCE_ID, Planner, SELECTION_ID, Template};
use loupe_core::wire::{Question, Request};

use crate::args::{Classify, Cli, Input, Output, Selection};
use crate::run::recovery::Registry;
use crate::run::{
    Kind, duration, interrupt_signal, json_error, output_error, prepare, read_units,
    reject_input_output_alias, write_json,
};
use crate::summary::Summary;

mod fallback;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u32,
    plans: Vec<Value>,
    evaluations: Vec<Vec<Evaluation>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fallbacks: BTreeMap<usize, BTreeMap<usize, IndexMap<String, loupe_core::fallback::Execution>>>,
    #[serde(skip)]
    complete: bool,
}

impl History {
    fn checkpoint(&self, registry: Option<&Registry>, index: u64) -> Result<()> {
        if !self.complete
            && let Some(registry) = registry
        {
            registry.plan(vec![index], json!({"selection_history":self}))?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.plans.len() != self.evaluations.len()
            || self.fallbacks.iter().any(|(round, requests)| {
                *round == 0
                    || *round > self.plans.len()
                    || requests.keys().any(|request| {
                        self.evaluations
                            .get(round.saturating_sub(1))
                            .is_none_or(|values| *request >= values.len())
                    })
            })
        {
            return Err(invalid("saved selection history is malformed"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Find,
    Pick,
    Independent,
    Extract(extract::Kind),
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Find => "find",
            Self::Pick => "pick",
            Self::Independent => "pick-independent",
            Self::Extract(_) => "extract",
        }
    }
}

enum Outcome {
    Choice(ChoiceOutcome),
    Independent(independent::Outcome),
}
struct Ranked {
    id: loupe_core::candidates::Id,
    value: f64,
    request_index: usize,
    uncertain: bool,
    external: bool,
}
impl Outcome {
    fn selected(&self) -> Vec<Ranked> {
        match self {
            Self::Choice(outcome) => outcome
                .tournament
                .selected
                .iter()
                .map(|value| Ranked {
                    id: value.id,
                    value: value.probability,
                    request_index: 0,
                    uncertain: outcome.status == Status::Uncertain,
                    external: outcome
                        .tournament
                        .ranking
                        .as_ref()
                        .is_some_and(|ranking| ranking.selection_override().is_some()),
                })
                .collect(),
            Self::Independent(outcome) => {
                outcome
                    .selected
                    .iter()
                    .map(|value| Ranked {
                        id: value.id,
                        value: value.ranking_score,
                        request_index: value.request_index,
                        uncertain: value.uncertain,
                        external: outcome.decisions.get(value.request_index).is_some_and(
                            |decision| decision.overrides.contains_key(independent::SCORE_ID),
                        ),
                    })
                    .collect()
            }
        }
    }
    fn scope(&self) -> &'static str {
        match self {
            Self::Choice(outcome) => outcome.tournament.probability_scope,
            Self::Independent(outcome) => outcome.probability_scope,
        }
    }
    fn json(&self) -> Result<Value> {
        match self {
            Self::Choice(outcome) => serde_json::to_value(outcome),
            Self::Independent(outcome) => serde_json::to_value(outcome),
        }
        .map_err(json_error)
    }
}

struct Emission<'a> {
    unit: &'a Unit,
    outcome: &'a Outcome,
    history: &'a History,
    restored: bool,
}

struct Execution<'a> {
    cli: &'a Cli,
    options: &'a Selection,
    profile: Profile,
    encoder: Option<Arc<ClefEncoder>>,
    backend: Option<Backend>,
    template: Template,
    policy: SelectionPolicy,
    budget: Budget,
    deadline: Option<Instant>,
    output: Output,
    keep_blank: bool,
    context_lines: usize,
    observed_model: Option<String>,
    recovery: Option<Registry>,
    mode: Mode,
    score_criteria: Vec<Value>,
    parser: Option<extract::Parser>,
    normalizer: Option<loupe_core::normalize::Normalizer>,
    signals: loupe_core::signals::Set,
    fallback: Option<crate::run::fallback::Plan>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Selection,
    store: &Store,
    mut profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    started: Instant,
    mode: Mode,
) -> Result<u8> {
    validate_options(options)?;
    let normalizer = if let crate::args::Command::Extract(options) = &cli.command {
        if options.normalize {
            Some(loupe_core::normalize::Normalizer::for_kind(
                options.candidates,
                loupe_core::normalize::Options {
                    now: options.now.clone(),
                    tz: options.tz.clone(),
                    currency: options.currency.clone(),
                    decimal_separator: options.decimal_separator,
                    date_order: options.date_order,
                },
            )?)
        } else {
            None
        }
    } else {
        None
    };
    let parser = match mode {
        Mode::Extract(kind) => {
            if options.common.keep_blank {
                return Err(usage(
                    "--keep-blank is inapplicable to parsed extraction spans",
                ));
            }
            Some(extract::Parser::new(kind)?)
        }
        _ => None,
    };
    let mut common = options.common.clone();
    // CLI selection thresholds apply separately to presence and ranking.
    common.min_confidence = None;
    common.min_gap = None;
    if mode != Mode::Find {
        common.threshold = None;
        common.margin = None;
    }
    let classify = Classify {
        common,
        truncate: None,
    };
    let prepared = prepare(
        if matches!(mode, Mode::Find | Mode::Extract(_)) {
            Kind::Keep
        } else {
            Kind::Pick
        },
        &classify,
        store,
        &profile.limits,
        &profile.model,
    )?;
    let fallback = crate::run::fallback::Plan::configure(
        prepared.fallback,
        options.common.fallback.as_deref(),
        options.common.fallback_timeout.as_deref(),
        options.common.on_fallback_error,
        fallback_settings,
    )?;
    if prepared
        .files
        .iter()
        .filter(|file| file.as_str() == "-")
        .count()
        > 1
    {
        return Err(usage("stdin may be specified only once"));
    }
    if loupe_core::calibration_pin::read(&prepared.document)?.is_some() {
        return Err(usage(
            "classification calibration pins do not describe a selection tournament; qualify a selection policy before applying it here",
        ));
    }
    let (id, question) = prepared
        .questions
        .first()
        .ok_or_else(|| usage("selection query is missing"))?;
    let (instructions, criteria, score_criteria) = match question {
        Question::Noul {
            instructions,
            criteria,
        } => (
            instructions,
            criteria.clone(),
            independent::default_criteria(),
        ),
        Question::Score {
            instructions,
            criteria,
        } if matches!(mode, Mode::Pick | Mode::Independent) => {
            (instructions, None, criteria.clone())
        }
        _ => {
            return Err(usage(
                "selection requires a Noul query or a Score relevance rubric",
            ));
        }
    };
    let mut presence = if question.kind() == QuestionKind::Noul {
        Policy::from_field(&prepared.document, id, QuestionKind::Noul)?
    } else {
        Policy::default()
    };
    if let Some(value) = options.common.threshold {
        presence.threshold = probability(value)?;
    }
    if let Some(value) = options.common.margin {
        presence.margin = value;
    }
    let mut choice = if question.kind() == QuestionKind::Score {
        Policy::from_field(&prepared.document, id, QuestionKind::Score)?
    } else {
        Policy::default()
    };
    if let Some(value) = options.common.min_confidence {
        choice.min_confidence = probability(value)?;
    }
    if let Some(value) = options.common.min_gap {
        choice.min_gap = value;
    }
    presence.validate()?;
    choice.validate()?;
    let policy = SelectionPolicy {
        presence,
        choice,
        on_uncertain: prepared.uncertain,
    };
    let mut reader = InputOptions::from_document(&prepared.document, Format::Whole)?;
    if matches!(options.common.input_format, Some(Input::Whole)) {
        reader.format = Format::Whole;
    }
    if options
        .common
        .input_format
        .is_some_and(|value| !matches!(value, Input::Whole))
        || reader.format != Format::Whole
        || reader.records.is_some()
        || reader.field.is_some()
        || !reader.select.is_empty()
    {
        return Err(usage(
            "selection reads one whole UTF-8 document per file; record/structured input flags are inapplicable",
        ));
    }
    let context_lines = options.common.with_context.unwrap_or(reader.with_context);
    let keep_blank = reader.keep_blank || options.common.keep_blank;
    reader.with_context = 0;
    reader.validate()?;
    if prepared.document.get("policy").is_some_and(|value| {
        value.get("splittable") == Some(&Value::Bool(true))
            || value.get("combine").is_some()
            || value
                .get("pack")
                .and_then(Value::as_str)
                .is_some_and(|mode| !matches!(mode, "auto" | "whole"))
    }) {
        return Err(usage(
            "selection uses candidate tournaments, not classification packing or split/combine policies",
        ));
    }
    if options.common.files_mode
        && (prepared.files.is_empty() || prepared.files.iter().any(|file| file == "-"))
    {
        return Err(usage("--files requires named file operands"));
    }
    if let Some(path) = &options.common.uncertain {
        reject_input_output_alias(path, &prepared.files)?;
    }
    let encoder = encoder::load_profile(&profile)?.map(Arc::new);
    if options.common.budget.is_some() && encoder.is_none() {
        return Err(usage(
            "hard --budget requires verified clef-v1 input-token counts",
        ));
    }
    let deadline = options
        .common
        .deadline
        .as_deref()
        .map(duration)
        .transpose()?
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| usage("deadline is outside the clock range"))
        })
        .transpose()?;
    let budget = Budget::new(options.common.budget);
    let mut summary = Summary::new(
        options.common.summary,
        options.common.dry_run,
        budget.clone(),
        profile.pricing.clone(),
    );
    let mut observed_model = None;
    let backend = if options.common.dry_run {
        None
    } else {
        let backend = with_deadline(deadline, Backend::new(&profile))
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery = with_deadline(deadline, backend.discover(&profile.limits)).await?;
        profile.limits = discovery.limits.clone();
        observed_model = discovery.full_model_identity.clone();
        Preset {
            name: "effective".to_owned(),
            document: prepared.document.clone(),
            origin: Origin::Builtin,
            trusted: true,
            content_hash: String::new(),
        }
        .check_policy_identity(discovery.full_model_identity.as_deref())?;
        let mode = if options.common.no_cache {
            CacheMode::Disabled
        } else if options.common.refresh {
            CacheMode::Refresh
        } else {
            CacheMode::Normal
        };
        let (backend, warning) = with_deadline(
            deadline,
            backend.with_cache(&discovery, encoder.clone(), &profile.cache_dir, mode),
        )
        .await?;
        if let Some(warning) = warning
            && !cli.quiet
        {
            eprintln!("loupe: {warning}");
        }
        Some(backend)
    };
    let output = if options.common.json {
        Output::Json
    } else {
        options.common.output_format.unwrap_or(Output::Plain)
    };
    let template = Template {
        model: profile.model.clone(),
        state: Value::Null,
        question: instructions.clone().unwrap_or_else(|| json!(id)),
        presence_criteria: criteria,
        descriptions: BTreeMap::new(),
        companions: prepared
            .questions
            .iter()
            .skip(1)
            .filter(|(id, _)| !prepared.ensembles.contains(id))
            .map(|(id, question)| (id.clone(), question.clone()))
            .collect(),
        ensemble: if options.common.ensemble {
            Some(loupe_core::selection_ensemble::Ensemble::from_query(
                &prepared.ensembles,
                id,
                prepared.min_agreement,
            )?)
        } else {
            None
        },
    };
    let mut identity = json!({"version":1,"kind":mode.name(),"template":template,"preset":prepared.document,"files":prepared.files,
        "reader":reader,"keep_blank":keep_blank,"context_lines":context_lines,"files_mode":options.common.files_mode,
        "policy":{"presence_threshold":presence.threshold.get(),"presence_margin":presence.margin,"min_confidence":choice.min_confidence.get(),"min_gap":choice.min_gap,"on_uncertain":format!("{:?}",prepared.uncertain)},
        "take":options.take,"max_rounds":options.max_rounds,"output":format!("{output:?}"),"uncertain_file":options.common.uncertain,
        "model":profile.model,"model_identity":observed_model,"encoder":encoder.as_ref().map_or("estimate-v1",|encoder|encoder.identity.as_str()),
        "limits":profile.limits,"endpoint":format!("{}{}",profile.base_url,profile.path),"strict":options.common.strict});
    if mode == Mode::Independent {
        identity["score_criteria"] = json!(score_criteria);
    }
    identity["companion_order"] = json!(template.companions.keys().collect::<Vec<_>>());
    if let Some(fallback) = &fallback {
        identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
    }
    if let Some(parser) = &parser {
        identity["candidate_parser"] = json!({"version":"extract-v1","kind":parser.kind()});
    }
    if let Some(normalizer) = &normalizer {
        identity["normalization"] = json!({"version":"normalize-v1","options":normalizer.options,"timezone_data":loupe_core::normalize::TIMEZONE_DATA_VERSION});
    }
    if options.common.why {
        identity["why"] = json!(true);
    }
    let recovery_session = options
        .common
        .failed
        .as_deref()
        .or(options.common.resume.as_deref())
        .map(|path| {
            Registry::open(
                path,
                options.common.resume.is_some(),
                identity,
                &prepared.files,
                options.common.uncertain.as_deref(),
            )
        })
        .transpose()?;
    let recovery = recovery_session.as_ref().map(|session| session.0.clone());
    let mut uncertain_file = if options.common.dry_run {
        None
    } else {
        options
            .common
            .uncertain
            .as_ref()
            .map(|path| {
                if let Some(registry) = &recovery {
                    registry.reject_output_alias(path)?;
                }
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|error| Error::io("opening uncertainty output", error))
            })
            .transpose()?
    };
    let rx = read_units(prepared.files, reader, 1, recovery.clone(), None)?;
    let mut execution = Execution {
        cli,
        options,
        profile,
        encoder,
        backend,
        template,
        policy,
        budget,
        deadline,
        output,
        keep_blank,
        context_lines,
        observed_model,
        recovery,
        mode,
        score_criteria,
        parser,
        normalizer,
        signals: prepared.signals,
        fallback,
    };
    tokio::select! {
        result = execution.run(rx, &mut summary, &mut uncertain_file) => result,
        signal = interrupt_signal() => signal,
    }
}

impl Execution<'_> {
    fn catalog<'a>(&self, document: &Document<'a>) -> Result<Vec<Candidate<'a>>> {
        match &self.parser {
            Some(parser) => parser.parse(document),
            None => document.lines(self.keep_blank),
        }
    }

    async fn run(
        &mut self,
        mut rx: tokio::sync::mpsc::Receiver<Result<Unit>>,
        summary: &mut Summary,
        uncertain: &mut Option<File>,
    ) -> Result<u8> {
        let mut emitted = self.recovery.as_ref().map_or(0, Registry::original_emitted);
        let mut stdout = io::stdout().lock();
        loop {
            let next = match with_deadline(self.deadline, async { Ok(rx.recv().await) }).await {
                Ok(next) => next,
                Err(error) => {
                    if let Some(registry) = &self.recovery {
                        registry.failure(None, &error)?;
                    }
                    return Err(error);
                }
            };
            let Some(result) = next else {
                break;
            };
            let unit = match result {
                Ok(unit) => unit,
                Err(error) => {
                    if let Some(registry) = &self.recovery {
                        registry.failure(None, &error)?;
                    }
                    return Err(error);
                }
            };
            let saved = self
                .recovery
                .as_ref()
                .map(|registry| registry.saved(unit.index))
                .transpose()?;
            if saved.as_ref().is_some_and(|saved| saved.handled.is_some())
                || self
                    .recovery
                    .as_ref()
                    .and_then(Registry::finished)
                    .is_some()
            {
                continue;
            }
            let restored = saved
                .as_ref()
                .is_some_and(|saved| saved.completed.is_some());
            let history = saved
                .and_then(|saved| {
                    saved.completed.or_else(|| {
                        saved
                            .plan
                            .and_then(|plan| plan.get("selection_history").cloned())
                    })
                })
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| invalid(&format!("saved selection history: {error}")))?;
            let history = history.map(|mut history: History| {
                history.complete = restored;
                history
            });
            summary.unit()?;
            let result = self.document(&unit, history, summary, uncertain).await;
            let classification = match result {
                Ok(result) => result,
                Err(error) => {
                    if error.kind != ErrorKind::Uncertain {
                        summary.error()?;
                    }
                    if let Some(registry) = &self.recovery {
                        registry.failure(Some(unit.index), &error)?;
                    }
                    return Err(error);
                }
            };
            let Some((outcome, history)) = classification else {
                continue;
            };
            if let Some(registry) = &self.recovery
                && !restored
            {
                registry.ready(
                    unit.index,
                    serde_json::to_value(&history).map_err(json_error)?,
                )?;
            }
            let document = Document::new(&unit.text, unit.location.source.clone());
            let catalog = self.catalog(&document)?;
            let mut payload = Vec::new();
            for selected in outcome.selected() {
                let candidate = catalog
                    .binary_search_by_key(&selected.id, |candidate| candidate.id)
                    .ok()
                    .and_then(|index| catalog.get(index))
                    .ok_or_else(|| {
                        usage("selected candidate is absent from its original document")
                    })?;
                self.emit(
                    &mut payload,
                    candidate,
                    &selected,
                    &Emission {
                        unit: &unit,
                        outcome: &outcome,
                        history: &history,
                        restored,
                    },
                )?;
            }
            stdout.write_all(&payload).map_err(output_error)?;
            stdout.flush().map_err(output_error)?;
            let delivered = !payload.is_empty();
            emitted = emitted
                .checked_add(u64::from(delivered))
                .ok_or_else(|| usage("selection output count overflow"))?;
            if let Some(registry) = &self.recovery {
                registry.handled(unit.index, delivered)?;
            }
        }
        if let Some((_, code)) = self.recovery.as_ref().and_then(Registry::finished) {
            return Ok(code);
        }
        let code = if self.options.common.dry_run {
            0
        } else {
            u8::from(emitted == 0)
        };
        if let Some(registry) = &self.recovery {
            registry.finish(emitted, code)?;
        }
        Ok(code)
    }

    async fn document(
        &mut self,
        unit: &Unit,
        saved: Option<History>,
        summary: &mut Summary,
        uncertain: &mut Option<File>,
    ) -> Result<Option<(Outcome, History)>> {
        let document = Document::new(&unit.text, unit.location.source.clone());
        let catalog = self.catalog(&document)?;
        let mut template = self.template.clone();
        if self.options.common.files_mode {
            template.state = json!({"source":unit.location.source});
        }
        template.descriptions = loupe_core::tournament::descriptions(
            &document,
            &catalog,
            self.parser.is_some(),
            self.context_lines,
        )?;
        if self.mode == Mode::Independent {
            return self
                .independent_document(unit, saved, summary, uncertain, template, &catalog)
                .await;
        }
        let planner = Planner::new(
            template,
            &catalog,
            self.encoder.as_deref(),
            &self.profile.limits,
            Options {
                take: self.options.take,
                max_rounds: self.options.max_rounds,
            },
        )?;
        let mut session = Session::new(planner, self.policy)?;
        let mut round = session.first()?;
        if self.options.common.dry_run {
            write_json(
                &mut io::stdout().lock(),
                &json!({"kind":"selection-plan","document_index":unit.index,"source":unit.location.source,"catalog":catalog,"round":round,"future_rounds":if round.is_final(){"none"}else{"conditional on actual presence and Choice results"}}),
            )?;
            return Ok(None);
        }
        let mut history = saved.unwrap_or(History {
            version: 1,
            ..History::default()
        });
        history.validate()?;
        let mut captured_uncertainty = false;
        loop {
            let plan = serde_json::to_value(&round).map_err(json_error)?;
            let index = round.number().saturating_sub(1);
            if let Some(saved_plan) = history.plans.get(index) {
                if saved_plan != &plan {
                    return Err(invalid("saved selection request plan or counts changed"));
                }
            } else {
                if history.complete || index != history.plans.len() {
                    return Err(invalid("saved selection round is missing"));
                }
                history.plans.push(plan);
                history.evaluations.push(Vec::new());
            }
            self.evaluate_round(
                unit,
                &mut history,
                index,
                round
                    .brackets()
                    .iter()
                    .map(|bracket| (bracket.request(), bracket.counts()))
                    .collect(),
            )
            .await?;
            let evaluations = history
                .evaluations
                .get(index)
                .cloned()
                .ok_or_else(|| invalid("selection response round missing"))?;
            Self::observe(&mut self.observed_model, self.cli.quiet, &evaluations)?;
            let mut decisions = Vec::new();
            for (request_index, (bracket, evaluation)) in
                round.brackets().iter().zip(&evaluations).enumerate()
            {
                let overrides = self
                    .resolve_fallback(
                        fallback::Input {
                            unit,
                            request: bracket.request(),
                            response: &evaluation.response,
                            round: round.number(),
                            request_index,
                            ensemble: bracket.ensemble(),
                        },
                        &mut history,
                        summary,
                    )
                    .await?;
                let mut decision = session.assess_response_with_overrides(
                    bracket,
                    &evaluation.response,
                    &overrides,
                )?;
                if let Some(values) = history
                    .fallbacks
                    .get(&round.number())
                    .and_then(|requests| requests.get(&request_index))
                {
                    for (field, value) in values {
                        if matches!(value, loupe_core::fallback::Execution::Failed { .. }) {
                            decision.mark_failed_fallback(field)?;
                        }
                    }
                }
                if decision.status() == Status::Uncertain {
                    self.capture_uncertainty(unit, summary, uncertain, &mut captured_uncertainty,
                        json!({"round":round.number(),"bracket":bracket.index(),"decision":decision}))?;
                }
                decisions.push(decision);
            }
            self.show_signals(unit.index, &history)?;
            let step = match session.advance(&round, &decisions) {
                Ok(step) => step,
                Err(error) => {
                    if error.kind == ErrorKind::Uncertain {
                        if history.plans.len() != round.number() {
                            return Err(invalid(
                                "saved selection history continues past its uncertainty failure",
                            ));
                        }
                        if let Some(registry) = &self.recovery
                            && !history.complete
                        {
                            registry.ready(
                                unit.index,
                                serde_json::to_value(&history).map_err(json_error)?,
                            )?;
                        }
                    }
                    return Err(error);
                }
            };
            match step {
                Step::Next { round: next } => round = next,
                Step::Final { outcome } => {
                    if history.plans.len() != round.number() {
                        return Err(invalid(
                            "saved selection history continues past its final round",
                        ));
                    }
                    return Ok(Some((Outcome::Choice(outcome), history)));
                }
            }
        }
    }

    async fn independent_document(
        &mut self,
        unit: &Unit,
        saved: Option<History>,
        summary: &mut Summary,
        uncertain: &mut Option<File>,
        template: Template,
        catalog: &[Candidate<'_>],
    ) -> Result<Option<(Outcome, History)>> {
        let plan = independent::Plan::new(
            template,
            catalog,
            self.score_criteria.clone(),
            self.encoder.as_deref(),
            &self.profile.limits,
        )?;
        let value = serde_json::to_value(&plan).map_err(json_error)?;
        if self.options.common.dry_run {
            write_json(
                &mut io::stdout().lock(),
                &json!({"kind":"independent-selection-plan","document_index":unit.index,"source":unit.location.source,"catalog":catalog,"plan":value}),
            )?;
            return Ok(None);
        }
        let mut history = saved.unwrap_or(History {
            version: 1,
            plans: vec![value.clone()],
            evaluations: vec![Vec::new()],
            ..History::default()
        });
        history.validate()?;
        if history.plans != [value] {
            return Err(invalid("saved independent plans/counts changed"));
        }
        self.evaluate_round(
            unit,
            &mut history,
            0,
            plan.items()
                .iter()
                .map(|item| (item.request(), item.counts()))
                .collect(),
        )
        .await?;
        let evaluations = history
            .evaluations
            .first()
            .cloned()
            .ok_or_else(|| invalid("independent responses missing"))?;
        Self::observe(&mut self.observed_model, self.cli.quiet, &evaluations)?;
        let mut decisions = Vec::new();
        let mut captured = false;
        for (index, evaluation) in evaluations.iter().enumerate() {
            let item = plan
                .items()
                .get(index)
                .ok_or_else(|| invalid("independent request missing"))?;
            let overrides = self
                .resolve_fallback(
                    fallback::Input {
                        unit,
                        request: item.request(),
                        response: &evaluation.response,
                        round: 1,
                        request_index: index,
                        ensemble: item.ensemble(),
                    },
                    &mut history,
                    summary,
                )
                .await?;
            let mut decision = plan.assess_response_with_overrides(
                index,
                &evaluation.response,
                self.policy,
                &overrides,
            )?;
            if let Some(values) = history
                .fallbacks
                .get(&1)
                .and_then(|requests| requests.get(&index))
            {
                for (field, value) in values {
                    if matches!(value, loupe_core::fallback::Execution::Failed { .. }) {
                        decision.mark_failed_fallback(field)?;
                    }
                }
            }
            if decision.status == Status::Uncertain {
                self.capture_uncertainty(
                    unit,
                    summary,
                    uncertain,
                    &mut captured,
                    json!({"candidate":decision.id,"decision":decision}),
                )?;
            }
            decisions.push(decision);
        }
        self.show_signals(unit.index, &history)?;
        let outcome = match plan.reduce(&decisions, self.policy, self.options.take) {
            Ok(outcome) => outcome,
            Err(error) => {
                if error.kind == ErrorKind::Uncertain
                    && !history.complete
                    && let Some(registry) = &self.recovery
                {
                    registry.ready(
                        unit.index,
                        serde_json::to_value(&history).map_err(json_error)?,
                    )?;
                }
                return Err(error);
            }
        };
        Ok(Some((Outcome::Independent(outcome), history)))
    }

    async fn evaluate_round(
        &self,
        unit: &Unit,
        history: &mut History,
        round: usize,
        requests: Vec<(&loupe_core::wire::Request, &loupe_core::encoder::Counts)>,
    ) -> Result<()> {
        let count = history
            .evaluations
            .get(round)
            .ok_or_else(|| invalid("selection response round missing"))?
            .len();
        if count > requests.len() || history.complete && count != requests.len() {
            return Err(invalid(
                "saved selection response count differs from its requests",
            ));
        }
        history.checkpoint(self.recovery.as_ref(), unit.index)?;
        if count == requests.len() {
            return Ok(());
        }
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| usage("selection backend is missing"))?;
        let results = futures::stream::iter(requests.into_iter().skip(count))
            .map(|(request, counts)| {
                backend.evaluate(
                    request,
                    counts,
                    &self.budget,
                    self.options.common.strict,
                    self.deadline,
                )
            })
            .buffered(self.profile.concurrency);
        futures::pin_mut!(results);
        while let Some(result) = results.next().await {
            let evaluation = result?;
            history
                .evaluations
                .get_mut(round)
                .ok_or_else(|| invalid("selection response round missing"))?
                .push(evaluation);
            history.checkpoint(self.recovery.as_ref(), unit.index)?;
        }
        Ok(())
    }

    fn observe(
        observed_model: &mut Option<String>,
        quiet: bool,
        evaluations: &[Evaluation],
    ) -> Result<()> {
        for evaluation in evaluations {
            let model = loupe_core::identity::Model::parse(&evaluation.response.model)
                .map(|model| model.as_str().to_owned())
                .unwrap_or_else(|_| evaluation.response.model.clone());
            if let Some(expected) = &observed_model {
                if expected != &model {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        "model identity changed during selection; refusing to combine responses",
                    ));
                }
            } else {
                *observed_model = Some(model);
            }
            if let Some(warning) = &evaluation.warning
                && !quiet
            {
                eprintln!("loupe: {warning}");
            }
        }
        Ok(())
    }

    fn capture_uncertainty(
        &self,
        unit: &Unit,
        summary: &mut Summary,
        file: &mut Option<File>,
        captured: &mut bool,
        details: Value,
    ) -> Result<()> {
        if !*captured {
            summary.uncertain()?;
            if let Some(file) = file {
                file.write_all(&unit.raw)
                    .map_err(|error| Error::io("writing uncertainty output", error))?;
                file.flush()
                    .map_err(|error| Error::io("flushing uncertainty output", error))?;
            }
            *captured = true;
        }
        if self.policy.on_uncertain == loupe_core::policy::UncertainAction::Stderr
            && !self.cli.quiet
        {
            eprintln!(
                "loupe: uncertain document={} source={} decision={}",
                unit.index,
                unit.location
                    .source
                    .as_ref()
                    .map_or_else(|| "stdin".to_owned(), |path| path.display().to_string()),
                details
            );
        }
        Ok(())
    }

    fn emit(
        &self,
        writer: &mut Vec<u8>,
        candidate: &Candidate<'_>,
        selected: &Ranked,
        context: &Emission<'_>,
    ) -> Result<()> {
        let Emission {
            unit,
            outcome,
            history,
            restored,
        } = context;
        let evaluation = history
            .evaluations
            .last()
            .and_then(|evaluations| evaluations.get(selected.request_index))
            .ok_or_else(|| usage("selected output has no final response"))?;
        let location = candidate.location.source.as_ref().map_or_else(
            || candidate.location.line_start.to_string(),
            |source| format!("{}:{}", source.display(), candidate.location.line_start),
        );
        let normalized = self
            .normalizer
            .as_ref()
            .zip(self.parser.as_ref())
            .map(|(normalizer, parser)| normalizer.normalize(parser.kind(), candidate.verbatim))
            .transpose()?;
        let text = normalized
            .as_ref()
            .map_or(candidate.raw, |value| value.text.as_str());
        let display = normalized
            .as_ref()
            .map_or(candidate.verbatim, |value| value.text.as_str());
        let ranking_text = if selected.external && self.mode != Mode::Independent {
            String::new()
        } else {
            format!("{:.6}", selected.value)
        };
        match self.output {
            Output::Csv | Output::Parquet => {
                return Err(usage("CSV/Parquet output requires featurize"));
            }
            Output::Plain => {
                if self.mode == Mode::Find {
                    write!(writer, "{location}:").map_err(output_error)?;
                }
                writer.write_all(text.as_bytes()).map_err(output_error)?;
                if !text.ends_with('\n') {
                    writer.push(b'\n');
                }
            }
            Output::Annotate => writeln!(
                writer,
                "{location}\t{}\t{}\t{}",
                candidate.id, ranking_text, display
            )
            .map_err(output_error)?,
            Output::Tsv => writeln!(
                writer,
                "{}\t{}\t{}\t{}",
                escape(&location),
                candidate.id,
                ranking_text,
                escape(display)
            )
            .map_err(output_error)?,
            Output::Json => {
                let answer_id = if self.mode == Mode::Independent {
                    independent::SCORE_ID
                } else {
                    SELECTION_ID
                };
                let mut value = json!({"index":candidate.location.line_start,"unit":candidate.verbatim,"answer":evaluation.response.answers.get(answer_id).ok_or_else(||usage("selection response is missing"))?,"model":evaluation.response.model,"cached":evaluation.cached,
                    "id":candidate.id,"location":candidate.location,"probability_scope":outcome.scope(),"uncertain":selected.uncertain,"restored":restored,
                    "provenance":{"document_index":unit.index,"outcome":outcome.json()?,"history":history}});
                let request = self.history_request(history, selected.request_index)?;
                let group = self
                    .template
                    .ensemble
                    .as_ref()
                    .map(|ensemble| ensemble.group(&request))
                    .transpose()?;
                if let Some(group) = &group {
                    let mut reports = serde_json::Map::new();
                    let mut decisions = serde_json::Map::new();
                    for (field, policy) in [
                        (PRESENCE_ID, self.policy.presence),
                        (
                            SELECTION_ID,
                            if self.mode == Mode::Independent {
                                Policy::default()
                            } else {
                                self.policy.choice
                            },
                        ),
                        (independent::SCORE_ID, self.policy.choice),
                    ] {
                        if !request.questions.contains_key(field) {
                            continue;
                        }
                        let derived = loupe_core::ensemble::field(
                            &request,
                            &evaluation.response,
                            field,
                            policy,
                            Some(group),
                        )?;
                        if let Some(report) = derived.ensemble {
                            reports.insert(field.into(), report);
                        }
                        decisions.insert(field.into(), json!({"selected":derived.assessment.selected,"source":"ensemble","confidence":derived.assessment.confidence,"uncertain":derived.assessment.uncertain}));
                    }
                    value["ensembles"] = Value::Object(reports);
                    value["selection_decisions"] = Value::Object(decisions);
                }
                if self.mode == Mode::Independent {
                    value["ranking_score"] = json!(selected.value);
                    value["score_calculation"] = json!(if selected.external {
                        "external_score"
                    } else if group.is_some() {
                        "equal_mean_level_probabilities"
                    } else {
                        "normalized_weighted_mean_of_returned_level_probabilities"
                    });
                } else {
                    value["probability"] = if selected.external {
                        Value::Null
                    } else {
                        json!(selected.value)
                    };
                    if selected.external {
                        value["model_probability"] = evaluation
                            .response
                            .answers
                            .get(answer_id)
                            .and_then(|answer| answer.get("probabilities"))
                            .and_then(|probabilities| probabilities.get(candidate.id.to_string()))
                            .cloned()
                            .ok_or_else(|| {
                                invalid("selected candidate lacks a raw model probability")
                            })?;
                        if group.is_some() {
                            value["ensemble_probability"] = json!(selected.value);
                        }
                    }
                    if group.is_some() {
                        value["probability_source"] = json!(if selected.external {
                            "fallback"
                        } else {
                            "ensemble"
                        });
                    }
                }
                if let Some(executions) = history
                    .fallbacks
                    .get(&history.plans.len())
                    .and_then(|requests| requests.get(&selected.request_index))
                {
                    value["fallback"] = json!(executions);
                    let mut decisions = value
                        .get("selection_decisions")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    for (field, execution) in executions {
                        decisions.insert(field.clone(), loupe_core::fallback::decision(execution));
                    }
                    value["selection_decisions"] = Value::Object(decisions);
                }
                if let Some(parser) = &self.parser {
                    value["candidate_type"] = json!(parser.kind());
                }
                if let Some(normalized) = &normalized {
                    value["normalized"] = serde_json::to_value(normalized).map_err(json_error)?;
                }
                if self.options.common.why {
                    value["signals"] =
                        serde_json::to_value(self.signal_reports(history, selected.request_index)?)
                            .map_err(json_error)?;
                }
                write_json(writer, &value)?;
            }
        }
        Ok(())
    }

    fn show_signals(&self, document: u64, history: &History) -> Result<()> {
        if self.options.common.why && !self.cli.quiet {
            let evaluations = history
                .evaluations
                .last()
                .ok_or_else(|| usage("signal evaluation history missing"))?;
            for index in 0..evaluations.len() {
                eprintln!(
                    "loupe: document {} round {} request {} [{}]",
                    document,
                    history.plans.len(),
                    index,
                    crate::run::signal_text(&self.signal_reports(history, index)?)
                );
            }
        }
        Ok(())
    }

    fn signal_reports<'a>(
        &self,
        history: &'a History,
        index: usize,
    ) -> Result<IndexMap<String, loupe_core::signals::Report<'a>>> {
        let request = self.history_request(history, index)?;
        let evaluation = history
            .evaluations
            .last()
            .and_then(|evaluations| evaluations.get(index))
            .ok_or_else(|| usage("signal response missing"))?;
        let primary = if self.mode == Mode::Independent {
            independent::SCORE_ID
        } else {
            SELECTION_ID
        };
        self.signals
            .reports(&request, &evaluation.response, primary, primary)
    }

    fn history_request(&self, history: &History, index: usize) -> Result<Request> {
        let plan = history
            .plans
            .last()
            .ok_or_else(|| usage("signal plan missing"))?;
        let bracket = if self.mode == Mode::Independent {
            plan.get("items")
                .and_then(|items| items.get(index))
                .and_then(|item| item.get("bracket"))
        } else {
            plan.get("brackets")
                .and_then(|brackets| brackets.get(index))
        }
        .ok_or_else(|| invalid("selection history bracket missing"))?;
        let mut request: Request = serde_json::from_value(
            bracket
                .get("request")
                .cloned()
                .ok_or_else(|| invalid("selection history request missing"))?,
        )
        .map_err(json_error)?;
        let order = bracket
            .get("question_order")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("selection question order missing"))?;
        let mut questions = IndexMap::new();
        for id in order {
            let id = id
                .as_str()
                .ok_or_else(|| invalid("selection question ID invalid"))?;
            let question = request
                .questions
                .shift_remove(id)
                .ok_or_else(|| invalid("selection question order differs from request"))?;
            questions.insert(id.to_owned(), question);
        }
        if !request.questions.is_empty() {
            return Err(invalid("selection question order is incomplete"));
        }
        request.questions = questions;
        Ok(request)
    }
}

fn validate_options(options: &Selection) -> Result<()> {
    let value = &options.common;
    if value.review.is_some() {
        return Err(usage("review is inapplicable to selection tournaments"));
    }
    if value.records.is_some()
        || value.field.is_some()
        || value.select.is_some()
        || value.split
        || value.combine.is_some()
        || value.overlap.is_some()
        || value.max_chunks.is_some()
        || value.focus.is_some()
        || value.truncate_field.is_some()
        || value.keep_going
        || value.on_error.is_some()
        || value
            .pack
            .is_some_and(|mode| !matches!(mode, PackMode::Auto | PackMode::Whole))
    {
        return Err(usage(
            "selection uses whole-document candidate tournaments; record, classification packing, split/focus/truncation and keep-going flags are inapplicable",
        ));
    }
    if value.dry_run && (value.failed.is_some() || value.resume.is_some()) {
        return Err(usage(
            "--failed/--resume apply to inference runs, not dry runs",
        ));
    }
    Ok(())
}
fn probability(value: f64) -> Result<Probability> {
    Probability::new(value).map_err(|error| usage(&error.to_string()))
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
