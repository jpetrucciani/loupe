use indexmap::IndexMap;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::fallback::Execution as External;
use loupe_core::input::Unit;
use loupe_core::policy::{Assessment, Policy, Selected};
use loupe_core::tournament::{NONE, PRESENCE_ID, SELECTION_ID};
use loupe_core::wire::{Request, Response};
use std::sync::Arc;

use crate::run::selection::{Execution, History, Mode, invalid};
use crate::summary::Summary;

pub(crate) struct Input<'a> {
    pub(crate) unit: &'a Unit,
    pub(crate) request: &'a Request,
    pub(crate) response: &'a Response,
    pub(crate) round: usize,
    pub(crate) request_index: usize,
    pub(crate) ensemble: Option<&'a loupe_core::ensemble::Group>,
}

impl Execution<'_> {
    pub(crate) async fn resolve_fallback(
        &self,
        input: Input<'_>,
        history: &mut History,
        summary: &mut Summary,
    ) -> Result<IndexMap<String, Selected>> {
        let previous = history
            .fallbacks
            .get(&input.round)
            .and_then(|requests| requests.get(&input.request_index));
        let Some(plan) = &self.fallback else {
            if previous.is_some_and(|values| !values.is_empty()) {
                return Err(invalid(
                    "saved selection fallback requires its pinned command",
                ));
            }
            return Ok(IndexMap::new());
        };
        let score_id = loupe_core::independent::SCORE_ID;
        if previous.is_some_and(|values| {
            values.keys().any(|field| {
                field != PRESENCE_ID
                    && field != SELECTION_ID
                    && !(self.mode == Mode::Independent && field == score_id)
            })
        }) {
            return Err(invalid("saved selection fallback field is invalid"));
        }
        let choice_policy = if self.mode == Mode::Independent {
            Policy::default()
        } else {
            self.policy.choice
        };
        let mut assessments = IndexMap::<&str, (Assessment, Policy)>::new();
        for (field, policy) in [
            (PRESENCE_ID, self.policy.presence),
            (SELECTION_ID, choice_policy),
        ] {
            let value = loupe_core::ensemble::field(
                input.request,
                input.response,
                field,
                policy,
                input.ensemble,
            )?;
            assessments.insert(field, (value.assessment, policy));
        }
        if self.mode == Mode::Independent {
            let value = loupe_core::ensemble::field(
                input.request,
                input.response,
                score_id,
                self.policy.choice,
                input.ensemble,
            )?;
            assessments.insert(score_id, (value.assessment, self.policy.choice));
        }
        let view = crate::run::fallback::request_unit(input.unit, input.request)?;
        let mut overrides = IndexMap::new();
        for field in [PRESENCE_ID, SELECTION_ID, score_id] {
            if !assessments.contains_key(field) {
                break;
            }
            let gate = assessments
                .get(PRESENCE_ID)
                .ok_or_else(|| invalid("selection presence missing"))?;
            let choice = assessments
                .get(SELECTION_ID)
                .ok_or_else(|| invalid("selection choice missing"))?;
            let skip = field != PRESENCE_ID
                && (gate.0.uncertain || gate.0.selected != Selected::Boolean(true))
                || field == score_id
                    && (choice.0.uncertain || choice.0.selected == Selected::Choice(NONE.into()));
            let previous = history
                .fallbacks
                .get(&input.round)
                .and_then(|requests| requests.get(&input.request_index))
                .and_then(|values| values.get(field))
                .cloned()
                .map(Arc::new);
            if skip {
                if previous.is_some() {
                    return Err(invalid(
                        "saved selection fallback lacks a resolved positive gate",
                    ));
                }
                continue;
            }
            let (assessment, policy) = assessments
                .get_mut(field)
                .ok_or_else(|| invalid("selection assessment missing"))?;
            let fresh = previous.is_none();
            if history.complete && assessment.uncertain && fresh {
                return Err(invalid(
                    "complete selection history is missing a fallback result",
                ));
            }
            let fallback_input = loupe_core::fallback::Input {
                unit: &view,
                request: input.request,
                response: input.response,
                field,
                policy: *policy,
            };
            let ensemble = input.ensemble.map(|group| loupe_core::fallback::Ensemble {
                set: &group.set,
                primary: field,
                min_agreement: group.min_agreement,
            });
            let external = plan
                .execute(
                    &fallback_input,
                    assessment,
                    previous,
                    self.deadline,
                    summary,
                    ensemble.as_ref(),
                )
                .await?;
            if let Some(external) = external {
                if let External::Resolved { selected, .. } = external.as_ref() {
                    overrides.insert(field.into(), selected.clone());
                }
                if fresh {
                    history
                        .fallbacks
                        .entry(input.round)
                        .or_default()
                        .entry(input.request_index)
                        .or_default()
                        .insert(field.into(), external.as_ref().clone());
                    history.checkpoint(self.recovery.as_ref(), input.unit.index)?;
                }
                if self
                    .deadline
                    .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
                {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        "overall selection deadline expired during fallback",
                    ));
                }
                plan.require_success(Some(external.as_ref()))?;
            }
        }
        Ok(overrides)
    }
}
