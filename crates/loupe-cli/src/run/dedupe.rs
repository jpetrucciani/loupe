use futures::StreamExt;
use loupe_core::backend::{Backend, Evaluation, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::dedupe::{Cluster, Forest, Member, pair_count};
use loupe_core::encoder::{self, ClefEncoder, Counts};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::{Location, Unit};
use loupe_core::pack::{self, SourceKind};
use loupe_core::policy::{Assessment, Selected, UncertainAction};
use loupe_core::preset_store::Store;
use loupe_core::records::{Format, Options};
use loupe_core::recovery::Disposition;
use loupe_core::wire::Request;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::time::Instant;

use crate::args::{Cli, Dedupe, Input, Output};
use crate::run::{
    Kind, Prepared, duration, interrupt_signal, json_error, output_error, prepare, write_json,
};

mod input;
mod journal;
mod streaming;

struct Work {
    ordinal: u64,
    left: usize,
    right: usize,
    request: Request,
    counts: Counts,
    admission: Unit,
    saved: Option<Evaluation>,
    fallback: Option<Arc<loupe_core::fallback::Execution>>,
}
#[derive(Serialize)]
struct Link {
    left: u64,
    right: u64,
    answer: Value,
    assessment: Assessment,
    model: String,
    cached: bool,
    input_tokens: usize,
    signals: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback: Option<loupe_core::fallback::Execution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ensemble: Option<Value>,
}
struct Pairs {
    left: usize,
    right: usize,
    count: usize,
    ordinal: u64,
}
impl Iterator for Pairs {
    type Item = Result<(u64, usize, usize)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.count < 2 || self.left >= self.count.saturating_sub(1) {
            return None;
        }
        if self.right >= self.count {
            self.left += 1;
            self.right = self.left + 1;
        }
        if self.left >= self.count.saturating_sub(1) {
            return None;
        }
        let Some(ordinal) = self.ordinal.checked_add(1) else {
            return Some(Err(Error::new(
                ErrorKind::Input,
                "dedupe pair index overflow",
            )));
        };
        self.ordinal = ordinal;
        let pair = (ordinal, self.left, self.right);
        self.right += 1;
        Some(Ok(pair))
    }
}
struct Execution {
    backend: Option<Backend>,
    encoder: Option<Arc<ClefEncoder>>,
    profile: Profile,
    budget: Budget,
    deadline: Option<Instant>,
    fallback: Option<crate::run::fallback::Plan>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Dedupe,
    store: &Store,
    profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    started: Instant,
) -> Result<u8> {
    let deadline = options
        .options
        .deadline
        .as_deref()
        .map(duration)
        .transpose()?
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| usage("dedupe deadline is outside clock range"))
        })
        .transpose()?;
    tokio::select! {
        biased;
        signal=interrupt_signal()=>signal,
        _=async{if let Some(deadline)=deadline{tokio::time::sleep_until(deadline).await;}else{std::future::pending::<()>().await;}}=>Err(Error::new(ErrorKind::Unavailable,"overall dedupe deadline expired")),
        result=async {
            if options.window.is_some() || options.window_time.is_some() {
                streaming::run(cli, options, store, profile, fallback_settings, deadline, started).await
            } else {
                run(cli,options,store,profile,fallback_settings,deadline).await
            }
        }=>result,
    }
}
async fn run(
    cli: &Cli,
    options: &Dedupe,
    store: &Store,
    mut profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    deadline: Option<Instant>,
) -> Result<u8> {
    validate(options)?;
    if options.max_index_bytes.is_some() || options.max_candidates.is_some() {
        return Err(usage(
            "streaming index flags require --window or --window-time",
        ));
    }
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
            "dedupe pair inputs require pair calibration; classification calibration pins are inapplicable",
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
            "finite dedupe evaluates whole pairs; splitting/combine/window preset policies are inapplicable",
        ));
    }
    let reader = reader(&options.options, &prepared)?;
    let source = match reader.format {
        Format::Lines => SourceKind::Lines,
        Format::Files => SourceKind::Files,
        Format::Whole | Format::Null => SourceKind::Units,
        _ => SourceKind::Records,
    };
    let contextual = reader.with_context != 0;
    let budget = Budget::new(options.options.budget);
    let mut summary = crate::summary::Summary::new(
        options.options.summary,
        options.options.dry_run,
        budget.clone(),
        profile.pricing.clone(),
    );
    let catalog = input::read(prepared.files.clone(), reader.clone()).await?;
    let count = pair_count(catalog.units.len());
    if options
        .max_pairs
        .is_some_and(|limit| count > u128::from(limit))
    {
        return Err(Error::new(
            ErrorKind::Input,
            format!("finite dedupe requires {count} pairs, exceeding --max-pairs"),
        ));
    }
    if count > u128::from(u64::MAX) {
        return Err(Error::new(
            ErrorKind::Input,
            "dedupe pair count exceeds journal index range",
        ));
    }
    let encoder = if count == 0 {
        None
    } else {
        let encoder_profile = profile.clone();
        tokio::task::spawn_blocking(move || encoder::load_profile(&encoder_profile))
            .await
            .map_err(|error| {
                Error::new(
                    ErrorKind::Io,
                    format!("dedupe tokenizer task stopped: {error}"),
                )
            })??
            .map(Arc::new)
    };
    let mut forest = Forest::new(
        catalog
            .units
            .iter()
            .map(|unit| {
                let state = if contextual {
                    unit.state.get("unit").unwrap_or(&unit.state)
                } else if reader.format == Format::Files {
                    unit.state.get("content").unwrap_or(&unit.state)
                } else {
                    &unit.state
                };
                Ok(Member {
                    index: unit.index,
                    characters: encoder::render(state)?.chars().count(),
                })
            })
            .collect::<Result<Vec<_>>>()?,
    )?;
    let mut model_identity = None;
    let backend = if options.options.dry_run || count == 0 {
        None
    } else {
        let backend = Backend::new(&profile)
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery = backend.discover(&profile.limits).await?;
        profile.limits = discovery.limits.clone();
        model_identity = discovery.full_model_identity.clone();
        if prepared
            .document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .is_some()
        {
            let preset = loupe_core::preset_store::Preset {
                name: "effective-dedupe".into(),
                document: prepared.document.clone(),
                origin: loupe_core::preset_store::Origin::Builtin,
                trusted: true,
                content_hash: String::new(),
            };
            preset.check_policy_identity(model_identity.as_deref())?;
        }
        let mode = if options.options.no_cache {
            CacheMode::Disabled
        } else if options.options.refresh {
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
    let output = if options.options.json {
        Output::Json
    } else {
        options.options.output_format.unwrap_or(Output::Plain)
    };
    let mut identity = json!({"version":1,"operation":"finite-dedupe","catalog":catalog.fingerprints,"reader":reader,"files":prepared.files,"document":prepared.document,"questions":prepared.questions.iter().collect::<Vec<_>>(),"policy":{"threshold":prepared.policy.threshold.get(),"margin":prepared.policy.margin,"uncertain":format!("{:?}",prepared.uncertain)},"keep":options.keep,"output":format!("{output:?}"),"why":options.options.why,"strict":options.options.strict,"endpoint":format!("{}{}",profile.base_url,profile.path),"model":profile.model,"model_identity":model_identity,"encoder":encoder.as_ref().map_or("estimate-v1",|encoder|encoder.identity.as_str()),"limits":profile.limits});
    if let Some(fallback) = &fallback {
        identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
    }
    if options.options.ensemble {
        identity["ensemble"] = json!({"enabled":true,"min_agreement":prepared.min_agreement});
    }
    let mut journal = if options.options.dry_run {
        None
    } else {
        journal::open(options, identity, &prepared.files)?
    };
    let original_last = journal.as_ref().map_or(0, |journal| journal.last_index());
    let finished = journal.as_ref().and_then(|journal| journal.finished());
    let execution = Execution {
        backend,
        encoder,
        profile,
        budget,
        deadline,
        fallback,
    };
    let primary = prepared
        .questions
        .first()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| usage("dedupe primary question missing"))?;
    let mut pairs = Pairs {
        left: 0,
        right: 1,
        count: catalog.units.len(),
        ordinal: 0,
    };
    let mut links = Vec::new();
    let mut stdout = io::stdout().lock();
    let mut planned = 0u64;
    loop {
        let mut work = Vec::new();
        for _ in 0..execution.profile.concurrency {
            let Some(pair) = pairs.next() else { break };
            let (ordinal, left, right) = pair?;
            let left_unit = catalog
                .units
                .get(left)
                .ok_or_else(|| usage("dedupe left record missing"))?;
            let right_unit = catalog
                .units
                .get(right)
                .ok_or_else(|| usage("dedupe right record missing"))?;
            let request = Request {
                model: execution.profile.model.clone(),
                state: json!({"left":{"id":source.id(left_unit.index),"record":left_unit.state},"right":{"id":source.id(right_unit.index),"record":right_unit.state}}),
                questions: prepared.questions.clone(),
            };
            let admission = pair_admission(ordinal, left_unit.index, right_unit.index)?;
            let (saved, fallback) = if let Some(journal) = &mut journal {
                if ordinal <= original_last {
                    journal.verify(&admission)?;
                    let saved = journal.saved(ordinal)?;
                    let fallback = saved
                        .fallback
                        .map(|value| {
                            serde_json::from_value::<loupe_core::fallback::Execution>(value)
                                .map(Arc::new)
                                .map_err(json_error)
                        })
                        .transpose()?;
                    let evaluation = saved
                        .completed
                        .map(|value| serde_json::from_value(value).map_err(json_error))
                        .transpose()?;
                    (evaluation, fallback)
                } else {
                    journal.admit(&admission)?;
                    (None, None)
                }
            } else {
                (None, None)
            };
            let plan = || -> Result<Counts> {
                execution.profile.limits.validate_request(&request)?;
                let counts = pack::count(execution.encoder.as_deref(), &request)?;
                counts.preflight(&execution.profile.limits)?;
                Ok(counts)
            };
            let counts = match plan() {
                Ok(counts) => counts,
                Err(error) => {
                    if let Some(journal) = &mut journal {
                        journal.failed(Some(ordinal), error.exit_code(), error.to_string())?;
                    }
                    return Err(error);
                }
            };
            if options.options.dry_run {
                write_json(
                    &mut stdout,
                    &json!({"pair":admission.subject,"ordinal":ordinal,"left":left_unit.index,"right":right_unit.index,"request":request,"counts":counts,"question_order":request.questions.keys().collect::<Vec<_>>()}),
                )?;
                planned = planned
                    .checked_add(1)
                    .ok_or_else(|| usage("dedupe plan count overflow"))?;
                continue;
            }
            if saved.is_none()
                && let Some(journal) = &mut journal
            {
                journal.plan(vec![ordinal],json!({"operation":"dedupe-pair","request":request,"counts":counts,"question_order":request.questions.keys().collect::<Vec<_>>()}))?;
            }
            work.push(Work {
                ordinal,
                left,
                right,
                request,
                counts,
                admission,
                saved,
                fallback,
            });
        }
        if work.is_empty() {
            if pairs.left >= pairs.count.saturating_sub(1) || pairs.count < 2 {
                break;
            }
            continue;
        }
        let results = futures::stream::iter(work)
            .map(|work| async {
                let evaluation = match work.saved.clone() {
                    Some(saved) => Ok(saved),
                    None => match &execution.backend {
                        Some(backend) => {
                            backend
                                .evaluate(
                                    &work.request,
                                    &work.counts,
                                    &execution.budget,
                                    options.options.strict,
                                    execution.deadline,
                                )
                                .await
                        }
                        None => Err(usage("dedupe backend missing")),
                    },
                };
                (work, evaluation)
            })
            .buffered(execution.profile.concurrency);
        futures::pin_mut!(results);
        let mut failure = None;
        while let Some((work, result)) = results.next().await {
            summary.unit()?;
            let evaluation = match result {
                Ok(evaluation) => evaluation,
                Err(error) => {
                    summary.error()?;
                    if let Some(journal) = &mut journal {
                        journal.failed(Some(work.ordinal), error.exit_code(), error.to_string())?;
                    }
                    if failure.is_none() {
                        failure = Some(error);
                    }
                    continue;
                }
            };
            evaluation.response.validate(&work.request)?;
            if work.saved.is_none()
                && let Some(journal) = &mut journal
            {
                journal.ready(
                    work.ordinal,
                    serde_json::to_value(&evaluation).map_err(json_error)?,
                )?;
            }
            if failure.is_some() {
                continue;
            }
            if let Some(warning) = &evaluation.warning
                && !cli.quiet
            {
                eprintln!("loupe: pair {}: {warning}", work.admission.text);
            }
            let raw = evaluation
                .response
                .answers
                .get(&primary)
                .ok_or_else(|| usage("dedupe answer missing"))?;
            let answer = evaluation
                .response
                .validate(&work.request)?
                .shift_remove(&primary)
                .ok_or_else(|| usage("dedupe answer missing"))?;
            let mut assessment = answer.assessment(&prepared.policy)?;
            let raw_assessment = assessment.clone();
            let mut ensemble_report = None;
            let ensemble = if prepared.ensembles.has_primary(&primary) {
                let report = prepared.ensembles.report(
                    &primary,
                    &primary,
                    &work.request,
                    &evaluation.response,
                    prepared.policy,
                    prepared.min_agreement,
                )?;
                assessment = report.assessment.clone();
                ensemble_report = Some(serde_json::to_value(report).map_err(json_error)?);
                Some(loupe_core::fallback::Ensemble {
                    set: &prepared.ensembles,
                    primary: &primary,
                    min_agreement: prepared.min_agreement,
                })
            } else {
                None
            };
            let external = if let Some(plan) = &execution.fallback {
                let pair_unit = crate::run::fallback::request_unit(&work.admission, &work.request)?;
                let input = loupe_core::fallback::Input {
                    unit: &pair_unit,
                    request: &work.request,
                    response: &evaluation.response,
                    field: &primary,
                    policy: prepared.policy,
                };
                let external = plan
                    .execute(
                        &input,
                        &mut assessment,
                        work.fallback.clone(),
                        execution.deadline,
                        &mut summary,
                        ensemble.as_ref(),
                    )
                    .await?;
                if work.fallback.is_none()
                    && let Some(external) = &external
                    && let Some(journal) = &mut journal
                {
                    journal.fallback(
                        work.ordinal,
                        serde_json::to_value(external.as_ref()).map_err(json_error)?,
                    )?;
                }
                if let Err(error) = plan.require_success(external.as_deref()) {
                    summary.error()?;
                    if let Some(journal) = &mut journal {
                        journal.failed(Some(work.ordinal), error.exit_code(), error.to_string())?;
                    }
                    if failure.is_none() {
                        failure = Some(error);
                    }
                    continue;
                }
                external
            } else {
                if work.fallback.is_some() {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "saved dedupe fallback requires its pinned command",
                    ));
                }
                None
            };
            if assessment.uncertain {
                summary.uncertain()?;
            }
            let signals = if options.options.why {
                serde_json::to_value(prepared.signals.reports(
                    &work.request,
                    &evaluation.response,
                    &primary,
                    &primary,
                )?)
                .map_err(json_error)?
            } else {
                Value::Null
            };
            if !cli.quiet
                && (options.options.why
                    || assessment.uncertain && prepared.uncertain == UncertainAction::Stderr)
            {
                eprintln!(
                    "loupe: pair {} answer={} uncertain={} signals={signals}",
                    work.admission.text, raw, raw_assessment.uncertain
                );
                if let Some(external) = &external {
                    eprintln!(
                        "loupe: pair {} fallback_decision={}",
                        work.admission.text,
                        loupe_core::fallback::decision(external.as_ref())
                    );
                }
            }
            if assessment.uncertain && prepared.uncertain == UncertainAction::Fail {
                if let Some(journal) = &mut journal {
                    journal.failed(Some(work.ordinal), 3, "dedupe pair is uncertain".into())?;
                }
                if failure.is_none() {
                    failure = Some(Error::new(ErrorKind::Uncertain, "dedupe pair is uncertain"));
                }
                continue;
            }
            let positive = if assessment.uncertain {
                prepared.uncertain == UncertainAction::Keep
            } else {
                matches!(assessment.selected, Selected::Boolean(true))
            };
            let left = catalog
                .units
                .get(work.left)
                .ok_or_else(|| usage("dedupe left record missing"))?
                .index;
            let right = catalog
                .units
                .get(work.right)
                .ok_or_else(|| usage("dedupe right record missing"))?
                .index;
            if positive && forest.connect(left, right)? {
                links.push(Link {
                    left,
                    right,
                    answer: raw.clone(),
                    assessment: raw_assessment,
                    model: evaluation.response.model,
                    cached: evaluation.cached,
                    input_tokens: work.counts.input_tokens,
                    signals,
                    decision: external.as_deref().map(loupe_core::fallback::decision).or_else(||ensemble_report.as_ref().map(|report|json!({"selected":report["assessment"]["selected"],"source":"ensemble","confidence":report["assessment"]["confidence"],"uncertain":report["assessment"]["uncertain"]}))),
                    fallback: external.map(|external| external.as_ref().clone()),
                    ensemble: ensemble_report,
                });
            }
            if let Some(journal) = &mut journal
                && journal.disposition(work.ordinal)?.is_none()
            {
                journal.handled(work.ordinal, Disposition::Omitted)?;
            }
        }
        if let Some(error) = failure {
            stdout.flush().map_err(output_error)?;
            return Err(error);
        }
    }
    if options.options.dry_run {
        stdout.flush().map_err(output_error)?;
        return Ok(u8::from(planned == 0));
    }
    if let Some(journal) = &mut journal {
        journal.end_input(json!({"catalog":catalog.fingerprints,"pairs":count}))?;
    }
    let clusters = forest.clusters(options.keep)?;
    if let Some((_, code)) = finished {
        return Ok(code);
    }
    let checkpoint = journal
        .as_ref()
        .and_then(|journal| journal.output_checkpoint("dedupe_representatives"));
    let skip = checkpoint.map_or(0, |checkpoint| checkpoint.0);
    let mut emitted = checkpoint.map_or(0, |checkpoint| checkpoint.1);
    let units = catalog
        .units
        .iter()
        .map(|unit| (unit.index, unit))
        .collect::<BTreeMap<_, _>>();
    let mut rendering = Rendering {
        units: &units,
        links: &links,
        output,
        options,
        header: None,
        resumed: skip > 0,
    };
    for cluster in &clusters {
        if cluster.representative <= skip {
            continue;
        }
        emit(&mut stdout, cluster, &mut rendering)?;
        stdout.flush().map_err(output_error)?;
        emitted = emitted
            .checked_add(1)
            .ok_or_else(|| usage("dedupe output count overflow"))?;
        if let Some(journal) = &mut journal {
            journal.checkpoint_output("dedupe_representatives", cluster.representative, emitted)?;
        }
    }
    let code = u8::from(emitted == 0);
    if let Some(journal) = &mut journal {
        journal.finish(emitted, code)?;
    }
    Ok(code)
}
fn validate(options: &Dedupe) -> Result<()> {
    let args = &options.options;
    if args.split
        || args.combine.is_some()
        || args.overlap.is_some()
        || args.max_chunks.is_some()
        || args.focus.is_some()
        || args.truncate.is_some()
        || args.truncate_field.is_some()
        || args.keep_going
        || args.on_error.is_some()
        || args.uncertain.is_some()
        || args
            .pack
            .is_some_and(|mode| mode != loupe_core::pack::Mode::Unit)
    {
        return Err(usage(
            "finite dedupe requires whole pair decisions; split/focus/truncate/pack/keep-going/uncertain-file flags are inapplicable",
        ));
    }
    if args.dry_run && (args.failed.is_some() || args.resume.is_some()) {
        return Err(usage("dedupe --dry-run does not use recovery journals"));
    }
    Ok(())
}
pub(crate) fn reader(args: &crate::args::Classify, prepared: &Prepared) -> Result<Options> {
    let mut reader = Options::from_document(&prepared.document, Format::Lines)?;
    if let Some(input) = args.input_format {
        reader.format = match input {
            Input::Lines => Format::Lines,
            Input::Whole => Format::Whole,
            Input::Null => Format::Null,
            Input::Paragraphs => Format::Paragraphs,
            Input::Records => Format::Records,
            Input::Jsonl => Format::Jsonl,
            Input::Csv => Format::Csv,
            Input::Tsv => Format::Tsv,
        };
    }
    if args.files_mode {
        if args
            .input_format
            .is_some_and(|input| !matches!(input, Input::Whole))
            || prepared.files.is_empty()
            || prepared.files.iter().any(|file| file == "-")
        {
            return Err(usage(
                "dedupe --files needs named file operands and whole-file input",
            ));
        }
        reader.format = Format::Files;
    }
    reader.keep_blank |= args.keep_blank;
    if args.records.is_some() {
        reader.records = args.records.clone();
    }
    if args.field.is_some() {
        reader.field = args.field.clone();
    }
    if let Some(select) = &args.select {
        reader.select = select
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect();
    }
    if let Some(context) = args.with_context {
        reader.with_context = context;
    }
    reader.validate()?;
    Ok(reader)
}
fn pair_admission(ordinal: u64, left: u64, right: u64) -> Result<Unit> {
    let text = format!("{left}:{right}");
    let mut raw = text.as_bytes().to_vec();
    raw.push(b'\n');
    Ok(Unit {
        index: ordinal,
        text: text.clone(),
        raw,
        subject: json!([left, right]),
        state: json!({"left":left,"right":right}),
        header: None,
        location: Location {
            source: None,
            byte_start: 0,
            byte_end: u64::try_from(text.len()).map_err(|_| usage("dedupe pair ID overflow"))?,
            line_start: ordinal,
            line_end: ordinal,
        },
    })
}
struct Rendering<'a> {
    units: &'a BTreeMap<u64, &'a Unit>,
    links: &'a [Link],
    output: Output,
    options: &'a Dedupe,
    header: Option<Vec<u8>>,
    resumed: bool,
}
fn emit(writer: &mut impl Write, cluster: &Cluster, rendering: &mut Rendering<'_>) -> Result<()> {
    let Rendering {
        units,
        links,
        output,
        options,
        header,
        resumed,
    } = rendering;
    let unit = units
        .get(&cluster.representative)
        .ok_or_else(|| usage("dedupe representative missing"))?;
    match *output {
        Output::Json => {
            let members=cluster.members.iter().map(|index|units.get(index).map(|unit|json!({"index":unit.index,"unit":unit.subject,"location":unit.location})).ok_or_else(||usage("dedupe member missing"))).collect::<Result<Vec<_>>>()?;
            let witness = links
                .iter()
                .filter(|link| cluster.members.binary_search(&link.left).is_ok())
                .collect::<Vec<_>>();
            write_json(
                writer,
                &json!({"index":unit.index,"unit":unit.subject,"cluster":cluster.id,"keep":options.keep,"members":members,"links":witness,"location":unit.location}),
            )
        }
        Output::Tsv => writeln!(
            writer,
            "{}\t{}\t{}\t{}",
            unit.index,
            cluster.members.len(),
            cluster
                .members
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(","),
            escape(&unit.text)
        )
        .map_err(output_error),
        Output::Plain | Output::Annotate => {
            if !*resumed
                && header.is_none()
                && let Some(value) = &unit.header
            {
                writer.write_all(value).map_err(output_error)?;
                *header = Some(value.clone());
            }
            if *output == Output::Annotate {
                write!(
                    writer,
                    "cluster={} members={}\t",
                    cluster.id,
                    cluster.members.len()
                )
                .map_err(output_error)?;
            }
            writer.write_all(&unit.raw).map_err(output_error)
        }
        Output::Csv | Output::Parquet => Err(usage("CSV/Parquet output requires featurize")),
    }
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
