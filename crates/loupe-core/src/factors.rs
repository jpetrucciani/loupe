//! Auditable weighted indices over a jointly evaluated factor schema.

use std::collections::BTreeSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Policy, Probability};
use crate::wire::{Answer, Question, Request, Response};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: String,
    weight: f64,
    #[serde(default)]
    invert: bool,
    #[serde(rename = "type")]
    kind: Option<String>,
    instructions: Option<Value>,
    criteria: Option<Value>,
    policy: Option<Value>,
}

#[derive(Clone, Debug)]
struct Factor {
    id: String,
    field: String,
    question: Question,
    weight: f64,
    normalized_weight: f64,
    invert: bool,
    policy: Policy,
}

#[derive(Clone, Debug, Default)]
pub struct Set {
    factors: Vec<Factor>,
}

pub struct Ensemble<'a> {
    pub set: &'a crate::ensemble::Set,
    pub min_agreement: Probability,
}

#[derive(Debug, Serialize)]
pub struct Contribution<'a> {
    pub field: String,
    pub answer: &'a Value,
    pub weight: f64,
    pub normalized_weight: f64,
    pub invert: bool,
    pub value: f64,
    pub contribution: f64,
    pub assessment: Assessment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_assessment: Option<Assessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Report<'a> {
    pub formula: &'static str,
    pub score: f64,
    pub factors: IndexMap<String, Contribution<'a>>,
}

impl Set {
    pub fn from_document(document: &Value) -> Result<Self> {
        let Some(definitions) = document.get("factors") else {
            return Ok(Self::default());
        };
        let definitions: Vec<Definition> = serde_json::from_value(definitions.clone())
            .map_err(|error| usage(&format!("factors: {error}")))?;
        if definitions.is_empty() {
            return Err(usage("factors must contain at least one weighted question"));
        }
        let mut factors = Vec::with_capacity(definitions.len());
        let mut seen = BTreeSet::new();
        for definition in definitions {
            if definition.id.is_empty()
                || definition.id.len() > 64
                || !definition
                    .id
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_lowercase)
                || !definition.id.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
            {
                return Err(usage("factor IDs use [a-z][a-z0-9_-]*, at most 64 bytes"));
            }
            if !seen.insert(definition.id.clone()) {
                return Err(usage("duplicate factor ID"));
            }
            if !definition.weight.is_finite() || definition.weight <= 0.0 {
                return Err(usage("factor weights must be finite and positive"));
            }
            let mut source = json!({"type":definition.kind.as_deref().unwrap_or("noul")});
            if let Some(instructions) = definition.instructions {
                source["instructions"] = instructions;
            }
            if let Some(criteria) = definition.criteria {
                source["criteria"] = criteria;
            }
            let question: Question = serde_json::from_value(source.clone())
                .map_err(|error| usage(&format!("factor {}: {error}", definition.id)))?;
            question.validate()?;
            match &question {
                Question::Choice { .. } => {
                    return Err(usage("factors accept Noul or Score questions"));
                }
                Question::Score { criteria, .. } if criteria.len() < 2 => {
                    return Err(usage("Score factors need at least two ordered levels"));
                }
                _ => {}
            }
            if let Some(policy) = definition.policy {
                let keys = policy
                    .as_object()
                    .ok_or_else(|| usage("factor policy must be a table"))?;
                if keys.keys().any(|key| {
                    !matches!(
                        key.as_str(),
                        "threshold" | "margin" | "min_confidence" | "min_gap"
                    )
                }) {
                    return Err(usage(
                        "factor policy accepts only decision thresholds/gates",
                    ));
                }
                source["policy"] = policy;
            }
            let policy = Policy::from_document(&source, question.kind())?;
            policy.validate()?;
            factors.push(Factor {
                field: format!("factor:{}", definition.id),
                id: definition.id,
                question,
                weight: definition.weight,
                normalized_weight: 0.0,
                invert: definition.invert,
                policy,
            });
        }
        // Scaling first avoids overflow when valid weights are close to f64::MAX.
        let scale = factors
            .iter()
            .map(|factor| factor.weight)
            .fold(0.0, f64::max);
        let sum: f64 = factors.iter().map(|factor| factor.weight / scale).sum();
        for factor in &mut factors {
            factor.normalized_weight = (factor.weight / scale) / sum;
            if factor.normalized_weight == 0.0 {
                return Err(usage("factor weight range is too wide to represent"));
            }
        }
        Ok(Self { factors })
    }

    pub fn is_empty(&self) -> bool {
        self.factors.is_empty()
    }

    pub fn contains(&self, field: &str) -> bool {
        self.factors.iter().any(|factor| factor.field == field)
    }

    pub fn append(&self, questions: &mut IndexMap<String, Question>) -> Result<()> {
        if self
            .factors
            .iter()
            .any(|factor| questions.contains_key(&factor.field))
        {
            return Err(usage(
                "primary questions collide with reserved factor fields",
            ));
        }
        questions.extend(
            self.factors
                .iter()
                .map(|factor| (factor.field.clone(), factor.question.clone())),
        );
        Ok(())
    }

    pub fn report<'a>(
        &self,
        request: &Request,
        response: &'a Response,
        primary: &str,
        anchor: &str,
        ensemble: Option<&Ensemble<'_>>,
    ) -> Result<Report<'a>> {
        if self.is_empty() {
            return Err(usage("explain requires declared weighted factors"));
        }
        let prefix = if anchor == primary {
            None
        } else {
            Some(
                anchor
                    .strip_suffix(&format!("/{primary}"))
                    .filter(|prefix| !prefix.is_empty())
                    .ok_or_else(|| protocol("invalid factor source binding"))?,
            )
        };
        let validated = response.validate(request)?;
        let mut factors = IndexMap::new();
        let mut score = 0.0;
        for factor in &self.factors {
            let field = prefix.map_or_else(
                || factor.field.clone(),
                |prefix| format!("{prefix}/{}", factor.field),
            );
            let answer = validated
                .get(&field)
                .ok_or_else(|| protocol("missing factor answer"))?;
            let raw_assessment = answer.assessment(&factor.policy)?;
            let report = ensemble
                .filter(|ensemble| ensemble.set.has_primary(&factor.field))
                .map(|ensemble| {
                    ensemble.set.report(
                        &factor.field,
                        &field,
                        request,
                        response,
                        factor.policy,
                        ensemble.min_agreement,
                    )
                })
                .transpose()?;
            let mean_answer = report
                .as_ref()
                .map(|report| {
                    report.mean_answer(
                        request
                            .questions
                            .get(&field)
                            .ok_or_else(|| protocol("missing factor question"))?,
                    )
                })
                .transpose()?;
            let value = match (&factor.question, mean_answer.as_ref().unwrap_or(answer)) {
                (Question::Noul { .. }, Answer::Noul { noul }) => *noul,
                (Question::Score { criteria, .. }, Answer::Score { probabilities, .. }) => {
                    let mass: f64 = probabilities.values().sum();
                    let mut expected = 0.0;
                    for level in 0..criteria.len() {
                        let probability = probabilities
                            .get(&level.to_string())
                            .ok_or_else(|| protocol("missing factor score level"))?;
                        expected += level as f64 * probability;
                    }
                    expected / mass / criteria.len().saturating_sub(1) as f64
                }
                _ => return Err(protocol("factor answer type mismatch")),
            };
            let value = if factor.invert { 1.0 - value } else { value };
            let contribution = factor.normalized_weight * value;
            score += contribution;
            factors.insert(
                factor.id.clone(),
                Contribution {
                    answer: response
                        .answers
                        .get(&field)
                        .ok_or_else(|| protocol("missing raw factor answer"))?,
                    assessment: report.as_ref().map_or_else(
                        || raw_assessment.clone(),
                        |report| report.assessment.clone(),
                    ),
                    raw_assessment: report.as_ref().map(|_| raw_assessment),
                    ensemble: report
                        .map(serde_json::to_value)
                        .transpose()
                        .map_err(|error| protocol(&error.to_string()))?,
                    field,
                    weight: factor.weight,
                    normalized_weight: factor.normalized_weight,
                    invert: factor.invert,
                    value,
                    contribution,
                },
            );
        }
        let tolerance = f64::EPSILON * self.factors.len() as f64;
        if !score.is_finite() || !(-tolerance..=1.0 + tolerance).contains(&score) {
            return Err(protocol("composite index is outside its numeric range"));
        }
        Ok(Report {
            formula: "sum(normalized_weight * value); value = p(true) or E(level)/(levels-1), complemented when invert=true",
            score: score.clamp(0.0, 1.0),
            factors,
        })
    }
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn protocol(message: &str) -> Error {
    Error::new(ErrorKind::Unavailable, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::factors::Set;
    use crate::wire::{Request, Response};
    use indexmap::IndexMap;
    use serde_json::json;

    #[test]
    fn hand_computed_weighted_probabilities_levels_and_complements() {
        let set = Set::from_document(&json!({"factors":[
            {"id":"impact","weight":2,"instructions":"Impact?"},
            {"id":"severity","weight":1,"type":"score","criteria":["none","some","full"]},
            {"id":"recovered","weight":1,"invert":true}
        ]}))
        .unwrap();
        let mut request = Request {
            state: json!("text"),
            model: "test".into(),
            questions: IndexMap::new(),
        };
        set.append(&mut request.questions).unwrap();
        let response: Response = serde_json::from_value(json!({"model":"test","usage":{"input_tokens":0,"output_tokens":0},"answers":{
            "factor:impact":{"type":"noul","noul":0.8},
            "factor:severity":{"type":"score","score":0.0,"confidence":0.6,"legend":{"0":"none","1":"some","2":"full"},"probabilities":{"0":0.1,"1":0.3,"2":0.6}},
            "factor:recovered":{"type":"noul","noul":0.2}
        }})).unwrap();
        let report = set
            .report(&request, &response, "decision", "decision", None)
            .unwrap();
        assert!((report.score - 0.7875).abs() < 1e-12);
        assert_eq!(report.factors["impact"].contribution, 0.4);
        assert!((report.factors["severity"].value - 0.75).abs() < 1e-12);
        assert_eq!(report.factors["severity"].answer["score"], 0.0);
        assert_eq!(report.factors["recovered"].value, 0.8);
    }

    #[test]
    fn arithmetic_factor_ensembles_preserve_raw_answers_weights_and_disagreement() {
        // Distribution arithmetic only, with no inference or server emulation.
        let document = json!({"factors":[
            {"id":"impact","weight":2},
            {"id":"severity","weight":1,"type":"score","criteria":["none","some","full"]},
            {"id":"recovered","weight":1,"invert":true}
        ],"variants":[{"id":"alt","field":"factor:impact"},{"id":"alt","field":"factor:severity"}]});
        let factors = Set::from_document(&document).unwrap();
        let mut request = Request {
            state: json!("text"),
            model: "test".into(),
            questions: IndexMap::new(),
        };
        factors.append(&mut request.questions).unwrap();
        let set = crate::ensemble::Set::from_document(&document, &request.questions).unwrap();
        set.append(&mut request.questions).unwrap();
        let response: Response = serde_json::from_value(json!({"model":"test","usage":{"input_tokens":0,"output_tokens":0},"answers":{
            "factor:impact":{"type":"noul","noul":0.8},
            "ensemble:factor:impact/alt":{"type":"noul","noul":0.2},
            "factor:severity":{"type":"score","score":2.0,"confidence":1.0,"legend":{"0":"none","1":"some","2":"full"},"probabilities":{"0":0.0,"1":0.0,"2":1.0}},
            "ensemble:factor:severity/alt":{"type":"score","score":0.0,"confidence":1.0,"legend":{"0":"none","1":"some","2":"full"},"probabilities":{"0":1.0,"1":0.0,"2":0.0}},
            "factor:recovered":{"type":"noul","noul":0.2}
        }})).unwrap();
        let original = serde_json::to_vec(&response).unwrap();
        let ensemble = crate::factors::Ensemble {
            set: &set,
            min_agreement: crate::policy::Probability::new(1.0).unwrap(),
        };
        let report = factors
            .report(&request, &response, "decision", "decision", Some(&ensemble))
            .unwrap();
        assert!((report.score - 0.575).abs() < 1e-12);
        assert_eq!(report.factors["impact"].value, 0.5);
        assert_eq!(report.factors["severity"].value, 0.5);
        assert!(report.factors["impact"].assessment.uncertain);
        assert!(report.factors["severity"].assessment.uncertain);
        assert!(
            !report.factors["impact"]
                .raw_assessment
                .as_ref()
                .unwrap()
                .uncertain
        );
        assert_eq!(report.factors["severity"].answer["score"], 2.0);
        assert_eq!(report.factors["recovered"].value, 0.8);
        assert!(report.factors["recovered"].ensemble.is_none());
        assert!(report.factors["recovered"].raw_assessment.is_none());
        assert_eq!(serde_json::to_vec(&response).unwrap(), original);
        let window = Request {
            questions: request
                .questions
                .iter()
                .map(|(id, q)| (format!("L7/{id}"), q.clone()))
                .collect(),
            ..request
        };
        let response = Response {
            answers: response
                .answers
                .iter()
                .map(|(id, a)| (format!("L7/{id}"), a.clone()))
                .collect(),
            ..response
        };
        let report = factors
            .report(
                &window,
                &response,
                "decision",
                "L7/decision",
                Some(&ensemble),
            )
            .unwrap();
        assert!((report.score - 0.575).abs() < 1e-12);
        assert_eq!(
            report.factors["severity"].ensemble.as_ref().unwrap()["members"][1]["field"],
            "L7/ensemble:factor:severity/alt"
        );
    }

    #[test]
    fn definitions_reject_ambiguous_shapes_weights_policies_and_collisions() {
        for document in [
            json!({"factors":[]}),
            json!({"factors":[{"id":"Bad","weight":1}]}),
            json!({"factors":[{"id":"a","weight":0}]}),
            json!({"factors":[{"id":"a","weight":-1}]}),
            json!({"factors":[{"id":"a","weight":1,"type":"choice","criteria":{"x":"X"}}]}),
            json!({"factors":[{"id":"a","weight":1,"type":"score","criteria":["one"]}]}),
            json!({"factors":[{"id":"a","weight":1},{"id":"a","weight":2}]}),
            json!({"factors":[{"id":"a","weight":1,"policy":{"exec":"true"}}]}),
            json!({"factors":[{"id":"a","weight":1,"policy":{"min_gap":0.1}}]}),
            json!({"factors":[{"id":"a","weight":1e308},{"id":"b","weight":1e-308}]}),
        ] {
            assert!(Set::from_document(&document).is_err(), "{document}");
        }
        let set = Set::from_document(
            &json!({"factors":[{"id":"a","weight":1e308},{"id":"b","weight":1e308}]}),
        )
        .unwrap();
        assert_eq!(set.factors[0].normalized_weight, 0.5);
        let mut questions = IndexMap::new();
        set.append(&mut questions).unwrap();
        assert!(set.append(&mut questions).is_err());
    }

    #[test]
    fn window_bindings_preserve_raw_answers_and_refuse_missing_fields() {
        let set = Set::from_document(&json!({"factors":[{"id":"a","weight":1}]})).unwrap();
        let mut questions = IndexMap::new();
        set.append(&mut questions).unwrap();
        let request = Request {
            state: json!("line"),
            model: "test".into(),
            questions: questions
                .into_iter()
                .map(|(key, value)| (format!("L7/{key}"), value))
                .collect(),
        };
        let response:Response=serde_json::from_value(json!({"model":"test","answers":{"L7/factor:a":{"type":"noul","noul":0.25}},"usage":{"input_tokens":0,"output_tokens":0}})).unwrap();
        let report = set
            .report(&request, &response, "decision", "L7/decision", None)
            .unwrap();
        assert_eq!(report.score, 0.25);
        assert_eq!(report.factors["a"].field, "L7/factor:a");
        assert!(
            set.report(&request, &response, "decision", "L8/decision", None)
                .is_err()
        );
        assert!(
            set.report(&request, &response, "decision", "unbound", None)
                .is_err()
        );
    }
}
