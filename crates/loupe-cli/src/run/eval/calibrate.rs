use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::PathBuf;

use loupe_core::budget::Budget;
use loupe_core::calibration::{self, Fit, Fixed, Options};
use loupe_core::calibration_pin::{self, Pin};
use loupe_core::config::Profile;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::policy::QuestionKind;
use loupe_core::preset_store::{Preset, Store};
use loupe_core::suites::Label;
use loupe_core::task_metrics::Observation;
use serde::Serialize;
use serde_json::{Value, json};

use crate::args::{Calibrate, Cli, EvalSplit};
use crate::run::eval::{
    RunReport, RunScope, SuiteReport, deadline, ensemble, output, parsed, run, usage,
};
use crate::run::{json_error, write_json};

mod views;

#[derive(Clone, Serialize)]
struct Fitted {
    suite: String,
    field: String,
    partition: String,
    fit: Fit,
    proposed_preset: Option<Value>,
    written: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ensemble: Option<loupe_core::calibration_pin::Ensemble>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    agreement_uncertain_cases: Vec<String>,
    #[serde(skip)]
    original: Option<Preset>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: String,
    status: String,
    error: Option<crate::run::eval::Failure>,
    targets: Vec<calibration::Target>,
    max_uncertain: Option<f64>,
    minimum_support: usize,
    fitted: Vec<Fitted>,
    evaluation: RunReport,
    notes: Vec<String>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Calibrate,
    store: &Store,
    profile: Profile,
) -> Result<u8> {
    let mut settings = options.evaluation.clone();
    if settings.baseline.is_some() {
        return Err(usage(
            "acceptance baselines belong to held-out eval, never calibration",
        ));
    }
    if settings.partition == Some(EvalSplit::Test) {
        return Err(usage("calibration cannot use the held-out test partition"));
    }
    if !settings.compare.is_empty() {
        return Err(usage(
            "calibrate measures execution views for one profile/preset configuration; --compare belongs to eval",
        ));
    }
    settings.partition = Some(settings.partition.unwrap_or(EvalSplit::Calibrate));
    if options.write && settings.dry_run {
        return Err(usage(
            "--write requires actual verified calibration answers",
        ));
    }
    if options.write {
        settings.allow_unverified = false;
        settings.strict = true;
    }
    let budget = Budget::new(settings.budget);
    let deadline = deadline(&settings)?;
    if !settings.sweep_pack.is_empty()
        || !settings.sweep_combine.is_empty()
        || !settings.sweep_focus.is_empty()
    {
        return views::execute(cli, options, store, profile, &settings, &budget, deadline).await;
    }
    let mut preview = settings.clone();
    preview.dry_run = true;
    preview.allow_unverified = false;
    let plan = run(
        cli,
        &preview,
        store,
        profile.clone(),
        RunScope {
            budget: &budget,
            deadline,
            initialize_splits: false,
            recalibrate: true,
            baseline: None,
        },
    )
    .await?;
    if let Some(failure) = &plan.error {
        return Err(Error::new(
            if failure.code == 64 {
                ErrorKind::Usage
            } else {
                ErrorKind::Input
            },
            failure.message.clone(),
        ));
    }
    let search = Options {
        targets: options.target.clone(),
        max_uncertain: options.max_uncertain,
        minimum_support: options.min_support.get(),
        fixed: Fixed {
            threshold: settings.threshold,
            margin: settings.margin,
            min_confidence: settings.min_confidence,
            min_gap: settings.min_gap,
        },
    };
    preflight(&plan, options.field.as_deref(), &search)?;
    let evaluation = run(
        cli,
        &settings,
        store,
        profile.clone(),
        RunScope {
            budget: &budget,
            deadline,
            initialize_splits: true,
            recalibrate: true,
            baseline: None,
        },
    )
    .await?;
    let mut report = Report {
        schema_version: 1, kind: "calibration".to_owned(), status: evaluation.status.clone(), error: evaluation.error.clone(),
        targets: options.target.clone(), max_uncertain: options.max_uncertain, minimum_support: options.min_support.get(),
        fitted: Vec::new(), evaluation,
        notes: vec![
            "Targets are empirical precision/coverage with explicit accepted support, not held-out acceptance or a population guarantee.".to_owned(),
            "Only the fixed calibration/development partition is fitted; test cases remain untouched.".to_owned(),
            "Existing policy identity does not constrain explicit recalibration; every new live run still requires fresh discovery or explicitly provisional diagnostics.".to_owned(),
        ],
    };
    if settings.ensemble {
        report.notes.push("The ensemble agreement gate is fixed throughout fitting; agreement-failed cases remain uncertain under every candidate policy. Original and variants are jointly evaluated and correlated.".to_owned());
    }
    let mut code = report
        .evaluation
        .error
        .as_ref()
        .map_or(0, |error| error.code);
    if code == 0 && !settings.dry_run {
        let mut destinations = BTreeSet::new();
        for suite in &report.evaluation.suites {
            let fitted = fit_suite(
                suite,
                options.field.as_deref(),
                &search,
                &report.evaluation,
                store,
                &profile,
            )?;
            if options.write {
                let preset = suite
                    .records
                    .first()
                    .and_then(|record| record.preset.get("name"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| usage("calibration preset missing"))?;
                if !destinations.insert(preset.to_owned()) {
                    return Err(usage(
                        "several suites target one preset; combine their labeled calibration data or write each fit separately",
                    ));
                }
            }
            report.fitted.push(fitted);
        }
        if report
            .fitted
            .iter()
            .any(|fitted| fitted.fit.selected.is_none())
        {
            report.status = "unmet".to_owned();
            code = 1;
        }
        if options.write
            && code == 0
            && let Err(error) = write_fitted(&mut report.fitted, store, &profile, deadline).await
        {
            code = error.exit_code();
            report.status = "incomplete".to_owned();
            report.error = Some(crate::run::eval::Failure {
                code,
                message: error.to_string(),
            });
            report.notes.push("Each preset is committed atomically; previously committed presets remain recorded when a later write fails.".to_owned());
        }
    }
    let markdown = markdown(&report);
    if let Some(root) = &settings.report {
        let destination = loupe_core::report_store::Store::open(root)?;
        let mut bytes = serde_json::to_vec_pretty(&report).map_err(json_error)?;
        bytes.push(b'\n');
        let path = destination.publish(&bytes, markdown.as_bytes())?;
        if !cli.quiet {
            eprintln!(
                "loupe: calibration reports published under {}",
                path.display()
            );
        }
    }
    if settings.json {
        write_json(&mut io::stdout().lock(), &report)?;
    } else {
        io::stdout()
            .lock()
            .write_all(markdown.as_bytes())
            .map_err(|error| Error::io("writing calibration report", error))?;
    }
    Ok(code)
}

fn preflight(plan: &RunReport, requested: Option<&str>, search: &Options) -> Result<()> {
    for suite in &plan.suites {
        let field = field(suite, requested)?;
        let first = suite
            .records
            .first()
            .and_then(|record| record.questions.get(&field))
            .ok_or_else(|| usage("calibration question missing"))?;
        for record in &suite.records {
            let question = record
                .questions
                .get(&field)
                .ok_or_else(|| usage("calibration field missing"))?;
            if question.kind() != first.kind() || question.options() != first.options() {
                return Err(usage(
                    "calibration requires one consistent type/option domain across cases",
                ));
            }
            let policy = record
                .policies
                .get(&field)
                .ok_or_else(|| usage("calibration policy missing"))?
                .policy()?;
            calibration::fit(question, policy, &[], search)?;
        }
    }
    Ok(())
}

async fn write_fitted(
    fitted: &mut [Fitted],
    store: &Store,
    profile: &Profile,
    deadline: Option<tokio::time::Instant>,
) -> Result<()> {
    let backend =
        loupe_core::backend::with_deadline(deadline, loupe_core::backend::Backend::new(profile))
            .await?;
    let discovery =
        loupe_core::backend::with_deadline(deadline, backend.discover(&profile.limits)).await?;
    let encoder = loupe_core::encoder::load_profile(profile)?;
    let local_encoder = encoder
        .as_ref()
        .map_or("estimate-v1", |encoder| encoder.identity.as_str());
    let mut originals = Vec::new();
    let mut destinations = BTreeSet::new();
    for fit in fitted.iter() {
        let original = fit.original.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::Unavailable,
                "calibrated writes require complete current model/encoder identity",
            )
        })?;
        if !destinations.insert(original.name.as_str()) {
            return Err(usage(
                "several suites target one preset; combine their labeled calibration data or write each fit separately",
            ));
        }
        let document = fit
            .proposed_preset
            .as_ref()
            .ok_or_else(|| usage("verified calibrated preset proposal missing"))?;
        let pin = calibration_pin::validate(document)?
            .ok_or_else(|| usage("calibration identity missing"))?;
        let mut view = pin.view.clone();
        view["limits"] = serde_json::to_value(&discovery.limits).map_err(json_error)?;
        pin.check(
            discovery.full_model_identity.as_deref(),
            local_encoder,
            discovery
                .identity
                .as_ref()
                .map(|identity| identity.encoder_identity.as_str()),
            &view,
        )?;
        let current = store.lookup(&original.name)?;
        if current.content_hash != original.content_hash
            || current.origin.path() != original.origin.path()
        {
            return Err(usage(
                "preset changed during calibration; original bytes retained",
            ));
        }
        originals.push(current);
    }
    for (fit, original) in fitted.iter_mut().zip(&originals) {
        let document = fit
            .proposed_preset
            .as_ref()
            .ok_or_else(|| usage("verified calibrated preset proposal missing"))?;
        fit.written = Some(calibration_pin::write(
            store,
            original,
            document,
            &discovery.limits,
            &profile.model,
        )?);
    }
    Ok(())
}

fn field(suite: &SuiteReport, requested: Option<&str>) -> Result<String> {
    let record = suite
        .records
        .first()
        .ok_or_else(|| usage("calibration partition has no records"))?;
    let primary = ensemble::primary_questions(record)?;
    if let Some(field) = requested {
        if !primary.contains_key(field) {
            return Err(usage(
                "--field must name an original rubric question; signals and generated variants are not fitted",
            ));
        }
        return Ok(field.to_owned());
    }
    if primary.len() != 1 {
        return Err(usage("mixed-rubric calibration requires --field NAME"));
    }
    primary
        .keys()
        .next()
        .cloned()
        .ok_or_else(|| usage("calibration question missing"))
}

fn fit_suite(
    suite: &SuiteReport,
    selected: Option<&str>,
    search: &Options,
    evaluation: &RunReport,
    store: &Store,
    profile: &Profile,
) -> Result<Fitted> {
    let field = field(suite, selected)?;
    let first = suite
        .records
        .first()
        .ok_or_else(|| usage("calibration record missing"))?;
    let question = first
        .questions
        .get(&field)
        .ok_or_else(|| usage("calibration question missing"))?;
    let policy = first
        .policies
        .get(&field)
        .ok_or_else(|| usage("calibration policy missing"))?
        .policy()?;
    let mut labels = Vec::new();
    let mut answers = Vec::new();
    let configuration = ensemble::calibration(first)?;
    let mut additional_uncertain = BTreeSet::new();
    for record in &suite.records {
        if ensemble::calibration(record)? != configuration {
            return Err(usage(
                "calibration ensemble configuration differs across cases",
            ));
        }
        if ensemble::uncertain(record, &field)? {
            additional_uncertain.insert(record.case_id.as_str());
        }
        let current = record
            .questions
            .get(&field)
            .ok_or_else(|| usage("calibration field differs across cases"))?;
        if current.kind() != question.kind() || current.options() != question.options() {
            return Err(usage(
                "calibration requires one consistent type/option domain across cases",
            ));
        }
        let label = match &record.label {
            Label::Fields(fields) => fields
                .get(&field)
                .ok_or_else(|| usage("calibration label missing"))?
                .clone(),
            label => label.clone(),
        };
        let binding = record
            .binding
            .get(&field)
            .ok_or_else(|| usage("calibration binding missing"))?;
        let parsed = parsed(record)?;
        answers.push(
            parsed
                .get(binding)
                .ok_or_else(|| usage("calibration answer missing"))?
                .clone(),
        );
        labels.push(label);
    }
    let observations = suite
        .records
        .iter()
        .zip(&labels)
        .zip(&answers)
        .map(|((record, label), answer)| Observation {
            id: &record.case_id,
            label,
            answer,
        })
        .collect::<Vec<_>>();
    let fitted = calibration::fit_with_uncertainty(
        question,
        policy,
        &observations,
        search,
        &additional_uncertain,
    )?;
    let mut proposed = None;
    let mut original = None;
    if let Some(parameters) = fitted
        .selected
        .as_ref()
        .map(|candidate| &candidate.parameters)
        && evaluation.identity.verified
    {
        let name = first
            .preset
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| usage("calibration preset name missing"))?;
        let preset = store.lookup(name)?;
        if first.preset.get("content_hash").and_then(Value::as_str)
            != Some(preset.content_hash.as_str())
        {
            return Err(usage(
                "preset changed during calibration; original bytes retained",
            ));
        }
        let limits = evaluation
            .server
            .get("limits")
            .ok_or_else(|| usage("effective calibration limits missing"))?;
        let view = calibration_pin::view(&first.packing, limits)?;
        for record in &suite.records {
            if calibration_pin::view(&record.packing, limits)? != view
                || record.preset != first.preset
            {
                return Err(usage(
                    "calibration requires one preset/execution view; measure or fit separate variants",
                ));
            }
        }
        let mut source = json!({"partition":suite.partition,"suite":suite.manifest.id,"suite_revision":suite.manifest.revision,"cases":suite.manifest.cases,"splits":suite.splits,"case_ids":suite.selected_case_ids,"targets":search.targets,"minimum_support":search.minimum_support,"fit":fitted.selected});
        if let Some(maximum) = search.max_uncertain {
            source["max_uncertain"] = json!(maximum);
        }
        if let Some(ensemble) = &configuration {
            source["ensemble"] = serde_json::to_value(ensemble).map_err(json_error)?;
            source["agreement_uncertain_cases"] = json!(additional_uncertain);
        }
        let pin = Pin {
            schema_version: 1,
            model: evaluation
                .identity
                .model
                .clone()
                .ok_or_else(|| usage("verified calibration model missing"))?,
            local_encoder: evaluation.identity.local_encoder.clone(),
            server_encoder: evaluation
                .identity
                .server_encoder
                .clone()
                .ok_or_else(|| usage("verified server encoder missing"))?,
            preset_version: 1,
            preset_blake3: "0".repeat(64),
            field: field.clone(),
            view,
            source,
            ensemble: configuration.clone(),
        };
        let document =
            calibration_pin::document(&preset.document, parameters, question.kind(), pin)?;
        let mut candidate = preset.clone();
        candidate.document = document.clone();
        let mut effective_limits = profile.limits.clone();
        effective_limits.constrain(limits)?;
        candidate.validate(&effective_limits, &profile.model)?;
        proposed = Some(document);
        original = Some(preset);
    }
    if question.kind() == QuestionKind::Score {
        return Err(usage("score calibration is unsupported"));
    }
    Ok(Fitted {
        suite: suite.manifest.id.clone(),
        field,
        partition: suite.partition.clone(),
        fit: fitted,
        proposed_preset: proposed,
        written: None,
        ensemble: configuration,
        agreement_uncertain_cases: additional_uncertain
            .into_iter()
            .map(str::to_owned)
            .collect(),
        original,
    })
}

fn markdown(report: &Report) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Loupe calibration\n\nStatus: **{}**. Minimum support: {}.\n\n",
        report.status, report.minimum_support
    );
    if let Some(error) = &report.error {
        let _ = writeln!(
            text,
            "Failure (exit {}): {}.\n",
            error.code,
            output::cell(&error.message)
        );
    }
    for fitted in &report.fitted {
        if let Some(ensemble) = &fitted.ensemble {
            let _ = writeln!(
                text,
                "Ensemble agreement gate: {}. Always uncertain from agreement: {} cases.\n",
                ensemble.min_agreement.get(),
                fitted.agreement_uncertain_cases.len()
            );
        }
        let _ = writeln!(
            text,
            "## {} / {} ({})\n\nCandidates checked: {}. Feasible: {}.\n",
            output::cell(&fitted.suite),
            output::cell(&fitted.field),
            output::cell(&fitted.partition),
            fitted.fit.candidates,
            fitted.fit.feasible_candidates
        );
        if let Some(selected) = &fitted.fit.selected {
            let _ = writeln!(
                text,
                "Accepted: {} of {}. Empirical coverage: {:.6}. Selective accuracy: {:.6}.\n\nThreshold: {:.17}. Margin: {:.17}. Minimum confidence: {:.17}. Minimum gap: {:.17}.\n",
                selected.accepted,
                selected.cases,
                selected.coverage,
                selected.selective_accuracy,
                selected.parameters.threshold,
                selected.parameters.margin,
                selected.parameters.min_confidence,
                selected.parameters.min_gap
            );
            for target in &selected.targets {
                let _ = writeln!(
                    text,
                    "Target {:?} >= {:.6}: {} correct of {} predicted, observed {:.6}.\n",
                    target.label,
                    target.minimum_precision,
                    target.correct,
                    target.predicted,
                    target.observed_precision
                );
            }
        } else {
            text.push_str("No policy meets the targets and minimum support.\n\n");
        }
        if let Some(path) = &fitted.written {
            let _ = writeln!(
                text,
                "Written preset: {}.\n",
                output::cell(&path.display().to_string())
            );
        }
    }
    for note in &report.notes {
        let _ = writeln!(text, "{note}\n");
    }
    text.push_str(&output::markdown(&report.evaluation));
    text
}
