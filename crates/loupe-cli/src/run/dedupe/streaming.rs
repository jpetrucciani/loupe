use crate::args::{Cli, Dedupe, Output};
use crate::run::dedupe::{reader, usage, validate};
use crate::run::{
    Kind, Prepared, duration, json_error, output_error, prepare, read_units, write_json,
};
use loupe_core::backend::{Backend, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::dedupe::{
    Keep,
    streaming::{Horizon, Index},
};
use loupe_core::encoder::{self, ClefEncoder};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::pack::{self, SourceKind};
use loupe_core::policy::{Selected, UncertainAction};
use loupe_core::preset_store::{Preset, Store};
use loupe_core::records::{Format, Options};
use loupe_core::wire::Request;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::time::Instant;

mod recovery;

struct Evaluator {
    profile: Profile,
    encoder: Option<Arc<ClefEncoder>>,
    backend: Option<Backend>,
    model_identity: Option<String>,
    budget: Budget,
    deadline: Option<Instant>,
}
impl Evaluator {
    async fn connect(&mut self, cli: &Cli, options: &Dedupe, prepared: &Prepared) -> Result<()> {
        if self.backend.is_some() {
            return Ok(());
        }
        let backend = Backend::new(&self.profile)
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery =
            with_deadline(self.deadline, backend.discover(&self.profile.limits)).await?;
        if prepared
            .document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .is_some()
        {
            let preset = Preset {
                name: "effective-streaming-dedupe".into(),
                document: prepared.document.clone(),
                origin: loupe_core::preset_store::Origin::Builtin,
                trusted: true,
                content_hash: String::new(),
            };
            preset.check_policy_identity(discovery.full_model_identity.as_deref())?;
        }
        self.profile.limits = discovery.limits.clone();
        self.model_identity = discovery.full_model_identity.clone();
        let mode = if options.options.no_cache {
            CacheMode::Disabled
        } else if options.options.refresh {
            CacheMode::Refresh
        } else {
            CacheMode::Normal
        };
        let (backend, warning) = with_deadline(
            self.deadline,
            backend.with_cache(
                &discovery,
                self.encoder.clone(),
                &self.profile.cache_dir,
                mode,
            ),
        )
        .await?;
        if let Some(warning) = warning
            && !cli.quiet
        {
            eprintln!("loupe: {warning}");
        }
        self.backend = Some(backend);
        Ok(())
    }
}

pub(crate) async fn run(
    cli: &Cli,
    options: &Dedupe,
    store: &Store,
    profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    deadline: Option<Instant>,
    started: Instant,
) -> Result<u8> {
    validate(options)?;
    if options.keep != Keep::First {
        return Err(usage(
            "streaming dedupe requires --keep first; last/longest need finite clustering",
        ));
    }
    let horizon = match (options.window, options.window_time.as_deref()) {
        (Some(count), None) => Horizon::Units(count),
        (None, Some(span)) => Horizon::Time {
            span: duration(span)?,
            capacity: options.index_capacity.unwrap_or(nonzero(1024)?),
        },
        _ => return Err(usage("choose exactly one streaming dedupe horizon")),
    };
    let max_bytes = options.max_index_bytes.unwrap_or(nonzero(67_108_864)?);
    let max_candidates = options.max_candidates.unwrap_or(nonzero(20)?);
    let mut history = Index::new(horizon, max_bytes)?;
    let prepared = prepare(
        Kind::Dedupe,
        &options.options,
        store,
        &profile.limits,
        &profile.model,
    )?;
    let fallback = crate::run::fallback::Plan::resolve(
        Kind::Dedupe,
        &options.options,
        &prepared,
        fallback_settings,
    )?;
    if loupe_core::calibration_pin::validate(&prepared.document)?.is_some() {
        return Err(usage(
            "streaming dedupe pair calibration is not classification calibration",
        ));
    }
    if let Some(policy) = prepared.document.get("policy")
        && (policy.get("splittable") == Some(&Value::Bool(true))
            || policy.get("combine").is_some()
            || policy
                .get("pack")
                .is_some_and(|mode| mode.as_str() != Some("unit")))
    {
        return Err(usage(
            "streaming dedupe requires whole pair decisions and unit packing",
        ));
    }
    let reader = reader(&options.options, &prepared)?;
    let source = match reader.format {
        Format::Lines => SourceKind::Lines,
        Format::Files => SourceKind::Files,
        Format::Whole | Format::Null => SourceKind::Units,
        _ => SourceKind::Records,
    };
    let primary = prepared
        .questions
        .first()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| usage("dedupe primary missing"))?;
    let output = if options.options.json {
        Output::Json
    } else {
        options.options.output_format.unwrap_or(Output::Plain)
    };
    let budget = Budget::new(options.options.budget);
    let mut summary = crate::summary::Summary::new(
        options.options.summary,
        options.options.dry_run,
        budget.clone(),
        profile.pricing.clone(),
    );
    let mut evaluator = Evaluator {
        profile,
        encoder: None,
        backend: None,
        model_identity: None,
        budget,
        deadline,
    };
    let mut tokenizer_loaded = false;
    let session = if let Some(path) = options
        .options
        .failed
        .as_deref()
        .or(options.options.resume.as_deref())
    {
        let profile = evaluator.profile.clone();
        evaluator.encoder = tokio::task::spawn_blocking(move || encoder::load_profile(&profile))
            .await
            .map_err(|error| {
                Error::new(
                    ErrorKind::Io,
                    format!("dedupe tokenizer task stopped: {error}"),
                )
            })??
            .map(Arc::new);
        tokenizer_loaded = true;
        evaluator.connect(cli, options, &prepared).await?;
        let mut identity = json!({"version":1,"operation":"streaming-dedupe","files":prepared.files,"reader":reader,"document":prepared.document,"questions":prepared.questions.iter().collect::<Vec<_>>(),"threshold":prepared.policy.threshold.get(),"margin":prepared.policy.margin,"uncertain":format!("{:?}",prepared.uncertain),"horizon":horizon,"max_bytes":max_bytes,"max_candidates":max_candidates,"output":format!("{output:?}"),"why":options.options.why,"strict":options.options.strict,"endpoint":format!("{}{}",evaluator.profile.base_url,evaluator.profile.path),"model":evaluator.profile.model,"model_identity":evaluator.model_identity,"encoder":evaluator.encoder.as_ref().map_or("estimate-v1",|encoder|encoder.identity.as_str()),"limits":evaluator.profile.limits});
        if let Some(fallback) = &fallback {
            identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
        }
        if options.options.ensemble {
            identity["ensemble"] = json!({"enabled":true,"min_agreement":prepared.min_agreement});
        }
        Some(crate::run::recovery::Registry::open(
            path,
            options.options.resume.is_some(),
            identity,
            &prepared.files,
            None,
        )?)
    } else {
        None
    };
    let registry = session.as_ref().map(|session| session.0.clone());
    let mut units = read_units(
        prepared.files.clone(),
        reader.clone(),
        1,
        registry.clone(),
        None,
    )?;
    let mut clock = recovery::Clock::new(started, horizon);
    let mut stdout = io::stdout().lock();
    let mut emitted = registry
        .as_ref()
        .map_or(0, crate::run::recovery::Registry::original_emitted);
    let mut header = (emitted > 0).then(Vec::new);
    let mut observed_header = None;
    let mut failed_index = None;
    let result=async {
    while let Some(unit) = units.recv().await {
        let unit = unit?;
        failed_index=Some(unit.index);
        summary.unit()?;
        if let Some(current) = &unit.header {
            if observed_header
                .as_ref()
                .is_some_and(|previous: &Vec<u8>| previous != current)
            {
                return Err(Error::new(
                    ErrorKind::Input,
                    "streaming dedupe requires identical CSV/TSV headers",
                ));
            }
            observed_header = Some(current.clone());
        }
        let saved=registry.as_ref().map(|registry|registry.saved(unit.index)).transpose()?;
        if let Some(completed)=saved.as_ref().and_then(|saved|saved.completed.clone()) {
            let completed:recovery::Completion=serde_json::from_value(completed).map_err(json_error)?;
            clock.restored(completed.history.clock(),completed.wall);
            history.restore(completed.history,|unit|comparison(unit,&reader))?;
            if saved.as_ref().is_some_and(|saved|saved.handled.is_none()) {
                if completed.kept {emit(&mut stdout,&unit,output,&completed.row,&mut header)?;stdout.flush().map_err(output_error)?;emitted=emitted.checked_add(1).ok_or_else(||usage("dedupe output counter overflow"))?;}
                if let Some(registry)=&registry {registry.handled(unit.index,completed.kept)?;}
            }
            continue;
        }
        if !tokenizer_loaded {
            let profile = evaluator.profile.clone();
            evaluator.encoder =
                tokio::task::spawn_blocking(move || encoder::load_profile(&profile))
                    .await
                    .map_err(|error| {
                        Error::new(
                            ErrorKind::Io,
                            format!("dedupe tokenizer task stopped: {error}"),
                        )
                    })??
                    .map(Arc::new);
            tokenizer_loaded = true;
        }
        let old_progress=saved.as_ref().and_then(|saved|saved.plan.as_ref()).and_then(|plan|plan.get("streaming")).map(|value|serde_json::from_value::<recovery::Progress>(value.clone()).map_err(json_error)).transpose()?;
        let replaying=old_progress.is_some();
        let mut progress=if let Some(progress)=old_progress {progress} else {let(admitted,wall)=clock.admit()?;recovery::Progress {admitted,wall,pairs:Vec::new()}};
        if let Some(registry)=&registry {registry.plan(vec![unit.index],json!({"streaming":progress}))?;}
        let expired = history.advance(unit.index, progress.admitted)?;
        let comparison = comparison(&unit, &reader);
        let candidates = history.candidates(comparison, max_candidates)?;
        // Even a new representative must fit the minimum whole-pair prompt.
        let minimum = Request {
            model: evaluator.profile.model.clone(),
            state: json!({"left":{"id":source.id(unit.index),"record":unit.state},"right":{"id":"none","record":null}}),
            questions: prepared.questions.clone(),
        };
        evaluator.profile.limits.validate_request(&minimum)?;
        pack::count(evaluator.encoder.as_deref(), &minimum)?
            .preflight(&evaluator.profile.limits)?;
        let mut pairs = Vec::new();
        let mut duplicate = None;
        if progress.pairs.len()>candidates.indices.len() {return Err(Error::new(ErrorKind::Input,"streaming saved candidate population changed"));}
        for (position,previous) in candidates.indices.iter().enumerate() {
            let retained = history.unit(*previous)?;
            let request = Request {
                model: evaluator.profile.model.clone(),
                state: json!({"left":{"id":source.id(retained.index),"record":retained.state},"right":{"id":source.id(unit.index),"record":unit.state}}),
                questions: prepared.questions.clone(),
            };
            if !options.options.dry_run {
                evaluator.connect(cli, options, &prepared).await?;
            }
            evaluator.profile.limits.validate_request(&request)?;
            let counts = pack::count(evaluator.encoder.as_deref(), &request)?;
            counts.preflight(&evaluator.profile.limits)?;
            if options.options.dry_run {
                pairs.push(json!({"representative":previous,"request":request,"counts":counts,"question_order":request.questions.keys().collect::<Vec<_>>()}));
                continue;
            }
            let backend = evaluator
                .backend
                .as_ref()
                .ok_or_else(|| usage("streaming dedupe backend missing"))?;
            let evaluation = if let Some(pair)=progress.pairs.get(position) {pair.verify(*previous,&request,&counts)?;pair.evaluation.clone()} else {let evaluation=backend
                .evaluate(
                    &request,
                    &counts,
                    &evaluator.budget,
                    options.options.strict,
                    evaluator.deadline,
                )
                .await?;
                progress.pairs.push(recovery::Pair {representative:*previous,request:request.clone(),counts:counts.clone(),evaluation:evaluation.clone(),fallback:None});
                if let Some(registry)=&registry {registry.plan(vec![unit.index],json!({"streaming":progress}))?;}
                evaluation};
            if let Some(warning) = &evaluation.warning
                && !cli.quiet
            {
                eprintln!("loupe: pair {previous}:{}: {warning}", unit.index);
            }
            let answer = evaluation
                .response
                .validate(&request)?
                .shift_remove(&primary)
                .ok_or_else(|| usage("dedupe pair answer missing"))?;
            let mut assessment = answer.assessment(&prepared.policy)?;
            let raw_assessment = assessment.clone();
            let mut ensemble_report = None;
            let ensemble = if prepared.ensembles.has_primary(&primary) {
                let report = prepared.ensembles.report(&primary,&primary,&request,&evaluation.response,prepared.policy,prepared.min_agreement)?;
                assessment = report.assessment.clone();
                ensemble_report = Some(serde_json::to_value(report).map_err(json_error)?);
                Some(loupe_core::fallback::Ensemble {set:&prepared.ensembles,primary:&primary,min_agreement:prepared.min_agreement})
            } else { None };
            let saved_fallback = progress.pairs.get(position).and_then(|pair| pair.fallback.clone()).map(Arc::new);
            let external = if let Some(plan) = &fallback {
                let pair_unit = crate::run::fallback::request_unit(&unit, &request)?;
                let input = loupe_core::fallback::Input {
                    unit: &pair_unit,
                    request: &request,
                    response: &evaluation.response,
                    field: &primary,
                    policy: prepared.policy,
                };
                let fresh = saved_fallback.is_none();
                let external = plan.execute(&input, &mut assessment, saved_fallback, evaluator.deadline, &mut summary, ensemble.as_ref()).await?;
                if fresh && let Some(external) = &external {
                    let pair = progress.pairs.get_mut(position).ok_or_else(|| usage("streaming fallback checkpoint missing"))?;
                    pair.fallback = Some(external.as_ref().clone());
                    if let Some(registry) = &registry { registry.plan(vec![unit.index],json!({"streaming":progress}))?; }
                }
                plan.require_success(external.as_deref())?;
                external
            } else {
                if saved_fallback.is_some() { return Err(Error::new(ErrorKind::Input,"saved streaming dedupe fallback requires its pinned command")); }
                None
            };
            if assessment.uncertain {
                summary.uncertain()?;
            }
            let signals = if options.options.why {
                serde_json::to_value(prepared.signals.reports(
                    &request,
                    &evaluation.response,
                    &primary,
                    &primary,
                )?)
                .map_err(json_error)?
            } else {
                Value::Null
            };
            if !cli.quiet
                && (cli.verbose > 1
                    || options.options.why
                    || assessment.uncertain && prepared.uncertain == UncertainAction::Stderr)
            {
                eprintln!(
                    "loupe: pair {previous}:{} assessment={} answers={} signals={signals}",
                    unit.index,
                    serde_json::to_value(&raw_assessment).map_err(json_error)?,
                    serde_json::to_value(&evaluation.response.answers).map_err(json_error)?
                );
                if let Some(external) = &external {
                    eprintln!("loupe: pair {previous}:{} fallback_decision={}", unit.index, loupe_core::fallback::decision(external.as_ref()));
                }
            }
            let positive = if assessment.uncertain {
                prepared.uncertain == UncertainAction::Keep
            } else {
                matches!(assessment.selected, Selected::Boolean(true))
            };
            let mut pair = json!({"representative":previous,"request":request,"counts":counts,"response":evaluation.response,"cached":evaluation.cached,"assessment":raw_assessment,"signals":signals});
            if let Some(report) = ensemble_report {
                pair["decision"] = json!({"selected":report["assessment"]["selected"],"source":"ensemble","confidence":report["assessment"]["confidence"],"uncertain":report["assessment"]["uncertain"]});
                pair["ensemble"] = report;
            }
            if let Some(external) = external {
                pair["decision"] = loupe_core::fallback::decision(external.as_ref());
                pair["fallback"] = serde_json::to_value(external.as_ref()).map_err(json_error)?;
            }
            pairs.push(pair);
            if assessment.uncertain && prepared.uncertain == UncertainAction::Fail {
                return Err(Error::new(
                    ErrorKind::Uncertain,
                    "streaming dedupe pair is uncertain",
                ));
            }
            if positive {
                duplicate = Some(*previous);
                break;
            }
        }
        if duplicate.is_none() {
            let comparison = comparison.clone();
            history.retain(unit.clone(), &comparison)?;
        }
        if options.options.dry_run {
            write_json(
                &mut stdout,
                &json!({"index":unit.index,"unit":unit.subject,"mode":"streaming-dry-run","population":"all_prior_units_assumed_representatives","candidates":candidates,"expired":expired,"pairs":pairs,"history":history.stats(),"location":unit.location}),
            )?;
            emitted = emitted
                .checked_add(1)
                .ok_or_else(|| usage("dedupe output counter overflow"))?;
        } else {
            let row = json!({"index":unit.index,"unit":unit.subject,"mode":"streaming","keep":"first","candidate_scope":"retained_lsh","candidates":candidates,"expired":expired,"pairs":pairs,"history":history.stats(),"location":unit.location});
            if let Some(registry)=&registry {registry.ready(unit.index,serde_json::to_value(recovery::Completion {history:history.snapshot(),wall:progress.wall,kept:duplicate.is_none(),row:row.clone()}).map_err(json_error)?)?;}
            if replaying {clock.restored(progress.admitted,progress.wall);}
            if duplicate.is_none() {emit(&mut stdout, &unit, output, &row, &mut header)?;
            emitted = emitted
                .checked_add(1)
                .ok_or_else(|| usage("dedupe output counter overflow"))?;}
            stdout.flush().map_err(output_error)?;
            if let Some(registry)=&registry {registry.handled(unit.index,duplicate.is_none())?;}
        }
        stdout.flush().map_err(output_error)?;
    }
    if options.options.summary && !cli.quiet {
        eprintln!(
            "loupe: dedupe_history={}",
            serde_json::to_value(history.stats()).map_err(json_error)?
        );
    }
    let code=registry.as_ref().and_then(crate::run::recovery::Registry::finished).map_or(u8::from(emitted==0),|(_,code)|code);
    if let Some(registry)=&registry {registry.finish(emitted,code)?;}
    Ok(code)
    }.await;
    if let Err(error) = &result {
        summary.error()?;
        if let Some(registry) = &registry {
            registry.failure(failed_index, error)?;
        }
    }
    result
}
fn comparison<'a>(unit: &'a Unit, reader: &Options) -> &'a Value {
    if reader.with_context != 0 {
        unit.state.get("unit").unwrap_or(&unit.state)
    } else if reader.format == Format::Files {
        unit.state.get("content").unwrap_or(&unit.state)
    } else {
        &unit.state
    }
}
fn nonzero(value: usize) -> Result<NonZeroUsize> {
    NonZeroUsize::new(value).ok_or_else(|| usage("dedupe index limit must be positive"))
}
fn emit(
    writer: &mut impl Write,
    unit: &Unit,
    output: Output,
    row: &Value,
    header: &mut Option<Vec<u8>>,
) -> Result<()> {
    match output {
        Output::Json => write_json(writer, row),
        Output::Tsv => writeln!(
            writer,
            "{}\tfirst\t{}\t{}",
            unit.index,
            row["candidates"]["eligible"],
            crate::run::dedupe::escape(&unit.text)
        )
        .map_err(output_error),
        Output::Plain | Output::Annotate => {
            if header.is_none()
                && let Some(value) = &unit.header
            {
                writer.write_all(value).map_err(output_error)?;
                *header = Some(value.clone());
            }
            if output == Output::Annotate {
                write!(writer, "representative={}\t", unit.index).map_err(output_error)?;
            }
            writer.write_all(&unit.raw).map_err(output_error)
        }
        Output::Csv | Output::Parquet => Err(usage("CSV/Parquet output requires featurize")),
    }
}
