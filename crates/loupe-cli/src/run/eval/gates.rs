use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use loupe_core::baseline::{Assessment, Check, Ratification, Record as BaselineRecord};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::suites::Metric;
use loupe_core::task_metrics::Value as MetricValue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::args::{Cli, Eval, EvalSplit};
use crate::run::eval::{Identity, RunReport, RunScope, SuiteReport, output, run, usage, variants};
use crate::run::{json_error, write_json};

pub(crate) struct Baseline {
    record: BaselineRecord,
    file_blake3: String,
    reference: RunReport,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Status {
    Plan,
    Passed,
    Failed,
    Incomplete,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checked {
    suite: String,
    field: Option<String>,
    metric: Metric,
    gate: loupe_core::suites::Gate,
    minimum_support: usize,
    baseline: MetricValue,
    assessment: Option<Assessment>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    schema_version: u32,
    pub(crate) status: Status,
    baseline_blake3: String,
    reference_report_blake3: String,
    reference_model: String,
    reference_local_encoder: String,
    reference_server_encoder: String,
    ratification: Ratification,
    checks: Vec<Checked>,
}

impl Baseline {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let loaded = loupe_core::baseline::load(path)?;
        let record = loaded.record;
        let report_bytes = loaded.report;
        let reference: RunReport = serde_json::from_slice(&report_bytes).map_err(json_error)?;
        if reference
            .suites
            .iter()
            .any(|suite| suite.partition != "test")
        {
            return Err(usage(
                "acceptance baselines require the held-out test partition, never calibration or captured cases",
            ));
        }
        if let Err(error) = variants::validate_reference(&reference) {
            if error.exit_code() == 69 {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "acceptance baselines require complete authoritative identity; --allow-unverified cannot ratify a provisional report",
                ));
            }
            return Err(error);
        }
        for suite in &reference.suites {
            let recomputed = crate::run::eval::question_reports(&suite.records)?;
            let parity = crate::run::eval::parity_report(&suite.records, suite.manifest.task)?;
            if serde_json::to_value(recomputed).map_err(json_error)?
                != serde_json::to_value(&suite.questions).map_err(json_error)?
                || serde_json::to_value(parity).map_err(json_error)?
                    != serde_json::to_value(&suite.parity).map_err(json_error)?
            {
                return Err(input(
                    "baseline metric summaries differ from their saved raw answers",
                ));
            }
            measured_latency(suite)?;
        }
        let mut resolved = BTreeSet::new();
        for check in &record.checks {
            let (field, metric) = metric(suite(&reference, &check.suite)?, check)?;
            if !resolved.insert((&check.suite, field, check.gate.metric)) {
                return Err(usage(
                    "baseline checks resolve to the same suite/field/metric more than once",
                ));
            }
            if metric.value.is_none_or(|value| !value.is_finite())
                || metric.undefined.is_some()
                || metric.support < check.minimum_support.get()
            {
                return Err(input(
                    "ratified baseline metric must be defined with the declared minimum support",
                ));
            }
        }
        Ok(Self {
            record,
            file_blake3: loaded.content_blake3,
            reference,
        })
    }

    pub(crate) fn preflight(&self, plan: &RunReport) -> Result<()> {
        for (reference, current) in variants::aligned(&self.reference, plan)? {
            if current.partition != "test" || current.manifest.id != reference.manifest.id {
                return Err(usage(
                    "baseline suite IDs and held-out cases must match the current run",
                ));
            }
            let records = current
                .records
                .iter()
                .map(|record| (&record.case_id, record))
                .collect::<BTreeMap<_, _>>();
            for previous in &reference.records {
                let current = records
                    .get(&previous.case_id)
                    .ok_or_else(|| usage("baseline case missing"))?;
                if snapshot(previous)? != snapshot(current)? {
                    return Err(usage(
                        "baseline preset/version, questions/order, decision policy, ensemble or execution view changed; compare explicitly and ratify a separate baseline",
                    ));
                }
            }
            self.check_declared(reference)?;
        }
        for check in &self.record.checks {
            suite(plan, &check.suite)?;
        }
        Ok(())
    }

    pub(in crate::run::eval) fn check_prepared(
        &self,
        prepared: &[crate::run::eval::PreparedSuite],
    ) -> Result<()> {
        if prepared.len() != self.reference.suites.len() {
            return Err(usage("baseline suites differ from the current run"));
        }
        for current in prepared {
            let reference = suite(&self.reference, &current.suite.manifest.id)?;
            if serde_json::to_value(&current.suite.manifest).map_err(json_error)?
                != serde_json::to_value(&reference.manifest).map_err(json_error)?
                || current.splits != reference.splits
                || current.selected.iter().collect::<BTreeSet<_>>()
                    != reference.selected_case_ids.iter().collect::<BTreeSet<_>>()
            {
                return Err(usage(
                    "baseline manifest, fixed held-out cases or split provenance changed",
                ));
            }
            let records = reference
                .records
                .iter()
                .map(|record| (&record.case_id, record))
                .collect::<BTreeMap<_, _>>();
            for case in &current.cases {
                let previous = records
                    .get(&case.case.id)
                    .ok_or_else(|| usage("baseline case missing"))?;
                if snapshot(previous)? != prepared_snapshot(case)? {
                    return Err(usage(
                        "baseline preset/version, questions/order, decision policy, ensemble or execution view changed; compare explicitly and ratify a separate baseline",
                    ));
                }
            }
            // The original sealed manifest is immutable, so its declared gates
            // can be checked against the ratification before credentials or writes.
            self.check_declared(reference)?;
        }
        Ok(())
    }

    fn check_declared(&self, suite: &SuiteReport) -> Result<()> {
        let checks = self
            .record
            .checks
            .iter()
            .filter(|check| check.suite == suite.manifest.id)
            .collect::<Vec<_>>();
        if checks.is_empty() {
            return Err(usage(
                "every evaluated suite requires explicit ratified baseline checks",
            ));
        }
        for gate in &suite.manifest.gates {
            let mut covered = BTreeSet::new();
            for check in &checks {
                if serde_json::to_value(&check.gate).map_err(json_error)?
                    == serde_json::to_value(gate).map_err(json_error)?
                {
                    covered.insert(metric(suite, check)?.0);
                }
            }
            if covered != targets(suite, gate.metric)? {
                return Err(usage(
                    "ratification must cover every declared suite gate with the same bounds and applicable fields",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn check_identity(&self, identity: &Identity, limits: &Value) -> Result<()> {
        if !identity_matches(&self.reference.identity, identity)
            || self.reference.server.get("limits") != Some(limits)
        {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "fresh model/encoder/effective limits differ from the ratified baseline; cross-model or quantization comparison requires --compare and a separate ratification",
            ));
        }
        Ok(())
    }

    pub(crate) fn assess(&self, report: &RunReport, planned: bool) -> Result<Report> {
        let mut checks = Vec::new();
        let complete = report.error.is_none() && !planned;
        if complete {
            self.preflight(report)?;
        }
        let mut passed = true;
        for check in &self.record.checks {
            let (field, reference) = metric(suite(&self.reference, &check.suite)?, check)?;
            let assessment = if complete {
                let (_, actual) = metric(suite(report, &check.suite)?, check)?;
                Some(loupe_core::baseline::assess(check, actual, reference)?)
            } else {
                None
            };
            passed &= assessment
                .as_ref()
                .is_some_and(|assessment| assessment.passed);
            checks.push(Checked {
                suite: check.suite.clone(),
                field,
                metric: check.gate.metric,
                gate: check.gate.clone(),
                minimum_support: check.minimum_support.get(),
                baseline: reference.clone(),
                assessment,
            });
        }
        Ok(Report {
            schema_version: 1,
            status: if planned {
                Status::Plan
            } else if !complete {
                Status::Incomplete
            } else if passed {
                Status::Passed
            } else {
                Status::Failed
            },
            baseline_blake3: self.file_blake3.clone(),
            reference_report_blake3: self.record.report_blake3.clone(),
            reference_model: self
                .reference
                .identity
                .model
                .as_ref()
                .ok_or_else(|| input("verified baseline model missing"))?
                .as_str()
                .to_owned(),
            reference_local_encoder: self.reference.identity.local_encoder.clone(),
            reference_server_encoder: self
                .reference
                .identity
                .server_encoder
                .clone()
                .ok_or_else(|| input("verified baseline encoder missing"))?,
            ratification: self.record.ratification.clone(),
            checks,
        })
    }
}

fn identity_matches(expected: &Identity, current: &Identity) -> bool {
    expected.verified
        && current.verified
        && expected.model.is_some()
        && expected.model == current.model
        && !expected.local_encoder.is_empty()
        && expected.local_encoder == current.local_encoder
        && expected
            .server_encoder
            .as_ref()
            .is_some_and(|encoder| !encoder.is_empty())
        && expected.server_encoder == current.server_encoder
}

fn snapshot(record: &crate::run::eval::Record) -> Result<Value> {
    let preset_blake3 = canonical_preset(&record.preset)?;
    let mut view = record.packing.clone();
    view.as_object_mut()
        .ok_or_else(|| usage("baseline packing metadata missing"))?
        .remove("source_indices");
    Ok(
        json!({"preset_blake3":preset_blake3,"effective_document":record.effective_document,"questions":record.questions.iter().collect::<Vec<_>>(),"base_question_order":record.base_question_order,"policies":record.policies,"binding":record.binding,"view":view,"ensemble":record.ensemble.as_ref().map(|saved|&saved.settings)}),
    )
}

fn prepared_snapshot(case: &crate::run::eval::PreparedCase) -> Result<Value> {
    let preset_blake3 = canonical_preset(&case.preset)?;
    let mut binding = BTreeMap::new();
    for id in case.questions.keys() {
        let bound = if matches!(case.settings.pack, loupe_core::pack::Mode::Window(_)) {
            let source = case.settings.source.id(case.unit.index);
            if case.questions.len() == 1 {
                source
            } else {
                format!("{source}/{id}")
            }
        } else {
            id.clone()
        };
        binding.insert(id.clone(), bound);
    }
    Ok(
        json!({"preset_blake3":preset_blake3,"effective_document":case.document,"questions":case.questions.iter().collect::<Vec<_>>(),"base_question_order":case.questions.keys().collect::<Vec<_>>(),"policies":case.policies,"binding":binding,"view":crate::run::eval::packing(&case.settings),"ensemble":case.ensemble}),
    )
}

fn canonical_preset(preset: &Value) -> Result<String> {
    let original = preset
        .get("document")
        .ok_or_else(|| input("baseline original preset document missing"))?;
    loupe_core::calibration_pin::fingerprint(original)
}

fn suite<'a>(report: &'a RunReport, id: &str) -> Result<&'a SuiteReport> {
    let mut matches = report.suites.iter().filter(|suite| suite.manifest.id == id);
    let found = matches
        .next()
        .ok_or_else(|| usage("baseline check targets an unknown suite"))?;
    if matches.next().is_some() {
        return Err(usage("baseline suite ID is ambiguous"));
    }
    Ok(found)
}

fn measured_latency(suite: &SuiteReport) -> Result<&crate::run::eval::Phase> {
    let mut matches = suite.phases.iter().filter(|phase| phase.name == "latency");
    let phase = matches
        .next()
        .ok_or_else(|| input("baseline needs a complete uncached latency phase"))?;
    if matches.next().is_some()
        || phase.cache_requested
        || phase.cache_hits != 0
        || phase.error.is_some()
        || phase.timing.completed_units != suite.selected_case_ids.len()
        || !phase.timing.elapsed_seconds.is_finite()
        || phase.timing.elapsed_seconds <= 0.0
    {
        return Err(input(
            "baseline latency must cover every held-out unit without cache hits or errors",
        ));
    }
    Ok(phase)
}

fn flat_metric(suite: &SuiteReport, metric: Metric) -> Result<Option<&MetricValue>> {
    Ok(match metric {
        Metric::LatencyP50Ms => Some(&measured_latency(suite)?.timing.p50_ms),
        Metric::LatencyP95Ms => Some(&measured_latency(suite)?.timing.p95_ms),
        Metric::UnitsPerSecond => Some(&measured_latency(suite)?.timing.units_per_second),
        Metric::ProbabilityMaxError => Some(
            &suite
                .parity
                .as_ref()
                .ok_or_else(|| usage("parity gate requires saved references"))?
                .max_absolute_error,
        ),
        Metric::ProbabilityMeanError => Some(
            &suite
                .parity
                .as_ref()
                .ok_or_else(|| usage("parity gate requires saved references"))?
                .mean_absolute_error,
        ),
        Metric::ArgmaxAgreement => Some(
            &suite
                .parity
                .as_ref()
                .ok_or_else(|| usage("parity gate requires saved references"))?
                .argmax_agreement,
        ),
        _ => None,
    })
}

fn metric<'a>(suite: &'a SuiteReport, check: &Check) -> Result<(Option<String>, &'a MetricValue)> {
    if let Some(value) = flat_metric(suite, check.gate.metric)? {
        if check.field.is_some() {
            return Err(usage("latency and parity gates do not select a field"));
        }
        return Ok((None, value));
    }
    let mut matches = suite.questions.iter().filter(|question| {
        check
            .field
            .as_ref()
            .is_none_or(|field| &question.field == field)
            && question.metrics.metric(check.gate.metric).is_ok()
    });
    let question = matches
        .next()
        .ok_or_else(|| usage("baseline metric or field is inapplicable"))?;
    if matches.next().is_some() {
        return Err(usage(
            "baseline classification gate requires one unambiguous field/type/policy domain; set the check's field or separate incompatible domains into suites",
        ));
    }
    Ok((
        Some(question.field.clone()),
        question.metrics.metric(check.gate.metric)?,
    ))
}

fn targets(suite: &SuiteReport, metric: Metric) -> Result<BTreeSet<Option<String>>> {
    if flat_metric(suite, metric)?.is_some() {
        return Ok(BTreeSet::from([None]));
    }
    let targets = suite
        .questions
        .iter()
        .filter(|question| question.metrics.metric(metric).is_ok())
        .map(|question| Some(question.field.clone()))
        .collect::<BTreeSet<_>>();
    if targets.is_empty() {
        return Err(usage("declared baseline metric is inapplicable"));
    }
    Ok(targets)
}

fn input(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

pub(crate) fn markdown(report: &Report) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "## Held-out acceptance\n\nStatus: **{:?}**. Baseline BLAKE3: {}.\n\nRatified by {} at {}: {}\n\n| Suite | Field | Metric | Reference | Actual | Passed |\n| --- | --- | --- | --- | --- | --- |\n",
        report.status,
        report.baseline_blake3,
        output::cell(&report.ratification.by),
        output::cell(&report.ratification.at),
        output::cell(&report.ratification.justification)
    );
    for check in &report.checks {
        let value = check
            .assessment
            .as_ref()
            .and_then(|assessment| assessment.current.value)
            .map_or_else(|| "unavailable".to_owned(), |value| format!("{value:.6}"));
        let _ = writeln!(
            text,
            "| {} | {} | {:?} | {:?} (n={}) | {} | {:?} |",
            output::cell(&check.suite),
            check.field.as_deref().map(output::cell).unwrap_or_default(),
            check.metric,
            check.baseline.value,
            check.baseline.support,
            value,
            check
                .assessment
                .as_ref()
                .map(|assessment| assessment.passed)
        );
    }
    text.push('\n');
    text
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Eval,
    store: &loupe_core::preset_store::Store,
    profile: loupe_core::config::Profile,
    budget: &loupe_core::budget::Budget,
    deadline: Option<tokio::time::Instant>,
) -> Result<u8> {
    use std::io::{self, Write};
    if options
        .partition
        .is_some_and(|partition| partition != EvalSplit::Test)
    {
        return Err(usage(
            "acceptance gates require the held-out test partition",
        ));
    }
    let path = options
        .baseline
        .as_ref()
        .ok_or_else(|| usage("baseline file missing"))?;
    let baseline = Baseline::load(path)?;
    let mut report = run(
        cli,
        options,
        store,
        profile,
        RunScope {
            budget,
            deadline,
            initialize_splits: true,
            recalibrate: false,
            baseline: Some(&baseline),
        },
    )
    .await?;
    let mut code = report.error.as_ref().map_or(0, |error| error.code);
    match baseline.assess(&report, options.dry_run) {
        Ok(assessment) => {
            if code == 0 && matches!(assessment.status, Status::Failed) {
                code = 1;
            }
            report.acceptance = Some(assessment);
        }
        Err(error) => {
            code = error.exit_code();
            report.status = "incomplete".into();
            report.error = Some(crate::run::eval::Failure {
                code,
                message: error.to_string(),
            });
        }
    }
    if let Some(root) = &options.report {
        let path = output::Destination::open(root)?.publish(&report)?;
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
            .map_err(|error| Error::io("writing gated evaluation", error))?;
    }
    Ok(code)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::run::eval::gates::{identity_matches, prepared_snapshot, snapshot};
    use crate::run::eval::{Identity, RunReport};
    use serde_json::{Value, json};

    fn identity(revision: char, compute: &str, quantization: &str, build: char) -> Identity {
        // Mathematical identity codec fixture only; no backend or discovery response.
        Identity {
            verified: true,
            model: Some(
                loupe_core::identity::Model::parse(&format!(
                    "fixture/model@{};compute={compute};quantization={quantization};server={}",
                    revision.to_string().repeat(40),
                    build.to_string().repeat(64)
                ))
                .unwrap(),
            ),
            requested_model: "alias".into(),
            local_encoder: "local-tokenizer-v1".into(),
            server_encoder: Some("server-tokenizer-v1".into()),
            reason: None,
        }
    }

    #[test]
    fn identity_math_requires_revision_compute_quantization_build_and_both_encoder_digests() {
        let reference = identity('1', "bf16", "nf4", '2');
        let same = identity('1', "bf16", "nf4", '2');
        assert!(identity_matches(&reference, &same));
        for changed in [
            identity('3', "bf16", "nf4", '2'),
            identity('1', "fp16", "nf4", '2'),
            identity('1', "bf16", "int8", '2'),
            identity('1', "bf16", "nf4", '3'),
        ] {
            assert!(!identity_matches(&reference, &changed));
        }
        for key in ["verified", "model", "local_encoder", "server_encoder"] {
            let mut changed = serde_json::to_value(&same).unwrap();
            changed[key] = match key {
                "verified" => json!(false),
                "local_encoder" => json!("changed"),
                _ => Value::Null,
            };
            assert!(!identity_matches(
                &reference,
                &serde_json::from_value(changed).unwrap()
            ));
        }
    }

    #[test]
    fn real_saved_request_metadata_binds_questions_order_policy_pack_and_ensemble_without_using_answers()
     {
        let evidence: Value = serde_json::from_str(include_str!(
            "../../../../loupe-core/testdata/captured-eval.json"
        ))
        .unwrap();
        let report: RunReport =
            serde_json::from_value(evidence["checks"][2]["report"].clone()).unwrap();
        let record = &report.suites[0].records[0];
        let reference = snapshot(record).unwrap();
        for target in [
            "preset",
            "effective_document",
            "policies",
            "packing",
            "base_question_order",
        ] {
            let mut changed = serde_json::to_value(record).unwrap();
            match target {
                "preset" => changed[target]["document"]["version"] = json!(987),
                "effective_document" => changed[target]["version"] = json!(987),
                "policies" => {
                    let first = changed[target]
                        .as_object()
                        .unwrap()
                        .keys()
                        .next()
                        .unwrap()
                        .clone();
                    changed[target][first]["threshold"] = json!(0.123);
                }
                "packing" => changed[target]["declared"] = json!("Window(4)"),
                _ => changed[target] = json!([]),
            }
            let changed = serde_json::from_value(changed).unwrap();
            assert_ne!(reference, snapshot(&changed).unwrap(), "{target}");
        }
        let mut changed = serde_json::to_value(record).unwrap();
        changed["preset"]["content_hash"] =
            json!("cosmetic formatting changes raw bytes, not the canonical preset");
        assert_eq!(
            reference,
            snapshot(&serde_json::from_value(changed).unwrap()).unwrap()
        );
        let mut changed = serde_json::to_value(record).unwrap();
        changed["packing"]["source_indices"] = json!([123]);
        assert_eq!(
            reference,
            snapshot(&serde_json::from_value(changed).unwrap()).unwrap()
        );
    }

    #[test]
    fn real_saved_metrics_project_field_and_uncached_timing_gates_without_accepting_a_baseline() {
        use crate::run::eval::gates::metric;
        use loupe_core::baseline::Check;
        use loupe_core::suites::{Gate, Metric};
        use std::num::NonZeroUsize;
        let evidence: Value = serde_json::from_str(include_str!(
            "../../../../loupe-core/testdata/captured-eval.json"
        ))
        .unwrap();
        let report: RunReport =
            serde_json::from_value(evidence["checks"][2]["report"].clone()).unwrap();
        assert!(!report.identity.verified);
        let suite = &report.suites[0];
        for (field, kind) in [
            (Some("available"), Metric::Accuracy),
            (Some("topic"), Metric::MacroF1),
            (Some("severity"), Metric::Mae),
            (None, Metric::LatencyP50Ms),
            (None, Metric::LatencyP95Ms),
            (None, Metric::UnitsPerSecond),
        ] {
            let mut check = Check {
                suite: suite.manifest.id.clone(),
                field: field.map(str::to_owned),
                minimum_support: NonZeroUsize::MIN,
                gate: Gate {
                    metric: kind,
                    minimum: Some(0.0),
                    maximum: None,
                    max_drop: Some(0.0),
                    max_increase: Some(0.0),
                },
            };
            let (resolved, value) = metric(suite, &check).unwrap();
            assert_eq!(resolved.as_deref(), field);
            check.gate.minimum = value.value;
            let assessed = loupe_core::baseline::assess(&check, value, value).unwrap();
            assert!(assessed.passed);
            assert_eq!(assessed.baseline.value, assessed.current.value);
        }
        let check = Check {
            suite: suite.manifest.id.clone(),
            field: None,
            minimum_support: NonZeroUsize::MIN,
            gate: Gate {
                metric: Metric::Accuracy,
                minimum: Some(0.0),
                maximum: None,
                max_drop: None,
                max_increase: None,
            },
        };
        assert!(metric(suite, &check).is_err());
        let mut incompatible = check;
        incompatible.field = Some("available".into());
        incompatible.gate.metric = Metric::Mae;
        assert!(metric(suite, &incompatible).is_err());
        incompatible.gate.metric = Metric::UnitsPerSecond;
        assert!(metric(suite, &incompatible).is_err());
    }

    #[test]
    fn prepared_unit_and_window_metadata_matches_real_saved_views_before_any_inference() {
        use crate::run::eval::{PreparedCase, ViewSettings};
        use loupe_core::input::Unit;
        let evidence: Value = serde_json::from_str(include_str!(
            "../../../../loupe-core/testdata/captured-eval.json"
        ))
        .unwrap();
        for capture in [2, 3] {
            let mut report: RunReport =
                serde_json::from_value(evidence["checks"][capture]["report"].clone()).unwrap();
            // Historical captures predate the explicit family tag. Their real
            // R<n> bindings establish the record family for this metadata codec.
            if capture == 3 {
                for record in &mut report.suites[0].records {
                    assert!(
                        record
                            .binding
                            .values()
                            .all(|binding| binding.starts_with('R'))
                    );
                    record.packing["question_id_source"] = json!("Records");
                }
            }
            for record in &report.suites[0].records {
                let view = &record.packing;
                let pack =
                    loupe_core::calibration_pin::pack_name(view["declared"].as_str().unwrap())
                        .unwrap()
                        .parse()
                        .unwrap();
                let mut prepared=PreparedCase {
                    selection:None,
                    ensemble:record.ensemble.as_ref().map(|saved|saved.settings.clone()),
                    case:serde_json::from_value(json!({"id":record.case_id,"unit":record.request.state,"label":record.label,"source":record.source,"reviewed":record.reviewed})).unwrap(),
                    unit:Unit {index:record.index,text:String::new(),raw:Vec::new(),state:record.request.state.clone(),subject:record.request.state.clone(),header:None,location:serde_json::from_value(record.provenance["source"].clone()).unwrap()},
                    preset:record.preset.clone(),document:record.effective_document.clone(),questions:record.questions.clone(),policies:record.policies.clone(),signature:"settings-only codec; no inference".into(),
                    settings:ViewSettings {pack,source:view.get("question_id_source").and_then(Value::as_str).unwrap_or("records").parse().unwrap(),split:view["split"].as_bool().unwrap(),modes:serde_json::from_value(view["combine"].clone()).unwrap(),overlap:256,max_chunks:64,focus:None,truncate:None,path:view["path"].as_str().unwrap().into()},
                };
                assert_eq!(
                    snapshot(record).unwrap(),
                    prepared_snapshot(&prepared).unwrap()
                );
                prepared.document["version"] = json!(987);
                assert_ne!(
                    snapshot(record).unwrap(),
                    prepared_snapshot(&prepared).unwrap()
                );
            }
        }
    }
}
