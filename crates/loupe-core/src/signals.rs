use std::collections::BTreeSet;
use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Policy, Probability, QuestionKind};
use crate::wire::{Question, Request, Response};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl TryFrom<String> for Id {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        if value.is_empty()
            || value.len() > 64
            || !value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(usage("signal IDs use [a-z][a-z0-9_-]*, at most 64 bytes"));
        }
        Ok(Self(value))
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}
impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Noul,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionPolicy {
    threshold: Option<f64>,
    margin: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: Id,
    #[serde(rename = "type")]
    _kind: Option<Kind>,
    #[serde(alias = "question")]
    instructions: Option<Value>,
    criteria: Option<IndexMap<String, Value>>,
    #[serde(default)]
    policy: DecisionPolicy,
}

#[derive(Clone, Debug)]
pub struct Signal {
    id: Id,
    field: String,
    question: Question,
    policy: Policy,
}

impl Signal {
    pub fn field(&self) -> &str {
        &self.field
    }
    pub fn policy(&self) -> Policy {
        self.policy
    }
    pub fn assessment(&self, probability: Probability) -> Result<Assessment> {
        self.policy.noul(probability)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Set {
    signals: Vec<Signal>,
}

#[derive(Serialize)]
pub struct Report<'a> {
    pub field: String,
    pub answer: &'a Value,
    pub probability: Probability,
    pub assessment: Assessment,
    pub threshold: Probability,
    pub margin: f64,
}

impl Set {
    pub fn from_document(document: &Value) -> Result<Self> {
        let Some(definitions) = document.get("signals") else {
            return Ok(Self::default());
        };
        let definitions: Vec<Definition> = serde_json::from_value(definitions.clone())
            .map_err(|error| usage(&format!("signals: {error}")))?;
        let mut signals = Vec::with_capacity(definitions.len());
        let mut seen = BTreeSet::new();
        for definition in definitions {
            let field = format!("signal:{}", definition.id);
            if !seen.insert(field.clone()) {
                return Err(usage("duplicate signal ID"));
            }
            let question = Question::Noul {
                instructions: definition.instructions,
                criteria: definition.criteria,
            };
            question.validate()?;
            let mut policy = Policy::default();
            if let Some(threshold) = definition.policy.threshold {
                policy.threshold =
                    Probability::new(threshold).map_err(|error| usage(&error.to_string()))?;
            }
            if let Some(margin) = definition.policy.margin {
                policy.margin = margin;
            }
            policy.validate()?;
            signals.push(Signal {
                id: definition.id,
                field,
                question,
                policy,
            });
        }
        Ok(Self { signals })
    }

    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    pub fn get(&self, field: &str) -> Option<&Signal> {
        self.signals.iter().find(|signal| signal.field == field)
    }

    pub fn primary(&self, questions: &IndexMap<String, Question>) -> IndexMap<String, Question> {
        questions
            .iter()
            .filter(|(id, _)| self.get(id).is_none())
            .map(|(id, question)| (id.clone(), question.clone()))
            .collect()
    }

    pub fn append(&self, questions: &mut IndexMap<String, Question>) -> Result<()> {
        if self
            .signals
            .iter()
            .any(|signal| questions.contains_key(&signal.field))
        {
            return Err(usage(
                "primary questions cannot collide with reserved signal fields",
            ));
        }
        questions.extend(
            self.signals
                .iter()
                .map(|signal| (signal.field.clone(), signal.question.clone())),
        );
        Ok(())
    }

    pub fn fields(&self, primary: &str, bound_primary: &str) -> Result<IndexMap<String, String>> {
        let source = if primary == bound_primary {
            None
        } else {
            Some(
                bound_primary
                    .strip_suffix(&format!("/{primary}"))
                    .filter(|source| !source.is_empty())
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::Unavailable,
                            "signal primary window binding is invalid",
                        )
                    })?,
            )
        };
        Ok(self
            .signals
            .iter()
            .map(|signal| {
                let field = source.map_or_else(
                    || signal.field.clone(),
                    |source| format!("{source}/{}", signal.field),
                );
                (signal.id.to_string(), field)
            })
            .collect())
    }

    pub fn reports<'a>(
        &self,
        request: &Request,
        response: &'a Response,
        primary: &str,
        bound_primary: &str,
    ) -> Result<IndexMap<String, Report<'a>>> {
        if self.is_empty() {
            return Ok(IndexMap::new());
        }
        if !request.questions.contains_key(bound_primary) {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "signal primary answer is missing",
            ));
        }
        let parsed = response.validate(request)?;
        let fields = self.fields(primary, bound_primary)?;
        self.signals
            .iter()
            .map(|signal| {
                let id = signal.id.to_string();
                let field = fields
                    .get(&id)
                    .ok_or_else(|| protocol("signal binding missing"))?;
                let answer = response
                    .answers
                    .get(field)
                    .ok_or_else(|| protocol("signal answer missing"))?;
                let typed = parsed
                    .get(field)
                    .ok_or_else(|| protocol("parsed signal answer missing"))?;
                if request.questions.get(field).map(Question::kind) != Some(QuestionKind::Noul) {
                    return Err(protocol("signal field must be Noul"));
                }
                let probability = Probability::new(
                    answer
                        .get("noul")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| protocol("signal probability missing"))?,
                )?;
                Ok((
                    id,
                    Report {
                        field: field.clone(),
                        answer,
                        probability,
                        assessment: typed.assessment(&signal.policy)?,
                        threshold: signal.policy.threshold,
                        margin: signal.policy.margin,
                    },
                ))
            })
            .collect()
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
    use indexmap::IndexMap;
    use serde_json::json;

    use crate::policy::{Probability, Selected};
    use crate::signals::Set;
    use crate::wire::Question;

    #[test]
    fn declarations_have_stable_order_fallbacks_criteria_and_atomic_collision_refusal() {
        let set=Set::from_document(&json!({"signals":[{"id":"impact","instructions":"Are users affected?","criteria":{"true":"Users are affected."}},{"id":"retried"}]})).unwrap();
        let mut questions = IndexMap::from([(
            "severity".to_owned(),
            Question::Choice {
                instructions: Some(json!("Severity?")),
                criteria: IndexMap::from([("page".to_owned(), json!("Urgent"))]),
            },
        )]);
        set.append(&mut questions).unwrap();
        assert_eq!(
            questions.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["severity", "signal:impact", "signal:retried"]
        );
        assert_eq!(
            questions["signal:retried"].kind(),
            crate::policy::QuestionKind::Noul
        );
        let original = questions.clone();
        assert!(set.append(&mut questions).is_err());
        assert_eq!(
            serde_json::to_value(questions).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        assert!(Set::from_document(&json!({})).unwrap().is_empty());
    }

    #[test]
    fn malformed_declarations_and_policies_are_rejected_without_ignoring_fields() {
        for signals in [
            json!({}),
            json!([{"id":"Bad"}]),
            json!([{"id":"bad/id"}]),
            json!([{"id":"same"},{"id":"same"}]),
            json!([{"id":"bad","type":"score"}]),
            json!([{"id":"bad","criteria":{"maybe":"Maybe"}}]),
            json!([{"id":"bad","policy":{"threshold":1.1}}]),
            json!([{"id":"bad","policy":{"margin":-0.1}}]),
            json!([{"id":"bad","policy":{"min_confidence":0.9}}]),
            json!([{"id":"bad","unknown":true}]),
        ] {
            assert!(Set::from_document(&json!({"signals":signals})).is_err());
        }
    }

    #[test]
    fn independent_policies_and_source_bindings_preserve_ties_and_uncertainty() {
        let set=Set::from_document(&json!({"signals":[{"id":"impact","policy":{"threshold":0.7,"margin":0.1}},{"id":"retried"}]})).unwrap();
        let signal = set.get("signal:impact").unwrap();
        let assessment = signal.assessment(Probability::new(0.7).unwrap()).unwrap();
        assert_eq!(assessment.selected, Selected::Boolean(true));
        assert!(assessment.uncertain);
        assert!(
            !signal
                .assessment(Probability::new(0.95).unwrap())
                .unwrap()
                .uncertain
        );
        assert_eq!(
            set.fields("severity", "severity").unwrap()["impact"],
            "signal:impact"
        );
        assert_eq!(
            set.fields("nested/rubric", "L42/nested/rubric").unwrap()["impact"],
            "L42/signal:impact"
        );
        assert!(set.fields("severity", "L42/other").is_err());
    }
}
