use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Policy, Probability, QuestionKind, Selected};
use crate::suites::Label;
use crate::task_metrics::Observation;
use crate::wire::Question;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub minimum_precision: f64,
}
impl FromStr for Target {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let (name, precision) = value
            .split_once('=')
            .ok_or_else(|| invalid("target must be precision=RATE or precision:LABEL=RATE"))?;
        let label = if name == "precision" {
            None
        } else {
            Some(
                name.strip_prefix("precision:")
                    .filter(|label| !label.is_empty())
                    .ok_or_else(|| {
                        invalid("target must be precision=RATE or precision:LABEL=RATE")
                    })?
                    .to_owned(),
            )
        };
        let minimum_precision = precision
            .parse::<f64>()
            .map_err(|_| invalid("target precision must be a finite probability"))?;
        Probability::new(minimum_precision)?;
        Ok(Self {
            label,
            minimum_precision,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct Fixed {
    pub threshold: Option<f64>,
    pub margin: Option<f64>,
    pub min_confidence: Option<f64>,
    pub min_gap: Option<f64>,
}
pub struct Options {
    pub targets: Vec<Target>,
    pub max_uncertain: Option<f64>,
    pub minimum_support: usize,
    pub fixed: Fixed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub threshold: f64,
    pub margin: f64,
    pub min_confidence: f64,
    pub min_gap: f64,
}
impl From<Policy> for Parameters {
    fn from(value: Policy) -> Self {
        Self {
            threshold: value.threshold.get(),
            margin: value.margin,
            min_confidence: value.min_confidence.get(),
            min_gap: value.min_gap,
        }
    }
}
impl Parameters {
    pub fn policy(&self) -> Result<Policy> {
        let policy = Policy {
            threshold: Probability::new(self.threshold)?,
            margin: self.margin,
            min_confidence: Probability::new(self.min_confidence)?,
            min_gap: self.min_gap,
        };
        policy.validate()?;
        Ok(policy)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Support {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub minimum_precision: f64,
    pub predicted: usize,
    pub correct: usize,
    pub observed_precision: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub parameters: Parameters,
    pub cases: usize,
    pub accepted: usize,
    pub correct: usize,
    pub coverage: f64,
    pub selective_accuracy: f64,
    pub targets: Vec<Support>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fit {
    pub cases: usize,
    pub candidates: usize,
    pub feasible_candidates: usize,
    pub selected: Option<Candidate>,
    pub objective: String,
}

#[derive(Clone)]
struct Row {
    value: f64,
    truth: String,
    predicted: String,
    confidence: f64,
    gap: f64,
    excluded: bool,
}
#[derive(Default)]
struct Counts {
    accepted: usize,
    correct: usize,
    classes: BTreeMap<String, (usize, usize)>,
}
impl Counts {
    fn add(&mut self, row: &Row) {
        self.accepted += 1;
        self.correct += usize::from(row.truth == row.predicted);
        let class = self.classes.entry(row.predicted.clone()).or_default();
        class.0 += 1;
        class.1 += usize::from(row.truth == row.predicted);
    }
    fn remove(&mut self, row: &Row) -> Result<()> {
        self.accepted = self
            .accepted
            .checked_sub(1)
            .ok_or_else(|| invalid("calibration accepted-count underflow"))?;
        self.correct = self
            .correct
            .checked_sub(usize::from(row.truth == row.predicted))
            .ok_or_else(|| invalid("calibration correct-count underflow"))?;
        let class = self
            .classes
            .get_mut(&row.predicted)
            .ok_or_else(|| invalid("calibration class missing"))?;
        class.0 = class
            .0
            .checked_sub(1)
            .ok_or_else(|| invalid("calibration predicted-count underflow"))?;
        class.1 = class
            .1
            .checked_sub(usize::from(row.truth == row.predicted))
            .ok_or_else(|| invalid("calibration correct-class underflow"))?;
        Ok(())
    }
}

pub fn fit(
    question: &Question,
    base: Policy,
    observations: &[Observation<'_>],
    options: &Options,
) -> Result<Fit> {
    fit_with_uncertainty(question, base, observations, options, &BTreeSet::new())
}

/// These case gates remain uncertain under every fitted decision threshold.
pub fn fit_with_uncertainty(
    question: &Question,
    base: Policy,
    observations: &[Observation<'_>],
    options: &Options,
    additional_uncertain: &BTreeSet<&str>,
) -> Result<Fit> {
    question.validate()?;
    base.validate()?;
    if question.kind() == QuestionKind::Score {
        return Err(invalid("calibrate supports noul and choice rubrics"));
    }
    if options.targets.is_empty() && options.max_uncertain.is_none() {
        return Err(invalid("calibration requires --target or --max-uncertain"));
    }
    if options.minimum_support == 0 {
        return Err(invalid("minimum calibration support must be positive"));
    }
    if let Some(maximum) = options.max_uncertain {
        Probability::new(maximum)?;
    }
    for value in [
        options.fixed.threshold,
        options.fixed.margin,
        options.fixed.min_confidence,
        options.fixed.min_gap,
    ]
    .into_iter()
    .flatten()
    {
        Probability::new(value)?;
    }
    if question.kind() == QuestionKind::Noul
        && (options.fixed.min_confidence.is_some() || options.fixed.min_gap.is_some())
        || question.kind() == QuestionKind::Choice
            && (options.fixed.threshold.is_some() || options.fixed.margin.is_some())
    {
        return Err(invalid(
            "fixed calibration parameters do not apply to this question type",
        ));
    }
    let mut targets = options.targets.clone();
    let choices = question
        .options()
        .into_iter()
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    for target in &mut targets {
        Probability::new(target.minimum_precision)?;
        if question.kind() == QuestionKind::Noul {
            target.label = Some(
                match target.label.as_deref().unwrap_or("true") {
                    "yes" | "true" => "true",
                    "no" | "false" => "false",
                    _ => {
                        return Err(invalid(
                            "noul precision labels must be true/false or yes/no",
                        ));
                    }
                }
                .to_owned(),
            );
        } else if target
            .label
            .as_ref()
            .is_some_and(|label| !choices.contains(label))
        {
            return Err(invalid("precision target label is not a declared option"));
        }
    }
    if targets
        .iter()
        .map(|target| &target.label)
        .collect::<BTreeSet<_>>()
        .len()
        != targets.len()
    {
        return Err(invalid("precision targets must name distinct labels"));
    }
    let mut ids = BTreeSet::new();
    let mut rows = Vec::new();
    for observation in observations {
        if observation.id.is_empty() || !ids.insert(observation.id) {
            return Err(invalid("calibration cases must have unique nonempty IDs"));
        }
        crate::task_metrics::validate_label(question, observation.label)?;
        observation.answer.validate(question)?;
        let assessment = observation.answer.assessment(&Policy::default())?;
        let truth = match observation.label {
            Label::Noul(value) => value.to_string(),
            Label::Choice(value) => value.clone(),
            _ => return Err(invalid("calibration requires point noul/choice labels")),
        };
        let predicted = match assessment.selected {
            Selected::Boolean(value) => value.to_string(),
            Selected::Choice(value) => value,
            Selected::Score(_) => return Err(invalid("score calibration is unsupported")),
        };
        let value = match observation.answer {
            crate::wire::Answer::Noul { noul } => *noul,
            _ => assessment.confidence.get(),
        };
        rows.push(Row {
            value,
            truth,
            predicted,
            confidence: assessment.confidence.get(),
            gap: assessment.gap.unwrap_or(0.0),
            excluded: assessment.uncertain || additional_uncertain.contains(observation.id),
        });
    }
    if additional_uncertain.iter().any(|id| !ids.contains(id)) {
        return Err(invalid(
            "additional calibration uncertainty names an unknown case",
        ));
    }
    let mut fit = Fit { cases: rows.len(), candidates: 0, feasible_candidates: 0, selected: None, objective: if targets.is_empty() { "maximize empirical selective accuracy subject to coverage, then maximize accepted support" } else { "maximize accepted support subject to empirical precision/coverage targets, then maximize correct decisions" }.to_owned() };
    if rows.is_empty() {
        return Ok(fit);
    }
    if question.kind() == QuestionKind::Noul {
        for threshold in thresholds(&rows, base.threshold.get(), options.fixed.threshold)? {
            let mut sorted = rows
                .iter()
                .filter(|row| !row.excluded)
                .cloned()
                .collect::<Vec<_>>();
            for row in &mut sorted {
                row.predicted = (row.value >= threshold).to_string();
                row.value = (row.value - threshold).abs();
            }
            sorted.sort_by(|left, right| left.value.total_cmp(&right.value));
            let mut policy = base;
            policy.threshold = Probability::new(threshold)?;
            search(&sorted, &targets, options, policy, base, true, &mut fit)?;
        }
    } else {
        for confidence in values(
            rows.iter().map(|row| row.confidence),
            base.min_confidence.get(),
            options.fixed.min_confidence,
        )? {
            let mut sorted = rows
                .iter()
                .filter(|row| !row.excluded && row.confidence >= confidence)
                .cloned()
                .collect::<Vec<_>>();
            for row in &mut sorted {
                row.value = row.gap;
            }
            sorted.sort_by(|left, right| left.value.total_cmp(&right.value));
            let mut policy = base;
            policy.min_confidence = Probability::new(confidence)?;
            search(&sorted, &targets, options, policy, base, false, &mut fit)?;
        }
    }
    Ok(fit)
}

fn values(values: impl Iterator<Item = f64>, base: f64, fixed: Option<f64>) -> Result<Vec<f64>> {
    if let Some(value) = fixed {
        Probability::new(value)?;
        return Ok(vec![value]);
    }
    let mut result = vec![0.0, 1.0, base];
    for value in values {
        result.push(value);
        if value.next_up() <= 1.0 {
            result.push(value.next_up());
        }
    }
    result.sort_by(f64::total_cmp);
    result.dedup();
    Ok(result)
}

fn thresholds(rows: &[Row], base: f64, fixed: Option<f64>) -> Result<Vec<f64>> {
    let mut values = values(rows.iter().map(|row| row.value), base, fixed)?;
    if fixed.is_some() {
        return Ok(values);
    }
    let mut probabilities = rows.iter().map(|row| row.value).collect::<Vec<_>>();
    probabilities.sort_by(f64::total_cmp);
    probabilities.dedup();
    // Absolute-distance order changes at pair midpoints. These events are needed
    // to fit a symmetric uncertainty band whose center is not an observed p.
    for (index, left) in probabilities.iter().enumerate() {
        for right in probabilities.iter().skip(index + 1) {
            let midpoint = (left + right) / 2.0;
            values.push(midpoint);
            if midpoint.next_up() <= 1.0 {
                values.push(midpoint.next_up());
            }
            if midpoint.next_down() >= 0.0 {
                values.push(midpoint.next_down());
            }
        }
    }
    values.sort_by(f64::total_cmp);
    values.dedup();
    Ok(values)
}

fn search(
    rows: &[Row],
    targets: &[Target],
    options: &Options,
    mut policy: Policy,
    base: Policy,
    noul: bool,
    fit: &mut Fit,
) -> Result<()> {
    let mut counts = Counts::default();
    for row in rows {
        counts.add(row);
    }
    let fixed = if noul {
        options.fixed.margin
    } else {
        options.fixed.min_gap
    };
    let base_gate = if noul { base.margin } else { base.min_gap };
    let candidates = values(rows.iter().map(|row| row.value), base_gate, fixed)?;
    let mut removed = 0;
    for gate in candidates {
        while rows.get(removed).is_some_and(|row| row.value < gate) {
            counts.remove(
                rows.get(removed)
                    .ok_or_else(|| invalid("calibration sorted row missing"))?,
            )?;
            removed += 1;
        }
        if noul {
            policy.margin = gate;
        } else {
            policy.min_gap = gate;
        }
        consider(&counts, targets, options, policy, base, fit)?;
    }
    Ok(())
}

fn consider(
    counts: &Counts,
    targets: &[Target],
    options: &Options,
    policy: Policy,
    base: Policy,
    fit: &mut Fit,
) -> Result<()> {
    policy.validate()?;
    fit.candidates = fit
        .candidates
        .checked_add(1)
        .ok_or_else(|| invalid("calibration candidate-count overflow"))?;
    if counts.accepted < options.minimum_support {
        return Ok(());
    }
    let coverage = counts.accepted as f64 / fit.cases as f64;
    if options
        .max_uncertain
        .is_some_and(|maximum| (fit.cases - counts.accepted) as f64 / fit.cases as f64 > maximum)
    {
        return Ok(());
    }
    let mut support = Vec::new();
    for target in targets {
        let (predicted, correct) = target
            .label
            .as_ref()
            .map_or((counts.accepted, counts.correct), |label| {
                counts.classes.get(label).copied().unwrap_or_default()
            });
        if predicted < options.minimum_support
            || correct as f64 / (predicted as f64) < target.minimum_precision
        {
            return Ok(());
        }
        support.push(Support {
            label: target.label.clone(),
            minimum_precision: target.minimum_precision,
            predicted,
            correct,
            observed_precision: correct as f64 / predicted as f64,
        });
    }
    fit.feasible_candidates += 1;
    let candidate = Candidate {
        parameters: policy.into(),
        cases: fit.cases,
        accepted: counts.accepted,
        correct: counts.correct,
        coverage,
        selective_accuracy: counts.correct as f64 / counts.accepted as f64,
        targets: support,
    };
    let preferred = fit.selected.as_ref().is_none_or(|previous| {
        let primary = if targets.is_empty() {
            candidate
                .selective_accuracy
                .total_cmp(&previous.selective_accuracy)
                .then(candidate.accepted.cmp(&previous.accepted))
        } else {
            candidate
                .accepted
                .cmp(&previous.accepted)
                .then(candidate.correct.cmp(&previous.correct))
        };
        primary.is_gt()
            || primary.is_eq()
                && drift(&candidate.parameters, base) < drift(&previous.parameters, base)
    });
    if preferred {
        fit.selected = Some(candidate);
    }
    Ok(())
}
fn drift(parameters: &Parameters, base: Policy) -> f64 {
    (parameters.threshold - base.threshold.get()).abs()
        + (parameters.margin - base.margin).abs()
        + (parameters.min_confidence - base.min_confidence.get()).abs()
        + (parameters.min_gap - base.min_gap).abs()
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::calibration::{Fixed, Options, Target, fit};
    use crate::policy::Policy;
    use crate::suites::Label;
    use crate::task_metrics::Observation;
    use crate::wire::{Answer, Question};

    fn options(targets: &[&str]) -> Options {
        Options {
            targets: targets.iter().map(|value| value.parse().unwrap()).collect(),
            max_uncertain: None,
            minimum_support: 1,
            fixed: Fixed::default(),
        }
    }
    fn noul(data: &[(f64, bool)], options: &Options) -> crate::calibration::Fit {
        let question: Question = serde_json::from_value(json!({"type":"noul"})).unwrap();
        let labels = data
            .iter()
            .map(|(_, label)| Label::Noul(*label))
            .collect::<Vec<_>>();
        let answers = data
            .iter()
            .map(|(probability, _)| Answer::Noul { noul: *probability })
            .collect::<Vec<_>>();
        let ids = (0..data.len())
            .map(|index| index.to_string())
            .collect::<Vec<_>>();
        let observations = ids
            .iter()
            .zip(&labels)
            .zip(&answers)
            .map(|((id, label), answer)| Observation { id, label, answer })
            .collect::<Vec<_>>();
        let fitted = fit(&question, Policy::default(), &observations, options).unwrap();
        if let Some(candidate) = &fitted.selected {
            let policy = candidate.parameters.policy().unwrap();
            let actual = observations
                .iter()
                .map(|row| row.answer.assessment(&policy).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                actual.iter().filter(|decision| !decision.uncertain).count(),
                candidate.accepted
            );
        }
        fitted
    }

    #[test]
    fn permanent_uncertainty_limits_every_noul_and_choice_candidate_and_coverage() {
        // Pure decision-search arithmetic, with no model or HTTP emulation.
        for choice in [false, true] {
            let question = if choice {
                Question::Choice {
                    instructions: None,
                    criteria: indexmap::IndexMap::from([
                        ("a".into(), json!("A")),
                        ("b".into(), json!("B")),
                    ]),
                }
            } else {
                Question::Noul {
                    instructions: None,
                    criteria: None,
                }
            };
            let probabilities = [0.99, 0.9, 0.15, 0.2];
            let labels = probabilities
                .iter()
                .map(|p| {
                    if choice {
                        Label::Choice(if *p > 0.5 { "a" } else { "b" }.into())
                    } else {
                        Label::Noul(*p > 0.5)
                    }
                })
                .collect::<Vec<_>>();
            let answers = probabilities
                .iter()
                .map(|p| {
                    if choice {
                        Answer::Choice {
                            choice: if *p > 0.5 { "a" } else { "b" }.into(),
                            confidence: if *p > 0.5 { *p } else { 1.0 - p },
                            probabilities: BTreeMap::from([
                                ("a".into(), *p),
                                ("b".into(), 1.0 - p),
                            ]),
                        }
                    } else {
                        Answer::Noul { noul: *p }
                    }
                })
                .collect::<Vec<_>>();
            let ids = ["a", "b", "c", "d"];
            let observations = ids
                .iter()
                .zip(&labels)
                .zip(&answers)
                .map(|((id, label), answer)| Observation { id, label, answer })
                .collect::<Vec<_>>();
            let excluded = std::collections::BTreeSet::from(["a"]);
            let mut search = options(&["precision=1"]);
            search.max_uncertain = Some(0.25);
            let fitted = crate::calibration::fit_with_uncertainty(
                &question,
                Policy::default(),
                &observations,
                &search,
                &excluded,
            )
            .unwrap();
            assert_eq!(fitted.cases, 4);
            let candidate = fitted.selected.unwrap();
            assert_eq!(candidate.accepted, 3);
            assert_eq!(candidate.correct, 3);
            assert_eq!(candidate.coverage, 0.75);
            let policy = candidate.parameters.policy().unwrap();
            let accepted = observations
                .iter()
                .filter(|observation| {
                    !excluded.contains(observation.id)
                        && !observation.answer.assessment(&policy).unwrap().uncertain
                })
                .count();
            assert_eq!(accepted, 3);
            search.max_uncertain = Some(0.24);
            assert!(
                crate::calibration::fit_with_uncertainty(
                    &question,
                    Policy::default(),
                    &observations,
                    &search,
                    &excluded
                )
                .unwrap()
                .selected
                .is_none()
            );
            assert!(
                crate::calibration::fit_with_uncertainty(
                    &question,
                    Policy::default(),
                    &observations,
                    &search,
                    &std::collections::BTreeSet::from(["unknown"])
                )
                .is_err()
            );
        }
    }

    #[test]
    fn positive_precision_changes_threshold_without_vacuous_zero_support() {
        let data = [(0.1, false), (0.55, false), (0.9, true), (0.95, true)];
        let fitted = noul(&data, &options(&["precision=1"]));
        let selected = fitted.selected.unwrap();
        assert_eq!(selected.accepted, 4);
        assert_eq!(selected.correct, 4);
        assert_eq!(selected.targets[0].label.as_deref(), Some("true"));
        assert_eq!(selected.targets[0].predicted, 2);
        assert_eq!(selected.targets[0].observed_precision, 1.0);
        assert!(selected.parameters.threshold > 0.55);
        let absent = noul(&[(0.1, false), (0.9, false)], &options(&["precision=1"]));
        assert!(absent.selected.is_none());
        assert_eq!(absent.feasible_candidates, 0);
    }

    #[test]
    fn two_label_targets_need_a_center_between_observed_probabilities() {
        let data = [(0.01, false), (0.02, true), (0.5, false), (0.51, true)];
        let fitted = noul(&data, &options(&["precision:yes=1", "precision:no=1"]));
        let selected = fitted.selected.unwrap();
        assert_eq!(selected.accepted, 2);
        assert_eq!(selected.correct, 2);
        assert_eq!(selected.coverage, 0.5);
        assert!(selected.parameters.threshold > 0.25 && selected.parameters.threshold < 0.27);
        assert!(
            selected
                .targets
                .iter()
                .all(|target| target.predicted == 1 && target.correct == 1)
        );
    }

    #[test]
    fn fixed_threshold_keeps_equality_yes_and_searches_strict_margin_boundaries() {
        let data = [(0.25, false), (0.5, false), (0.75, true)];
        let mut settings = options(&["precision=1"]);
        settings.fixed.threshold = Some(0.5);
        settings.fixed.margin = Some(0.25);
        let selected = noul(&data, &settings).selected.unwrap();
        assert_eq!(selected.accepted, 2);
        assert_eq!(selected.parameters.threshold, 0.5);
        assert_eq!(selected.parameters.margin, 0.25);
        settings.fixed.margin = Some(0.0);
        assert!(noul(&data, &settings).selected.is_none());
    }

    #[test]
    fn maximum_uncertain_uses_the_actual_fraction_and_optimizes_selective_accuracy() {
        let mut settings = options(&[]);
        settings.max_uncertain = Some(0.3);
        settings.fixed.threshold = Some(0.5);
        let data = [
            (0.01, false),
            (0.02, false),
            (0.03, false),
            (0.04, false),
            (0.5, false),
            (0.5, false),
            (0.5, false),
            (0.97, true),
            (0.98, true),
            (0.99, true),
        ];
        let selected = noul(&data, &settings).selected.unwrap();
        assert_eq!(selected.accepted, 7);
        assert_eq!(selected.correct, 7);
        assert_eq!(selected.selective_accuracy, 1.0);
    }

    #[test]
    fn choice_precision_can_use_gap_and_never_accepts_an_exact_tie() {
        let question: Question =
            serde_json::from_value(json!({"type":"choice","criteria":{"a":"A","b":"B","c":"C"}}))
                .unwrap();
        let labels = [
            Label::Choice("a".to_owned()),
            Label::Choice("b".to_owned()),
            Label::Choice("a".to_owned()),
            Label::Choice("a".to_owned()),
        ];
        let distributions = [
            BTreeMap::from([("a", 0.8), ("b", 0.1), ("c", 0.1)]),
            BTreeMap::from([("a", 0.6), ("b", 0.39), ("c", 0.01)]),
            BTreeMap::from([("a", 0.6), ("b", 0.2), ("c", 0.2)]),
            BTreeMap::from([("a", 0.5), ("b", 0.5), ("c", 0.0)]),
        ];
        let answers = distributions
            .into_iter()
            .map(|probabilities| Answer::Choice {
                choice: "a".to_owned(),
                confidence: *probabilities.get("a").unwrap(),
                probabilities: probabilities
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let ids = ["one", "two", "three", "tie"];
        let observations = ids
            .iter()
            .zip(&labels)
            .zip(&answers)
            .map(|((id, label), answer)| Observation { id, label, answer })
            .collect::<Vec<_>>();
        let fitted = fit(
            &question,
            Policy::default(),
            &observations,
            &options(&["precision:a=1"]),
        )
        .unwrap();
        let selected = fitted.selected.unwrap();
        assert_eq!(selected.accepted, 2);
        assert_eq!(selected.correct, 2);
        let policy = selected.parameters.policy().unwrap();
        assert!(answers[1].assessment(&policy).unwrap().uncertain);
        assert!(answers[3].assessment(&policy).unwrap().uncertain);
        assert!(!answers[2].assessment(&policy).unwrap().uncertain);
    }

    #[test]
    fn invalid_targets_support_and_fixed_flags_are_rejected_even_without_samples() {
        for value in [
            "precision=NaN",
            "precision=1.1",
            "recall=0.9",
            "precision:=0.9",
            "precision",
        ] {
            assert!(value.parse::<Target>().is_err());
        }
        let question: Question = serde_json::from_value(json!({"type":"noul"})).unwrap();
        let mut settings = options(&["precision=0.9"]);
        settings.fixed.min_gap = Some(0.1);
        assert!(fit(&question, Policy::default(), &[], &settings).is_err());
        settings.fixed = Fixed::default();
        settings.minimum_support = 0;
        assert!(fit(&question, Policy::default(), &[], &settings).is_err());
        settings.minimum_support = 1;
        settings.targets.push("precision:yes=0.95".parse().unwrap());
        assert!(fit(&question, Policy::default(), &[], &settings).is_err());
    }

    #[test]
    fn captured_server_probabilities_fit_without_inventing_model_answers() {
        let evidence: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-eval.json")).unwrap();
        let records = evidence["checks"][2]["report"]["suites"][0]["records"]
            .as_array()
            .unwrap();
        let data = records
            .iter()
            .map(|record| {
                (
                    record["evaluation"]["response"]["answers"]["available"]["noul"]
                        .as_f64()
                        .unwrap(),
                    record["label"]["available"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let selected = noul(&data, &options(&["precision=0"])).selected.unwrap();
        assert_eq!(selected.accepted, 4);
        let policy = selected.parameters.policy().unwrap();
        let decisions = data
            .iter()
            .map(|(probability, _)| {
                policy
                    .noul(crate::policy::Probability::new(*probability).unwrap())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let predicted = decisions
            .iter()
            .filter(|assessment| {
                matches!(assessment.selected, crate::policy::Selected::Boolean(true))
                    && !assessment.uncertain
            })
            .count();
        let correct = decisions
            .iter()
            .zip(&data)
            .filter(|(assessment, (_, truth))| {
                matches!(assessment.selected, crate::policy::Selected::Boolean(true))
                    && !assessment.uncertain
                    && *truth
            })
            .count();
        assert_eq!(selected.targets[0].predicted, predicted);
        assert_eq!(selected.targets[0].correct, correct);
    }

    #[test]
    fn exact_boundary_candidates_dominate_an_independent_dense_policy_grid() {
        let probabilities = [0.03, 0.16, 0.31, 0.53, 0.72, 0.91];
        for mask in 0_u32..64 {
            let data = probabilities
                .iter()
                .enumerate()
                .map(|(index, probability)| (*probability, mask & (1 << index) != 0))
                .collect::<Vec<_>>();
            let settings = options(&["precision:true=0.75", "precision:false=0.75"]);
            let fitted = noul(&data, &settings);
            let mut maximum_accepted = 0;
            let mut maximum_correct = 0;
            for threshold in 0..=20 {
                for margin in 0..=20 {
                    let policy = crate::policy::Policy {
                        threshold: crate::policy::Probability::new(f64::from(threshold) / 20.0)
                            .unwrap(),
                        margin: f64::from(margin) / 20.0,
                        ..crate::policy::Policy::default()
                    };
                    let mut predicted = [0_usize; 2];
                    let mut correct = [0_usize; 2];
                    for (probability, truth) in &data {
                        let assessment = policy
                            .noul(crate::policy::Probability::new(*probability).unwrap())
                            .unwrap();
                        if !assessment.uncertain
                            && let crate::policy::Selected::Boolean(value) = assessment.selected
                        {
                            let index = usize::from(value);
                            predicted[index] += 1;
                            correct[index] += usize::from(value == *truth);
                        }
                    }
                    if predicted.contains(&0)
                        || (0..2)
                            .any(|index| correct[index] as f64 / (predicted[index] as f64) < 0.75)
                    {
                        continue;
                    }
                    let accepted = predicted.iter().sum();
                    let correct = correct.iter().sum();
                    if (accepted, correct) > (maximum_accepted, maximum_correct) {
                        maximum_accepted = accepted;
                        maximum_correct = correct;
                    }
                }
            }
            if maximum_accepted > 0 {
                let selected = fitted.selected.unwrap();
                assert!(
                    (selected.accepted, selected.correct) >= (maximum_accepted, maximum_correct),
                    "mask={mask}"
                );
            }
        }
    }
}
