use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::wire::{Answer as SystemOneAnswer, Question as SystemOneQuestion};
use crate::wire::{Request as SystemOneRequest, Response as SystemOneResponse, Usage};

#[derive(Debug, Serialize)]
pub(crate) struct Request<'a> {
    model: &'a str,
    input: String,
    questions: Vec<Question<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Question<'a> {
    Predicate {
        name: &'a str,
        instructions: String,
    },
    Choice {
        name: &'a str,
        instructions: String,
        choices: Vec<Choice<'a>>,
    },
    Score {
        name: &'a str,
        instructions: String,
        levels: Vec<Level>,
    },
}

#[derive(Debug, Serialize)]
struct Choice<'a> {
    value: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct Level {
    label: String,
}

fn text(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        _ => serde_json::to_string(value)
            .map_err(|_| Error::new(ErrorKind::Input, "could not render Decisions text")),
    }
}

impl<'a> Request<'a> {
    pub(crate) fn from_systemone(request: &'a SystemOneRequest) -> Result<Self> {
        request.validate()?;
        if request.questions.len() > 128 {
            return Err(Error::new(
                ErrorKind::Input,
                "Decisions accepts at most 128 questions",
            ));
        }
        let questions = request
            .questions
            .iter()
            .map(|(name, question)| {
                let instructions = question.instructions().map(text).transpose()?;
                Ok(match question {
                    SystemOneQuestion::Noul { criteria, .. } => {
                        let mut parts: Vec<String> = instructions.into_iter().collect();
                        if let Some(criteria) = criteria {
                            for (answer, rule) in criteria {
                                if !rule.is_null() {
                                    parts.push(format!("Answer {answer} when: {}", text(rule)?));
                                }
                            }
                        }
                        Question::Predicate {
                            name,
                            instructions: if parts.is_empty() {
                                "Is this true of the input?".to_owned()
                            } else {
                                parts.join("\n\n")
                            },
                        }
                    }
                    SystemOneQuestion::Choice { criteria, .. } => {
                        if !(2..=255).contains(&criteria.len()) {
                            return Err(Error::new(
                                ErrorKind::Input,
                                format!("Decisions Choice {name} requires 2 to 255 choices"),
                            ));
                        }
                        Question::Choice {
                            name,
                            instructions: instructions
                                .unwrap_or_else(|| "Which choice best fits the input?".to_owned()),
                            choices: criteria
                                .iter()
                                .map(|(value, description)| {
                                    Ok(Choice {
                                        value,
                                        description: (!description.is_null())
                                            .then(|| text(description))
                                            .transpose()?,
                                    })
                                })
                                .collect::<Result<_>>()?,
                        }
                    }
                    SystemOneQuestion::Score { criteria, .. } => {
                        if !(2..=10).contains(&criteria.len()) {
                            return Err(Error::new(
                                ErrorKind::Input,
                                format!("Decisions Score {name} requires 2 to 10 levels"),
                            ));
                        }
                        Question::Score {
                            name,
                            instructions: instructions
                                .unwrap_or_else(|| "Which level best fits the input?".to_owned()),
                            levels: criteria
                                .iter()
                                .map(|value| {
                                    Ok(Level {
                                        label: text(value)?,
                                    })
                                })
                                .collect::<Result<_>>()?,
                        }
                    }
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            model: &request.model,
            input: text(&request.state)?,
            questions,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Response {
    model: String,
    answers: Vec<Answer>,
    usage: Usage,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Answer {
    Predicate {
        #[serde(default)]
        name: Option<String>,
        #[serde(deserialize_with = "number")]
        probability: f64,
    },
    Choice {
        #[serde(default)]
        name: Option<String>,
        choice: ChoiceValue,
        probabilities: Vec<ChoiceProbability>,
        #[serde(deserialize_with = "number")]
        confidence: f64,
    },
    Score {
        #[serde(default)]
        name: Option<String>,
        #[serde(deserialize_with = "number")]
        score: f64,
        probabilities: Vec<ScoreProbability>,
        #[serde(deserialize_with = "number")]
        confidence: f64,
    },
    Refusal {
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum ChoiceValue {
    Text(String),
    Boolean(bool),
}

impl ChoiceValue {
    fn key(self) -> String {
        match self {
            Self::Text(value) => value,
            Self::Boolean(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ChoiceProbability {
    value: ChoiceValue,
    #[serde(deserialize_with = "number")]
    probability: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ScoreProbability {
    value: usize,
    label: String,
    #[serde(deserialize_with = "number")]
    probability: f64,
}

fn number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<f64, D::Error> {
    // Number also handles arbitrary_precision's buffered representation in tagged enums.
    serde_json::Number::deserialize(deserializer)?
        .as_f64()
        .ok_or_else(|| serde::de::Error::custom("expected a finite decimal number"))
}

fn invalid(message: &str) -> Error {
    Error::new(
        ErrorKind::Unavailable,
        format!("invalid Decisions response: {message}"),
    )
}

impl Answer {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. }
            | Self::Choice { name, .. }
            | Self::Score { name, .. }
            | Self::Refusal { name } => name.as_deref(),
        }
    }

    fn normalize(self, question: &SystemOneQuestion) -> Result<Value> {
        let answer = match self {
            Self::Predicate { probability, .. } => SystemOneAnswer::Noul { noul: probability },
            Self::Choice {
                choice,
                probabilities,
                confidence,
                ..
            } => {
                let count = probabilities.len();
                let probabilities: BTreeMap<_, _> = probabilities
                    .into_iter()
                    .map(|item| (item.value.key(), item.probability))
                    .collect();
                if probabilities.len() != count {
                    return Err(invalid("duplicate choice probabilities"));
                }
                SystemOneAnswer::Choice {
                    choice: choice.key(),
                    probabilities,
                    confidence,
                }
            }
            Self::Score {
                score,
                probabilities,
                confidence,
                ..
            } => {
                let mut legend = BTreeMap::new();
                let mut distribution = BTreeMap::new();
                for item in probabilities {
                    let label = match question {
                        SystemOneQuestion::Score { criteria, .. } => {
                            criteria.get(item.value).cloned()
                        }
                        _ => None,
                    }
                    .unwrap_or(Value::String(item.label));
                    legend.insert(item.value.to_string(), label);
                    if distribution
                        .insert(item.value.to_string(), item.probability)
                        .is_some()
                    {
                        return Err(invalid("duplicate score probabilities"));
                    }
                }
                SystemOneAnswer::Score {
                    score,
                    legend,
                    probabilities: distribution,
                    confidence,
                }
            }
            Self::Refusal { name } => {
                // Preserve reported usage before the shared validator rejects the refusal.
                return serde_json::to_value(Self::Refusal { name })
                    .map_err(|_| invalid("could not normalize refusal"));
            }
        };
        serde_json::to_value(answer).map_err(|_| invalid("could not normalize answer"))
    }
}

impl Response {
    pub(crate) fn into_systemone(self, request: &SystemOneRequest) -> Result<SystemOneResponse> {
        if self.answers.len() != request.questions.len() {
            return Err(invalid("missing or extra answers"));
        }
        let mut answers = IndexMap::new();
        for ((id, question), answer) in request.questions.iter().zip(self.answers) {
            if answer.name().is_some_and(|name| name != id) {
                return Err(invalid("answer name does not match question order"));
            }
            answers.insert(id.clone(), answer.normalize(question)?);
        }
        Ok(SystemOneResponse {
            model: self.model,
            answers,
            usage: self.usage,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::wire::decisions::{Request, Response};
    use crate::wire::{Answer, Question, Request as SystemOneRequest};

    const REQUEST: &str = r#"{
      "model":"h2o-lightning-4b",
      "state":{"event":"checkout failed"},
      "questions":{
        "urgent":{"type":"noul","instructions":"Is it urgent?","criteria":{"true":"outage","false":"healthy"}},
        "priority":{"type":"choice","instructions":"Pick priority","criteria":{"p1":"outage","p2":null}},
        "severity":{"type":"score","instructions":"Rate severity","criteria":["healthy","outage"]}
      }
    }"#;

    const RESPONSE: &str = r#"{
      "model":"h2oai/h2o-lightning-4b",
      "answers":[
        {"type":"predicate","name":"urgent","probability":0.9000},
        {"type":"choice","name":"priority","choice":"p1","confidence":0.5,"probabilities":[{"value":"p1","probability":0.75},{"value":"p2","probability":0.25}]},
        {"type":"score","name":"severity","score":0.25,"confidence":0.5,"probabilities":[{"value":0,"label":"healthy","probability":0.75},{"value":1,"label":"outage","probability":0.25}]}
      ],
      "usage":{"input_tokens":95,"output_tokens":0,"total_tokens":95,"input_tokens_details":{"cached_tokens":0}}
    }"#;

    #[test]
    fn decisions_request_matches_the_external_contract() {
        let request: SystemOneRequest = serde_json::from_str(REQUEST).unwrap();
        let encoded = Request::from_systemone(&request).unwrap();
        assert_eq!(
            serde_json::to_string(&encoded).unwrap(),
            r#"{"model":"h2o-lightning-4b","input":"{\"event\":\"checkout failed\"}","questions":[{"type":"predicate","name":"urgent","instructions":"Is it urgent?\n\nAnswer true when: outage\n\nAnswer false when: healthy"},{"type":"choice","name":"priority","instructions":"Pick priority","choices":[{"value":"p1","description":"outage"},{"value":"p2"}]},{"type":"score","name":"severity","instructions":"Rate severity","levels":[{"label":"healthy"},{"label":"outage"}]}]}"#
        );
        let mut unsupported = request;
        if let Some(Question::Choice { criteria, .. }) = unsupported.questions.get_mut("priority") {
            criteria.swap_remove("p2");
        }
        assert_eq!(
            Request::from_systemone(&unsupported)
                .unwrap_err()
                .exit_code(),
            65
        );
    }

    #[test]
    fn decisions_answers_use_the_shared_validation_and_preserve_usage() {
        let request = serde_json::from_str(REQUEST).unwrap();
        let decoded: Response = serde_json::from_str(RESPONSE).unwrap();
        let response = decoded.into_systemone(&request).unwrap();
        let answers = response.validate(&request).unwrap();
        assert_eq!(
            answers.keys().map(String::as_str).collect::<Vec<_>>(),
            ["urgent", "priority", "severity"]
        );
        assert!(matches!(answers.get("urgent"), Some(Answer::Noul { noul }) if *noul == 0.9));
        assert!(
            matches!(answers.get("priority"), Some(Answer::Choice { choice, probabilities, .. }) if choice == "p1" && probabilities.get("p2") == Some(&0.25))
        );
        assert!(
            matches!(answers.get("severity"), Some(Answer::Score { score, legend, probabilities, .. }) if *score == 0.25 && legend.get("1") == Some(&serde_json::Value::String("outage".into())) && probabilities.get("0") == Some(&0.75))
        );
        assert_eq!(response.usage.input_tokens, 95);
        assert_eq!(response.usage.output_tokens, 0);
    }

    #[test]
    fn decisions_refusals_and_mismatched_names_are_unavailable() {
        let request: SystemOneRequest = serde_json::from_str(
            r#"{"model":"m","state":"s","questions":{"urgent":{"type":"noul","instructions":"Is it urgent?"}}}"#,
        ).unwrap();
        let refused: Response = serde_json::from_str(
            r#"{"model":"m","answers":[{"type":"refusal","name":"urgent"}],"usage":{"input_tokens":95,"output_tokens":0}}"#,
        ).unwrap();
        let refused = refused.into_systemone(&request).unwrap();
        assert_eq!(refused.usage.input_tokens, 95);
        assert_eq!(refused.validate(&request).unwrap_err().exit_code(), 69);
        let mismatched: Response = serde_json::from_str(
            r#"{"model":"m","answers":[{"type":"predicate","name":"other","probability":0.9}],"usage":{"input_tokens":95,"output_tokens":0}}"#,
        ).unwrap();
        assert_eq!(
            mismatched.into_systemone(&request).unwrap_err().exit_code(),
            69
        );
    }
}
