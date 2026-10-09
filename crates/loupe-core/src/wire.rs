use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Policy, Probability, QuestionKind};

pub(crate) mod decisions;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiFormat {
    #[default]
    SystemOne,
    Decisions,
}

impl ApiFormat {
    pub fn default_path(self) -> &'static str {
        match self {
            Self::SystemOne => "/v1/systemone",
            Self::Decisions => "/v1/decisions",
        }
    }
}

impl std::str::FromStr for ApiFormat {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "systemone" => Ok(Self::SystemOne),
            "decisions" => Ok(Self::Decisions),
            _ => Err(Error::new(
                ErrorKind::Config,
                "API format must be systemone or decisions",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    Noul {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<IndexMap<String, Value>>,
    },
    Choice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        criteria: IndexMap<String, Value>,
    },
    Score {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        criteria: Vec<Value>,
    },
}

impl Question {
    pub fn kind(&self) -> QuestionKind {
        match self {
            Self::Noul { .. } => QuestionKind::Noul,
            Self::Choice { .. } => QuestionKind::Choice,
            Self::Score { .. } => QuestionKind::Score,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }

    pub fn instructions(&self) -> Option<&Value> {
        match self {
            Self::Noul { instructions, .. }
            | Self::Choice { instructions, .. }
            | Self::Score { instructions, .. } => instructions.as_ref(),
        }
    }

    pub fn options(&self) -> Vec<(String, Value)> {
        match self {
            Self::Noul { criteria, .. } => [
                ("true", "The proposition is true or the answer is yes."),
                ("false", "The proposition is false or the answer is no."),
            ]
            .into_iter()
            .map(|(key, default)| {
                (
                    key.to_owned(),
                    criteria
                        .as_ref()
                        .and_then(|options| options.get(key))
                        .cloned()
                        .unwrap_or_else(|| Value::String(default.to_owned())),
                )
            })
            .collect(),
            Self::Choice { criteria, .. } => {
                let mut options: Vec<_> = criteria
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                options.sort_by(|a, b| a.0.cmp(&b.0));
                options
            }
            Self::Score { criteria, .. } => criteria
                .iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value.clone()))
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Noul {
                criteria: Some(criteria),
                ..
            } if criteria.keys().any(|key| key != "true" && key != "false") => Err(Error::new(
                ErrorKind::Usage,
                "noul criteria may contain only true and false",
            )),
            Self::Choice { criteria, .. }
                if criteria.is_empty() || criteria.keys().any(|key| key.is_empty()) =>
            {
                Err(Error::new(
                    ErrorKind::Usage,
                    "choice requires nonempty option keys",
                ))
            }
            Self::Score { criteria, .. } if criteria.is_empty() => Err(Error::new(
                ErrorKind::Usage,
                "score requires at least one level",
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub state: Value,
    pub model: String,
    pub questions: IndexMap<String, Question>,
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty()
            || self.questions.is_empty()
            || self.questions.keys().any(|id| id.is_empty())
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "model and question IDs must be nonempty; at least one question is required",
            ));
        }
        for question in self.questions.values() {
            question.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response {
    pub model: String,
    pub answers: IndexMap<String, Value>,
    pub usage: Usage,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, Value>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

impl Answer {
    pub fn assessment(&self, policy: &Policy) -> Result<Assessment> {
        match self {
            Self::Noul { noul } => policy.noul(Probability::new(*noul)?),
            Self::Choice { probabilities, .. } => policy.choice(
                &probabilities
                    .iter()
                    .map(|(key, p)| Ok((key.clone(), Probability::new(*p)?)))
                    .collect::<Result<_>>()?,
            ),
            Self::Score {
                score,
                probabilities,
                ..
            } => policy.score(
                *score,
                &probabilities
                    .iter()
                    .map(|(key, p)| Ok((key.clone(), Probability::new(*p)?)))
                    .collect::<Result<_>>()?,
            ),
        }
    }

    pub(crate) fn validate(&self, question: &Question) -> Result<()> {
        match (self, question) {
            (Self::Noul { noul }, Question::Noul { .. }) => {
                Probability::new(*noul)?;
            }
            (
                Self::Choice {
                    choice,
                    probabilities,
                    confidence,
                },
                Question::Choice { .. },
            ) => {
                validate_distribution(question, probabilities, *confidence)?;
                if !probabilities.contains_key(choice) {
                    return Err(protocol("choice is not a declared option"));
                }
            }
            (
                Self::Score {
                    score,
                    legend,
                    probabilities,
                    confidence,
                },
                Question::Score { criteria, .. },
            ) => {
                validate_distribution(question, probabilities, *confidence)?;
                if !score.is_finite()
                    || *score < 0.0
                    || *score > criteria.len().saturating_sub(1) as f64
                {
                    return Err(protocol("score is outside the level range"));
                }
                if legend.keys().ne(probabilities.keys()) {
                    return Err(protocol("score legend keys do not match options"));
                }
            }
            _ => return Err(protocol("answer type does not match question")),
        }
        Ok(())
    }
}

fn protocol(message: &str) -> Error {
    Error::new(
        ErrorKind::Unavailable,
        format!("invalid System One response: {message}"),
    )
}

fn validate_distribution(
    question: &Question,
    probabilities: &BTreeMap<String, f64>,
    confidence: f64,
) -> Result<()> {
    let keys: BTreeMap<_, _> = question.options().into_iter().collect();
    if keys.keys().ne(probabilities.keys()) {
        return Err(protocol("distribution keys do not match declared options"));
    }
    Probability::new(confidence)?;
    let mut sum = 0.0;
    for probability in probabilities.values() {
        Probability::new(*probability)?;
        sum += probability;
    }
    // The real server rounds each probability to four decimal places.
    if (sum - 1.0).abs() > probabilities.len() as f64 * 0.000051 + 1e-8 {
        return Err(protocol("option probabilities do not sum to one"));
    }
    Ok(())
}

impl Response {
    pub fn validate(&self, request: &Request) -> Result<IndexMap<String, Answer>> {
        if self.model.is_empty() || self.answers.len() != request.questions.len() {
            return Err(protocol("empty model or missing/extra answers"));
        }
        request
            .questions
            .iter()
            .map(|(id, question)| {
                let raw = self
                    .answers
                    .get(id)
                    .ok_or_else(|| protocol(&format!("missing answer for {id}")))?;
                let answer: Answer = serde_json::from_value(raw.clone())
                    .map_err(|error| protocol(&error.to_string()))?;
                answer
                    .validate(question)
                    .map_err(|error| protocol(&format!("{id}: {error}")))?;
                Ok((id.clone(), answer))
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::wire::{Request, Response};
    use serde_json::json;

    #[test]
    fn real_saved_server_response_validates_and_missing_answers_fail() {
        let evidence: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let request: Request = serde_json::from_value(evidence["request"].clone()).unwrap();
        let mut response: Response = serde_json::from_value(evidence["response"].clone()).unwrap();
        request.validate().unwrap();
        assert_eq!(response.validate(&request).unwrap().len(), 3);
        response.answers.swap_remove("outage");
        assert_eq!(response.validate(&request).unwrap_err().exit_code(), 69);
    }

    #[test]
    fn malformed_distribution_and_question_criteria_are_rejected() {
        let request: Request = serde_json::from_value(json!({"model":"m","state":"s","questions":{"q":{"type":"choice","criteria":{"a":null,"b":null}}}})).unwrap();
        let response: Response = serde_json::from_value(json!({"model":"m","answers":{"q":{"type":"choice","choice":"a","confidence":0.9,"probabilities":{"a":0.9,"b":0.9}}},"usage":{"input_tokens":1,"output_tokens":0}})).unwrap();
        assert!(response.validate(&request).is_err());
        let invalid: Request = serde_json::from_value(json!({"model":"m","state":"s","questions":{"q":{"type":"noul","criteria":{"maybe":null}}}})).unwrap();
        assert_eq!(invalid.validate().unwrap_err().exit_code(), 64);
    }
}
