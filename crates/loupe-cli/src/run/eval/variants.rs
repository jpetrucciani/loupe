use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;

use loupe_core::budget::{Budget, Usage};
use loupe_core::config::{Config, Profile, Settings};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::pack::Mode as PackMode;
use loupe_core::preset_store::Store;
use serde::Serialize;
use tokio::time::Instant;

use crate::args::{Cli, Eval, EvalSplit};
use crate::run::eval::{Failure, RunReport, RunScope, output, run, usage};
use crate::run::{json_error, write_json};

mod compare;

enum Target {
    Ensemble(bool),
    Report(PathBuf),
    Profile(String),
    Preset(String),
}
impl Target {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "standalone" => return Ok(Self::Ensemble(false)),
            "ensemble" => return Ok(Self::Ensemble(true)),
            _ => {}
        }
        if let Some(name) = value.strip_prefix("profile:") {
            if name.trim().is_empty() {
                return Err(usage("comparison profile must be nonempty"));
            }
            return Ok(Self::Profile(name.to_owned()));
        }
        if let Some(name) = value.strip_prefix("preset:") {
            if name.trim().is_empty() {
                return Err(usage("comparison preset must be nonempty"));
            }
            return Ok(Self::Preset(name.to_owned()));
        }
        let path = value.strip_prefix("report:").unwrap_or(value);
        if path.is_empty() {
            return Err(usage("comparison report bundle path must be nonempty"));
        }
        Ok(Self::Report(PathBuf::from(path)))
    }
}
enum Prepared {
    Saved(Box<RunReport>),
    Live(Box<Eval>, Box<Profile>),
}
#[derive(Serialize)]
struct Variant {
    name: String,
    source: String,
    report: RunReport,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    generated_at: String,
    kind: String,
    status: String,
    token_usage: Usage,
    variants: Vec<Variant>,
    comparisons: Vec<compare::Report>,
    error: Option<Failure>,
    notes: Vec<String>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Eval,
    store: &Store,
    profile: Profile,
    config: &Config,
    budget: &Budget,
    deadline: Option<Instant>,
) -> Result<u8> {
    let comparing = !options.compare.is_empty();
    let same_backend = !comparing
        || options
            .compare
            .iter()
            .all(|target| matches!(target.as_str(), "standalone" | "ensemble"));
    let mut prepared = if comparing {
        prepare_comparison(cli, options, profile, config)?
    } else {
        prepare_sweep(options, profile)?
    };
    // Validate every live variant and its fixed cases before discovery or split publication.
    let mut plans = Vec::new();
    for (name, item) in &prepared {
        if let Prepared::Live(settings, profile) = item
            && crate::run::eval::load_suites(settings, store, profile)?
                .iter()
                .any(|suite| {
                    matches!(
                        suite.manifest.task,
                        loupe_core::suites::Task::Join | loupe_core::suites::Task::Retrieval
                    )
                })
        {
            return Err(usage(
                "selection/join comparisons and classification view sweeps are inapplicable",
            ));
        }
        let plan = match item {
            Prepared::Saved(report) => {
                compare::validate_saved(report, options.allow_unverified)?;
                None
            }
            Prepared::Live(settings, profile) => {
                let mut offline = settings.as_ref().clone();
                offline.dry_run = true;
                offline.allow_unverified = false;
                Some(
                    run(
                        cli,
                        &offline,
                        store,
                        profile.as_ref().clone(),
                        RunScope {
                            budget,
                            deadline,
                            initialize_splits: false,
                            recalibrate: false,
                            baseline: None,
                        },
                    )
                    .await?,
                )
            }
        };
        plans.push((name, plan));
    }
    compare::validate_inputs(&prepared, &plans)?;
    let mut report = Report {
        schema_version: 1,
        generated_at: loupe_core::report_store::Stamp::now().generated_at,
        kind: if comparing { "comparison" } else { "sweep" }.to_owned(),
        status: if options.dry_run { "plan" } else { "complete" }.to_owned(),
        token_usage: Usage::default(),
        variants: Vec::new(),
        comparisons: Vec::new(),
        error: None,
        notes: vec![
            "Each variant retains its own complete model/encoder/preset/packing identity and metrics.".to_owned(),
            "Submitted token usage covers this command only; saved reports retain historical usage separately.".to_owned(),
            "All live variants share one input budget and overall deadline. Latency and warmup bypass cache.".to_owned(),
        ],
    };
    if !comparing {
        report.notes.push("Sweeps use fixed calibration cases only. Observed comparisons do not ratify a held-out gate, baseline or preset policy.".to_owned());
    }
    drop(plans);
    for (name, item) in prepared.drain(..) {
        let (source, result) = match item {
            Prepared::Saved(saved) => ("saved-report", Ok(*saved)),
            Prepared::Live(settings, profile) => (
                "live",
                run(
                    cli,
                    &settings,
                    store,
                    *profile,
                    RunScope {
                        budget,
                        deadline,
                        initialize_splits: true,
                        recalibrate: false,
                        baseline: None,
                    },
                )
                .await,
            ),
        };
        match result {
            Ok(item) => {
                if item.status == "provisional" && report.status != "plan" {
                    report.status = "provisional".to_owned();
                }
                report.error = item.error.clone();
                report.variants.push(Variant {
                    name,
                    source: source.to_owned(),
                    report: item,
                });
            }
            Err(error) => {
                report.error = Some(Failure {
                    code: error.exit_code(),
                    message: error.to_string(),
                });
            }
        }
        if report.error.is_some() {
            report.status = "incomplete".to_owned();
            break;
        }
        if report.variants.len() > 1 {
            let first = report
                .variants
                .first()
                .ok_or_else(|| usage("missing first variant"))?;
            let current = report
                .variants
                .last()
                .ok_or_else(|| usage("missing current variant"))?;
            if same_backend
                && let Err(error) =
                    crate::run::eval::variants::same_backend(&first.report, &current.report)
            {
                report.status = "incomplete".to_owned();
                report.error = Some(Failure {
                    code: 69,
                    message: error.to_string(),
                });
                break;
            }
            if !options.dry_run {
                match compare::reports(&first.name, &first.report, &current.name, &current.report) {
                    Ok(comparison) => report.comparisons.push(comparison),
                    Err(error) => {
                        report.status = "incomplete".to_owned();
                        report.error = Some(Failure {
                            code: error.exit_code(),
                            message: error.to_string(),
                        });
                        break;
                    }
                }
            }
        }
    }
    report.token_usage = budget.usage()?;
    let code = report.error.as_ref().map_or(0, |error| error.code);
    let markdown = markdown(&report);
    if let Some(root) = &options.report {
        let store = loupe_core::report_store::Store::open(root)?;
        let mut json = serde_json::to_vec_pretty(&report).map_err(json_error)?;
        json.push(b'\n');
        let path = store.publish(&json, markdown.as_bytes())?;
        if !cli.quiet {
            eprintln!("loupe: reports published under {}", path.display());
        }
    }
    if options.json {
        write_json(&mut io::stdout().lock(), &report)?;
    } else {
        io::stdout()
            .lock()
            .write_all(markdown.as_bytes())
            .map_err(|error| Error::io("writing comparison/sweep report", error))?;
    }
    Ok(code)
}

fn prepare_comparison(
    cli: &Cli,
    options: &Eval,
    profile: Profile,
    config: &Config,
) -> Result<Vec<(String, Prepared)>> {
    if options.compare.len() != 2 {
        return Err(usage("--compare requires exactly two targets"));
    }
    let environment = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect::<HashMap<_, _>>();
    let overrides = Settings {
        base_url: cli.url.clone(),
        path: cli.path.clone(),
        api_format: cli.api_format,
        model: cli.model.clone(),
        concurrency: cli.concurrency,
        ..Settings::default()
    };
    let mut output = Vec::new();
    let base_preset = options.preset.clone().or_else(|| {
        options
            .compare
            .iter()
            .find_map(|target| target.strip_prefix("preset:").map(str::to_owned))
    });
    for name in &options.compare {
        let prepared = match Target::parse(name)? {
            Target::Ensemble(enabled) => {
                let mut settings = options.clone();
                settings.ensemble = enabled;
                if !enabled {
                    settings.min_agreement = None;
                }
                settings.preset = base_preset.clone();
                Prepared::Live(Box::new(settings), Box::new(profile.clone()))
            }
            Target::Report(path) => {
                if options.dry_run {
                    return Err(usage(
                        "saved report comparison is already offline; --dry-run applies to live targets only",
                    ));
                }
                let bytes = loupe_core::report_store::read(&path)?;
                let report: RunReport = serde_json::from_slice(&bytes).map_err(|error| {
                    Error::new(
                        ErrorKind::Input,
                        format!("invalid saved eval report: {error}"),
                    )
                })?;
                Prepared::Saved(Box::new(report))
            }
            Target::Profile(name) => {
                let mut settings = options.clone();
                settings.preset = base_preset.clone();
                Prepared::Live(
                    Box::new(settings),
                    Box::new(config.resolve(Some(&name), &overrides, &environment)?),
                )
            }
            Target::Preset(name) => {
                let mut settings = options.clone();
                settings.preset = base_preset.clone();
                settings.preset_override = Some(name);
                Prepared::Live(Box::new(settings), Box::new(profile.clone()))
            }
        };
        output.push((name.clone(), prepared));
    }
    if output
        .iter()
        .all(|(_, item)| matches!(item, Prepared::Saved(_)))
        && (options.dataset.is_some()
            || options.labels.is_some()
            || !options.suite.is_empty()
            || options.preset.is_some()
            || options.init_splits
            || options.partition.is_some()
            || options.pack.is_some()
            || options.question_id_source.is_some()
            || options.combine.is_some()
            || options.split_text
            || options.focus.is_some()
            || options.truncate.is_some()
            || options.truncate_field.is_some()
            || options.no_cache
            || options.refresh
            || options.threshold.is_some()
            || options.margin.is_some()
            || options.min_confidence.is_some()
            || options.min_gap.is_some()
            || !options.arg.is_empty()
            || options.overlap.is_some()
            || options.max_chunks.is_some())
    {
        return Err(usage(
            "saved report comparison does not accept live dataset, preset, policy or view overrides",
        ));
    }
    Ok(output)
}

fn prepare_sweep(options: &Eval, profile: Profile) -> Result<Vec<(String, Prepared)>> {
    Ok(sweep(options)?
        .into_iter()
        .map(|(name, settings)| {
            (
                name,
                Prepared::Live(Box::new(settings), Box::new(profile.clone())),
            )
        })
        .collect())
}

pub(crate) fn sweep(options: &Eval) -> Result<Vec<(String, Eval)>> {
    if options
        .partition
        .is_some_and(|partition| partition != EvalSplit::Calibrate)
    {
        return Err(usage(
            "packing/combine/focus sweeps require the fixed calibrate partition",
        ));
    }
    if options.sweep_pack.contains(&PackMode::Auto) {
        return Err(usage(
            "sweep candidates must name concrete packing; auto selects a previously measured policy",
        ));
    }
    let packs = if options.sweep_pack.is_empty() {
        vec![options.pack]
    } else {
        options.sweep_pack.iter().copied().map(Some).collect()
    };
    let combines = if options.sweep_combine.is_empty() {
        vec![options.combine]
    } else {
        options.sweep_combine.iter().copied().map(Some).collect()
    };
    let focuses = if options.sweep_focus.is_empty() {
        vec![options.focus.map(NonZeroUsize::get)]
    } else {
        options.sweep_focus.iter().copied().map(Some).collect()
    };
    let mut output = Vec::new();
    let mut names = BTreeSet::new();
    for pack in packs {
        for combine in &combines {
            for focus in &focuses {
                let mut settings = options.clone();
                settings.partition = Some(EvalSplit::Calibrate);
                settings.pack = pack;
                settings.combine = *combine;
                if let Some(focus) = focus {
                    settings.focus = NonZeroUsize::new(*focus);
                    settings.split_text = *focus == 0;
                    if *focus > 0 {
                        settings.combine = None;
                    }
                } else if !options.sweep_combine.is_empty() {
                    settings.split_text = true;
                }
                let name = format!(
                    "pack={:?},combine={:?},focus={:?},split={}",
                    settings.pack, settings.combine, settings.focus, settings.split_text
                );
                if names.insert(name.clone()) {
                    settings.sweep_pack.clear();
                    settings.sweep_combine.clear();
                    settings.sweep_focus.clear();
                    output.push((name, settings));
                }
            }
        }
    }
    Ok(output)
}

pub(crate) fn same_backend(left: &RunReport, right: &RunReport) -> Result<()> {
    if left.identity.model != right.identity.model
        || left.identity.local_encoder != right.identity.local_encoder
        || left.identity.server_encoder != right.identity.server_encoder
        || compare::observed_model(left)? != compare::observed_model(right)?
    {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "model/encoder identity changed during the same-backend experiment; restart it",
        ));
    }
    Ok(())
}

pub(crate) fn same_cases(left: &RunReport, right: &RunReport) -> Result<()> {
    compare::reports("first", left, "current", right).map(|_| ())
}

pub(crate) fn aligned<'a>(
    left: &'a RunReport,
    right: &'a RunReport,
) -> Result<
    Vec<(
        &'a crate::run::eval::SuiteReport,
        &'a crate::run::eval::SuiteReport,
    )>,
> {
    compare::alignment(left, right)
}

pub(crate) fn validate_reference(report: &RunReport) -> Result<()> {
    compare::validate_saved(report, false)
}

fn markdown(report: &Report) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Loupe {}\n\nStatus: **{}**. Generated: {}.\n\nInput tokens charged by this command: {}. Attempts: {}. HTTP attempts: {}. Cache hits: {}.\n\n",
        report.kind,
        report.status,
        report.generated_at,
        report.token_usage.spent_input,
        report.token_usage.attempts,
        report.token_usage.http_attempts,
        report.token_usage.cache_hits
    );
    for variant in &report.variants {
        let _ = writeln!(
            text,
            "## Variant {} ({})\n",
            output::cell(&variant.name),
            variant.source
        );
        text.push_str(&output::markdown(&variant.report));
    }
    for comparison in &report.comparisons {
        let _ = writeln!(
            text,
            "## Disagreements: {} versus {}\n\nPaired fields: {}. Changed decisions: {}.\n\n| Field | Left decision | Right decision | Count |\n| --- | --- | --- | --- |",
            output::cell(&comparison.left),
            output::cell(&comparison.right),
            comparison.paired_fields,
            comparison.disagreements.len()
        );
        for field in &comparison.fields {
            for cell in &field.matrix {
                let _ = writeln!(
                    text,
                    "| {} | {} | {} | {} |",
                    output::cell(&field.field),
                    output::cell(&format!("{:?}", cell.left)),
                    output::cell(&format!("{:?}", cell.right)),
                    cell.count
                );
            }
        }
        text.push('\n');
        for field in &comparison.fields {
            if let Some(raw) = &field.raw_primary_shift {
                text.push_str(&format!("Raw primary shift for {}: max {:.6}, mean {:.6}, {} cases / {} option probabilities. This excludes ensemble averaging; request/view differences remain observable, not independent causal measurements.\n\n", output::cell(&field.field), raw.max_probability_difference, raw.mean_probability_difference, raw.cases, raw.probabilities));
            }
        }
    }
    for note in &report.notes {
        let _ = writeln!(text, "{}\n", note);
    }
    if let Some(error) = &report.error {
        let _ = writeln!(
            text,
            "Stopped with exit {}: {}.\n",
            error.code,
            output::cell(&error.message)
        );
    }
    text
}
