use std::collections::{BTreeMap, BTreeSet};

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::policy::{Probability, Selected};
use loupe_core::wire::{Answer, Question};
use serde::Serialize;
use serde_json::json;

use crate::run::eval::variants::Prepared;
use crate::run::eval::{Record, RunReport, SuiteReport, ensemble, parsed, usage};
use crate::run::json_error;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub(crate) enum Decision {
    Uncertain,
    Boolean(bool),
    Choice(String),
    Score(f64),
}
#[derive(Serialize)]
pub(crate) struct Cell {
    pub(crate) left: Decision,
    pub(crate) right: Decision,
    pub(crate) count: usize,
}
#[derive(Serialize)]
pub(crate) struct Field {
    pub(crate) suite_left: String,
    pub(crate) suite_right: String,
    pub(crate) field: String,
    pub(crate) cases: usize,
    pub(crate) changed_decisions: usize,
    pub(crate) matrix: Vec<Cell>,
    pub(crate) max_probability_difference: f64,
    pub(crate) mean_probability_difference: f64,
    pub(crate) probabilities: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) raw_primary_shift: Option<RawPrimaryShift>,
}

#[derive(Serialize)]
pub(crate) struct RawPrimaryShift {
    pub(crate) cases: usize,
    pub(crate) max_probability_difference: f64,
    pub(crate) mean_probability_difference: f64,
    pub(crate) probabilities: usize,
}

#[derive(Default)]
struct RawShift {
    cases: usize,
    maximum: f64,
    sum: f64,
    probabilities: usize,
}
#[derive(Serialize)]
pub(crate) struct Disagreement {
    suite_left: String,
    suite_right: String,
    case_id: String,
    field: String,
    field_left: String,
    field_right: String,
    left: Decision,
    right: Decision,
}
#[derive(Serialize)]
pub(crate) struct Report {
    pub(crate) left: String,
    pub(crate) right: String,
    pub(crate) paired_fields: usize,
    pub(crate) fields: Vec<Field>,
    pub(crate) disagreements: Vec<Disagreement>,
}

#[derive(Default)]
struct Bucket {
    raw: Option<RawShift>,
    matrix: BTreeMap<(String, String), Cell>,
    cases: usize,
    changed: usize,
    maximum: f64,
    sum: f64,
    probabilities: usize,
}

pub(crate) fn validate_saved(report: &RunReport, allow_unverified: bool) -> Result<()> {
    if report
        .suites
        .iter()
        .any(|suite| suite.join.is_some() || suite.selection.is_some())
    {
        return Err(usage(
            "selection/join comparisons and baselines require stage-specific alignment and are not supported",
        ));
    }
    if report.schema_version != 1
        || report.error.is_some()
        || !matches!(report.status.as_str(), "complete" | "provisional")
    {
        return Err(invalid(
            "comparison requires completed accuracy and measurement reports, not plans or incomplete runs",
        ));
    }
    if (!report.identity.verified || report.status != "complete") && !allow_unverified {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "saved comparison requires complete identities; --allow-unverified permits explicitly provisional diagnostics",
        ));
    }
    if report.identity.verified
        && (report.identity.model.is_none()
            || report
                .identity
                .server_encoder
                .as_ref()
                .is_none_or(String::is_empty))
    {
        return Err(invalid("saved verified identity is incomplete"));
    }
    for suite in &report.suites {
        validate_suite(suite, true)?;
        for record in &suite.records {
            parsed(record)?;
            if report.identity.verified
                && record.evaluation.as_ref().is_none_or(|evaluation| {
                    report
                        .identity
                        .model
                        .as_ref()
                        .is_none_or(|model| evaluation.response.model != model.as_str())
                })
            {
                return Err(invalid(
                    "saved response model differs from the report identity",
                ));
            }
        }
    }
    if report.suites.is_empty() {
        return Err(invalid("comparison report has no suites"));
    }
    observed_model(report)?;
    Ok(())
}

pub(crate) fn observed_model(report: &RunReport) -> Result<Option<&str>> {
    let mut model = None;
    for record in report.suites.iter().flat_map(|suite| &suite.records) {
        if let Some(evaluation) = &record.evaluation {
            let current = evaluation.response.model.as_str();
            if model.is_some_and(|observed| observed != current) {
                return Err(invalid(
                    "saved accuracy responses disagree on model identity",
                ));
            }
            model = Some(current);
        }
    }
    Ok(model)
}

pub(crate) fn validate_inputs(
    prepared: &[(String, Prepared)],
    plans: &[(&String, Option<RunReport>)],
) -> Result<()> {
    let reports = prepared
        .iter()
        .zip(plans)
        .map(|((_, prepared), (_, plan))| match prepared {
            Prepared::Saved(report) => Ok(report.as_ref()),
            Prepared::Live(_, _) => plan
                .as_ref()
                .ok_or_else(|| usage("live variant plan is missing")),
        })
        .collect::<Result<Vec<_>>>()?;
    let first = reports
        .first()
        .ok_or_else(|| usage("no evaluation variants"))?;
    for report in &reports {
        if let Some(error) = &report.error {
            return Err(Error::new(
                if error.code == 64 {
                    ErrorKind::Usage
                } else {
                    ErrorKind::Input
                },
                format!("variant plan failed: {}", error.message),
            ));
        }
        alignment(first, report)?;
    }
    Ok(())
}

fn validate_suite(suite: &SuiteReport, actual: bool) -> Result<()> {
    let selected = suite.selected_case_ids.iter().collect::<BTreeSet<_>>();
    let records = suite
        .records
        .iter()
        .map(|record| &record.case_id)
        .collect::<BTreeSet<_>>();
    let declared = match suite.partition.as_str() {
        "test" => &suite.splits.test,
        "calibrate" => &suite.splits.calibrate,
        "captured" => &suite.splits.captured,
        _ => return Err(invalid("comparison report partition is invalid")),
    };
    if suite.manifest.schema_version != 1
        || suite.splits.schema_version != 1
        || suite.splits.suite_id != suite.manifest.id
        || suite.splits.suite_revision != suite.manifest.revision
        || selected.is_empty()
        || selected.len() != suite.selected_case_ids.len()
        || records.len() != suite.records.len()
        || records != selected
        || declared.iter().collect::<BTreeSet<_>>() != selected
        || suite.splits.cases_blake3 != suite.manifest.cases.blake3
        || suite.splits.dataset_revision != suite.manifest.cases.revision
        || actual && suite.phases.iter().any(|phase| phase.error.is_some())
    {
        return Err(invalid(
            "comparison requires every fixed selected case exactly once with matching dataset and split provenance",
        ));
    }
    for record in &suite.records {
        if record.questions.is_empty()
            || record.questions.len() != record.policies.len()
            || record.questions.len() != record.binding.len()
            || record.questions.keys().any(|field| {
                !record.policies.contains_key(field) || !record.binding.contains_key(field)
            })
        {
            return Err(invalid(
                "comparison record lacks complete question, binding or policy metadata",
            ));
        }
        if record.binding.values().collect::<BTreeSet<_>>().len() != record.binding.len() {
            return Err(invalid(
                "comparison answer bindings must be unique per original case",
            ));
        }
        let signals = loupe_core::signals::Set::from_document(&record.effective_document)?;
        let ensembles = ensemble::set(record)?;
        let primary = ensemble::primary_questions(record)?;
        let labels = match &record.label {
            loupe_core::suites::Label::Fields(labels) => {
                if primary.keys().any(|id| !labels.contains_key(id))
                    || labels
                        .keys()
                        .any(|id| !record.questions.contains_key(id) || ensembles.contains(id))
                {
                    return Err(invalid(
                        "comparison label fields do not cover every question",
                    ));
                }
                labels
                    .iter()
                    .map(|(id, label)| (id.as_str(), label))
                    .collect::<BTreeMap<_, _>>()
            }
            label if primary.len() == 1 => BTreeMap::from([(
                record
                    .questions
                    .first()
                    .ok_or_else(|| invalid("comparison question missing"))?
                    .0
                    .as_str(),
                label,
            )]),
            _ => return Err(invalid("comparison multi-question label map is missing")),
        };
        for (id, question) in &record.questions {
            if let Some(label) = labels.get(id.as_str()) {
                loupe_core::task_metrics::validate_label(question, label)?;
            } else if signals.get(id).is_none() && !ensembles.contains(id) {
                return Err(invalid("comparison label missing"));
            }
            record
                .policies
                .get(id)
                .ok_or_else(|| invalid("comparison policy missing"))?
                .policy()?;
            let binding = record
                .binding
                .get(id)
                .ok_or_else(|| invalid("comparison binding missing"))?;
            compatible(
                question,
                record
                    .request
                    .questions
                    .get(binding)
                    .ok_or_else(|| invalid("bound request question missing"))?,
            )?;
        }
    }
    Ok(())
}

fn suite_key(suite: &SuiteReport) -> Result<String> {
    let value = json!({
        "cases": suite.manifest.cases,
        "fixtures": suite.manifest.fixtures,
        "task": suite.manifest.task,
        "partition": suite.partition,
        "split": suite.splits.config,
        "calibrate": suite.splits.calibrate,
        "test": suite.splits.test,
        "captured": suite.splits.captured,
    });
    Ok(
        blake3::hash(&serde_json::to_vec(&value).map_err(json_error)?)
            .to_hex()
            .to_string(),
    )
}
fn suites(report: &RunReport) -> Result<BTreeMap<String, &SuiteReport>> {
    let mut output = BTreeMap::new();
    for suite in &report.suites {
        validate_suite(suite, false)?;
        if output.insert(suite_key(suite)?, suite).is_some() {
            return Err(invalid(
                "suite provenance is ambiguous; compare one suite at a time when datasets/splits are identical",
            ));
        }
    }
    if output.is_empty() {
        return Err(invalid("comparison report has no suites"));
    }
    Ok(output)
}
type SuitePair<'a> = (&'a SuiteReport, &'a SuiteReport);
pub(crate) fn alignment<'a>(
    left: &'a RunReport,
    right: &'a RunReport,
) -> Result<Vec<SuitePair<'a>>> {
    let left = suites(left)?;
    let right = suites(right)?;
    if left.keys().ne(right.keys()) {
        return Err(invalid(
            "comparison dataset pins, fixture pins, partitions or fixed split assignments differ",
        ));
    }
    let mut output = Vec::new();
    for (key, left) in left {
        let right = right
            .get(&key)
            .copied()
            .ok_or_else(|| invalid("comparison suite missing"))?;
        let right_records = right
            .records
            .iter()
            .map(|record| (&record.case_id, record))
            .collect::<BTreeMap<_, _>>();
        for record in &left.records {
            let other = right_records
                .get(&record.case_id)
                .copied()
                .ok_or_else(|| invalid("paired case missing"))?;
            if record.label != other.label
                || record.source != other.source
                || record.reviewed != other.reviewed
                || record.sampling != other.sampling
            {
                return Err(invalid(
                    "paired cases have different labels or capture provenance",
                ));
            }
            fields(record, other)?;
        }
        output.push((left, right));
    }
    Ok(output)
}
type FieldPair<'a> = (String, &'a str, &'a str);
fn fields<'a>(left: &'a Record, right: &'a Record) -> Result<Vec<FieldPair<'a>>> {
    let left_ensemble = ensemble::set(left)?;
    let right_ensemble = ensemble::set(right)?;
    let left_signals = loupe_core::signals::Set::from_document(&left.effective_document)?;
    let right_signals = loupe_core::signals::Set::from_document(&right.effective_document)?;
    let labelled = |record: &Record, signals: &loupe_core::signals::Set, id: &str| {
        signals.get(id).is_none()
            || matches!(&record.label, loupe_core::suites::Label::Fields(labels) if labels.contains_key(id))
    };
    let left_fields = left
        .questions
        .iter()
        .filter(|(id, _)| !left_ensemble.contains(id) && labelled(left, &left_signals, id))
        .collect::<Vec<_>>();
    let right_fields = right
        .questions
        .iter()
        .filter(|(id, _)| !right_ensemble.contains(id) && labelled(right, &right_signals, id))
        .collect::<Vec<_>>();
    let single = left_fields.len() == 1
        && right_fields.len() == 1
        && !matches!(left.label, loupe_core::suites::Label::Fields(_))
        && !matches!(right.label, loupe_core::suites::Label::Fields(_));
    let mut output: Vec<FieldPair<'_>> = Vec::new();
    if single {
        let (left_id, left_question) = left_fields
            .first()
            .copied()
            .ok_or_else(|| invalid("left question missing"))?;
        let (right_id, right_question) = right_fields
            .first()
            .copied()
            .ok_or_else(|| invalid("right question missing"))?;
        compatible(left_question, right_question)?;
        output.push(("primary".to_owned(), left_id, right_id));
    } else {
        if left_fields
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<BTreeSet<_>>()
            != right_fields
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<BTreeSet<_>>()
        {
            return Err(invalid(
                "mixed comparison fields must have matching question IDs",
            ));
        }
        for (id, question) in left_fields {
            let other = right
                .questions
                .get(id)
                .ok_or_else(|| invalid("right question missing"))?;
            compatible(question, other)?;
            output.push((id.clone(), id, id));
        }
    }
    Ok(output)
}
fn compatible(left: &Question, right: &Question) -> Result<()> {
    left.validate()?;
    right.validate()?;
    if left.kind() != right.kind()
        || left
            .options()
            .iter()
            .map(|(key, _)| key)
            .collect::<BTreeSet<_>>()
            != right
                .options()
                .iter()
                .map(|(key, _)| key)
                .collect::<BTreeSet<_>>()
    {
        return Err(invalid(
            "comparison question types and declared label keys must match",
        ));
    }
    Ok(())
}

pub(crate) fn reports(
    left_name: &str,
    left: &RunReport,
    right_name: &str,
    right: &RunReport,
) -> Result<Report> {
    let mut report = Report {
        left: left_name.to_owned(),
        right: right_name.to_owned(),
        paired_fields: 0,
        fields: Vec::new(),
        disagreements: Vec::new(),
    };
    for (left, right) in alignment(left, right)? {
        let right_records = right
            .records
            .iter()
            .map(|record| (&record.case_id, record))
            .collect::<BTreeMap<_, _>>();
        let mut buckets = BTreeMap::<String, Bucket>::new();
        for record in &left.records {
            let other = right_records
                .get(&record.case_id)
                .copied()
                .ok_or_else(|| invalid("paired case missing"))?;
            let left_answers = parsed(record)?;
            let right_answers = parsed(other)?;
            for (field, left_id, right_id) in fields(record, other)? {
                let (left_decision, left_distribution) = decision(record, left_id, &left_answers)?;
                let (right_decision, right_distribution) =
                    decision(other, right_id, &right_answers)?;
                let bucket = buckets.entry(field.clone()).or_default();
                if record.ensemble.is_some() || other.ensemble.is_some() {
                    let left = raw_distribution(record, left_id)?;
                    let right = raw_distribution(other, right_id)?;
                    let raw = bucket.raw.get_or_insert_with(RawShift::default);
                    raw.cases += 1;
                    for (key, probability) in left {
                        let other = right
                            .get(&key)
                            .ok_or_else(|| invalid("raw paired option is missing"))?;
                        let delta = (probability - other).abs();
                        raw.maximum = raw.maximum.max(delta);
                        raw.sum += delta;
                        raw.probabilities += 1;
                    }
                }
                bucket.cases += 1;
                report.paired_fields += 1;
                if left_decision != right_decision {
                    bucket.changed += 1;
                    report.disagreements.push(Disagreement {
                        suite_left: left.manifest.id.clone(),
                        suite_right: right.manifest.id.clone(),
                        case_id: record.case_id.clone(),
                        field: field.clone(),
                        field_left: left_id.to_owned(),
                        field_right: right_id.to_owned(),
                        left: left_decision.clone(),
                        right: right_decision.clone(),
                    });
                }
                let key = (
                    serde_json::to_string(&left_decision).map_err(json_error)?,
                    serde_json::to_string(&right_decision).map_err(json_error)?,
                );
                bucket
                    .matrix
                    .entry(key)
                    .and_modify(|cell| cell.count += 1)
                    .or_insert(Cell {
                        left: left_decision,
                        right: right_decision,
                        count: 1,
                    });
                for (key, left) in left_distribution {
                    let right = right_distribution
                        .get(&key)
                        .ok_or_else(|| invalid("paired probability key missing"))?;
                    let delta = (left - right).abs();
                    bucket.maximum = bucket.maximum.max(delta);
                    bucket.sum += delta;
                    bucket.probabilities += 1;
                }
            }
        }
        for (field, bucket) in buckets {
            report.fields.push(Field {
                suite_left: left.manifest.id.clone(),
                suite_right: right.manifest.id.clone(),
                field,
                cases: bucket.cases,
                changed_decisions: bucket.changed,
                matrix: bucket.matrix.into_values().collect(),
                max_probability_difference: bucket.maximum,
                mean_probability_difference: bucket.sum / bucket.probabilities as f64,
                probabilities: bucket.probabilities,
                raw_primary_shift: bucket.raw.map(|raw| RawPrimaryShift {
                    cases: raw.cases,
                    max_probability_difference: raw.maximum,
                    mean_probability_difference: raw.sum / raw.probabilities as f64,
                    probabilities: raw.probabilities,
                }),
            });
        }
    }
    Ok(report)
}
fn decision(
    record: &Record,
    id: &str,
    answers: &indexmap::IndexMap<String, Answer>,
) -> Result<(Decision, BTreeMap<String, f64>)> {
    let binding = record
        .binding
        .get(id)
        .ok_or_else(|| invalid("answer binding missing"))?;
    let answer = answers
        .get(binding)
        .ok_or_else(|| invalid("bound answer missing"))?;
    let policy = record
        .policies
        .get(id)
        .ok_or_else(|| invalid("decision policy missing"))?
        .policy()?;
    let assessment = answer.assessment(&policy)?;
    let decision = if assessment.uncertain || ensemble::uncertain(record, id)? {
        Decision::Uncertain
    } else {
        match assessment.selected {
            Selected::Boolean(value) => Decision::Boolean(value),
            Selected::Choice(value) => Decision::Choice(value),
            Selected::Score(value) => Decision::Score(if value == 0.0 { 0.0 } else { value }),
        }
    };
    Ok((decision, distribution(answer)?))
}

fn raw_distribution(record: &Record, id: &str) -> Result<BTreeMap<String, f64>> {
    let binding = record
        .binding
        .get(id)
        .ok_or_else(|| invalid("raw primary binding is missing"))?;
    let answers = record
        .evaluation
        .as_ref()
        .ok_or_else(|| invalid("raw primary response is missing"))?
        .response
        .validate(&record.request)?;
    distribution(
        answers
            .get(binding)
            .ok_or_else(|| invalid("raw primary answer is missing"))?,
    )
}

fn distribution(answer: &Answer) -> Result<BTreeMap<String, f64>> {
    match answer {
        Answer::Noul { noul } => Ok(BTreeMap::from([
            ("false".to_owned(), 1.0 - noul),
            ("true".to_owned(), *noul),
        ])),
        Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => probabilities
            .iter()
            .map(|(key, probability)| {
                (
                    key.clone(),
                    Probability::new(*probability).map(|value| value.get()),
                )
            })
            .map(|(key, value)| value.map(|value| (key, value)))
            .collect(),
    }
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::run::eval::RunReport;
    use crate::run::eval::variants::compare::{reports, validate_saved};
    use serde_json::Value;

    #[test]
    fn arithmetic_comparison_separates_raw_primary_shift_from_aggregation_and_uncertainty() {
        use crate::run::eval::ensemble::{capture, primary_questions};
        use crate::run::eval::parsed;
        use crate::run::eval::variants::compare::{Decision, decision, fields, raw_distribution};
        let mut joint = crate::run::eval::ensemble::tests::arithmetic_record();
        let mut standalone = crate::run::eval::ensemble::tests::arithmetic_record();
        let primary = primary_questions(&standalone).unwrap();
        standalone.ensemble = None;
        standalone.questions = primary.clone();
        standalone.request.questions = primary.clone();
        standalone.policies.retain(|id, _| primary.contains_key(id));
        standalone.binding.retain(|id, _| primary.contains_key(id));
        standalone
            .evaluation
            .as_mut()
            .unwrap()
            .response
            .answers
            .retain(|id, _| primary.contains_key(id));
        assert_eq!(fields(&standalone, &joint).unwrap().len(), 3);
        let raw = raw_distribution(&standalone, "present").unwrap();
        assert_eq!(raw_distribution(&joint, "present").unwrap(), raw);
        let alone = decision(&standalone, "present", &parsed(&standalone).unwrap()).unwrap();
        let combined = decision(&joint, "present", &parsed(&joint).unwrap()).unwrap();
        assert_eq!(alone.0, Decision::Boolean(true));
        assert_eq!(combined.0, Decision::Uncertain);
        assert!((alone.1["true"] - combined.1["true"] - 0.295).abs() < 1e-12);
        joint.evaluation.as_mut().unwrap().response.answers["present"] =
            serde_json::json!({"type":"noul","noul":0.8});
        capture(&mut joint).unwrap();
        assert!(
            (raw["true"] - raw_distribution(&joint, "present").unwrap()["true"] - 0.19).abs()
                < 1e-12
        );
    }

    fn captured() -> RunReport {
        let evidence: Value = serde_json::from_str(include_str!(
            "../../../../../loupe-core/testdata/captured-eval.json"
        ))
        .unwrap();
        serde_json::from_value(evidence["checks"][2]["report"].clone()).unwrap()
    }
    #[test]
    fn captured_real_responses_pair_by_case_and_keep_uncertainty_distinct() {
        let left = captured();
        let mut right = captured();
        right.suites[0].records.reverse();
        let same = reports("left", &left, "right", &right).unwrap();
        assert_eq!(same.paired_fields, 12);
        assert!(same.disagreements.is_empty());
        assert!(
            same.fields
                .iter()
                .all(|field| field.max_probability_difference == 0.0)
        );
        for record in &mut right.suites[0].records {
            record.policies.get_mut("available").unwrap().margin = 1.0;
        }
        let changed = reports("left", &left, "right", &right).unwrap();
        assert_eq!(changed.disagreements.len(), 4);
        assert_eq!(
            changed
                .fields
                .iter()
                .map(|field| field.cases)
                .sum::<usize>(),
            12
        );
        assert!(
            changed
                .fields
                .iter()
                .all(|field| field.max_probability_difference == 0.0)
        );
        assert!(validate_saved(&left, false).is_err());
        validate_saved(&left, true).unwrap();
    }
    #[test]
    fn mismatched_fixed_case_provenance_and_incomplete_runs_are_rejected() {
        let left = captured();
        let mut right = captured();
        right.suites[0].splits.config.seed += 1;
        assert!(reports("left", &left, "right", &right).is_err());
        let mut right = captured();
        right.suites[0].records[0].label = loupe_core::suites::Label::Noul(false);
        assert!(reports("left", &left, "right", &right).is_err());
        let mut right = captured();
        right.suites[0].records.pop();
        assert!(reports("left", &left, "right", &right).is_err());
        let mut right = captured();
        right.status = "incomplete".to_owned();
        assert!(validate_saved(&right, true).is_err());
        let mut right = captured();
        // A malformed saved artifact, not a simulated server identity.
        right.suites[0].records[0]
            .evaluation
            .as_mut()
            .unwrap()
            .response
            .model = "Cloudflare/clef-flash".to_owned();
        assert!(validate_saved(&right, true).is_err());
    }

    #[test]
    fn actual_unit_and_joint_window_reports_compare_complete_mixed_fields() {
        let left = captured();
        let evidence: Value = serde_json::from_str(include_str!(
            "../../../../../loupe-core/testdata/captured-eval.json"
        ))
        .unwrap();
        let right: RunReport =
            serde_json::from_value(evidence["checks"][3]["report"].clone()).unwrap();
        let comparison = reports("unit", &left, "window:4", &right).unwrap();
        assert_eq!(comparison.paired_fields, 12);
        assert_eq!(comparison.fields.len(), 3);
        assert!(
            comparison
                .fields
                .iter()
                .any(|field| field.max_probability_difference > 0.0)
        );
        assert_eq!(
            comparison
                .fields
                .iter()
                .map(|field| field.matrix.iter().map(|cell| cell.count).sum::<usize>())
                .sum::<usize>(),
            12
        );
    }
}
