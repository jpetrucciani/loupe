use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use serde::Serialize;
use serde_json::{Value, json};

use crate::candidates::{Candidate, Id};
use crate::config::Limits;
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::policy::{
    Assessment, Destination, Policy as DecisionPolicy, Probability, Selected as Selection,
    UncertainAction,
};
use crate::selection::{Decision as Gate, Policy, Status};
use crate::tournament::{Bracket, Options, PRESENCE_ID, Planner, SELECTION_ID, Template};
use crate::wire::{Answer, Question, Request, Response};

pub const SCORE_ID: &str = "selection-relevance";

pub fn default_criteria() -> Vec<Value> {
    vec![
        json!("Unrelated or does not answer the query"),
        json!("Related context without direct evidence answering the query"),
        json!("Direct evidence answering the query"),
    ]
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    id: Id,
    bracket: Bracket,
}
impl Item {
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn request(&self) -> &Request {
        self.bracket.request()
    }
    pub fn counts(&self) -> &Counts {
        self.bracket.counts()
    }
    pub fn ensemble(&self) -> Option<&crate::ensemble::Group> {
        self.bracket.ensemble()
    }
}

#[derive(Debug, Serialize)]
pub struct Plan {
    items: Vec<Item>,
    criteria: Vec<Value>,
    #[serde(skip)]
    binding: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    pub id: Id,
    pub status: Status,
    pub gate: Gate,
    pub assessment: Assessment,
    pub ranking_score: f64,
    pub probabilities: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub overrides: IndexMap<String, Selection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<Value>,
    #[serde(skip)]
    binding: String,
    #[serde(skip)]
    policy: String,
    #[serde(skip)]
    action: UncertainAction,
}

#[derive(Clone, Debug, Serialize)]
pub struct Selected {
    pub id: Id,
    pub request_index: usize,
    pub ranking_score: f64,
    pub uncertain: bool,
}

impl Decision {
    pub fn mark_failed_fallback(&mut self, field: &str) -> Result<()> {
        if self.overrides.contains_key(field) {
            return Err(usage(
                "resolved relevance field cannot also have failed fallback",
            ));
        }
        match field {
            PRESENCE_ID | SELECTION_ID => self.gate.mark_failed_fallback(field)?,
            SCORE_ID if self.assessment.uncertain && self.gate.status() == Status::Selected => {}
            _ => {
                return Err(usage(
                    "failed independent fallback requires an eligible uncertain field",
                ));
            }
        }
        self.status = Status::Uncertain;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub original_candidates: usize,
    pub selected: Vec<Selected>,
    pub decisions: Vec<Decision>,
    pub probability_scope: &'static str,
    pub uncertain_candidates: usize,
    pub exit_code: u8,
}

impl Plan {
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    pub fn criteria(&self) -> &[Value] {
        &self.criteria
    }
    pub fn new(
        mut template: Template,
        candidates: &[Candidate<'_>],
        criteria: Vec<Value>,
        encoder: Option<&ClefEncoder>,
        limits: &Limits,
    ) -> Result<Self> {
        if template.companions.contains_key(SCORE_ID) {
            return Err(usage("companions cannot replace selection-relevance"));
        }
        if criteria.is_empty() {
            return Err(usage(
                "independent ranking requires ordered relevance levels",
            ));
        }
        template.companions.insert(SCORE_ID.to_owned(), Question::Score {
            instructions: Some(json!({"query":template.question,"task":"Score how directly the one listed candidate answers or matches the query. Judge only that candidate using its evidence and supplied context."})),
            criteria: criteria.clone(),
        });
        let mut base = template.clone();
        base.descriptions.clear();
        Planner::new(
            base,
            &[],
            encoder,
            limits,
            Options {
                take: NonZeroUsize::MIN,
                max_rounds: NonZeroUsize::MIN,
            },
        )?;
        let declared = candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<BTreeSet<_>>();
        if template
            .descriptions
            .keys()
            .any(|id| !declared.contains(id))
        {
            return Err(usage(
                "independent description refers to an absent candidate",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut items = Vec::new();
        for candidate in candidates {
            if !ids.insert(candidate.id) {
                return Err(usage("independent candidate IDs must be unique"));
            }
            let mut single = template.clone();
            single.descriptions.retain(|id, _| *id == candidate.id);
            let planner = Planner::new(
                single,
                std::slice::from_ref(candidate),
                encoder,
                limits,
                Options {
                    take: NonZeroUsize::MIN,
                    max_rounds: NonZeroUsize::MIN,
                },
            )?;
            let round = planner.first()?;
            let bracket = round
                .brackets()
                .first()
                .cloned()
                .ok_or_else(|| usage("independent candidate has no request"))?;
            items.push(Item {
                id: candidate.id,
                bracket,
            });
        }
        let binding = digest(
            &json!({"version":1,"items":items,"criteria":criteria,"candidates":candidates,"ensemble_min_agreement":template.ensemble.as_ref().map(|ensemble|ensemble.min_agreement)}),
        )?;
        Ok(Self {
            items,
            criteria,
            binding,
        })
    }

    /// Pure arithmetic on a candidate's original gate and Score probabilities.
    pub fn assess_values(
        &self,
        index: usize,
        presence: f64,
        choice: BTreeMap<String, f64>,
        probabilities: BTreeMap<String, f64>,
        policy: Policy,
    ) -> Result<Decision> {
        self.assess_values_with_overrides(
            index,
            presence,
            choice,
            probabilities,
            policy,
            &IndexMap::new(),
        )
    }

    pub fn assess_values_with_overrides(
        &self,
        index: usize,
        presence: f64,
        choice: BTreeMap<String, f64>,
        probabilities: BTreeMap<String, f64>,
        policy: Policy,
        overrides: &IndexMap<String, Selection>,
    ) -> Result<Decision> {
        let item = self
            .items
            .get(index)
            .ok_or_else(|| usage("independent request index is out of range"))?;
        let gate_policy = Policy {
            choice: DecisionPolicy::default(),
            ..policy
        };
        let mut gate_overrides = overrides.clone();
        gate_overrides.shift_remove(SCORE_ID);
        let gate =
            gate_policy.assess_with_overrides(&item.bracket, presence, choice, &gate_overrides)?;
        self.resolve(
            index,
            Prepared {
                gate,
                probabilities,
                uncertain: false,
                ensemble: None,
            },
            policy,
            overrides,
        )
    }

    fn resolve(
        &self,
        index: usize,
        prepared: Prepared,
        policy: Policy,
        overrides: &IndexMap<String, Selection>,
    ) -> Result<Decision> {
        let item = self
            .items
            .get(index)
            .ok_or_else(|| usage("independent request index is out of range"))?;
        let Prepared {
            gate,
            probabilities,
            uncertain,
            ensemble,
        } = prepared;
        let keys = (0..self.criteria.len())
            .map(|index| index.to_string())
            .collect::<BTreeSet<_>>();
        if probabilities.keys().cloned().collect::<BTreeSet<_>>() != keys {
            return Err(protocol(
                "relevance probabilities differ from the declared levels",
            ));
        }
        let mut typed = BTreeMap::new();
        let mut sum = 0.0;
        let mut ranking_score = 0.0;
        for (index, _) in self.criteria.iter().enumerate() {
            let key = index.to_string();
            let value = *probabilities
                .get(&key)
                .ok_or_else(|| protocol("relevance level is missing"))?;
            let p = Probability::new(value).map_err(|error| protocol(&error.to_string()))?;
            sum += value;
            ranking_score += index as f64 * value;
            typed.insert(key, p);
        }
        if (sum - 1.0).abs() > probabilities.len() as f64 * 0.000051 + 1e-8 {
            return Err(protocol("relevance probabilities do not sum to one"));
        }
        ranking_score /= sum;
        let mut assessment = policy.choice.score(ranking_score, &typed)?;
        assessment.uncertain |= uncertain;
        let mut score_uncertain = assessment.uncertain;
        if let Some(selected) = overrides.get(SCORE_ID) {
            let Selection::Score(value) = selected else {
                return Err(usage("external relevance selection must be a Score"));
            };
            if !value.is_finite()
                || !(0.0..=self.criteria.len().saturating_sub(1) as f64).contains(value)
            {
                return Err(usage("external relevance is outside its declared levels"));
            }
            if !assessment.uncertain || gate.status() != Status::Selected {
                return Err(usage(
                    "external relevance requires an uncertain score and resolved positive gate",
                ));
            }
            ranking_score = *value;
            score_uncertain = false;
        }
        let status = if gate.status() == Status::Absent {
            Status::Absent
        } else if gate.status() == Status::Uncertain || score_uncertain {
            Status::Uncertain
        } else {
            Status::Selected
        };
        Ok(Decision {
            id: item.id,
            status,
            gate,
            assessment,
            ranking_score,
            probabilities,
            overrides: overrides.clone(),
            ensemble,
            binding: self.binding.clone(),
            policy: policy_binding(policy)?,
            action: policy.on_uncertain,
        })
    }

    pub fn assess_response(
        &self,
        index: usize,
        response: &Response,
        policy: Policy,
    ) -> Result<Decision> {
        self.assess_response_with_overrides(index, response, policy, &IndexMap::new())
    }

    pub fn assess_response_with_overrides(
        &self,
        index: usize,
        response: &Response,
        policy: Policy,
        overrides: &IndexMap<String, Selection>,
    ) -> Result<Decision> {
        let item = self
            .items
            .get(index)
            .ok_or_else(|| usage("independent request index is out of range"))?;
        let gate_policy = Policy {
            choice: DecisionPolicy::default(),
            ..policy
        };
        let presence = crate::ensemble::field(
            item.request(),
            response,
            PRESENCE_ID,
            policy.presence,
            item.ensemble(),
        )?;
        let choice = crate::ensemble::field(
            item.request(),
            response,
            SELECTION_ID,
            DecisionPolicy::default(),
            item.ensemble(),
        )?;
        let score = crate::ensemble::field(
            item.request(),
            response,
            SCORE_ID,
            policy.choice,
            item.ensemble(),
        )?;
        let mut gate_overrides = overrides.clone();
        gate_overrides.shift_remove(SCORE_ID);
        let gate = gate_policy.assess_fields(&item.bracket, presence, choice, &gate_overrides)?;
        let Answer::Score { probabilities, .. } = score.answer else {
            return Err(protocol("independent response lacks relevance Score"));
        };
        self.resolve(
            index,
            Prepared {
                gate,
                probabilities,
                uncertain: score.assessment.uncertain,
                ensemble: score.ensemble,
            },
            policy,
            overrides,
        )
    }

    pub fn reduce(
        &self,
        decisions: &[Decision],
        policy: Policy,
        take: NonZeroUsize,
    ) -> Result<Outcome> {
        let expected = policy_binding(policy)?;
        if decisions.len() != self.items.len()
            || decisions.iter().zip(&self.items).any(|(decision, item)| {
                decision.id != item.id
                    || decision.binding != self.binding
                    || decision.policy != expected
            })
        {
            return Err(usage(
                "independent decisions are incomplete, reordered or belong to another plan/policy",
            ));
        }
        let mut selected = Vec::new();
        for (index, decision) in decisions.iter().enumerate() {
            if decision.destination()? == Destination::Stdout {
                selected.push(Selected {
                    id: decision.id,
                    request_index: index,
                    ranking_score: decision.ranking_score,
                    uncertain: decision.status == Status::Uncertain,
                });
            }
        }
        selected.sort_by(|a, b| {
            b.ranking_score
                .total_cmp(&a.ranking_score)
                .then_with(|| a.id.cmp(&b.id))
        });
        selected.truncate(take.get());
        let exit_code = u8::from(selected.is_empty());
        Ok(Outcome {
            original_candidates: self.items.len(),
            selected,
            decisions: decisions.to_vec(),
            probability_scope: "independent-candidate-score",
            uncertain_candidates: decisions
                .iter()
                .filter(|decision| decision.status == Status::Uncertain)
                .count(),
            exit_code,
        })
    }
}

struct Prepared {
    gate: Gate,
    probabilities: BTreeMap<String, f64>,
    uncertain: bool,
    ensemble: Option<Value>,
}

impl Decision {
    pub fn destination(&self) -> Result<Destination> {
        match self.status {
            Status::Absent => Ok(Destination::Omit),
            Status::Selected => Ok(Destination::Stdout),
            Status::Uncertain => {
                let mut assessment = self.assessment.clone();
                assessment.uncertain = true;
                let destination = assessment.destination(self.action)?;
                Ok(
                    if destination == Destination::Stdout && self.gate.ranking().none_wins() {
                        Destination::Omit
                    } else {
                        destination
                    },
                )
            }
        }
    }
}

fn policy_binding(policy: Policy) -> Result<String> {
    policy.presence.validate()?;
    policy.choice.validate()?;
    digest(
        &json!({"threshold":policy.presence.threshold.get(),"margin":policy.presence.margin,"confidence":policy.choice.min_confidence.get(),"gap":policy.choice.min_gap,"action":format!("{:?}",policy.on_uncertain)}),
    )
}
fn digest(value: &Value) -> Result<String> {
    serde_json::to_vec(value)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|error| usage(&error.to_string()))
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
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use indexmap::IndexMap;
    use serde_json::json;

    use crate::candidates::{Document, Id};
    use crate::config::Limits;
    use crate::independent::{Plan, SCORE_ID, default_criteria};
    use crate::policy::{Destination, Probability, Selected as Selection, UncertainAction};
    use crate::selection::{Policy, Status};
    use crate::tournament::{NONE, PRESENCE_ID, SELECTION_ID, Template};

    fn template() -> Template {
        Template {
            model: "clef-flash".to_owned(),
            state: json!({"reference":"fixed"}),
            question: json!("Which line describes a checkout outage?"),
            presence_criteria: None,
            descriptions: BTreeMap::new(),
            companions: IndexMap::new(),
            ensemble: None,
        }
    }
    fn choice(id: Id, candidate: f64) -> BTreeMap<String, f64> {
        BTreeMap::from([
            (id.to_string(), candidate),
            (NONE.to_owned(), 1.0 - candidate),
        ])
    }
    fn scores(a: f64, b: f64, c: f64) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("0".to_owned(), a),
            ("1".to_owned(), b),
            ("2".to_owned(), c),
        ])
    }

    #[test]
    fn external_independent_scores_rank_candidates_without_rewriting_distributions() {
        let document = Document::new("first\nsecond", None);
        let catalog = document.lines(false).unwrap();
        let plan = Plan::new(
            template(),
            &catalog,
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        let mut policy = Policy::default();
        policy.presence.margin = 1.0;
        policy.choice.min_confidence = Probability::new(1.0).unwrap();
        let probabilities = scores(0.1, 0.2, 0.7);
        let decisions = plan
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let overrides = IndexMap::from([
                    (PRESENCE_ID.into(), Selection::Boolean(true)),
                    (
                        SCORE_ID.into(),
                        Selection::Score(if index == 0 { 0.5 } else { 1.5 }),
                    ),
                ]);
                let decision = plan
                    .assess_values_with_overrides(
                        index,
                        0.5,
                        choice(item.id(), 0.8),
                        probabilities.clone(),
                        policy,
                        &overrides,
                    )
                    .unwrap();
                assert_eq!(decision.probabilities, probabilities);
                assert!(decision.assessment.uncertain);
                assert_eq!(decision.status, Status::Selected);
                decision
            })
            .collect::<Vec<_>>();
        let result = plan
            .reduce(&decisions, policy, NonZeroUsize::new(2).unwrap())
            .unwrap();
        assert_eq!(result.selected[0].id, catalog[1].id);
        assert_eq!(result.selected[0].ranking_score, 1.5);
        assert_eq!(result.selected[1].ranking_score, 0.5);
        for selected in [
            Selection::Score(-1.0),
            Selection::Score(3.0),
            Selection::Score(f64::NAN),
            Selection::Boolean(true),
        ] {
            assert!(
                plan.assess_values_with_overrides(
                    0,
                    0.5,
                    choice(catalog[0].id, 0.8),
                    probabilities.clone(),
                    policy,
                    &IndexMap::from([
                        (PRESENCE_ID.into(), Selection::Boolean(true)),
                        (SCORE_ID.into(), selected)
                    ])
                )
                .is_err()
            );
        }
        assert!(
            plan.assess_values_with_overrides(
                0,
                0.5,
                choice(catalog[0].id, 0.8),
                probabilities,
                policy,
                &IndexMap::from([(SCORE_ID.into(), Selection::Score(1.0))])
            )
            .is_err()
        );
    }

    #[test]
    fn each_pair_has_only_its_candidate_fixed_context_reserved_none_and_one_score() {
        let document = Document::new("first evidence\nsecond evidence\nthird evidence", None);
        let catalog = document.lines(false).unwrap();
        let mut query = template();
        query.descriptions.insert(
            catalog[1].id,
            json!({"unit":"second evidence","previous_lines":[]}),
        );
        let plan = Plan::new(
            query,
            &catalog,
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 3);
        for (index, item) in plan.items.iter().enumerate() {
            assert_eq!(item.id, catalog[index].id);
            assert_eq!(item.request().questions.len(), 3);
            assert_eq!(
                item.request().questions[SELECTION_ID]
                    .options()
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>(),
                [item.id.to_string(), NONE.to_owned()]
            );
            assert_eq!(item.request().questions[SCORE_ID].options().len(), 3);
            assert!(item.request().state.as_str().unwrap().contains("fixed"));
            for (other, candidate) in catalog.iter().enumerate() {
                assert_eq!(
                    item.request()
                        .state
                        .as_str()
                        .unwrap()
                        .contains(candidate.verbatim),
                    other == index
                );
            }
            item.counts().preflight(&Limits::default()).unwrap();
        }
    }

    #[test]
    fn means_and_top_k_are_per_candidate_with_source_order_ties_and_original_probabilities() {
        let doc = Document::new("one\ntwo\nthree", None);
        let catalog = doc.lines(false).unwrap();
        let plan = Plan::new(
            template(),
            &catalog,
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        let policy = Policy::default();
        let decisions = [
            scores(0.1, 0.2, 0.7),
            scores(0.0, 0.5, 0.5),
            scores(0.1, 0.2, 0.7),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, p)| {
            plan.assess_values(index, 1.0, choice(catalog[index].id, 0.9), p, policy)
                .unwrap()
        })
        .collect::<Vec<_>>();
        let outcome = plan
            .reduce(&decisions, policy, NonZeroUsize::new(2).unwrap())
            .unwrap();
        assert_eq!(
            outcome
                .selected
                .iter()
                .map(|value| value.id)
                .collect::<Vec<_>>(),
            [catalog[0].id, catalog[2].id]
        );
        assert_eq!(outcome.selected[0].request_index, 0);
        assert_eq!(outcome.selected[1].request_index, 2);
        assert!((outcome.selected[0].ranking_score - 1.6).abs() < 1e-12);
        assert_eq!(outcome.decisions[0].probabilities, scores(0.1, 0.2, 0.7));
        assert_eq!(outcome.probability_scope, "independent-candidate-score");
        let rounded = plan
            .assess_values(
                0,
                1.0,
                choice(catalog[0].id, 0.9),
                scores(0.1001, 0.2, 0.7),
                policy,
            )
            .unwrap();
        assert!((rounded.ranking_score - 1.6 / 1.0001).abs() < 1e-12);
    }

    #[test]
    fn absence_precedence_and_every_uncertainty_action_keep_none_out_of_selected_output() {
        let doc = Document::new("one", None);
        let catalog = doc.lines(false).unwrap();
        let plan = Plan::new(
            template(),
            &catalog,
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        for (action, destination) in [
            (UncertainAction::Keep, Some(Destination::Stdout)),
            (UncertainAction::Drop, Some(Destination::Omit)),
            (UncertainAction::Stderr, Some(Destination::Stderr)),
            (UncertainAction::Fail, None),
        ] {
            let mut policy = Policy {
                on_uncertain: action,
                ..Policy::default()
            };
            policy.choice.min_confidence = Probability::new(0.8).unwrap();
            let decision = plan
                .assess_values(
                    0,
                    1.0,
                    choice(catalog[0].id, 0.9),
                    scores(0.1, 0.2, 0.7),
                    policy,
                )
                .unwrap();
            assert_eq!(decision.status, Status::Uncertain);
            if let Some(destination) = destination {
                assert_eq!(decision.destination().unwrap(), destination);
            } else {
                assert_eq!(decision.destination().unwrap_err().exit_code(), 3);
            }
            for (presence, candidate) in [(0.0, 0.9), (1.0, 0.2)] {
                let absent = plan
                    .assess_values(
                        0,
                        presence,
                        choice(catalog[0].id, candidate),
                        scores(0.1, 0.2, 0.7),
                        policy,
                    )
                    .unwrap();
                assert_eq!(absent.status, Status::Absent);
                assert_eq!(absent.destination().unwrap(), Destination::Omit);
            }
            policy.on_uncertain = UncertainAction::Keep;
            policy.presence.margin = 0.2;
            let uncertain_none = plan
                .assess_values(
                    0,
                    0.5,
                    choice(catalog[0].id, 0.2),
                    scores(0.1, 0.2, 0.7),
                    policy,
                )
                .unwrap();
            assert_eq!(uncertain_none.status, Status::Uncertain);
            assert_eq!(uncertain_none.destination().unwrap(), Destination::Omit);
        }
    }

    #[test]
    fn malformed_distributions_changed_plans_policies_and_schema_overflow_are_rejected() {
        let doc = Document::new("one\ntwo", None);
        let catalog = doc.lines(false).unwrap();
        let policy = Policy::default();
        let plan = Plan::new(
            template(),
            &catalog,
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        for p in [
            scores(0.1, 0.1, 0.1),
            scores(f64::NAN, 0.0, 1.0),
            BTreeMap::from([("0".to_owned(), 1.0)]),
        ] {
            assert_eq!(
                plan.assess_values(0, 1.0, choice(catalog[0].id, 0.9), p, policy)
                    .unwrap_err()
                    .exit_code(),
                69
            );
        }
        let mut decisions = (0..2)
            .map(|index| {
                plan.assess_values(
                    index,
                    1.0,
                    choice(catalog[index].id, 0.9),
                    scores(0.1, 0.2, 0.7),
                    policy,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        decisions.reverse();
        assert_eq!(
            plan.reduce(&decisions, policy, NonZeroUsize::MIN)
                .unwrap_err()
                .exit_code(),
            64
        );
        decisions.reverse();
        let mut changed = policy;
        changed.presence.margin = 0.1;
        assert_eq!(
            plan.reduce(&decisions, changed, NonZeroUsize::MIN)
                .unwrap_err()
                .exit_code(),
            64
        );
        for limits in [
            Limits {
                max_questions: 2,
                ..Limits::default()
            },
            Limits {
                max_options: 2,
                ..Limits::default()
            },
            Limits {
                max_total_options: 6,
                ..Limits::default()
            },
        ] {
            assert_eq!(
                Plan::new(template(), &catalog, default_criteria(), None, &limits)
                    .unwrap_err()
                    .exit_code(),
                65
            );
        }
        let empty = Plan::new(
            template(),
            &[],
            default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            empty
                .reduce(&[], policy, NonZeroUsize::MIN)
                .unwrap()
                .exit_code,
            1
        );
        changed.presence.margin = f64::NAN;
        assert_eq!(
            empty
                .reduce(&[], changed, NonZeroUsize::MIN)
                .unwrap_err()
                .exit_code(),
            64
        );
    }
}
