use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::fallback::{Execution, FailurePolicy, Input, Settings};
use loupe_core::policy::Assessment;
use serde::Serialize;
use tokio::time::Instant;

use crate::args::{Classify, FallbackError};
use crate::run::{Kind, Prepared, duration};
use crate::summary::Summary;

#[derive(Serialize)]
pub(crate) struct Plan {
    command: String,
    timeout: Duration,
    failure: FailurePolicy,
}

impl Plan {
    pub(crate) fn resolve(
        kind: Kind,
        args: &Classify,
        prepared: &Prepared,
        settings: &Settings,
    ) -> Result<Option<Self>> {
        if prepared.fallback
            && !matches!(
                kind,
                Kind::Is
                    | Kind::Keep
                    | Kind::Drop
                    | Kind::Tag
                    | Kind::Route
                    | Kind::Rate
                    | Kind::Sort
                    | Kind::Count
                    | Kind::On
                    | Kind::Dedupe
            )
        {
            return Err(usage("this command does not support process fallback"));
        }
        Self::configure(
            prepared.fallback,
            args.fallback.as_deref(),
            args.fallback_timeout.as_deref(),
            args.on_fallback_error,
            settings,
        )
    }

    pub(crate) fn configure(
        enabled: bool,
        command: Option<&str>,
        timeout: Option<&str>,
        failure: Option<FallbackError>,
        settings: &Settings,
    ) -> Result<Option<Self>> {
        if !enabled {
            if command.is_some() || timeout.is_some() || failure.is_some() {
                return Err(usage(
                    "fallback flags require --on-uncertain fallback or a matching preset policy",
                ));
            }
            return Ok(None);
        }
        let command = command.or(settings.command.as_deref())
            .filter(|command| !command.trim().is_empty() && !command.contains('\0'))
            .ok_or_else(|| usage("fallback requires a nonempty command from --fallback or trusted [fallback] config"))?.to_owned();
        let timeout = duration(timeout.or(settings.timeout.as_deref()).unwrap_or("30s"))?;
        if timeout.is_zero() {
            return Err(usage("--fallback-timeout must be positive"));
        }
        let failure = match failure {
            Some(FallbackError::Uncertain) => FailurePolicy::Uncertain,
            Some(FallbackError::Fail) => FailurePolicy::Fail,
            None => settings.on_error.unwrap_or_default(),
        };
        Ok(Some(Self {
            command,
            timeout,
            failure,
        }))
    }

    pub(crate) async fn execute(
        &self,
        input: &Input<'_>,
        assessment: &mut Assessment,
        previous: Option<Arc<Execution>>,
        deadline: Option<Instant>,
        summary: &mut Summary,
        ensemble: Option<&loupe_core::fallback::Ensemble<'_>>,
    ) -> Result<Option<Arc<Execution>>> {
        if !assessment.uncertain {
            if previous.is_some() {
                return Err(invalid(
                    "saved fallback belongs to a certain model decision",
                ));
            }
            return Ok(None);
        }
        let execution = if let Some(previous) = previous {
            summary.fallback_replay()?;
            previous
        } else {
            let started = Instant::now();
            let expires = started
                .checked_add(self.timeout)
                .ok_or_else(|| usage("fallback timeout outside clock range"))?;
            let expires = deadline.map_or(expires, |deadline| deadline.min(expires));
            summary.fallback_start()?;
            let execution = if let Some(ensemble) = ensemble {
                loupe_core::fallback::execute_ensemble(&self.command, input, ensemble, expires)
                    .await?
            } else {
                loupe_core::fallback::execute(&self.command, input, expires).await?
            };
            summary.fallback_finish(
                matches!(&execution, Execution::Resolved { .. }),
                started.elapsed(),
            )?;
            Arc::new(execution)
        };
        match execution.as_ref() {
            Execution::Resolved { selected, outcome } => {
                let question = input
                    .request
                    .questions
                    .get(input.field)
                    .ok_or_else(|| invalid("fallback question missing"))?;
                if outcome.failed()
                    || outcome.stdout.truncated
                    || loupe_core::fallback::parse(question, &outcome.stdout.bytes)? != *selected
                {
                    return Err(invalid(
                        "saved fallback selection differs from its successful command output",
                    ));
                }
                assessment.selected = selected.clone();
                assessment.uncertain = false;
            }
            Execution::Failed { kind, message, .. } => {
                eprintln!(
                    "loupe: unit {}: fallback {kind:?}: {message}; decision remains uncertain",
                    input.unit.index
                );
            }
        }
        Ok(Some(execution))
    }

    pub(crate) fn require_success(&self, execution: Option<&Execution>) -> Result<()> {
        if matches!(self.failure, FailurePolicy::Fail)
            && matches!(execution, Some(Execution::Failed { .. }))
        {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "fallback failed under --on-fallback-error fail",
            ));
        }
        Ok(())
    }
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

pub(crate) fn action_environment(execution: Option<&Execution>) -> BTreeMap<String, String> {
    if matches!(execution, Some(Execution::Resolved { .. })) {
        BTreeMap::from([
            ("LOUPE_CONFIDENCE".to_owned(), String::new()),
            ("LOUPE_DECISION_SOURCE".to_owned(), "fallback".to_owned()),
        ])
    } else {
        BTreeMap::new()
    }
}

pub(crate) fn request_unit(
    unit: &loupe_core::input::Unit,
    request: &loupe_core::wire::Request,
) -> Result<loupe_core::input::Unit> {
    let text =
        serde_json::to_string(&request.state).map_err(|error| invalid(&error.to_string()))?;
    let mut raw = text.as_bytes().to_vec();
    raw.push(b'\n');
    Ok(loupe_core::input::Unit {
        index: unit.index,
        text,
        raw,
        subject: request.state.clone(),
        state: request.state.clone(),
        header: None,
        location: unit.location.clone(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use clap::Parser;
    use loupe_core::budget::Budget;
    use loupe_core::fallback::{FailurePolicy, Input, Settings};
    use loupe_core::policy::{Policy, Selected};
    use loupe_core::records::{Format, Options, Source};
    use loupe_core::wire::{Request, Response};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::args::{Cli, Command};
    use crate::run::fallback::Plan;
    use crate::run::{Kind, prepare};
    use crate::summary::Summary;

    fn saved() -> (loupe_core::input::Unit, Request, Response, Policy) {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../loupe-core/testdata/captured-trigger.json"
        ))
        .unwrap();
        let unit = Source::new(
            std::io::Cursor::new(fixture["input"].as_str().unwrap().as_bytes()),
            Options::new(Format::Lines),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap();
        (
            unit,
            serde_json::from_value(fixture["request"].clone()).unwrap(),
            serde_json::from_value(fixture["response"].clone()).unwrap(),
            Policy {
                margin: 1.0,
                ..Policy::default()
            },
        )
    }

    #[tokio::test]
    async fn durable_external_result_replays_without_a_second_child_or_model_budget_charge() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("fired");
        let plan = Plan {
            command: format!("printf once >> '{}'; printf no", marker.display()),
            timeout: Duration::from_secs(2),
            failure: FailurePolicy::Uncertain,
        };
        let (unit, request, response, policy) = saved();
        let raw = serde_json::to_value(&response).unwrap();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        let mut assessment = response.validate(&request).unwrap()["decision"]
            .assessment(&policy)
            .unwrap();
        let budget = Budget::new(Some(0));
        let mut summary = Summary::new(false, false, budget.clone(), None);
        let result = plan
            .execute(&input, &mut assessment, None, None, &mut summary, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(assessment.selected, Selected::Boolean(false));
        assert!(!assessment.uncertain);
        let journal = dir.path().join("journal");
        let identity =
            json!({"source":"saved genuine server response and real subprocess","fallback":plan});
        let mut store =
            loupe_core::recovery::Store::create(&journal, identity.clone(), &[]).unwrap();
        store.admit(&unit).unwrap();
        store.ready(unit.index, raw.clone()).unwrap();
        store
            .fallback(unit.index, serde_json::to_value(result.as_ref()).unwrap())
            .unwrap();
        drop(store);
        let mut store = loupe_core::recovery::Store::resume(&journal, &identity, &[]).unwrap();
        let saved = store.saved(unit.index).unwrap();
        assert_eq!(saved.completed, Some(raw.clone()));
        let previous = serde_json::from_value(saved.fallback.unwrap()).unwrap();
        let mut replayed = response.validate(&request).unwrap()["decision"]
            .assessment(&policy)
            .unwrap();
        let result = plan
            .execute(
                &input,
                &mut replayed,
                Some(Arc::new(previous)),
                None,
                &mut summary,
                None,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(&marker).unwrap(), b"once");
        assert_eq!(replayed.selected, Selected::Boolean(false));
        assert_eq!(budget.usage().unwrap().attempts, 0);
        assert_eq!(budget.usage().unwrap().spent_input, 0);
        assert_eq!(serde_json::to_value(&response).unwrap(), raw);
        let mut changed = serde_json::to_value(result.as_ref()).unwrap();
        changed["selected"] = json!({"boolean":true});
        let changed = Arc::new(serde_json::from_value(changed).unwrap());
        let mut assessment = response.validate(&request).unwrap()["decision"]
            .assessment(&policy)
            .unwrap();
        assert!(
            plan.execute(
                &input,
                &mut assessment,
                Some(changed),
                None,
                &mut summary,
                None
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read(marker).unwrap(), b"once");
    }

    #[tokio::test]
    async fn failures_stay_uncertain_and_only_the_explicit_policy_is_fatal() {
        let (unit, request, response, policy) = saved();
        let input = Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "decision",
            policy,
        };
        let mut assessment = response.validate(&request).unwrap()["decision"]
            .assessment(&policy)
            .unwrap();
        let mut summary = Summary::new(false, false, Budget::new(None), None);
        let mut plan = Plan {
            command: "printf yes; exit 7".into(),
            timeout: Duration::from_secs(2),
            failure: FailurePolicy::Uncertain,
        };
        let result = plan
            .execute(&input, &mut assessment, None, None, &mut summary, None)
            .await
            .unwrap();
        assert!(assessment.uncertain);
        plan.require_success(result.as_deref()).unwrap();
        plan.failure = FailurePolicy::Fail;
        assert_eq!(
            plan.require_success(result.as_deref())
                .unwrap_err()
                .exit_code(),
            69
        );
    }

    #[test]
    fn trusted_config_is_explicitly_enabled_and_cli_settings_override_it() {
        let root = tempfile::tempdir().unwrap();
        let env = std::collections::HashMap::from([(
            "HOME".to_owned(),
            root.path().to_string_lossy().into_owned(),
        )]);
        let store = loupe_core::preset_store::Store::new(root.path(), &env).unwrap();
        let settings = Settings {
            command: Some("printf no".into()),
            timeout: Some("2s".into()),
            on_error: Some(FailurePolicy::Fail),
        };
        let cli = Cli::try_parse_from(["loupe", "keep", "Question", "--on-uncertain", "fallback"])
            .unwrap();
        let Command::Keep(args) = cli.command else {
            panic!("wrong command")
        };
        let prepared = prepare(
            Kind::Keep,
            &args,
            &store,
            &loupe_core::config::Limits::default(),
            "clef-flash",
        )
        .unwrap();
        let plan = Plan::resolve(Kind::Keep, &args, &prepared, &settings)
            .unwrap()
            .unwrap();
        assert_eq!(plan.command, "printf no");
        assert_eq!(plan.timeout, Duration::from_secs(2));
        assert!(matches!(plan.failure, FailurePolicy::Fail));
        let cli = Cli::try_parse_from([
            "loupe",
            "keep",
            "Question",
            "--on-uncertain",
            "fallback",
            "--fallback",
            "printf yes",
            "--fallback-timeout",
            "20ms",
            "--on-fallback-error",
            "uncertain",
        ])
        .unwrap();
        let Command::Keep(args) = cli.command else {
            panic!("wrong command")
        };
        let prepared = prepare(
            Kind::Keep,
            &args,
            &store,
            &loupe_core::config::Limits::default(),
            "clef-flash",
        )
        .unwrap();
        let plan = Plan::resolve(Kind::Keep, &args, &prepared, &settings)
            .unwrap()
            .unwrap();
        assert_eq!(plan.command, "printf yes");
        assert_eq!(plan.timeout, Duration::from_millis(20));
        assert!(matches!(plan.failure, FailurePolicy::Uncertain));
        let cli =
            Cli::try_parse_from(["loupe", "keep", "Question", "--fallback", "printf yes"]).unwrap();
        let Command::Keep(args) = cli.command else {
            panic!("wrong command")
        };
        let prepared = prepare(
            Kind::Keep,
            &args,
            &store,
            &loupe_core::config::Limits::default(),
            "clef-flash",
        )
        .unwrap();
        assert!(Plan::resolve(Kind::Keep, &args, &prepared, &settings).is_err());
    }

    #[tokio::test]
    async fn retained_choice_and_joint_score_preserve_context_and_external_action_metadata() {
        let fixtures: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../loupe-core/testdata/captured-fallback.json"
        ))
        .unwrap();
        for fixture in fixtures {
            let format = match fixture["format"].as_str().unwrap() {
                "lines" => Format::Lines,
                "whole" => Format::Whole,
                _ => panic!("invalid retained fixture format"),
            };
            let unit = Source::new(
                std::io::Cursor::new(fixture["input"].as_str().unwrap().as_bytes()),
                Options::new(format),
                None,
            )
            .unwrap()
            .next_unit()
            .unwrap()
            .unwrap();
            let request: Request = serde_json::from_value(fixture["request"].clone()).unwrap();
            let response: Response = serde_json::from_value(fixture["response"].clone()).unwrap();
            assert_eq!(unit.state, request.state);
            let field = fixture["field"].as_str().unwrap();
            let expected: Selected =
                serde_json::from_value(fixture["external_selection"].clone()).unwrap();
            let (command, label) = match &expected {
                Selected::Choice(label) => (format!("printf '%s' '{label}'"), label.clone()),
                Selected::Score(score) => (format!("printf '%s' '{score}'"), score.to_string()),
                _ => panic!("unexpected fixture type"),
            };
            let policy = Policy {
                min_confidence: loupe_core::policy::Probability::new(1.0).unwrap(),
                ..Policy::default()
            };
            let input = Input {
                unit: &unit,
                request: &request,
                response: &response,
                field,
                policy,
            };
            let mut assessment = response.validate(&request).unwrap()[field]
                .assessment(&policy)
                .unwrap();
            assert!(assessment.uncertain);
            let plan = Plan {
                command,
                timeout: Duration::from_secs(2),
                failure: FailurePolicy::Fail,
            };
            let mut summary = Summary::new(false, false, Budget::new(Some(0)), None);
            let execution = plan
                .execute(&input, &mut assessment, None, None, &mut summary, None)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(assessment.selected, expected);
            assert!(!assessment.uncertain);
            assert_eq!(
                serde_json::to_value(&response).unwrap(),
                fixture["response"]
            );
            assert_eq!(serde_json::to_value(&request).unwrap(), fixture["request"]);
            if matches!(expected, Selected::Score(_)) {
                assert_eq!(
                    request.questions.len(),
                    4,
                    "retain every jointly deciding field"
                );
                assert_eq!(
                    request
                        .questions
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    [
                        "incident-factors",
                        "factor:customer_impact",
                        "factor:scope",
                        "factor:recovered"
                    ]
                );
            }
            let action = loupe_core::trigger::execute_with_env(
                "printf '%s|%s|%s|%s\\n' \"$LOUPE_CONFIDENCE\" \"$LOUPE_DECISION_SOURCE\" \"$LOUPE_LABEL\" \"$LOUPE_UNCERTAIN\"; cat",
                &unit, &label, &assessment, tokio::time::Instant::now() + Duration::from_secs(2),
                &crate::run::fallback::action_environment(Some(&execution)),
            ).await.unwrap();
            assert_eq!(action.code, Some(0));
            let mut expected = format!("|fallback|{label}|false\n").into_bytes();
            expected.extend_from_slice(&unit.raw);
            assert_eq!(action.stdout.bytes, expected);
        }
    }
}
