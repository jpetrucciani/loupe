use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::policy::QuestionKind;
use crate::wire::{Answer, Question};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Any,
    All,
    NoisyOr,
    Mean,
    MaxConfidence,
    Vote,
    Max,
    Min,
}
impl FromStr for Mode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "any" => Ok(Self::Any),
            "all" => Ok(Self::All),
            "noisy-or" => Ok(Self::NoisyOr),
            "mean" => Ok(Self::Mean),
            "max-confidence" => Ok(Self::MaxConfidence),
            "vote" => Ok(Self::Vote),
            "max" => Ok(Self::Max),
            "min" => Ok(Self::Min),
            _ => Err(Error::new(ErrorKind::Usage, "unknown combine mode")),
        }
    }
}
impl Mode {
    pub fn validate(self, kind: QuestionKind) -> Result<()> {
        let valid = match kind {
            QuestionKind::Noul => {
                matches!(self, Self::Any | Self::All | Self::NoisyOr | Self::Mean)
            }
            QuestionKind::Choice => matches!(self, Self::Mean | Self::MaxConfidence | Self::Vote),
            QuestionKind::Score => matches!(self, Self::Mean | Self::Max | Self::Min),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!("combine {self:?} is not valid for {kind:?}"),
            ))
        }
    }

    pub fn apply(self, question: &Question, values: &[(Value, u64)]) -> Result<Value> {
        self.validate(question.kind())?;
        if values.is_empty() {
            return Err(Error::new(
                ErrorKind::Input,
                "cannot combine an empty answer set",
            ));
        }
        let answers = values
            .iter()
            .map(|(raw, weight)| {
                if *weight == 0 {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "combine weights must be positive unique source-token counts",
                    ));
                }
                let answer: Answer = serde_json::from_value(raw.clone())
                    .map_err(|error| Error::new(ErrorKind::Unavailable, error.to_string()))?;
                answer.validate(question)?;
                Ok(answer)
            })
            .collect::<Result<Vec<_>>>()?;
        if question.kind() == QuestionKind::Noul {
            let probabilities = answers
                .iter()
                .map(|answer| match answer {
                    Answer::Noul { noul } => Ok(*noul),
                    _ => Err(Error::new(ErrorKind::Input, "noul answer required")),
                })
                .collect::<Result<Vec<_>>>()?;
            let p = match self {
                Self::Any => probabilities.iter().copied().fold(0.0, f64::max),
                Self::All => probabilities.iter().copied().fold(1.0, f64::min),
                Self::NoisyOr => {
                    1.0 - probabilities
                        .iter()
                        .fold(1.0, |product, p| product * (1.0 - p))
                }
                Self::Mean => probabilities.iter().sum::<f64>() / (probabilities.len() as f64),
                _ => return Err(Error::new(ErrorKind::Usage, "invalid noul reducer")),
            };
            return Ok(json!({"type":"noul","noul":p}));
        }
        if matches!(self, Self::Max | Self::Min | Self::MaxConfidence) {
            let mut selected = 0usize;
            let mut best = None;
            for (index, answer) in answers.iter().enumerate() {
                let value = match answer {
                    Answer::Score { score, .. } if self != Self::MaxConfidence => *score,
                    Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
                        probabilities.values().copied().fold(0.0, f64::max)
                    }
                    _ => return Err(Error::new(ErrorKind::Input, "option distribution required")),
                };
                if best.is_none_or(|best| {
                    if self == Self::Min {
                        value < best
                    } else {
                        value > best
                    }
                }) {
                    best = Some(value);
                    selected = index;
                }
            }
            let mut raw = values
                .get(selected)
                .ok_or_else(|| Error::new(ErrorKind::Input, "combined selection out of range"))?
                .0
                .clone();
            let probabilities = distribution(
                answers
                    .get(selected)
                    .ok_or_else(|| Error::new(ErrorKind::Input, "combined answer missing"))?,
            )?;
            raw["confidence"] = json!(probabilities.values().copied().fold(0.0, f64::max));
            if question.kind() == QuestionKind::Choice {
                raw["choice"] = json!(winner(probabilities)?);
            }
            return Ok(raw);
        }
        let total = values.iter().try_fold(0u64, |total, (_, weight)| {
            total
                .checked_add(*weight)
                .ok_or_else(|| Error::new(ErrorKind::Input, "combine weight overflow"))
        })?;
        let mut probabilities: BTreeMap<String, f64> = question
            .options()
            .into_iter()
            .map(|(id, _)| (id, 0.0))
            .collect();
        let mut score = 0.0;
        for (answer, (_, weight)) in answers.iter().zip(values) {
            let source = distribution(answer)?;
            if self == Self::Vote {
                let label = winner(source)?;
                let count = probabilities
                    .get_mut(&label)
                    .ok_or_else(|| Error::new(ErrorKind::Input, "vote label missing"))?;
                *count += 1.0 / (answers.len() as f64);
            } else {
                let sum = source.values().sum::<f64>();
                for (id, value) in source {
                    let target = probabilities.get_mut(id).ok_or_else(|| {
                        Error::new(ErrorKind::Input, "distribution option missing")
                    })?;
                    *target += (value / sum) * (*weight as f64) / (total as f64);
                }
                if let Answer::Score { score: value, .. } = answer {
                    score += value * (*weight as f64) / (total as f64);
                }
            }
        }
        // Convex sums can exceed an endpoint by a floating-point rounding unit.
        for probability in probabilities.values_mut() {
            *probability = probability.clamp(0.0, 1.0);
        }
        if let Question::Score { criteria, .. } = question {
            score = score.clamp(0.0, criteria.len().saturating_sub(1) as f64);
        }
        let confidence = probabilities.values().copied().fold(0.0, f64::max);
        let raw = if question.kind() == QuestionKind::Choice {
            json!({"type":"choice","choice":winner(&probabilities)?,"probabilities":probabilities,"confidence":confidence})
        } else {
            let legend: BTreeMap<_, _> = question.options().into_iter().collect();
            json!({"type":"score","score":score,"legend":legend,"probabilities":probabilities,"confidence":confidence})
        };
        let answer: Answer = serde_json::from_value(raw.clone())
            .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?;
        answer.validate(question)?;
        Ok(raw)
    }
}
fn distribution(answer: &Answer) -> Result<&BTreeMap<String, f64>> {
    match answer {
        Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
            Ok(probabilities)
        }
        _ => Err(Error::new(ErrorKind::Input, "option distribution required")),
    }
}
fn winner(probabilities: &BTreeMap<String, f64>) -> Result<String> {
    let mut selected = None;
    for (id, p) in probabilities {
        if selected.as_ref().is_none_or(|(_, best)| p > best) {
            selected = Some((id.clone(), *p));
        }
    }
    selected
        .map(|(id, _)| id)
        .ok_or_else(|| Error::new(ErrorKind::Input, "empty option distribution"))
}

pub fn resolve(
    questions: &indexmap::IndexMap<String, Question>,
    value: Option<&Value>,
    cli: Option<Mode>,
) -> Result<indexmap::IndexMap<String, Mode>> {
    if let Some(object) = value.and_then(Value::as_object)
        && object.keys().any(|id| !questions.contains_key(id))
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "combine map contains an unknown question ID",
        ));
    }
    questions
        .iter()
        .map(|(id, question)| {
            let mode = if let Some(mode) = cli {
                mode
            } else {
                let configured = value
                    .and_then(|value| {
                        if value.is_object() {
                            value.get(id)
                        } else {
                            Some(value)
                        }
                    })
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::Usage,
                            format!(
                                "splitting requires an explicit combine mode for question {id}"
                            ),
                        )
                    })?;
                configured.parse::<Mode>()?
            };
            mode.validate(question.kind())?;
            Ok((id.clone(), mode))
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::combine::{Mode, resolve};
    use crate::policy::Policy;
    use crate::wire::{Answer, Question, Request};
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    fn question(kind: &str) -> Question {
        serde_json::from_value(match kind {
            "noul" => json!({"type":"noul"}),
            "choice" => json!({"type":"choice","criteria":{"a":"A","b":"B"}}),
            _ => json!({"type":"score","criteria":["Low","High"]}),
        })
        .unwrap()
    }
    fn choice(a: f64, b: f64) -> Value {
        json!({"type":"choice","choice":if a>=b {"a"} else {"b"},"confidence":a.max(b),"probabilities":{"a":a,"b":b}})
    }
    fn near(value: &Value, expected: f64) {
        assert!(
            (value.as_f64().unwrap() - expected).abs() < 1e-12,
            "{value} != {expected}"
        );
    }

    #[test]
    fn noul_reducers_match_hand_computed_chunk_mean_and_union() {
        let values = [
            (json!({"type":"noul","noul":0.2}), 100),
            (json!({"type":"noul","noul":0.8}), 900),
        ];
        for (mode, expected) in [
            (Mode::Any, 0.8),
            (Mode::All, 0.2),
            (Mode::NoisyOr, 0.84),
            (Mode::Mean, 0.5),
        ] {
            near(
                &mode.apply(&question("noul"), &values).unwrap()["noul"],
                expected,
            );
        }
    }

    #[test]
    fn choice_weighted_mean_extreme_and_vote_recompute_confidence_and_sorted_ties() {
        let question = question("choice");
        let values = [(choice(0.9, 0.1), 100), (choice(0.2, 0.8), 300)];
        let mean = Mode::Mean.apply(&question, &values).unwrap();
        near(&mean["probabilities"]["a"], 0.375);
        near(&mean["confidence"], 0.625);
        assert_eq!(mean["choice"], "b");
        let extreme = Mode::MaxConfidence.apply(&question, &values).unwrap();
        assert_eq!(extreme, values[0].0);
        let vote = Mode::Vote.apply(&question, &values).unwrap();
        near(&vote["confidence"], 0.5);
        assert_eq!(vote["choice"], "a");
        let answer: Answer = serde_json::from_value(vote).unwrap();
        assert!(answer.assessment(&Policy::default()).unwrap().uncertain);
        // Rounded server distributions are normalized for averaging.
        let rounded = choice(0.6000, 0.3999);
        let normalized = Mode::Mean.apply(&question, &[(rounded, 1)]).unwrap();
        near(&normalized["probabilities"]["a"], 0.6 / 0.9999);
    }

    #[test]
    fn saved_real_score_reducers_preserve_extreme_distribution_and_weight_expected_values() {
        let saved: Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let request: Request = serde_json::from_value(saved["request"].clone()).unwrap();
        let question = &request.questions["urgency"];
        let high = saved["response"]["answers"]["urgency"].clone();
        let low = json!({"type":"score","score":0.0,"confidence":1.0,"legend":high["legend"],"probabilities":{"0":1.0,"1":0.0,"2":0.0}});
        let values = [(low.clone(), 3), (high.clone(), 1)];
        assert_eq!(Mode::Max.apply(question, &values).unwrap(), high);
        assert_eq!(Mode::Min.apply(question, &values).unwrap(), low);
        let mean = Mode::Mean.apply(question, &values).unwrap();
        near(&mean["score"], 1.8972 / 4.0);
        near(&mean["probabilities"]["0"], 0.75 + 0.0254 / 4.0);
        near(&mean["confidence"], 0.75 + 0.0254 / 4.0);
    }

    #[test]
    fn invalid_inputs_and_missing_mixed_reducers_are_errors() {
        let noul = question("noul");
        assert_eq!(Mode::Vote.apply(&noul, &[]).unwrap_err().exit_code(), 64);
        assert!(Mode::Mean.apply(&noul, &[]).is_err());
        assert!(
            Mode::Mean
                .apply(&noul, &[(json!({"type":"noul","noul":1.1}), 1)])
                .is_err()
        );
        assert!(
            Mode::Mean
                .apply(&noul, &[(json!({"type":"noul","noul":0.5}), 0)])
                .is_err()
        );
        let choice = question("choice");
        assert!(
            Mode::Mean
                .apply(
                    &choice,
                    &[
                        (self::choice(1.0, 0.0), u64::MAX),
                        (self::choice(1.0, 0.0), 1)
                    ]
                )
                .is_err()
        );
        let questions =
            IndexMap::from([("present".to_owned(), noul), ("label".to_owned(), choice)]);
        let valid = json!({"present":"any","label":"mean"});
        assert_eq!(resolve(&questions, Some(&valid), None).unwrap().len(), 2);
        for value in [
            json!({"present":"any"}),
            json!({"present":"any","label":"max"}),
            json!({"present":"any","label":"vote","unknown":"mean"}),
        ] {
            assert_eq!(
                resolve(&questions, Some(&value), None)
                    .unwrap_err()
                    .exit_code(),
                64
            );
        }
        assert!(resolve(&questions, None, Some(Mode::Any)).is_err());
        assert!(resolve(&questions, None, Some(Mode::Mean)).is_ok());
    }
}
