use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use indexmap::IndexMap;
use loupe_core::backend::{Backend, Evaluation, with_deadline};
use loupe_core::blocking::{Candidate, Fields, Index, Selection};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::encoder::{self, ClefEncoder};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::matching::{Assignment, Decision, GATE_ID, Plan, SCORE_ID, Template};
use loupe_core::policy::{Selected, UncertainAction};
use loupe_core::preset_store::Store;
use loupe_core::records::{Format, Options, Source};
use loupe_core::recovery::Disposition;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::args::{Cli, Input, Join, OnError, Output, Uncertain, Unmatched};
use crate::run::fallback::Plan as Fallback;
use crate::run::matching::journal::{Journal, Pair, Snapshot};
use crate::run::recovery::{Fingerprint, HashReader};
use crate::run::{duration, interrupt_signal, json_error, output_error, write_json};

mod journal;
pub(crate) mod rubric;

enum InputItem {
    Unit(Box<Unit>),
    End(Value),
}
struct PairResult {
    candidate: Candidate,
    plan: Plan,
    evaluation: Evaluation,
    decision: Decision,
    fallbacks: IndexMap<String, loupe_core::fallback::Execution>,
}
struct Row {
    pairs: Vec<PairResult>,
    selected: Vec<usize>,
    eligible: usize,
    unavailable: usize,
}
struct Execution<'a> {
    options: &'a Join,
    template: &'a Template,
    backend: Option<Backend>,
    encoder: Option<Arc<ClefEncoder>>,
    limits: loupe_core::config::Limits,
    budget: Budget,
    concurrency: usize,
    deadline: Option<Instant>,
    uncertain: UncertainAction,
    fallback: Option<Fallback>,
    quiet: bool,
}
struct Progress<'a> {
    journal: &'a mut Option<Journal>,
    snapshot: &'a mut Snapshot,
    summary: &'a mut crate::summary::Summary,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Join,
    store: &Store,
    profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    started: Instant,
) -> Result<u8> {
    let deadline = options
        .deadline
        .as_deref()
        .map(duration)
        .transpose()?
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| Error::new(ErrorKind::Usage, "join deadline is outside clock range"))
        })
        .transpose()?;
    tokio::select! {
        result=run(cli,options,store,profile,fallback_settings,deadline)=>result,
        signal=interrupt_signal()=>signal,
        _=async{if let Some(deadline)=deadline {tokio::time::sleep_until(deadline).await;}else{std::future::pending::<()>().await;}}=>Err(Error::new(ErrorKind::Unavailable,"overall join deadline expired")),
    }
}
async fn run(
    cli: &Cli,
    options: &Join,
    store: &Store,
    mut profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    deadline: Option<Instant>,
) -> Result<u8> {
    if options.left == "-" && options.right == "-" {
        return Err(usage("join can read stdin on only one side"));
    }
    if options.dry_run
        && (options.failed.is_some() || options.resume.is_some() || options.keep_going)
    {
        return Err(usage(
            "join --dry-run does not use journals or --keep-going",
        ));
    }
    let output = options.output_format.unwrap_or(Output::Json);
    if !matches!(output, Output::Json | Output::Tsv) {
        return Err(usage("join output must be json or tsv"));
    }
    let format = match options.input_format {
        Input::Jsonl => Format::Jsonl,
        Input::Csv => Format::Csv,
        Input::Tsv => Format::Tsv,
        _ => return Err(usage("join requires JSONL, CSV or TSV records")),
    };
    let fields = Fields::parse(&options.on)?;
    let reader = Options::new(format);
    let (template, document) = rubric::prepare(options, store, &profile, fields.names())?;
    let (uncertain, fallback_enabled) = match options.on_uncertain {
        Some(Uncertain::Keep) => (UncertainAction::Keep, false),
        Some(Uncertain::Drop) => (UncertainAction::Drop, false),
        Some(Uncertain::Fail) => (UncertainAction::Fail, false),
        Some(Uncertain::Stderr) => (UncertainAction::Stderr, false),
        Some(Uncertain::Fallback) => (UncertainAction::Drop, true),
        None => match document
            .get("policy")
            .and_then(|policy| policy.get("on_uncertain"))
            .and_then(Value::as_str)
        {
            Some("keep") => (UncertainAction::Keep, false),
            Some("fail") => (UncertainAction::Fail, false),
            Some("stderr") => (UncertainAction::Stderr, false),
            Some("drop") | None => (UncertainAction::Drop, false),
            Some("fallback") => (UncertainAction::Drop, true),
            _ => return Err(usage("invalid pair uncertainty action")),
        },
    };
    let fallback = Fallback::configure(
        fallback_enabled,
        options.fallback.as_deref(),
        options.fallback_timeout.as_deref(),
        options.on_fallback_error,
        fallback_settings,
    )?;
    let encoder = encoder::load_profile(&profile)?.map(Arc::new);
    let budget = Budget::new(options.budget);
    let mut summary = crate::summary::Summary::new(
        options.summary,
        options.dry_run,
        budget.clone(),
        profile.pricing.clone(),
    );
    let right_path = options.right.clone();
    let right_reader = reader.clone();
    let right_fields = fields.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let loaded = open(&right_path)
            .and_then(|input| Index::load(input, right_reader, source(&right_path), right_fields));
        let _ = sender.send(loaded);
    });
    let mut right = receiver
        .await
        .map_err(|_| Error::new(ErrorKind::Io, "right index reader stopped"))??;
    let stats = right.stats()?;
    if !cli.quiet {
        eprintln!(
            "loupe: right records={} token_postings={} band_postings={} blocking={:?} max_candidates={}",
            stats.records,
            stats.token_postings,
            stats.band_postings,
            options.blocking,
            options.max_candidates
        );
    }
    let (backend, model_identity) = if options.dry_run {
        (None, None)
    } else {
        let backend = Backend::new(&profile)
            .await?
            .with_request_trace(cli.verbose > 1);
        let discovery = backend.discover(&profile.limits).await?;
        if document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .is_some()
        {
            let preset = loupe_core::preset_store::Preset {
                name: "effective-join".into(),
                document: document.clone(),
                origin: loupe_core::preset_store::Origin::Builtin,
                trusted: true,
                content_hash: String::new(),
            };
            preset.check_policy_identity(discovery.full_model_identity.as_deref())?;
        }
        profile.limits = discovery.limits.clone();
        let mode = if options.no_cache {
            CacheMode::Disabled
        } else if options.refresh {
            CacheMode::Refresh
        } else {
            CacheMode::Normal
        };
        let model_identity = discovery.full_model_identity.clone();
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
        (Some(backend), model_identity)
    };
    let execution = Execution {
        options,
        template: &template,
        backend,
        encoder: encoder.clone(),
        limits: profile.limits.clone(),
        budget,
        concurrency: profile.concurrency,
        deadline,
        uncertain,
        fallback,
        quiet: cli.quiet,
    };
    let mut identity = json!({"version":1,"operation":"join","right_identity":right.identity(),"left":options.left,"right":options.right,"reader":reader,"fields":fields.names(),"blocking":options.blocking,"max_candidates":options.max_candidates.get(),"cardinality":options.cardinality,"unmatched":format!("{:?}",options.unmatched),"output":format!("{output:?}"),"document":document,"questions":template.questions().iter().collect::<Vec<_>>(),"policies":template.policies().iter().map(|(id,policy)|json!({"id":id,"threshold":policy.threshold.get(),"margin":policy.margin,"min_confidence":policy.min_confidence.get(),"min_gap":policy.min_gap})).collect::<Vec<_>>(),"on_uncertain":format!("{uncertain:?}"),"why":options.why,"strict":options.strict,"errors":{"keep_going":options.keep_going,"action":format!("{:?}",options.on_error.unwrap_or_default())},"model":profile.model,"model_identity":model_identity,"encoder":encoder.as_ref().map_or("estimate-v1",|encoder|encoder.identity.as_str()),"limits":profile.limits,"endpoint":format!("{}{}",profile.base_url,profile.path)});
    if let Some(fallback) = &execution.fallback {
        identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
    }
    if let Some(group) = template.ensemble() {
        identity["ensemble"] = json!({"enabled":true,"min_agreement":group.min_agreement});
    }
    let mut journal = Journal::open(options, identity)?;
    let mut assignment =
        Assignment::new(options.cardinality, right.units().map(|unit| unit.index))?;
    let mut input = read_left(options.left.clone(), reader)?;
    let mut end = None;
    if let Some(journal) = &mut journal
        && options.resume.is_some()
    {
        let mut last = 0;
        while last < journal.original_last {
            match input.recv().await.ok_or_else(|| {
                Error::new(
                    ErrorKind::Input,
                    "join resume input ended before saved prefix",
                )
            })?? {
                InputItem::Unit(unit) => {
                    journal.verify(&unit)?;
                    last = unit.index;
                }
                InputItem::End(_) => {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "join resume input ended before saved prefix",
                    ));
                }
            }
        }
        if let Some(original_end) = &journal.original_end {
            match input.recv().await.ok_or_else(|| {
                Error::new(ErrorKind::Input, "join resume source end is missing")
            })?? {
                InputItem::End(fingerprint) if fingerprint == *original_end => {
                    end = Some(fingerprint)
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "join resume source fingerprint changed",
                    ));
                }
            }
        }
    }
    let original_last = journal.as_ref().map_or(0, |journal| journal.original_last);
    let mut replay_cursor = 0;
    let mut stdout = io::stdout().lock();
    let mut emitted = 0u64;
    loop {
        let unit = if replay_cursor < original_last {
            let journal = journal
                .as_mut()
                .ok_or_else(|| usage("join replay journal missing"))?;
            let index = journal
                .store
                .next_index(replay_cursor)?
                .ok_or_else(|| usage("join replay prefix has a gap"))?;
            replay_cursor = index;
            Box::new(journal.store.saved(index)?.unit)
        } else if end.is_some() {
            break;
        } else {
            match input.recv().await {
                Some(Ok(InputItem::Unit(unit))) => unit,
                Some(Ok(InputItem::End(fingerprint))) => {
                    end = Some(fingerprint);
                    break;
                }
                Some(Err(error)) => {
                    if let Some(journal) = &mut journal {
                        journal.failed(None, &error)?;
                    }
                    stdout.flush().map_err(output_error)?;
                    return Err(error);
                }
                None => {
                    return Err(Error::new(
                        ErrorKind::Io,
                        "left input reader stopped before EOF",
                    ));
                }
            }
        };
        summary.unit()?;
        let (mut snapshot, handled) = match &mut journal {
            Some(journal) => {
                if unit.index > journal.original_last {
                    journal.store.admit(&unit)?;
                }
                journal.snapshot(unit.index)?
            }
            None => (Snapshot::default(), None),
        };
        if handled.is_some() && snapshot.error.is_some() {
            assignment.assign(unit.index, &[])?;
            emitted = emitted
                .checked_add(u64::from(handled == Some(Disposition::Emitted)))
                .ok_or_else(|| usage("join output count overflow"))?;
            continue;
        }
        let selected = right.select_available(
            &unit,
            options.max_candidates,
            options.blocking,
            assignment.excluded(),
        )?;
        if options.dry_run {
            let candidates=selected.candidates.iter().map(|candidate| {
                let right_unit=right.unit(candidate)?;
                let plan=template.plan(&unit,right_unit,encoder.as_deref(),&execution.limits)?;
                Ok(json!({"blocking":candidate,"right":{"index":right_unit.index,"unit":right_unit.subject,"location":right_unit.location},"pair_plan":plan,"question_order":plan.request().questions.keys().collect::<Vec<_>>()}))
            }).collect::<Result<Vec<_>>>()?;
            write_json(
                &mut stdout,
                &json!({"index":unit.index,"left":{"unit":unit.subject,"location":unit.location},"fields":fields.names(),"blocking_mode":options.blocking,"eligible_records":selected.eligible_records,"unavailable_records":selected.unavailable_records,"candidates":candidates,"right_index":stats}),
            )?;
            stdout.flush().map_err(output_error)?;
            emitted = emitted
                .checked_add(1)
                .ok_or_else(|| usage("join output count overflow"))?;
            continue;
        }
        let result = evaluate(
            &unit,
            &right,
            selected,
            &execution,
            Progress {
                journal: &mut journal,
                snapshot: &mut snapshot,
                summary: &mut summary,
            },
            &mut assignment,
        )
        .await;
        let row = match result {
            Ok(row) => row,
            Err(error) => {
                if error.kind == ErrorKind::Uncertain {
                    summary.uncertain()?;
                }
                summary.error()?;
                if let Some(journal) = &mut journal {
                    journal.failed(Some(unit.index), &error)?;
                }
                if !options.keep_going
                    || !matches!(error.kind, ErrorKind::Input | ErrorKind::Unavailable)
                    || options.on_error == Some(OnError::Fail)
                    || deadline.is_some_and(|deadline| Instant::now() >= deadline)
                {
                    stdout.flush().map_err(output_error)?;
                    return Err(error);
                }
                let keep = options.on_error == Some(OnError::Keep);
                if !cli.quiet {
                    eprintln!("loupe: left {}: {error}", unit.index);
                }
                assignment.assign(unit.index, &[])?;
                snapshot.complete = true;
                snapshot.error = Some((error.exit_code(), error.to_string()));
                snapshot.selected_rights.clear();
                if let Some(journal) = &mut journal {
                    journal.ready(unit.index, &snapshot)?;
                }
                if keep {
                    emit_error(&mut stdout, &unit, &error, output)?;
                    stdout.flush().map_err(output_error)?;
                    emitted = emitted
                        .checked_add(1)
                        .ok_or_else(|| usage("join output count overflow"))?;
                }
                if let Some(journal) = &mut journal {
                    journal.store.handled(
                        unit.index,
                        if keep {
                            Disposition::Emitted
                        } else {
                            Disposition::Omitted
                        },
                    )?;
                }
                continue;
            }
        };
        for pair in &row.pairs {
            if pair.decision.uncertain {
                summary.uncertain()?;
            }
            if !cli.quiet
                && (options.why || pair.decision.uncertain && uncertain == UncertainAction::Stderr)
            {
                let probability = pair
                    .evaluation
                    .response
                    .answers
                    .get(loupe_core::matching::GATE_ID)
                    .and_then(|answer| answer.get("noul"))
                    .and_then(Value::as_f64)
                    .ok_or_else(|| usage("join gate probability missing"))?;
                eprintln!(
                    "loupe: pair L{}:R{} score={:.6} gate={:.4} eligible={} uncertain={} companions={}",
                    unit.index,
                    pair.candidate.right_index,
                    pair.decision.ranking_score,
                    probability,
                    pair.decision.eligible,
                    pair.decision.uncertain,
                    serde_json::to_string(&pair.decision.companions).map_err(json_error)?
                );
            }
        }
        let deliver = !row.selected.is_empty()
            || matches!(options.unmatched, Some(Unmatched::Left | Unmatched::Both));
        if handled.is_some() {
            emitted = emitted
                .checked_add(u64::from(handled == Some(Disposition::Emitted)))
                .ok_or_else(|| usage("join output count overflow"))?;
            continue;
        }
        if deliver {
            emit_row(&mut stdout, &unit, &right, &row, options, output)?;
            stdout.flush().map_err(output_error)?;
            emitted = emitted
                .checked_add(1)
                .ok_or_else(|| usage("join output count overflow"))?;
        }
        if let Some(journal) = &mut journal {
            journal.store.handled(
                unit.index,
                if deliver {
                    Disposition::Emitted
                } else {
                    Disposition::Omitted
                },
            )?;
        }
    }
    if let Some(journal) = &mut journal {
        if journal.original_end.is_none() {
            journal.store.end_input(
                end.clone()
                    .ok_or_else(|| usage("join input fingerprint missing"))?,
            )?;
        }
        if let Some((_, code)) = journal.finished {
            return Ok(code);
        }
    }
    if matches!(options.unmatched, Some(Unmatched::Right | Unmatched::Both)) {
        let checkpoint = journal
            .as_ref()
            .and_then(|journal| journal.store.output_checkpoint("join_unmatched_right"));
        let last_right = checkpoint.map_or(0, |checkpoint| checkpoint.0);
        if let Some((_, count)) = checkpoint {
            if count < emitted {
                return Err(Error::new(
                    ErrorKind::Input,
                    "join unmatched-output checkpoint count is invalid",
                ));
            }
            emitted = count;
        }
        let unmatched = assignment.unmatched().collect::<BTreeSet<_>>();
        for unit in right
            .units()
            .filter(|unit| unit.index > last_right && unmatched.contains(&unit.index))
        {
            if output == Output::Json {
                write_json(
                    &mut stdout,
                    &json!({"kind":"unmatched_right","left":null,"right":{"index":unit.index,"unit":unit.subject,"location":unit.location}}),
                )?;
            } else {
                writeln!(
                    stdout,
                    "\t{}\t\t\t{}",
                    unit.index,
                    escape(&unit.subject.to_string())
                )
                .map_err(output_error)?;
            }
            stdout.flush().map_err(output_error)?;
            emitted = emitted
                .checked_add(1)
                .ok_or_else(|| usage("join output count overflow"))?;
            if let Some(journal) = &mut journal {
                journal
                    .store
                    .checkpoint_output("join_unmatched_right", unit.index, emitted)?;
            }
        }
    }
    let code = u8::from(emitted == 0);
    if let Some(journal) = &mut journal {
        journal.store.finish(emitted, code)?;
    }
    Ok(code)
}

async fn evaluate(
    unit: &Unit,
    right: &Index,
    selection: Selection,
    execution: &Execution<'_>,
    progress: Progress<'_>,
    assignment: &mut Assignment,
) -> Result<Row> {
    let Progress {
        journal,
        snapshot,
        summary,
    } = progress;
    let Selection {
        candidates,
        eligible_records: eligible,
        unavailable_records: unavailable,
    } = selection;
    let plans = candidates
        .into_iter()
        .map(|candidate| {
            let plan = execution.template.plan(
                unit,
                right.unit(&candidate)?,
                execution.encoder.as_deref(),
                &execution.limits,
            )?;
            Ok((candidate, plan))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut saved = snapshot
        .pairs
        .iter()
        .map(|pair| (pair.right_index, pair.clone()))
        .collect::<BTreeMap<_, _>>();
    if saved.len() != snapshot.pairs.len()
        || saved.keys().any(|index| {
            !plans
                .iter()
                .any(|(candidate, _)| candidate.right_index == *index)
        })
    {
        return Err(Error::new(
            ErrorKind::Input,
            "saved join pairs do not match current candidates",
        ));
    }
    let provenance=json!(plans.iter().map(|(_,plan)|json!({"plan":plan,"question_order":plan.request().questions.keys().collect::<Vec<_>>()})).collect::<Vec<_>>());
    if !snapshot.complete
        && let Some(journal) = journal
    {
        journal.progress(unit.index, provenance.clone(), snapshot)?;
    }
    {
        let missing = plans
            .iter()
            .filter(|(candidate, _)| !saved.contains_key(&candidate.right_index))
            .collect::<Vec<_>>();
        if snapshot.complete && !missing.is_empty() {
            return Err(Error::new(
                ErrorKind::Input,
                "complete join snapshot is missing pair responses",
            ));
        }
        let backend = execution
            .backend
            .as_ref()
            .ok_or_else(|| usage("join backend missing"))?;
        let results = futures::stream::iter(missing)
            .map(|(candidate, plan)| async move {
                (
                    candidate.right_index,
                    backend
                        .evaluate(
                            plan.request(),
                            plan.counts(),
                            &execution.budget,
                            execution.options.strict,
                            execution.deadline,
                        )
                        .await,
                )
            })
            .buffered(execution.concurrency);
        futures::pin_mut!(results);
        let mut failure = None;
        while let Some((right_index, result)) = results.next().await {
            match result {
                Ok(evaluation) => {
                    if let Some(warning) = &evaluation.warning
                        && !execution.quiet
                    {
                        eprintln!("loupe: pair L{}:R{right_index}: {warning}", unit.index);
                    }
                    let pair = Pair {
                        right_index,
                        evaluation: evaluation.clone(),
                        fallbacks: IndexMap::new(),
                    };
                    snapshot.pairs.push(pair.clone());
                    saved.insert(right_index, pair);
                    if let Some(journal) = journal {
                        journal.progress(unit.index, provenance.clone(), snapshot)?;
                    }
                }
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    }
    let mut pairs = Vec::with_capacity(plans.len());
    for (candidate, plan) in plans {
        let mut pair = saved
            .remove(&candidate.right_index)
            .ok_or_else(|| usage("join pair response missing"))?;
        let overrides = resolve_pair(
            unit,
            &plan,
            &mut pair,
            execution,
            &mut Progress {
                journal: &mut *journal,
                snapshot: &mut *snapshot,
                summary: &mut *summary,
            },
            &provenance,
        )
        .await?;
        let evaluation = pair.evaluation;
        let decision = match execution.template.assess_with_overrides(
            &plan,
            &evaluation.response,
            execution.uncertain,
            &overrides,
        ) {
            Ok(decision) => decision,
            Err(error) => {
                if error.kind == ErrorKind::Uncertain && execution.options.why && !execution.quiet {
                    eprintln!(
                        "loupe: pair L{}:R{} uncertain answers={}",
                        unit.index,
                        candidate.right_index,
                        serde_json::to_string(&evaluation.response.answers).map_err(json_error)?
                    );
                }
                return Err(error);
            }
        };
        decision.validate_plan(&plan)?;
        pairs.push(PairResult {
            candidate,
            plan,
            evaluation,
            decision,
            fallbacks: pair.fallbacks,
        });
    }
    let decisions = pairs
        .iter()
        .map(|pair| pair.decision.clone())
        .collect::<Vec<_>>();
    let selected = assignment.assign(unit.index, &decisions)?;
    let selected_rights = selected
        .iter()
        .map(|index| {
            pairs
                .get(*index)
                .map(|pair| pair.candidate.right_index)
                .ok_or_else(|| usage("join selected index missing"))
        })
        .collect::<Result<Vec<_>>>()?;
    if snapshot.complete && snapshot.selected_rights != selected_rights {
        return Err(Error::new(
            ErrorKind::Input,
            "saved join assignment changed",
        ));
    }
    if !snapshot.complete {
        snapshot.complete = true;
        snapshot.selected_rights = selected_rights;
        if let Some(journal) = journal {
            journal.ready(unit.index, snapshot)?;
        }
    }
    Ok(Row {
        pairs,
        selected,
        eligible,
        unavailable,
    })
}
async fn resolve_pair(
    unit: &Unit,
    plan: &Plan,
    pair: &mut Pair,
    execution: &Execution<'_>,
    progress: &mut Progress<'_>,
    provenance: &Value,
) -> Result<IndexMap<String, Selected>> {
    let Progress {
        journal,
        snapshot,
        summary,
    } = progress;
    let mut overrides = IndexMap::new();
    if let Some(fallback) = &execution.fallback {
        if pair
            .fallbacks
            .keys()
            .any(|id| id != GATE_ID && id != SCORE_ID)
        {
            return Err(Error::new(
                ErrorKind::Input,
                "saved join fallback field is invalid",
            ));
        }
        let raw =
            execution
                .template
                .assess(plan, &pair.evaluation.response, UncertainAction::Drop)?;
        let mut gate = raw.assessment(GATE_ID)?.clone();
        let mut score = raw.assessment(SCORE_ID)?.clone();
        let pair_unit = crate::run::fallback::request_unit(unit, plan.request())?;
        for field in [GATE_ID, SCORE_ID] {
            if field == SCORE_ID && (gate.uncertain || gate.selected != Selected::Boolean(true)) {
                if pair.fallbacks.contains_key(SCORE_ID) {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "saved score fallback lacks a resolved positive gate",
                    ));
                }
                break;
            }
            let assessment = if field == GATE_ID {
                &mut gate
            } else {
                &mut score
            };
            let previous = pair.fallbacks.get(field).cloned().map(Arc::new);
            let fresh = previous.is_none();
            if snapshot.complete && assessment.uncertain && previous.is_none() {
                return Err(Error::new(
                    ErrorKind::Input,
                    "complete join snapshot is missing a fallback result",
                ));
            }
            let policy = execution
                .template
                .policies()
                .get(field)
                .copied()
                .ok_or_else(|| usage("join fallback policy missing"))?;
            let input = loupe_core::fallback::Input {
                unit: &pair_unit,
                request: plan.request(),
                response: &pair.evaluation.response,
                field,
                policy,
            };
            let ensemble = execution
                .template
                .ensemble()
                .filter(|group| group.set.has_primary(field))
                .map(|group| loupe_core::fallback::Ensemble {
                    set: &group.set,
                    primary: field,
                    min_agreement: group.min_agreement,
                });
            let result = fallback
                .execute(
                    &input,
                    assessment,
                    previous,
                    execution.deadline,
                    summary,
                    ensemble.as_ref(),
                )
                .await?;
            if let Some(result) = result {
                if let loupe_core::fallback::Execution::Resolved { selected, .. } = result.as_ref()
                {
                    overrides.insert(field.to_owned(), selected.clone());
                }
                pair.fallbacks
                    .insert(field.to_owned(), result.as_ref().clone());
                if fresh {
                    let checkpoint = snapshot
                        .pairs
                        .iter_mut()
                        .find(|saved| saved.right_index == plan.right_index())
                        .ok_or_else(|| usage("join fallback checkpoint missing"))?;
                    checkpoint.fallbacks = pair.fallbacks.clone();
                    if let Some(journal) = journal {
                        journal.progress(unit.index, provenance.clone(), snapshot)?;
                    }
                }
                fallback.require_success(Some(result.as_ref()))?;
            }
        }
    } else if !pair.fallbacks.is_empty() {
        return Err(Error::new(
            ErrorKind::Input,
            "saved join fallback requires its pinned command",
        ));
    }
    Ok(overrides)
}
fn emit_row(
    writer: &mut impl Write,
    left: &Unit,
    right: &Index,
    row: &Row,
    options: &Join,
    output: Output,
) -> Result<()> {
    let matches=row.selected.iter().map(|index| {
        let pair=row.pairs.get(*index).ok_or_else(||usage("join selected pair missing"))?;
        let unit=right.unit(&pair.candidate)?;
        Ok(with_fallback(json!({"right":{"index":unit.index,"unit":unit.subject,"location":unit.location},"ranking_score":pair.decision.ranking_score,"decision":pair.decision,"model":pair.evaluation.response.model,"cached":pair.evaluation.cached}), pair))
    }).collect::<Result<Vec<_>>>()?;
    if output == Output::Json {
        let pairs=row.pairs.iter().map(|pair|with_fallback(json!({"id":format!("L{}:R{}",left.index,pair.candidate.right_index),"blocking":pair.candidate,"request":pair.plan.request(),"question_order":pair.plan.request().questions.keys().collect::<Vec<_>>(),"counts":pair.plan.counts(),"response":pair.evaluation.response,"cached":pair.evaluation.cached,"decision":pair.decision}), pair)).collect::<Vec<_>>();
        write_json(
            writer,
            &json!({"index":left.index,"kind":if matches.is_empty(){"unmatched_left"}else{"matched"},"left":{"unit":left.subject,"location":left.location},"matches":matches,"evaluated_pairs":pairs,"cardinality":options.cardinality,"blocking":{"eligible_records":row.eligible,"unavailable_records":row.unavailable}}),
        )
    } else if row.selected.is_empty() {
        writeln!(
            writer,
            "{}\t\t\t{}\t",
            left.index,
            escape(&left.subject.to_string())
        )
        .map_err(output_error)
    } else {
        for index in &row.selected {
            let pair = row
                .pairs
                .get(*index)
                .ok_or_else(|| usage("join selected pair missing"))?;
            writeln!(
                writer,
                "{}\t{}\t{:.6}\t{}\t{}",
                left.index,
                pair.candidate.right_index,
                pair.decision.ranking_score,
                escape(&left.subject.to_string()),
                escape(&right.unit(&pair.candidate)?.subject.to_string())
            )
            .map_err(output_error)?;
        }
        Ok(())
    }
}
fn with_fallback(mut value: Value, pair: &PairResult) -> Value {
    if !pair.fallbacks.is_empty() {
        value["fallback"] = Value::Object(pair.fallbacks.iter().map(|(field, execution)| {
            (field.clone(), json!({"execution":execution,"decision":loupe_core::fallback::decision(execution)}))
        }).collect());
    }
    value
}
fn emit_error(writer: &mut impl Write, left: &Unit, error: &Error, output: Output) -> Result<()> {
    if output == Output::Json {
        write_json(
            writer,
            &json!({"index":left.index,"kind":"error","left":{"unit":left.subject,"location":left.location},"matches":[],"evaluated_pairs":[],"error":{"code":error.exit_code(),"message":error.to_string()}}),
        )
    } else {
        writeln!(
            writer,
            "{}\terror\t\t{}\t{}",
            left.index,
            escape(&left.subject.to_string()),
            escape(&error.to_string())
        )
        .map_err(output_error)
    }
}
fn read_left(
    path: String,
    options: Options,
) -> Result<tokio::sync::mpsc::Receiver<Result<InputItem>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    std::thread::spawn(move || {
        let run = || -> Result<()> {
            let fingerprint = Fingerprint::default();
            let input = HashReader::new(open(&path)?, fingerprint.clone());
            let mut source = Source::new(input, options, source(&path))?;
            while let Some(unit) = source.next_unit()? {
                if sender
                    .blocking_send(Ok(InputItem::Unit(Box::new(unit))))
                    .is_err()
                {
                    return Ok(());
                }
            }
            let _ = sender.blocking_send(Ok(InputItem::End(
                fingerprint.value(&path, source.consumed_units())?,
            )));
            Ok(())
        };
        if let Err(error) = run() {
            let _ = sender.blocking_send(Err(error));
        }
    });
    Ok(receiver)
}
fn source(path: &str) -> Option<PathBuf> {
    (path != "-").then(|| PathBuf::from(path))
}
fn open(path: &str) -> Result<Box<dyn BufRead>> {
    if path == "-" {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        File::open(path)
            .map(|file| Box::new(BufReader::new(file)) as Box<dyn BufRead>)
            .map_err(|error| Error::io("opening join input", error))
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
