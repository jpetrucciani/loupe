//! External decisions preserve the raw model response and carry no confidence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::encoder;
use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::policy::{Assessment, Policy, Selected};
use crate::trigger::Outcome;
use crate::wire::{Question, Request, Response};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    InvalidOutput,
    Exit,
    Timeout,
    Metadata,
    Process,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Execution {
    Resolved {
        selected: Selected,
        outcome: Outcome,
    },
    Failed {
        kind: FailureKind,
        message: String,
        outcome: Option<Outcome>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    #[default]
    Uncertain,
    Fail,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub command: Option<String>,
    pub timeout: Option<String>,
    pub on_error: Option<FailurePolicy>,
}

pub fn parse(question: &Question, output: &[u8]) -> Result<Selected> {
    question.validate()?;
    let text = std::str::from_utf8(output).map_err(|_| invalid("fallback output must be UTF-8"))?;
    let printed = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let value = if matches!(question, Question::Choice { criteria, .. } if criteria.contains_key(printed))
    {
        printed
    } else {
        text.trim()
    };
    if value.is_empty() || value.contains(['\r', '\n', '\0']) {
        return Err(invalid(
            "fallback must print exactly one label or score number",
        ));
    }
    match question {
        Question::Noul { .. } => match value {
            "yes" => Ok(Selected::Boolean(true)),
            "no" => Ok(Selected::Boolean(false)),
            _ => Err(invalid("Noul fallback must print yes or no")),
        },
        Question::Choice { criteria, .. } => {
            if criteria.contains_key(value) {
                Ok(Selected::Choice(value.to_owned()))
            } else {
                Err(invalid("fallback label is not a declared Choice key"))
            }
        }
        Question::Score { criteria, .. } => {
            let number: serde_json::Number = serde_json::from_str(value)
                .map_err(|_| invalid("Score fallback must print one JSON number"))?;
            let value = number
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| invalid("fallback score must be finite"))?;
            if !(0.0..=criteria.len().saturating_sub(1) as f64).contains(&value) {
                return Err(invalid(
                    "fallback score is outside the declared level range",
                ));
            }
            Ok(Selected::Score(value))
        }
    }
}

pub struct Input<'a> {
    pub unit: &'a Unit,
    pub request: &'a Request,
    pub response: &'a Response,
    pub field: &'a str,
    pub policy: Policy,
}

pub struct Ensemble<'a> {
    pub set: &'a crate::ensemble::Set,
    pub primary: &'a str,
    pub min_agreement: crate::policy::Probability,
}

pub async fn execute(command: &str, input: &Input<'_>, expires: Instant) -> Result<Execution> {
    execute_decision(command, input, None, expires).await
}

pub async fn execute_ensemble(
    command: &str,
    input: &Input<'_>,
    ensemble: &Ensemble<'_>,
    expires: Instant,
) -> Result<Execution> {
    execute_decision(command, input, Some(ensemble), expires).await
}

async fn execute_decision(
    command: &str,
    input: &Input<'_>,
    ensemble: Option<&Ensemble<'_>>,
    expires: Instant,
) -> Result<Execution> {
    if command.trim().is_empty() || command.contains('\0') {
        return Err(Error::new(
            ErrorKind::Usage,
            "fallback command must be nonempty without NUL",
        ));
    }
    let question = input
        .request
        .questions
        .get(input.field)
        .ok_or_else(|| invalid("fallback question is missing"))?;
    let mut assessment = input
        .response
        .validate(input.request)?
        .get(input.field)
        .ok_or_else(|| invalid("fallback raw answer is missing"))?
        .assessment(&input.policy)?;
    let ensemble_metadata = if let Some(ensemble) = ensemble {
        let report = ensemble.set.report(
            ensemble.primary,
            input.field,
            input.request,
            input.response,
            input.policy,
            ensemble.min_agreement,
        )?;
        assessment = report.assessment.clone();
        Some(encoder::render(
            &serde_json::to_value(report).map_err(json_error)?,
        )?)
    } else {
        None
    };
    if !assessment.uncertain {
        return Err(Error::new(
            ErrorKind::Usage,
            "fallback executes only for an uncertain decision",
        ));
    }
    let raw = input
        .response
        .answers
        .get(input.field)
        .ok_or_else(|| invalid("fallback raw answer is missing"))?;
    let question = serde_json::to_value(question).map_err(json_error)?;
    let question = encoder::render(&question)?;
    let answer = encoder::render(raw)?;
    let mut environment = match environment(input.field, question, answer) {
        Ok(environment) => environment,
        Err(error) => {
            return Ok(Execution::Failed {
                kind: FailureKind::Metadata,
                message: error.to_string(),
                outcome: None,
            });
        }
    };
    if let Some(metadata) = ensemble_metadata {
        if metadata.len() > 65536 {
            return Ok(Execution::Failed {
                kind: FailureKind::Metadata,
                message: "ensemble metadata exceeds its environment limit".into(),
                outcome: None,
            });
        }
        environment.insert("LOUPE_ENSEMBLE_JSON".into(), metadata);
        environment.insert("LOUPE_DECISION_SOURCE".into(), "ensemble".into());
    }
    if Instant::now() >= expires {
        return Ok(Execution::Failed {
            kind: FailureKind::Timeout,
            message: "fallback deadline expired before launch".into(),
            outcome: None,
        });
    }
    let label = label(&assessment);
    let outcome = match crate::trigger::execute_with_env(
        command,
        input.unit,
        &label,
        &assessment,
        expires,
        &environment,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            return Ok(Execution::Failed {
                kind: FailureKind::Process,
                message: error.to_string(),
                outcome: None,
            });
        }
    };
    if outcome.timed_out {
        return Ok(Execution::Failed {
            kind: FailureKind::Timeout,
            message: "fallback command timed out".into(),
            outcome: Some(outcome),
        });
    }
    if outcome.code != Some(0) {
        return Ok(Execution::Failed {
            kind: FailureKind::Exit,
            message: "fallback command exited unsuccessfully".into(),
            outcome: Some(outcome),
        });
    }
    if outcome.stdout.truncated {
        return Ok(Execution::Failed {
            kind: FailureKind::InvalidOutput,
            message: "fallback output exceeds its capture limit".into(),
            outcome: Some(outcome),
        });
    }
    let question = input
        .request
        .questions
        .get(input.field)
        .ok_or_else(|| invalid("fallback question is missing"))?;
    match parse(question, &outcome.stdout.bytes) {
        Ok(selected) => Ok(Execution::Resolved { selected, outcome }),
        Err(error) => Ok(Execution::Failed {
            kind: FailureKind::InvalidOutput,
            message: error.to_string(),
            outcome: Some(outcome),
        }),
    }
}

fn environment(field: &str, question: String, answer: String) -> Result<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for (key, value, limit) in [
        ("LOUPE_QUESTION_ID", field.to_owned(), 8192),
        ("LOUPE_QUESTION", question, 32768),
        ("LOUPE_ANSWER_JSON", answer, 65536),
    ] {
        if value.len() > limit || value.contains('\0') {
            return Err(invalid("fallback metadata exceeds its environment limit"));
        }
        environment.insert(key.to_owned(), value);
    }
    Ok(environment)
}

fn label(assessment: &Assessment) -> String {
    match &assessment.selected {
        Selected::Boolean(true) => "yes".into(),
        Selected::Boolean(false) => "no".into(),
        Selected::Choice(value) => value.clone(),
        Selected::Score(value) => value.to_string(),
    }
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}
fn json_error(error: serde_json::Error) -> Error {
    invalid(&error.to_string())
}

/// Pure presentation metadata. No distribution or probability is synthesized.
pub fn decision(execution: &Execution) -> Value {
    match execution {
        Execution::Resolved { selected, .. } => {
            json!({"selected":selected,"source":"fallback","confidence":null,"uncertain":false})
        }
        Execution::Failed { .. } => {
            json!({"selected":null,"source":"fallback","confidence":null,"uncertain":true})
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::fallback::{Execution, FailureKind, Input, execute, parse};
    use crate::policy::{Policy, Selected};
    use crate::records::{Format, Options, Source};
    use crate::wire::{Question, Request, Response};
    use serde_json::json;
    use std::time::Duration;
    use tokio::time::Instant;

    fn saved() -> (crate::input::Unit, Request, Response, Policy) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-trigger.json")).unwrap();
        let unit = Source::new(
            std::io::Cursor::new(fixture["input"].as_str().unwrap().as_bytes()),
            Options::new(Format::Lines),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap();
        let request = serde_json::from_value(fixture["request"].clone()).unwrap();
        let response = serde_json::from_value(fixture["response"].clone()).unwrap();
        let policy = Policy {
            margin: 1.0,
            ..Policy::default()
        };
        (unit, request, response, policy)
    }

    #[test]
    fn strict_noul_choice_and_score_parsing_never_invents_a_distribution() {
        let noul: Question = serde_json::from_value(json!({"type":"noul"})).unwrap();
        assert_eq!(parse(&noul, b" yes\r\n").unwrap(), Selected::Boolean(true));
        for text in [
            b"true".as_slice(),
            b"YES",
            b"0.9",
            b"yes\nno",
            b"yes\0",
            b"",
            &[255],
        ] {
            assert!(parse(&noul, text).is_err());
        }
        let choice: Question = serde_json::from_value(
            json!({"type":"choice","criteria":{"page":"Page","none":"Absent"," spaced ":"Spaced"}}),
        )
        .unwrap();
        assert_eq!(
            parse(&choice, b"none\n").unwrap(),
            Selected::Choice("none".into())
        );
        assert!(parse(&choice, b"other").is_err());
        assert_eq!(
            parse(&choice, b" spaced \n").unwrap(),
            Selected::Choice(" spaced ".into())
        );
        let score: Question =
            serde_json::from_value(json!({"type":"score","criteria":["none","some","full"]}))
                .unwrap();
        assert_eq!(parse(&score, b"1.25\n").unwrap(), Selected::Score(1.25));
        for text in [
            "NaN", "Infinity", "1e999", "-1", "2.1", "+1", "01", "1 2", "\"1\"",
        ] {
            assert!(parse(&score, text.as_bytes()).is_err(), "{text}");
        }
    }

    #[tokio::test]
    async fn real_child_receives_original_bytes_and_canonical_model_metadata() {
        let (unit, request, response, policy) = saved();
        let original = serde_json::to_value(&response).unwrap();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        let command = "cat >&2; printf '%s\\n' \"$LOUPE_QUESTION\" \"$LOUPE_ANSWER_JSON\" \"$LOUPE_QUESTION_ID\" >&2; printf no";
        let execution = execute(command, &input, Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        assert!(crate::fallback::decision(&execution)["confidence"].is_null());
        let Execution::Resolved { selected, outcome } = execution else {
            panic!("fallback failed");
        };
        assert_eq!(selected, Selected::Boolean(false));
        assert_eq!(outcome.code, Some(0));
        assert!(outcome.stderr.bytes.starts_with(&unit.raw));
        let lines = std::str::from_utf8(&outcome.stderr.bytes[unit.raw.len()..])
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),
            serde_json::to_value(&request.questions["decision"]).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[1]).unwrap(),
            response.answers["decision"]
        );
        assert_eq!(lines[2], "decision");
        assert_eq!(serde_json::to_value(&response).unwrap(), original);
    }

    #[tokio::test]
    async fn nonzero_and_invalid_output_are_failures_even_with_a_valid_label_prefix() {
        let (unit, request, response, policy) = saved();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        for (command, expected) in [
            ("printf yes; exit 7", FailureKind::Exit),
            ("printf 'yes\\nno\\n'", FailureKind::InvalidOutput),
            ("printf unexpected", FailureKind::InvalidOutput),
        ] {
            let result = execute(command, &input, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap();
            assert!(matches!(result,Execution::Failed{kind,..} if kind==expected));
        }
    }

    #[tokio::test]
    async fn timeout_kills_real_background_children_and_the_command_is_not_retried() {
        let (unit, request, response, policy) = saved();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let leaked = dir.path().join("leaked");
        let command = format!(
            "printf once >> '{}'; (sleep 0.2; printf leaked > '{}') & sleep 30",
            marker.display(),
            leaked.display()
        );
        let result = execute(&command, &input, Instant::now() + Duration::from_millis(50))
            .await
            .unwrap();
        assert!(matches!(
            result,
            Execution::Failed {
                kind: FailureKind::Timeout,
                ..
            }
        ));
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(std::fs::read(marker).unwrap(), b"once");
        assert!(!leaked.exists());
    }

    #[tokio::test]
    async fn certain_answers_do_not_launch_and_environment_limits_are_validated() {
        let (unit, request, response, mut policy) = saved();
        policy.margin = 0.0;
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = format!("printf bad > '{}'", marker.display());
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        assert!(
            execute(&command, &input, Instant::now() + Duration::from_secs(2))
                .await
                .is_err()
        );
        assert!(!marker.exists());
        let uncertain = Input {
            policy: Policy {
                margin: 1.0,
                ..policy
            },
            ..input
        };
        let expired = execute(&command, &uncertain, Instant::now()).await.unwrap();
        assert!(matches!(
            expired,
            Execution::Failed {
                kind: FailureKind::Timeout,
                outcome: None,
                ..
            }
        ));
        assert!(!marker.exists());
        assert!(crate::fallback::environment("decision", "x".repeat(32769), "{}".into()).is_err());
    }

    #[tokio::test]
    async fn truncated_valid_prefix_is_never_accepted() {
        let (unit, request, response, policy) = saved();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        let result = execute(
            r"printf yes; head -c 70000 /dev/zero | tr '\000' ' '",
            &input,
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        let Execution::Failed {
            kind: FailureKind::InvalidOutput,
            outcome: Some(outcome),
            ..
        } = result
        else {
            panic!("truncated fallback output was accepted");
        };
        assert!(outcome.stdout.truncated);
        assert_eq!(outcome.stdout.bytes.len(), 65536);
        assert_eq!(outcome.stdout.total_bytes, 70003);
    }
}
