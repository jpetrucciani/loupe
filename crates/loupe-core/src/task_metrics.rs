use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Policy, Probability, Selected};
use crate::suites::{Label, Metric, Task};
use crate::wire::{Answer, Question};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Undefined {
    NoSamples,
    NoPointLabels,
    NoAcceptedDecisions,
    NoPositiveLabels,
    NoNegativeLabels,
    NoPredictedLabels,
    NoTrueLabels,
    NoLabelOutcomes,
    UndefinedClassF1,
    ZeroElapsed,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Value {
    pub value: Option<f64>,
    pub support: usize,
    pub undefined: Option<Undefined>,
}
impl Value {
    fn defined(value: f64, support: usize) -> Self {
        Self {
            value: Some(value),
            support,
            undefined: None,
        }
    }
    fn missing(reason: Undefined, support: usize) -> Self {
        Self {
            value: None,
            support,
            undefined: Some(reason),
        }
    }
    fn mean(sum: f64, support: usize, reason: Undefined) -> Self {
        if support == 0 {
            Self::missing(reason, 0)
        } else {
            Self::defined(sum / support as f64, support)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationKind {
    PositiveProbability,
    TopLabel,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BrierScale {
    Binary,
    Multiclass,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bin {
    pub lower: f64,
    pub upper: f64,
    pub lower_inclusive: bool,
    pub count: usize,
    pub predicted: Option<f64>,
    pub observed: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub label: String,
    pub support: usize,
    pub predicted: usize,
    pub true_positive: usize,
    pub false_positive: usize,
    pub false_negative: usize,
    pub precision: Value,
    pub recall: Value,
    pub f1: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selective {
    pub min_confidence: f64,
    pub accepted: usize,
    pub coverage: Value,
    pub accuracy: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub task: Task,
    pub cases: usize,
    pub point_labels: usize,
    pub ambiguous_labels: usize,
    pub accepted: usize,
    pub accuracy: Value,
    pub macro_f1: Value,
    pub f1: Value,
    pub auroc: Value,
    pub brier: Value,
    pub brier_scale: BrierScale,
    pub ece: Value,
    pub calibration_kind: CalibrationKind,
    pub reliability: Vec<Bin>,
    pub coverage: Value,
    pub selective_accuracy: Value,
    pub confidence_gates: Vec<Selective>,
    pub mae: Value,
    pub mae_bounds: Option<Bounds>,
    pub within_one_accuracy: Value,
    pub per_label: Vec<Class>,
}
impl Report {
    pub fn metric(&self, metric: Metric) -> Result<&Value> {
        match metric {
            Metric::Accuracy => Ok(&self.accuracy),
            Metric::MacroF1 => Ok(&self.macro_f1),
            Metric::F1 if self.task == Task::Noul => Ok(&self.f1),
            Metric::Auroc if self.task == Task::Noul => Ok(&self.auroc),
            Metric::Brier => Ok(&self.brier),
            Metric::Ece => Ok(&self.ece),
            Metric::Coverage => Ok(&self.coverage),
            Metric::SelectiveAccuracy => Ok(&self.selective_accuracy),
            Metric::Mae if self.task == Task::Score => Ok(&self.mae),
            Metric::WithinOneAccuracy if self.task == Task::Score => Ok(&self.within_one_accuracy),
            _ => Err(Error::new(
                ErrorKind::Usage,
                format!("metric {metric:?} is not applicable to this task report"),
            )),
        }
    }
}

pub struct Observation<'a> {
    pub id: &'a str,
    pub label: &'a Label,
    pub answer: &'a Answer,
}
pub fn validate_label(question: &Question, label: &Label) -> Result<()> {
    question.validate()?;
    let task = match question {
        Question::Noul { .. } => Task::Noul,
        Question::Choice { .. } => Task::Choice,
        Question::Score { .. } => Task::Score,
    };
    let labels = if task == Task::Noul {
        vec!["false".to_owned(), "true".to_owned()]
    } else {
        question.options().into_iter().map(|(id, _)| id).collect()
    };
    target(label, task, &labels).map(|_| ())
}
struct Sample {
    predicted: usize,
    point: Option<usize>,
    correct: bool,
    confidence: f64,
    uncertain: bool,
    static_uncertain: bool,
    binary: Option<(f64, bool)>,
    brier: Option<f64>,
    calibration: Option<(f64, bool)>,
    distance: Option<(f64, f64)>,
}

pub fn report(
    question: &Question,
    policy: &Policy,
    observations: &[Observation<'_>],
    gates: &[f64],
) -> Result<Report> {
    report_with_uncertainty(question, policy, observations, gates, &BTreeSet::new())
}

/// Additional uncertainty gates persist across the confidence coverage curve.
pub fn report_with_uncertainty(
    question: &Question,
    policy: &Policy,
    observations: &[Observation<'_>],
    gates: &[f64],
    additional_uncertain: &BTreeSet<&str>,
) -> Result<Report> {
    question.validate()?;
    policy.validate()?;
    let gates = gates
        .iter()
        .map(|gate| Probability::new(*gate).map(Probability::get))
        .collect::<Result<Vec<_>>>()?;
    if gates.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid("confidence gates must be unique and increasing"));
    }
    let task = match question {
        Question::Noul { .. } => Task::Noul,
        Question::Choice { .. } => Task::Choice,
        Question::Score { .. } => Task::Score,
    };
    let labels: Vec<_> = match question {
        Question::Noul { .. } => vec!["false".to_owned(), "true".to_owned()],
        _ => question.options().into_iter().map(|(id, _)| id).collect(),
    };
    let mut ids = BTreeSet::new();
    let mut samples = Vec::new();
    for observation in observations {
        if observation.id.is_empty() || !ids.insert(observation.id) {
            return Err(invalid(
                "metric observations require unique nonempty case IDs",
            ));
        }
        observation.answer.validate(question)?;
        let (low, high) = target(observation.label, task, &labels)?;
        let probabilities = distribution(observation.answer);
        let winner = winner(question, &probabilities)?;
        let mut assessment = observation.answer.assessment(policy)?;
        let predicted = match &assessment.selected {
            Selected::Boolean(value) => usize::from(*value),
            _ => labels
                .iter()
                .position(|id| id == &winner)
                .ok_or_else(|| invalid("prediction is outside declared options"))?,
        };
        let uncertain = assessment.uncertain || additional_uncertain.contains(observation.id);
        let mut static_policy = *policy;
        static_policy.min_confidence = Probability::new(0.0)?;
        assessment = observation.answer.assessment(&static_policy)?;
        let point = (low == high).then_some(low);
        let binary = match observation.answer {
            Answer::Noul { noul } => Some((*noul, low == 1)),
            _ => None,
        };
        let brier = point.map(|target| {
            if let Some((p, y)) = binary {
                (p - f64::from(y)).powi(2)
            } else {
                probabilities
                    .iter()
                    .map(|(id, p)| {
                        let actual = f64::from(labels.get(target) == Some(id));
                        (p - actual).powi(2)
                    })
                    .sum()
            }
        });
        let confidence = assessment.confidence.get();
        let calibration = point.map(|target| {
            if let Some((p, y)) = binary {
                (p, y)
            } else {
                (confidence, labels.get(target) == Some(&winner))
            }
        });
        let distance = match observation.answer {
            Answer::Score { score, .. } => {
                let low = low as f64;
                let high = high as f64;
                Some((
                    (low - score).max(score - high).max(0.0),
                    (score - low).abs().max((score - high).abs()),
                ))
            }
            _ => None,
        };
        samples.push(Sample {
            predicted,
            point,
            correct: (low..=high).contains(&predicted),
            confidence,
            uncertain,
            static_uncertain: assessment.uncertain || additional_uncertain.contains(observation.id),
            binary,
            brier,
            calibration,
            distance,
        });
    }
    if additional_uncertain.iter().any(|id| !ids.contains(id)) {
        return Err(invalid("additional uncertainty names an unknown case"));
    }
    let cases = samples.len();
    let point_labels = samples
        .iter()
        .filter(|sample| sample.point.is_some())
        .count();
    let point_reason = if cases == 0 {
        Undefined::NoSamples
    } else {
        Undefined::NoPointLabels
    };
    let correct = samples.iter().filter(|sample| sample.correct).count();
    let accepted = samples.iter().filter(|sample| !sample.uncertain).count();
    let selected_correct = samples
        .iter()
        .filter(|sample| !sample.uncertain && sample.correct)
        .count();
    let per_label = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let support = samples
                .iter()
                .filter(|sample| sample.point == Some(index))
                .count();
            let predicted = samples
                .iter()
                .filter(|sample| sample.point.is_some() && sample.predicted == index)
                .count();
            let tp = samples
                .iter()
                .filter(|sample| sample.point == Some(index) && sample.predicted == index)
                .count();
            let fp = predicted - tp;
            let fn_count = support - tp;
            Class {
                label: label.clone(),
                support,
                predicted,
                true_positive: tp,
                false_positive: fp,
                false_negative: fn_count,
                precision: Value::mean(tp as f64, predicted, Undefined::NoPredictedLabels),
                recall: Value::mean(tp as f64, support, Undefined::NoTrueLabels),
                f1: if predicted == 0 && support == 0 {
                    Value::missing(Undefined::NoLabelOutcomes, 0)
                } else {
                    Value::defined(
                        2.0 * tp as f64 / (predicted as f64 + support as f64),
                        support,
                    )
                },
            }
        })
        .collect::<Vec<_>>();
    let macro_f1 = if point_labels == 0 {
        Value::missing(point_reason, 0)
    } else if per_label.iter().any(|label| label.f1.value.is_none()) {
        Value::missing(Undefined::UndefinedClassF1, point_labels)
    } else {
        Value::defined(
            per_label
                .iter()
                .filter_map(|label| label.f1.value)
                .sum::<f64>()
                / per_label.len() as f64,
            point_labels,
        )
    };
    let mut bins: [(usize, f64, f64); 10] = [(0, 0.0, 0.0); 10];
    for sample in &samples {
        if let Some((p, y)) = sample.calibration {
            let index = (1..10)
                .position(|edge| p <= edge as f64 / 10.0)
                .unwrap_or(9);
            if let Some((count, predicted, observed)) = bins.get_mut(index) {
                *count += 1;
                *predicted += p;
                *observed += f64::from(y);
            }
        }
    }
    let ece = Value::mean(
        bins.iter()
            .map(|(_, predicted, observed)| (predicted - observed).abs())
            .sum(),
        point_labels,
        point_reason,
    );
    let reliability = bins
        .into_iter()
        .enumerate()
        .map(|(index, (count, predicted, observed))| Bin {
            lower: index as f64 / 10.0,
            upper: (index + 1) as f64 / 10.0,
            lower_inclusive: index == 0,
            count,
            predicted: (count > 0).then(|| predicted / count as f64),
            observed: (count > 0).then(|| observed / count as f64),
        })
        .collect();
    let confidence_gates = gates
        .into_iter()
        .map(|gate| {
            let accepted = samples
                .iter()
                .filter(|sample| !sample.static_uncertain && sample.confidence >= gate)
                .count();
            let correct = samples
                .iter()
                .filter(|sample| {
                    !sample.static_uncertain && sample.confidence >= gate && sample.correct
                })
                .count();
            Selective {
                min_confidence: gate,
                accepted,
                coverage: Value::mean(accepted as f64, cases, Undefined::NoSamples),
                accuracy: Value::mean(correct as f64, accepted, Undefined::NoAcceptedDecisions),
            }
        })
        .collect();
    let distances = samples
        .iter()
        .filter_map(|sample| sample.distance)
        .collect::<Vec<_>>();
    let distance_support = distances.len();
    let score_reason = if task == Task::Score {
        Undefined::NoSamples
    } else {
        Undefined::NotApplicable
    };
    let mae = Value::mean(
        distances.iter().map(|(low, _)| low).sum(),
        distance_support,
        score_reason,
    );
    let mae_bounds = mae.value.map(|minimum| Bounds {
        minimum,
        maximum: distances.iter().map(|(_, high)| high).sum::<f64>() / distance_support as f64,
    });
    Ok(Report {
        task,
        cases,
        point_labels,
        ambiguous_labels: cases - point_labels,
        accepted,
        accuracy: Value::mean(correct as f64, cases, Undefined::NoSamples),
        macro_f1,
        f1: if task == Task::Noul {
            per_label
                .get(1)
                .map(|label| label.f1.clone())
                .ok_or_else(|| invalid("noul positive label missing"))?
        } else {
            Value::missing(Undefined::NotApplicable, 0)
        },
        auroc: if task == Task::Noul {
            auroc(&samples)
        } else {
            Value::missing(Undefined::NotApplicable, 0)
        },
        brier: Value::mean(
            samples.iter().filter_map(|sample| sample.brier).sum(),
            point_labels,
            point_reason,
        ),
        brier_scale: if task == Task::Noul {
            BrierScale::Binary
        } else {
            BrierScale::Multiclass
        },
        ece,
        calibration_kind: if task == Task::Noul {
            CalibrationKind::PositiveProbability
        } else {
            CalibrationKind::TopLabel
        },
        reliability,
        coverage: Value::mean(accepted as f64, cases, Undefined::NoSamples),
        selective_accuracy: Value::mean(
            selected_correct as f64,
            accepted,
            Undefined::NoAcceptedDecisions,
        ),
        confidence_gates,
        mae,
        mae_bounds,
        within_one_accuracy: Value::mean(
            distances
                .iter()
                .filter(|(distance, _)| *distance <= 1.0)
                .count() as f64,
            distance_support,
            score_reason,
        ),
        per_label,
    })
}
fn target(label: &Label, task: Task, labels: &[String]) -> Result<(usize, usize)> {
    match (task, label) {
        (Task::Noul, Label::Noul(value)) => Ok((usize::from(*value), usize::from(*value))),
        (Task::Choice, Label::Choice(value)) => labels
            .iter()
            .position(|id| id == value)
            .map(|index| (index, index))
            .ok_or_else(|| invalid("expected choice label is not a declared option")),
        (Task::Score, Label::Level(value)) => score_target(*value, *value, labels.len()),
        (Task::Score, Label::Interval([low, high])) => score_target(*low, *high, labels.len()),
        _ => Err(invalid("expected label does not match question type")),
    }
}
fn score_target(low: u32, high: u32, levels: usize) -> Result<(usize, usize)> {
    if low <= high && (high as usize) < levels {
        Ok((low as usize, high as usize))
    } else {
        Err(invalid("expected score label is outside declared levels"))
    }
}
fn distribution(answer: &Answer) -> BTreeMap<String, f64> {
    match answer {
        Answer::Noul { noul } => {
            BTreeMap::from([("false".to_owned(), 1.0 - noul), ("true".to_owned(), *noul)])
        }
        Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
            probabilities.clone()
        }
    }
}
fn winner(question: &Question, probabilities: &BTreeMap<String, f64>) -> Result<String> {
    if matches!(question, Question::Noul { .. }) {
        return probabilities
            .get("true")
            .map(|p| if *p >= 0.5 { "true" } else { "false" }.to_owned())
            .ok_or_else(|| invalid("noul probability is missing"));
    }
    probabilities
        .iter()
        .min_by(|(a, x), (b, y)| {
            y.total_cmp(x).then_with(|| {
                if matches!(question, Question::Score { .. }) {
                    a.len().cmp(&b.len()).then(a.cmp(b))
                } else {
                    a.cmp(b)
                }
            })
        })
        .map(|(label, _)| label.clone())
        .ok_or_else(|| invalid("empty metric distribution"))
}
fn auroc(samples: &[Sample]) -> Value {
    let mut scores = samples
        .iter()
        .filter_map(|sample| sample.binary)
        .collect::<Vec<_>>();
    if scores.is_empty() {
        return Value::missing(Undefined::NoSamples, 0);
    }
    let positive = scores.iter().filter(|(_, label)| *label).count();
    let negative = scores.len() - positive;
    if positive == 0 {
        return Value::missing(Undefined::NoPositiveLabels, scores.len());
    }
    if negative == 0 {
        return Value::missing(Undefined::NoNegativeLabels, scores.len());
    }
    scores.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    let mut earlier_negative = 0usize;
    let mut area = 0.0;
    for tied in scores.chunk_by(|(left, _), (right, _)| left == right) {
        let tied_positive = tied.iter().filter(|(_, label)| *label).count();
        let tied_negative = tied.len() - tied_positive;
        area += tied_positive as f64 * (earlier_negative as f64 + 0.5 * tied_negative as f64);
        earlier_negative += tied_negative;
    }
    Value::defined(area / (positive as f64 * negative as f64), scores.len())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub samples: usize,
    pub completed_units: usize,
    pub elapsed_seconds: f64,
    pub p50_ms: Value,
    pub p95_ms: Value,
    pub units_per_second: Value,
}
pub fn timing(latencies: &[Duration], completed_units: usize, elapsed: Duration) -> Timing {
    let mut sorted = latencies.to_vec();
    sorted.sort();
    let percentile = |percent: usize| {
        if sorted.is_empty() {
            return Value::missing(Undefined::NoSamples, 0);
        }
        let rank = sorted.len() / 100 * percent + (sorted.len() % 100 * percent).div_ceil(100);
        sorted.get(rank.saturating_sub(1)).map_or_else(
            || Value::missing(Undefined::NoSamples, 0),
            |latency| Value::defined(latency.as_secs_f64() * 1000.0, sorted.len()),
        )
    };
    Timing {
        samples: sorted.len(),
        completed_units,
        elapsed_seconds: elapsed.as_secs_f64(),
        p50_ms: percentile(50),
        p95_ms: percentile(95),
        units_per_second: if elapsed.is_zero() {
            Value::missing(Undefined::ZeroElapsed, completed_units)
        } else {
            Value::defined(
                completed_units as f64 / elapsed.as_secs_f64(),
                completed_units,
            )
        },
    }
}

pub struct Comparison<'a> {
    pub case_id: &'a str,
    pub question_id: &'a str,
    pub question: &'a Question,
    pub reference: &'a Answer,
    pub actual: &'a Answer,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parity {
    pub cases: usize,
    pub fields: usize,
    pub probabilities: usize,
    pub max_absolute_error: Value,
    pub mean_absolute_error: Value,
    pub argmax_agreement: Value,
}
pub fn parity(observations: &[Comparison<'_>]) -> Result<Parity> {
    let mut fields = BTreeSet::new();
    let mut cases = BTreeSet::new();
    let mut count = 0usize;
    let mut maximum = 0.0f64;
    let mut total = 0.0;
    let mut agreements = 0;
    for observation in observations {
        if observation.case_id.is_empty()
            || observation.question_id.is_empty()
            || !fields.insert((observation.case_id, observation.question_id))
        {
            return Err(invalid("parity requires unique nonempty case/field pairs"));
        }
        observation.question.validate()?;
        observation.reference.validate(observation.question)?;
        observation.actual.validate(observation.question)?;
        let reference = distribution(observation.reference);
        let actual = distribution(observation.actual);
        for (key, p) in &reference {
            let q = actual
                .get(key)
                .ok_or_else(|| invalid("parity distribution keys differ"))?;
            let delta = (p - q).abs();
            maximum = maximum.max(delta);
            total += delta;
            count += 1;
        }
        agreements += usize::from(
            winner(observation.question, &reference)? == winner(observation.question, &actual)?,
        );
        cases.insert(observation.case_id);
    }
    Ok(Parity {
        cases: cases.len(),
        fields: fields.len(),
        probabilities: count,
        max_absolute_error: if count == 0 {
            Value::missing(Undefined::NoSamples, 0)
        } else {
            Value::defined(maximum, count)
        },
        mean_absolute_error: Value::mean(total, count, Undefined::NoSamples),
        argmax_agreement: Value::mean(agreements as f64, fields.len(), Undefined::NoSamples),
    })
}

pub struct Selection<'a> {
    pub id: &'a str,
    pub expected: &'a [String],
    pub actual: &'a [String],
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Retrieval {
    pub cases: usize,
    pub expected: usize,
    pub selected: usize,
    pub true_positive: usize,
    pub exact_match: Value,
    pub precision: Value,
    pub recall: Value,
    pub f1: Value,
}
pub fn retrieval(observations: &[Selection<'_>]) -> Result<Retrieval> {
    let mut ids = BTreeSet::new();
    let mut expected = 0usize;
    let mut selected = 0usize;
    let mut tp = 0usize;
    let mut exact = 0;
    for observation in observations {
        if observation.id.is_empty() || !ids.insert(observation.id) {
            return Err(invalid("retrieval requires unique nonempty case IDs"));
        }
        let wanted: BTreeSet<_> = observation.expected.iter().collect();
        let actual: BTreeSet<_> = observation.actual.iter().collect();
        if wanted.len() != observation.expected.len()
            || actual.len() != observation.actual.len()
            || wanted.iter().chain(&actual).any(|id| id.is_empty())
        {
            return Err(invalid(
                "retrieval candidate IDs must be unique and nonempty",
            ));
        }
        expected = expected
            .checked_add(wanted.len())
            .ok_or_else(|| invalid("retrieval expected count overflow"))?;
        selected = selected
            .checked_add(actual.len())
            .ok_or_else(|| invalid("retrieval selection count overflow"))?;
        tp = tp
            .checked_add(wanted.intersection(&actual).count())
            .ok_or_else(|| invalid("retrieval intersection count overflow"))?;
        exact += usize::from(wanted == actual);
    }
    Ok(Retrieval {
        cases: observations.len(),
        expected,
        selected,
        true_positive: tp,
        exact_match: Value::mean(exact as f64, observations.len(), Undefined::NoSamples),
        precision: Value::mean(tp as f64, selected, Undefined::NoPredictedLabels),
        recall: Value::mean(tp as f64, expected, Undefined::NoTrueLabels),
        f1: if selected == 0 && expected == 0 {
            Value::missing(Undefined::NoLabelOutcomes, 0)
        } else {
            Value::defined(
                2.0 * tp as f64 / (selected as f64 + expected as f64),
                expected,
            )
        },
    })
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn additional_uncertainty_preserves_accuracy_and_brier_but_limits_every_coverage_gate() {
        let question: crate::wire::Question = serde_json::from_value(
            serde_json::json!({"type":"choice","criteria":{"a":"A","b":"B"}}),
        )
        .unwrap();
        let labels = [
            crate::suites::Label::Choice("a".into()),
            crate::suites::Label::Choice("b".into()),
        ];
        let answers = [
            serde_json::from_value(serde_json::json!({"type":"choice","choice":"a","confidence":0.8,"probabilities":{"a":0.8,"b":0.2}})).unwrap(),
            serde_json::from_value(serde_json::json!({"type":"choice","choice":"b","confidence":0.8,"probabilities":{"a":0.2,"b":0.8}})).unwrap(),
        ];
        let observations = [
            crate::task_metrics::Observation {
                id: "joint-disagrees",
                label: &labels[0],
                answer: &answers[0],
            },
            crate::task_metrics::Observation {
                id: "joint-agrees",
                label: &labels[1],
                answer: &answers[1],
            },
        ];
        let policy = crate::policy::Policy {
            min_confidence: crate::policy::Probability::new(0.9).unwrap(),
            ..crate::policy::Policy::default()
        };
        let normal =
            crate::task_metrics::report(&question, &policy, &observations, &[0.0, 0.5, 0.9])
                .unwrap();
        let additional = std::collections::BTreeSet::from(["joint-disagrees"]);
        let measured = crate::task_metrics::report_with_uncertainty(
            &question,
            &policy,
            &observations,
            &[0.0, 0.5, 0.9],
            &additional,
        )
        .unwrap();
        assert_eq!(normal.accuracy, measured.accuracy);
        assert_eq!(normal.brier, measured.brier);
        assert_eq!(measured.coverage.value, Some(0.0));
        assert_eq!(normal.confidence_gates[0].coverage.value, Some(1.0));
        assert_eq!(measured.confidence_gates[0].coverage.value, Some(0.5));
        assert_eq!(measured.confidence_gates[1].coverage.value, Some(0.5));
        assert_eq!(measured.confidence_gates[2].coverage.value, Some(0.0));
        assert!(
            crate::task_metrics::report_with_uncertainty(
                &question,
                &policy,
                &observations,
                &[],
                &std::collections::BTreeSet::from(["missing"])
            )
            .is_err()
        );
    }
    use crate::policy::{Policy, Probability};
    use crate::suites::{Label, Metric};
    use crate::task_metrics::{
        Comparison, Observation, Selection, Undefined, parity, report, retrieval, timing,
    };
    use crate::wire::{Answer, Question, Request, Response};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;
    fn approx(value: Option<f64>, expected: f64) {
        assert!(
            (value.unwrap() - expected).abs() < 1e-12,
            "{value:?} != {expected}"
        );
    }
    fn noul() -> Question {
        Question::Noul {
            instructions: None,
            criteria: None,
        }
    }
    fn observations<'a>(
        labels: &'a [Label],
        answers: &'a [Answer],
        ids: &'a [String],
    ) -> Vec<Observation<'a>> {
        labels
            .iter()
            .zip(answers)
            .zip(ids)
            .map(|((label, answer), id)| Observation { id, label, answer })
            .collect()
    }
    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|index| format!("case-{index}")).collect()
    }
    fn choice_question(keys: &[&str]) -> Question {
        Question::Choice {
            instructions: None,
            criteria: keys
                .iter()
                .map(|key| ((*key).to_owned(), json!(key)))
                .collect(),
        }
    }
    fn choice(values: &[(&str, f64)]) -> Answer {
        Answer::Choice {
            choice: values[0].0.to_owned(),
            confidence: 0.0,
            probabilities: values
                .iter()
                .map(|(key, p)| ((*key).to_owned(), *p))
                .collect(),
        }
    }
    fn score(values: &[f64], issued: f64) -> Answer {
        Answer::Score {
            score: issued,
            confidence: 0.0,
            legend: values
                .iter()
                .enumerate()
                .map(|(index, _)| (index.to_string(), json!(index)))
                .collect(),
            probabilities: values
                .iter()
                .enumerate()
                .map(|(index, p)| (index.to_string(), *p))
                .collect(),
        }
    }
    #[test]
    fn binary_hand_calculation_covers_confusion_brier_auc_reliability_and_margin_coverage() {
        let labels = [
            Label::Noul(true),
            Label::Noul(false),
            Label::Noul(true),
            Label::Noul(false),
        ];
        let answers = [
            Answer::Noul { noul: 0.9 },
            Answer::Noul { noul: 0.7 },
            Answer::Noul { noul: 0.4 },
            Answer::Noul { noul: 0.1 },
        ];
        let ids = ids(4);
        let observations = observations(&labels, &answers, &ids);
        let policy = Policy {
            margin: 0.2,
            ..Policy::default()
        };
        let metrics = report(&noul(), &policy, &observations, &[0.0, 0.8, 1.0]).unwrap();
        approx(metrics.accuracy.value, 0.5);
        approx(metrics.f1.value, 0.5);
        approx(metrics.macro_f1.value, 0.5);
        approx(metrics.brier.value, 0.2175);
        approx(metrics.auroc.value, 0.75);
        approx(metrics.ece.value, 0.375);
        assert_eq!(metrics.accepted, 2);
        approx(metrics.coverage.value, 0.5);
        approx(metrics.selective_accuracy.value, 1.0);
        assert_eq!(metrics.per_label[1].true_positive, 1);
        assert_eq!(metrics.per_label[1].false_positive, 1);
        assert_eq!(metrics.per_label[1].false_negative, 1);
        assert_eq!(metrics.confidence_gates[1].accepted, 2);
        assert_eq!(metrics.confidence_gates[2].accepted, 0);
        assert_eq!(
            metrics.confidence_gates[2].accuracy.undefined,
            Some(Undefined::NoAcceptedDecisions)
        );
        assert_eq!(
            metrics
                .reliability
                .iter()
                .map(|bin| bin.count)
                .sum::<usize>(),
            4
        );
    }
    #[test]
    fn auc_ties_credit_half_and_single_class_and_empty_metrics_remain_undefined() {
        let labels = [
            Label::Noul(true),
            Label::Noul(false),
            Label::Noul(true),
            Label::Noul(false),
        ];
        let answers = [
            Answer::Noul { noul: 0.5 },
            Answer::Noul { noul: 0.5 },
            Answer::Noul { noul: 0.9 },
            Answer::Noul { noul: 0.1 },
        ];
        let ids = ids(4);
        let observations = observations(&labels, &answers, &ids);
        approx(
            report(&noul(), &Policy::default(), &observations, &[])
                .unwrap()
                .auroc
                .value,
            0.875,
        );
        let single = report(&noul(), &Policy::default(), &observations[..1], &[]).unwrap();
        assert_eq!(single.auroc.undefined, Some(Undefined::NoNegativeLabels));
        let negative = report(&noul(), &Policy::default(), &observations[1..2], &[]).unwrap();
        assert_eq!(negative.auroc.undefined, Some(Undefined::NoPositiveLabels));
        let empty = report(&noul(), &Policy::default(), &[], &[0.0]).unwrap();
        for metric in [
            empty.accuracy,
            empty.brier,
            empty.auroc,
            empty.ece,
            empty.coverage,
        ] {
            assert_eq!(metric.undefined, Some(Undefined::NoSamples));
        }
        assert!(
            empty
                .reliability
                .iter()
                .all(|bin| bin.count == 0 && bin.predicted.is_none() && bin.observed.is_none())
        );
    }

    #[test]
    fn every_three_case_discrete_auc_matches_the_direct_positive_negative_pair_definition() {
        let ids = ids(3);
        for first in [0.25, 0.5, 0.75] {
            for second in [0.25, 0.5, 0.75] {
                for third in [0.25, 0.5, 0.75] {
                    let probabilities = [first, second, third];
                    let answers = probabilities.map(|noul| Answer::Noul { noul });
                    for mask in 1..7 {
                        let truth = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
                        let labels = truth.map(Label::Noul);
                        let mut wins = 0.0;
                        let mut pairs = 0;
                        for (positive, p) in probabilities.iter().enumerate() {
                            if truth[positive] {
                                for (negative, q) in probabilities.iter().enumerate() {
                                    if !truth[negative] {
                                        wins += if p > q {
                                            1.0
                                        } else if p == q {
                                            0.5
                                        } else {
                                            0.0
                                        };
                                        pairs += 1;
                                    }
                                }
                            }
                        }
                        let metrics = report(
                            &noul(),
                            &Policy::default(),
                            &observations(&labels, &answers, &ids),
                            &[],
                        )
                        .unwrap();
                        approx(metrics.auroc.value, wins / f64::from(pairs));
                    }
                }
            }
        }
    }
    #[test]
    fn changing_binary_threshold_does_not_redefine_probability_calibration() {
        let labels = [Label::Noul(false), Label::Noul(true)];
        let answers = [Answer::Noul { noul: 0.1 }, Answer::Noul { noul: 0.8 }];
        let ids = ids(2);
        let observations = observations(&labels, &answers, &ids);
        let baseline = report(&noul(), &Policy::default(), &observations, &[]).unwrap();
        let shifted = report(
            &noul(),
            &Policy {
                threshold: Probability::new(0.05).unwrap(),
                ..Policy::default()
            },
            &observations,
            &[],
        )
        .unwrap();
        approx(baseline.accuracy.value, 1.0);
        approx(shifted.accuracy.value, 0.5);
        assert_eq!(baseline.brier, shifted.brier);
        assert_eq!(baseline.ece, shifted.ece);
        assert_eq!(baseline.auroc, shifted.auroc);
    }
    #[test]
    fn choice_metrics_use_local_probabilities_declared_classes_and_swept_gates() {
        let question = choice_question(&["a", "b"]);
        let labels = [
            Label::Choice("a".to_owned()),
            Label::Choice("a".to_owned()),
            Label::Choice("b".to_owned()),
        ];
        let answers = [
            choice(&[("a", 0.8), ("b", 0.2)]),
            choice(&[("a", 0.4), ("b", 0.6)]),
            choice(&[("a", 0.3), ("b", 0.7)]),
        ];
        let ids = ids(3);
        let observations = observations(&labels, &answers, &ids);
        let metrics = report(
            &question,
            &Policy {
                min_confidence: Probability::new(0.75).unwrap(),
                ..Policy::default()
            },
            &observations,
            &[0.0, 0.7, 0.75],
        )
        .unwrap();
        approx(metrics.accuracy.value, 2.0 / 3.0);
        approx(metrics.macro_f1.value, 2.0 / 3.0);
        approx(metrics.brier.value, 0.98 / 3.0);
        approx(metrics.ece.value, 1.1 / 3.0);
        assert_eq!(metrics.accepted, 1);
        assert_eq!(metrics.confidence_gates[0].accepted, 3);
        assert_eq!(metrics.confidence_gates[1].accepted, 2);
        approx(metrics.confidence_gates[1].accuracy.value, 1.0);
        assert!(metrics.metric(Metric::Auroc).is_err());
        let missing = report(
            &choice_question(&["a", "b", "c"]),
            &Policy::default(),
            &[Observation {
                id: "one",
                label: &Label::Choice("a".to_owned()),
                answer: &choice(&[("a", 1.0), ("b", 0.0), ("c", 0.0)]),
            }],
            &[],
        )
        .unwrap();
        assert_eq!(
            missing.macro_f1.undefined,
            Some(Undefined::UndefinedClassF1)
        );
        assert_eq!(missing.per_label[2].f1.value, None);
    }
    #[test]
    fn choice_ties_remain_uncertain_and_all_ten_right_closed_bins_include_zero_and_one() {
        let question = choice_question(&["a", "b"]);
        let answer = choice(&[("b", 0.5), ("a", 0.5)]);
        let metrics = report(
            &question,
            &Policy::default(),
            &[Observation {
                id: "tie",
                label: &Label::Choice("a".to_owned()),
                answer: &answer,
            }],
            &[0.0],
        )
        .unwrap();
        approx(metrics.accuracy.value, 1.0);
        assert_eq!(metrics.accepted, 0);
        assert_eq!(metrics.confidence_gates[0].accepted, 0);
        let answers = (0..=10)
            .map(|index| Answer::Noul {
                noul: index as f64 / 10.0,
            })
            .collect::<Vec<_>>();
        let labels = vec![Label::Noul(true); 11];
        let ids = ids(11);
        let metrics = report(
            &noul(),
            &Policy::default(),
            &observations(&labels, &answers, &ids),
            &[],
        )
        .unwrap();
        assert_eq!(metrics.reliability[0].count, 2);
        assert!(metrics.reliability[1..].iter().all(|bin| bin.count == 1));
        approx(metrics.reliability[9].predicted, 1.0);
    }
    #[test]
    fn ordinal_intervals_use_issued_score_bounds_without_inventing_a_point_label() {
        let question = Question::Score {
            instructions: None,
            criteria: (0..4).map(|index| json!(index)).collect(),
        };
        let labels = [
            Label::Level(1),
            Label::Interval([1, 2]),
            Label::Interval([0, 1]),
        ];
        let answers = [
            score(&[0.1, 0.5, 0.3, 0.1], 1.4),
            score(&[0.0, 0.3, 0.6, 0.1], 1.8),
            score(&[0.0, 0.0, 0.5, 0.5], 2.5),
        ];
        let ids = ids(3);
        let metrics = report(
            &question,
            &Policy::default(),
            &observations(&labels, &answers, &ids),
            &[],
        )
        .unwrap();
        assert_eq!(metrics.point_labels, 1);
        assert_eq!(metrics.ambiguous_labels, 2);
        approx(metrics.accuracy.value, 2.0 / 3.0);
        approx(metrics.mae.value, 1.9 / 3.0);
        approx(metrics.within_one_accuracy.value, 2.0 / 3.0);
        let bounds = metrics.mae_bounds.unwrap();
        approx(Some(bounds.minimum), 1.9 / 3.0);
        approx(Some(bounds.maximum), 3.7 / 3.0);
        approx(metrics.brier.value, 0.36);
        assert_eq!(metrics.brier.support, 1);
        approx(metrics.ece.value, 0.5);
        let ambiguous = report(
            &question,
            &Policy::default(),
            &[Observation {
                id: "interval",
                label: &labels[1],
                answer: &answers[1],
            }],
            &[],
        )
        .unwrap();
        assert_eq!(ambiguous.brier.undefined, Some(Undefined::NoPointLabels));
        assert_eq!(ambiguous.ece.undefined, Some(Undefined::NoPointLabels));
        let question = Question::Score {
            instructions: None,
            criteria: (0..12).map(|index| json!(index)).collect(),
        };
        let mut tied = vec![0.0; 12];
        tied[2] = 0.5;
        tied[10] = 0.5;
        let answer = score(&tied, 6.0);
        let metrics = report(
            &question,
            &Policy::default(),
            &[Observation {
                id: "numeric-tie",
                label: &Label::Level(2),
                answer: &answer,
            }],
            &[],
        )
        .unwrap();
        approx(metrics.accuracy.value, 1.0);
    }
    #[test]
    fn invalid_expected_labels_duplicate_cases_gates_and_actual_distributions_are_rejected() {
        let answer = Answer::Noul { noul: 0.8 };
        let wrong = Label::Choice("yes".to_owned());
        assert!(
            report(
                &noul(),
                &Policy::default(),
                &[Observation {
                    id: "one",
                    label: &wrong,
                    answer: &answer
                }],
                &[]
            )
            .is_err()
        );
        let label = Label::Noul(true);
        let observations = [
            Observation {
                id: "one",
                label: &label,
                answer: &answer,
            },
            Observation {
                id: "one",
                label: &label,
                answer: &answer,
            },
        ];
        assert!(report(&noul(), &Policy::default(), &observations, &[]).is_err());
        for gates in [vec![f64::NAN], vec![-0.1], vec![0.5, 0.5], vec![1.0, 0.0]] {
            assert!(report(&noul(), &Policy::default(), &[], &gates).is_err());
        }
        let question = Question::Score {
            instructions: None,
            criteria: vec![json!("low"), json!("high")],
        };
        let answer = score(&[0.8, 0.2], 0.2);
        assert!(
            report(
                &question,
                &Policy::default(),
                &[Observation {
                    id: "one",
                    label: &Label::Level(2),
                    answer: &answer
                }],
                &[]
            )
            .is_err()
        );
        let malformed = Answer::Choice {
            choice: "a".to_owned(),
            confidence: 0.9,
            probabilities: BTreeMap::from([("a".to_owned(), 0.9), ("b".to_owned(), 0.9)]),
        };
        assert!(
            report(
                &choice_question(&["a", "b"]),
                &Policy::default(),
                &[Observation {
                    id: "one",
                    label: &Label::Choice("a".to_owned()),
                    answer: &malformed
                }],
                &[]
            )
            .is_err()
        );
    }
    #[test]
    fn parity_micro_averages_probabilities_and_timing_uses_exact_nearest_rank() {
        let question = noul();
        let a = Answer::Noul { noul: 0.2 };
        let b = Answer::Noul { noul: 0.3 };
        let c = Answer::Noul { noul: 0.8 };
        let metrics = parity(&[
            Comparison {
                case_id: "one",
                question_id: "q",
                question: &question,
                reference: &a,
                actual: &b,
            },
            Comparison {
                case_id: "two",
                question_id: "q",
                question: &question,
                reference: &a,
                actual: &c,
            },
        ])
        .unwrap();
        assert_eq!(metrics.probabilities, 4);
        approx(metrics.max_absolute_error.value, 0.6);
        approx(metrics.mean_absolute_error.value, 0.35);
        approx(metrics.argmax_agreement.value, 0.5);
        let tie = Answer::Noul { noul: 0.5 };
        approx(
            parity(&[Comparison {
                case_id: "tie",
                question_id: "q",
                question: &question,
                reference: &tie,
                actual: &c,
            }])
            .unwrap()
            .argmax_agreement
            .value,
            1.0,
        );
        let elapsed = timing(
            &[
                Duration::from_millis(40),
                Duration::from_millis(10),
                Duration::from_millis(30),
                Duration::from_millis(20),
            ],
            8,
            Duration::from_secs(2),
        );
        approx(elapsed.p50_ms.value, 20.0);
        approx(elapsed.p95_ms.value, 40.0);
        approx(elapsed.units_per_second.value, 4.0);
        assert_eq!(
            timing(&[], 0, Duration::ZERO).units_per_second.undefined,
            Some(Undefined::ZeroElapsed)
        );
        assert_eq!(
            parity(&[]).unwrap().argmax_agreement.undefined,
            Some(Undefined::NoSamples)
        );
    }
    #[test]
    fn retrieval_absence_exact_match_and_micro_counts_keep_undefined_denominators_visible() {
        let expected = vec!["L1".to_owned(), "L2".to_owned()];
        let actual = vec!["L2".to_owned(), "L3".to_owned()];
        let metrics = retrieval(&[
            Selection {
                id: "answer",
                expected: &expected,
                actual: &actual,
            },
            Selection {
                id: "absent",
                expected: &[],
                actual: &[],
            },
        ])
        .unwrap();
        approx(metrics.exact_match.value, 0.5);
        approx(metrics.precision.value, 0.5);
        approx(metrics.recall.value, 0.5);
        approx(metrics.f1.value, 0.5);
        let absent = retrieval(&[Selection {
            id: "absent",
            expected: &[],
            actual: &[],
        }])
        .unwrap();
        approx(absent.exact_match.value, 1.0);
        assert_eq!(
            absent.precision.undefined,
            Some(Undefined::NoPredictedLabels)
        );
        assert_eq!(absent.recall.undefined, Some(Undefined::NoTrueLabels));
        let duplicates = vec!["L1".to_owned(), "L1".to_owned()];
        assert!(
            retrieval(&[Selection {
                id: "one",
                expected: &expected,
                actual: &duplicates
            }])
            .is_err()
        );
    }
    #[test]
    fn saved_real_mixed_response_metrics_preserve_actual_score_and_probabilities() {
        let evidence: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let request: Request = serde_json::from_value(evidence["request"].clone()).unwrap();
        let response: Response = serde_json::from_value(evidence["response"].clone()).unwrap();
        let answers = response.validate(&request).unwrap();
        let label = Label::Level(2);
        let question = request
            .questions
            .iter()
            .find(|(_, question)| matches!(question, Question::Score { .. }))
            .unwrap();
        let answer = answers.get(question.0).unwrap();
        let metrics = report(
            question.1,
            &Policy::default(),
            &[Observation {
                id: "saved-server",
                label: &label,
                answer,
            }],
            &[],
        )
        .unwrap();
        if let Answer::Score { score, .. } = answer {
            approx(metrics.mae.value, (2.0 - score).abs());
        }
        assert_eq!(metrics.brier.support, 1);
        let serialized = serde_json::to_value(&metrics).unwrap();
        assert!(serialized["auroc"]["value"].is_null());
    }

    #[tokio::test]
    #[ignore = "requires real LOUPE_TEST_URL System One service"]
    async fn actual_joint_responses_feed_labeled_metrics_with_exact_counts_and_no_cache() {
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    base_url: Some(std::env::var("LOUPE_TEST_URL").unwrap()),
                    concurrency: Some(4),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = crate::backend::Backend::new(&profile).await.unwrap();
        let discovery = backend.discover(&profile.limits).await.unwrap();
        let questions = serde_json::from_value::<indexmap::IndexMap<String,Question>>(json!({
            "failure":{"type":"noul","instructions":"Is checkout currently unavailable?"},
            "category":{"type":"choice","instructions":"Classify the current checkout status.","criteria":{"down":"Checkout unavailable","healthy":"Checkout working normally"}},
            "urgency":{"type":"score","instructions":"Rate the urgency of the checkout status.","criteria":["Working normally; no action","Nonurgent issue","Checkout unavailable; act now"]}
        })).unwrap();
        let cases = [
            (
                "Checkout is down and customers cannot pay.",
                true,
                "down",
                2,
            ),
            (
                "Checkout is working normally. All customers can pay.",
                false,
                "healthy",
                0,
            ),
            (
                "Checkout remains unavailable; every payment attempt fails.",
                true,
                "down",
                2,
            ),
            (
                "Checkout is healthy. All payment attempts succeed.",
                false,
                "healthy",
                0,
            ),
        ];
        let budget = crate::budget::Budget::new(Some(10000));
        let mut responses = Vec::new();
        let mut captured = Vec::new();
        for (state, failure, category, urgency) in cases {
            let request = Request {
                state: json!(state),
                model: profile.model.clone(),
                questions: questions.clone(),
            };
            let (_, counts) = encoder.encode(&request).unwrap();
            counts.preflight(&discovery.limits).unwrap();
            let evaluation = backend
                .evaluate(&request, &counts, &budget, true, None)
                .await
                .unwrap();
            assert!(!evaluation.cached);
            assert!(evaluation.warning.is_none());
            assert_eq!(
                evaluation.response.usage.input_tokens,
                counts.input_tokens as u64
            );
            responses.push(evaluation.response.validate(&request).unwrap());
            captured.push(json!({"request":request,"question_order":request.questions.keys().collect::<Vec<_>>(),"response":evaluation.response,"counts":counts,"labels":{"failure":failure,"category":category,"urgency":urgency}}));
        }
        let ids = ids(cases.len());
        let labels: BTreeMap<&str, Vec<Label>> = BTreeMap::from([
            (
                "failure",
                cases
                    .iter()
                    .map(|(_, label, _, _)| Label::Noul(*label))
                    .collect(),
            ),
            (
                "category",
                cases
                    .iter()
                    .map(|(_, _, label, _)| Label::Choice((*label).to_owned()))
                    .collect(),
            ),
            (
                "urgency",
                cases
                    .iter()
                    .map(|(_, _, _, label)| Label::Level(*label))
                    .collect(),
            ),
        ]);
        let mut reports = BTreeMap::new();
        for (id, question) in &questions {
            let labels = labels.get(id.as_str()).unwrap();
            let observations = responses
                .iter()
                .zip(labels)
                .zip(&ids)
                .map(|((response, label), case_id)| Observation {
                    id: case_id,
                    label,
                    answer: response.get(id).unwrap(),
                })
                .collect::<Vec<_>>();
            let metrics = report(
                question,
                &Policy::default(),
                &observations,
                &[0.0, 0.5, 0.9, 1.0],
            )
            .unwrap();
            assert_eq!(metrics.cases, 4);
            assert_eq!(metrics.point_labels, 4);
            assert_eq!(metrics.brier.support, 4);
            assert_eq!(
                metrics
                    .reliability
                    .iter()
                    .map(|bin| bin.count)
                    .sum::<usize>(),
                4
            );
            assert!(metrics.brier.value.unwrap().is_finite());
            reports.insert(id, metrics);
        }
        let usage = budget.usage().unwrap();
        assert_eq!(usage.attempts, 4);
        assert_eq!(usage.http_attempts, 4);
        assert_eq!(usage.cache_hits, 0);
        assert_eq!(usage.spent_input, usage.reported_input);
        eprintln!(
            "{}",
            json!({"qualification":"actual-joint-labeled-metrics","endpoint":format!("{}{}",profile.base_url,profile.path),"authoritative_identity":discovery.full_model_identity,"encoder_identity":encoder.identity,"input_tokens":usage.reported_input,"records":captured,"reports":reports})
        );
    }
}
