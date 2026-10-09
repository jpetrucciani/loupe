use crate::args::{Cli, On, Output};
use crate::run::{
    Kind, duration, interrupt_signal, json_error, output_error, prepare, read_units, write_json,
};
use loupe_core::backend::{Backend, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::config::Profile;
use loupe_core::encoder;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::pack;
use loupe_core::policy::{Selected, UncertainAction};
use loupe_core::preset_store::{Preset, Store};
use loupe_core::records::Format;
use loupe_core::trigger::{self, Appends, Target};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use tokio::time::Instant;

pub(crate) async fn execute(
    cli: &Cli,
    options: &On,
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
        .map(|span| {
            started
                .checked_add(span)
                .ok_or_else(|| usage("on deadline outside clock range"))
        })
        .transpose()?;
    tokio::select! {biased;
        signal=interrupt_signal()=>signal,
        _=async{if let Some(deadline)=deadline {tokio::time::sleep_until(deadline).await;}else{std::future::pending::<()>().await;}}=>Err(Error::new(ErrorKind::Unavailable,"overall on deadline expired")),
        result=run(cli,options,store,profile,fallback_settings,deadline)=>result,
    }
}
async fn run(
    cli: &Cli,
    options: &On,
    store: &Store,
    mut profile: Profile,
    fallback_settings: &loupe_core::fallback::Settings,
    deadline: Option<Instant>,
) -> Result<u8> {
    let args = &options.options;
    if args.split
        || args.combine.is_some()
        || args.overlap.is_some()
        || args.max_chunks.is_some()
        || args.focus.is_some()
        || args.truncate.is_some()
        || args.truncate_field.is_some()
        || args.pack.is_some_and(|mode| mode != pack::Mode::Unit)
        || args.keep_going
        || args.on_error.is_some()
        || args.failed.is_some()
        || args.resume.is_some()
        || args.uncertain.is_some()
    {
        return Err(usage(
            "on uses whole per-unit decisions; packing/splitting/truncation/keep-going/filter recovery flags are inapplicable",
        ));
    }
    if args
        .output_format
        .is_some_and(|format| !matches!(format, Output::Plain | Output::Json))
    {
        return Err(usage("on output is action logs; use plain or JSON"));
    }
    let timeout = duration(&options.exec_timeout)?;
    if timeout.is_zero() {
        return Err(usage("--exec-timeout must be positive"));
    }
    let window = options.dedupe_window.as_deref().map(duration).transpose()?;
    if window.is_some_and(|span| span.is_zero()) {
        return Err(usage("--dedupe-window must be positive"));
    }
    if window.is_none() && options.max_fired.is_some() {
        return Err(usage("--max-fired requires --dedupe-window"));
    }
    let prepared = prepare(Kind::On, args, store, &profile.limits, &profile.model)?;
    let fallback =
        crate::run::fallback::Plan::resolve(Kind::On, args, &prepared, fallback_settings)?;
    if loupe_core::calibration_pin::validate(&prepared.document)?.is_some() {
        return Err(usage(
            "on requires trigger-specific calibration; classification pins are inapplicable",
        ));
    }
    if let Some(policy) = prepared.document.get("policy")
        && (policy.get("splittable") == Some(&Value::Bool(true))
            || policy.get("combine").is_some()
            || policy
                .get("pack")
                .is_some_and(|mode| mode.as_str() != Some("unit")))
    {
        return Err(usage("on preset policies require whole unit decisions"));
    }
    let (primary, question) = prepared
        .questions
        .first()
        .ok_or_else(|| usage("on primary question missing"))?;
    let actions = trigger::plan(&options.ordered, question)?;
    let reader = crate::run::dedupe::reader(args, &prepared)?;
    let mut protected = Vec::new();
    for path in prepared
        .files
        .iter()
        .filter(|file| file.as_str() != "-")
        .map(Path::new)
        .chain([Path::new("/dev/fd/0"), Path::new("/dev/fd/1")])
    {
        match std::fs::metadata(path) {
            Ok(meta) => protected.push((meta.dev(), meta.ino())),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && matches!(path.to_str(), Some("/dev/fd/0" | "/dev/fd/1")) => {}
            Err(error) => return Err(Error::io("checking action aliases", error)),
        }
    }
    let capacity = options.max_fired.map_or(65536, |count| count.get());
    let state = if let Some(path) = &options.state
        && !args.dry_run
    {
        let clock = Clock::new()?;
        let window_ns = window
            .map(|span| {
                u64::try_from(span.as_nanos())
                    .map_err(|_| usage("dedupe window outside durable clock range"))
            })
            .transpose()?;
        let mut identity = json!({
            "version":1,"cwd":std::env::current_dir().map_err(|error| Error::io("reading on working directory",error))?,
            "document":prepared.document,"questions":prepared.questions,"question_order":prepared.questions.keys().collect::<Vec<_>>(),
            "reader":reader,"actions":actions,
            "policy":{"threshold":prepared.policy.threshold,"margin":prepared.policy.margin,"min_confidence":prepared.policy.min_confidence,"min_gap":prepared.policy.min_gap,"uncertain":format!("{:?}",prepared.uncertain)},
            "backend":{"url":profile.base_url,"path":profile.path,"model":profile.model,"encoder":profile.encoder,"tokenizer":profile.tokenizer,"revision":profile.tokenizer_revision}
        });
        if let Some(fallback) = &fallback {
            identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
        }
        if args.ensemble {
            identity["ensemble"] = json!({"enabled":true,"min_agreement":prepared.min_agreement});
        }
        if args.review.is_some() {
            identity["review"] = json!({"mode":args.review,"queue":args.review_queue,"labels":args.review_labels,"audit_rate":crate::run::review::rate(args)?});
        }
        let state = loupe_core::trigger_state::Store::open(
            path,
            &identity,
            window_ns,
            window
                .map(|_| {
                    u64::try_from(capacity).map_err(|_| usage("claim capacity outside state range"))
                })
                .transpose()?,
            clock.now()?,
            &protected,
        )?;
        protected.push(state.inode());
        Some((state, clock))
    } else {
        None
    };
    let mut appends = if args.dry_run {
        None
    } else {
        Some(Appends::open(&actions, &protected)?)
    };
    let budget = Budget::new(args.budget);
    let mut summary = crate::summary::Summary::new(
        args.summary,
        args.dry_run,
        budget.clone(),
        profile.pricing.clone(),
    );
    let mut units = read_units(prepared.files.clone(), reader.clone(), 1, None, None)?;
    let mut encoder = None;
    let mut loaded = false;
    let mut backend = None;
    let mut review = None;
    let mut fired = BTreeMap::<([u8; 32], usize), Instant>::new();
    let mut total_actions = 0u64;
    let mut action_failures = false;
    let mut stdout = io::stdout().lock();
    let mut header = None;
    while let Some(unit) = units.recv().await {
        let unit = unit?;
        summary.unit()?;
        if let Some(current) = &unit.header {
            if header
                .as_ref()
                .is_some_and(|previous: &Vec<u8>| previous != current)
            {
                return Err(Error::new(
                    ErrorKind::Input,
                    "on CSV/TSV inputs require identical headers",
                ));
            }
            header = Some(current.clone());
        }
        if !loaded {
            let profile = profile.clone();
            encoder = tokio::task::spawn_blocking(move || encoder::load_profile(&profile))
                .await
                .map_err(|error| {
                    Error::new(ErrorKind::Io, format!("on tokenizer task stopped: {error}"))
                })??
                .map(Arc::new);
            loaded = true;
        }
        if args.review.is_some() && review.is_none() {
            review = crate::run::review::Plan::new(
                args,
                &store.project_root,
                &prepared.files,
                encoder
                    .as_ref()
                    .map_or("estimate-v1", |encoder| encoder.identity.as_str()),
                None,
                cli.quiet,
            )?;
        }
        if !args.dry_run && backend.is_none() {
            let next = Backend::new(&profile)
                .await?
                .with_request_trace(cli.verbose > 1);
            let discovery = with_deadline(deadline, next.discover(&profile.limits)).await?;
            profile.limits = discovery.limits.clone();
            if prepared
                .document
                .get("policy")
                .and_then(|policy| policy.get("model_identity"))
                .is_some()
            {
                let preset = Preset {
                    name: "effective-on".into(),
                    document: prepared.document.clone(),
                    origin: loupe_core::preset_store::Origin::Builtin,
                    trusted: true,
                    content_hash: String::new(),
                };
                preset.check_policy_identity(discovery.full_model_identity.as_deref())?;
            }
            let mode = if args.no_cache {
                CacheMode::Disabled
            } else if args.refresh {
                CacheMode::Refresh
            } else {
                CacheMode::Normal
            };
            let (next, warning) = with_deadline(
                deadline,
                next.with_cache(&discovery, encoder.clone(), &profile.cache_dir, mode),
            )
            .await?;
            if let Some(warning) = warning
                && !cli.quiet
            {
                eprintln!("loupe: {warning}");
            }
            backend = Some(next);
        }
        let request = loupe_core::wire::Request {
            model: profile.model.clone(),
            state: unit.state.clone(),
            questions: prepared.questions.clone(),
        };
        profile.limits.validate_request(&request)?;
        let counts = pack::count(encoder.as_deref(), &request)?;
        counts.preflight(&profile.limits)?;
        if args.dry_run {
            let mut plan = json!({"index":unit.index,"unit":unit.subject,"request":request,"counts":counts,"question_order":request.questions.keys().collect::<Vec<_>>(),"actions":actions});
            if args.ensemble {
                plan["ensemble"] = json!({"method":"equal_mean_probabilities","independent":false,"min_agreement":prepared.min_agreement});
            }
            write_json(&mut stdout, &plan)?;
            stdout.flush().map_err(output_error)?;
            continue;
        }
        let evaluation = backend
            .as_ref()
            .ok_or_else(|| usage("on backend missing"))?
            .evaluate(&request, &counts, &budget, args.strict, deadline)
            .await?;
        if let Some(warning) = &evaluation.warning
            && !cli.quiet
        {
            eprintln!("loupe: unit {}: {warning}", unit.index);
        }
        let raw_assessment = evaluation
            .response
            .validate(&request)?
            .shift_remove(primary)
            .ok_or_else(|| usage("on answer missing"))?
            .assessment(&prepared.policy)?;
        let mut assessment = raw_assessment.clone();
        let ensemble = if prepared.ensembles.has_primary(primary) {
            Some(loupe_core::fallback::Ensemble {
                set: &prepared.ensembles,
                primary,
                min_agreement: prepared.min_agreement,
            })
        } else {
            None
        };
        let ensemble_report = if let Some(ensemble) = &ensemble {
            let report = ensemble.set.report(
                primary,
                primary,
                &request,
                &evaluation.response,
                prepared.policy,
                ensemble.min_agreement,
            )?;
            assessment = report.assessment.clone();
            Some(serde_json::to_value(report).map_err(json_error)?)
        } else {
            None
        };
        let fallback_result = if let Some(fallback) = &fallback {
            let input = loupe_core::fallback::Input {
                unit: &unit,
                request: &request,
                response: &evaluation.response,
                field: primary,
                policy: prepared.policy,
            };
            let result = fallback
                .execute(
                    &input,
                    &mut assessment,
                    None,
                    deadline,
                    &mut summary,
                    ensemble.as_ref(),
                )
                .await?;
            fallback.require_success(result.as_deref())?;
            result
        } else {
            None
        };
        let reviewed = if let Some(review) = &mut review {
            let completed = crate::run::Completed {
                unit: unit.clone(),
                request: Arc::new(request.clone()),
                counts: Arc::new(counts.clone()),
                evaluation: Some(Arc::new(evaluation.clone())),
                answer_id: primary.clone(),
                packed_indices: None,
                split_plan: None,
                view: None,
                focus_plan: None,
                fallback: None,
                ensemble: None,
                review: None,
            };
            Some(review.assess(&completed, &mut assessment).await?)
        } else {
            None
        };
        if assessment.uncertain {
            summary.uncertain()?;
            if prepared.uncertain == UncertainAction::Fail {
                return Err(Error::new(ErrorKind::Uncertain, "on decision is uncertain"));
            }
        }
        let label = if assessment.uncertain && prepared.uncertain != UncertainAction::Keep {
            "uncertain".to_owned()
        } else {
            match &assessment.selected {
                Selected::Boolean(true) => "yes".into(),
                Selected::Boolean(false) => "no".into(),
                Selected::Choice(label) => label.clone(),
                Selected::Score(_) => return Err(usage("on does not accept Score decisions")),
            }
        };
        let signals = if args.why {
            serde_json::to_value(prepared.signals.reports(
                &request,
                &evaluation.response,
                primary,
                primary,
            )?)
            .map_err(json_error)?
        } else {
            Value::Null
        };
        if !cli.quiet
            && (args.why || assessment.uncertain && prepared.uncertain == UncertainAction::Stderr)
        {
            eprintln!("loupe: unit {} label={label} signals={signals}", unit.index);
        }
        if window.is_some() {
            let now = Instant::now();
            fired.retain(|_, until| *until > now);
        }
        let content = if reader.with_context != 0 {
            unit.state.get("unit").unwrap_or(&unit.state)
        } else if reader.format == Format::Files {
            unit.state.get("content").unwrap_or(&unit.state)
        } else {
            &unit.state
        };
        let digest = *blake3::hash(encoder::render(content)?.as_bytes()).as_bytes();
        let mut outcomes = Vec::new();
        for action in &actions {
            if !action.matches(&assessment, prepared.uncertain) {
                continue;
            }
            let key = (digest, action.id);
            let suppressed = if let Some((state, clock)) = &state {
                !state.claim(&digest, action.id, clock.now()?)?
            } else {
                fired.contains_key(&key)
            };
            if suppressed {
                outcomes.push(json!({"id":action.id,"suppressed":true}));
                continue;
            }
            if let Some(span) = window
                && state.is_none()
            {
                if fired.len() >= capacity {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "on live dedupe claim capacity exhausted",
                    ));
                }
                let until = Instant::now()
                    .checked_add(span)
                    .ok_or_else(|| usage("dedupe expiry outside clock range"))?;
                fired.insert(key, until);
            }
            total_actions = total_actions
                .checked_add(1)
                .ok_or_else(|| usage("on action count overflow"))?;
            let outcome = match &action.target {
                Target::Append(path) => match appends
                    .as_mut()
                    .ok_or_else(|| usage("append state missing"))?
                    .write(path, &unit)
                {
                    Ok(()) => json!({"id":action.id,"kind":"append","success":true}),
                    Err(error) => {
                        action_failures = true;
                        json!({"id":action.id,"kind":"append","error":error.to_string()})
                    }
                },
                Target::Exec(command) => {
                    let expires = Instant::now()
                        .checked_add(timeout)
                        .ok_or_else(|| usage("action timeout outside clock range"))?;
                    let expires = deadline.map_or(expires, |deadline| deadline.min(expires));
                    let mut environment =
                        crate::run::fallback::action_environment(fallback_result.as_deref());
                    if crate::run::review::human(reviewed.as_ref())?.is_some() {
                        environment.insert("LOUPE_DECISION_SOURCE".into(), "review".into());
                        environment.insert("LOUPE_CONFIDENCE".into(), String::new());
                    }
                    if ensemble_report.is_some() && environment.is_empty() {
                        environment.insert("LOUPE_DECISION_SOURCE".into(), "ensemble".into());
                    }
                    match trigger::execute_with_env(
                        command,
                        &unit,
                        &label,
                        &assessment,
                        expires,
                        &environment,
                    )
                    .await
                    {
                        Ok(result) => {
                            action_failures |= result.failed();
                            if !cli.quiet {
                                for capture in [&result.stdout, &result.stderr] {
                                    if !capture.bytes.is_empty() {
                                        eprint!("{}", String::from_utf8_lossy(&capture.bytes));
                                    }
                                }
                            }
                            json!({"id":action.id,"kind":"exec","result":result})
                        }
                        Err(error) => {
                            action_failures = true;
                            json!({"id":action.id,"kind":"exec","error":error.to_string()})
                        }
                    }
                }
            };
            if !cli.quiet {
                eprintln!("loupe: action unit={} outcome={outcome}", unit.index);
            }
            outcomes.push(outcome);
        }
        if args.json || args.output_format == Some(Output::Json) {
            let mut value = json!({"index":unit.index,"unit":unit.subject,"answer":evaluation.response.answers.get(primary),"model":evaluation.response.model,"cached":evaluation.cached,"label":label,"assessment":raw_assessment,"actions":outcomes,"signals":signals,"request":request,"counts":counts,"response":evaluation.response,"location":unit.location});
            if let Some(report) = &ensemble_report {
                value["ensemble"] = report.clone();
                value["decision"] = json!({"selected":assessment.selected,"source":"ensemble","confidence":assessment.confidence,"uncertain":assessment.uncertain});
            }
            if let Some(fallback) = &fallback_result {
                value["decision"] = loupe_core::fallback::decision(fallback);
                value["fallback"] = serde_json::to_value(fallback.as_ref()).map_err(json_error)?;
            }
            if let Some(review) = &reviewed
                && !review["task"].is_null()
            {
                value["review"] = review.clone();
                if crate::run::review::human(Some(review))?.is_some() {
                    value["decision"] = json!({"selected":assessment.selected,"source":"review","confidence":null,"uncertain":false});
                }
            }
            write_json(&mut stdout, &value)?;
            stdout.flush().map_err(output_error)?;
        }
    }
    Ok(if action_failures {
        69
    } else if total_actions == 0 && !args.dry_run {
        1
    } else {
        0
    })
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

struct Clock {
    wall_ns: u64,
    anchor: Instant,
}
impl Clock {
    fn new() -> Result<Self> {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| usage("on state clock precedes Unix epoch"))?;
        Ok(Self {
            wall_ns: u64::try_from(elapsed.as_nanos())
                .map_err(|_| usage("on state clock outside range"))?,
            anchor: Instant::now(),
        })
    }
    fn now(&self) -> Result<u64> {
        let elapsed = u64::try_from(self.anchor.elapsed().as_nanos())
            .map_err(|_| usage("on clock elapsed outside range"))?;
        self.wall_ns
            .checked_add(elapsed)
            .ok_or_else(|| usage("on state clock overflow"))
    }
}
