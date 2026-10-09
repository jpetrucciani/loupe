use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ensemble::{Group, Set};
use crate::error::{Error, ErrorKind, Result};
use crate::independent::SCORE_ID;
use crate::policy::Probability;
use crate::tournament::{PRESENCE_ID, SELECTION_ID};
use crate::wire::{Question, Request};

#[derive(Clone, Debug, Serialize)]
struct Variant {
    id: String,
    query: Value,
    presence_criteria: Option<IndexMap<String, Value>>,
    score_criteria: Option<Vec<Value>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Ensemble {
    variants: Vec<Variant>,
    pub min_agreement: Probability,
}

impl Ensemble {
    pub fn from_query(set: &Set, primary: &str, min_agreement: Probability) -> Result<Self> {
        let variants = set
            .variants(primary)
            .map(|(id, question)| {
                let query = match question.instructions() {
                    None | Some(Value::Null) => json!(format!("ensemble:{primary}/{id}")),
                    Some(Value::String(text)) if text.is_empty() => {
                        json!(format!("ensemble:{primary}/{id}"))
                    }
                    Some(value) => value.clone(),
                };
                let (presence_criteria, score_criteria) = match question {
                    Question::Noul { criteria, .. } => (criteria.clone(), None),
                    Question::Score { criteria, .. } => (None, Some(criteria.clone())),
                    Question::Choice { .. } => {
                        return Err(usage("selection query variants require Noul or Score"));
                    }
                };
                Ok(Variant {
                    id: id.to_owned(),
                    query,
                    presence_criteria,
                    score_criteria,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if variants.is_empty() {
            return Err(usage("selection ensemble requires query variants"));
        }
        Ok(Self {
            variants,
            min_agreement,
        })
    }

    pub(crate) fn choice_heads(&self) -> usize {
        self.variants.len().saturating_add(1)
    }

    pub(crate) fn expand(&self, request: &mut Request) -> Result<()> {
        let bases = [PRESENCE_ID, SELECTION_ID, SCORE_ID]
            .into_iter()
            .filter_map(|id| {
                request
                    .questions
                    .get(id)
                    .map(|question| (id, question.clone()))
            })
            .collect::<Vec<_>>();
        for variant in &self.variants {
            for (base, question) in &bases {
                let mut question = question.clone();
                match &mut question {
                    Question::Noul {
                        instructions,
                        criteria,
                    } if *base == PRESENCE_ID => {
                        *instructions = Some(
                            json!({"question":variant.query,"task":"Does at least one candidate option in the selection field answer or match this question? Consider only the listed candidates, using state as reference context."}),
                        );
                        *criteria = variant.presence_criteria.clone();
                    }
                    Question::Choice { instructions, .. } if *base == SELECTION_ID => {
                        *instructions = Some(variant.query.clone());
                    }
                    Question::Score {
                        instructions,
                        criteria,
                    } if *base == SCORE_ID => {
                        *instructions = Some(
                            json!({"query":variant.query,"task":"Score how directly the one listed candidate answers or matches the query. Judge only that candidate using its evidence and supplied context."}),
                        );
                        if let Some(levels) = &variant.score_criteria {
                            *criteria = levels.clone();
                        }
                    }
                    _ => return Err(usage("selection ensemble base field has the wrong type")),
                }
                let id = format!("ensemble:{base}/{}", variant.id);
                if request.questions.insert(id, question).is_some() {
                    return Err(usage("selection ensemble fields collide with companions"));
                }
            }
        }
        Ok(())
    }

    pub fn group(&self, request: &Request) -> Result<Group> {
        let mut definitions = Vec::new();
        let mut bases = IndexMap::new();
        for base in [PRESENCE_ID, SELECTION_ID, SCORE_ID] {
            let Some(question) = request.questions.get(base) else {
                continue;
            };
            bases.insert(base.to_owned(), question.clone());
            for variant in &self.variants {
                let id = format!("ensemble:{base}/{}", variant.id);
                let question = request
                    .questions
                    .get(&id)
                    .ok_or_else(|| usage("selection ensemble member is missing"))?;
                let mut definition =
                    serde_json::to_value(question).map_err(|error| usage(&error.to_string()))?;
                definition["id"] = json!(variant.id);
                definition["field"] = json!(base);
                definitions.push(definition);
            }
        }
        Ok(Group {
            set: Set::from_document(&json!({"variants":definitions}), &bases)?,
            min_agreement: self.min_agreement,
        })
    }
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use indexmap::IndexMap;
    use serde_json::{Value, json};

    use crate::candidates::Document;
    use crate::config::Limits;
    use crate::ensemble::Set;
    use crate::policy::{Probability, Selected, UncertainAction};
    use crate::selection::{Policy, Session, Status, Step};
    use crate::selection_ensemble::Ensemble;
    use crate::tournament::{Options, PRESENCE_ID, Planner, SELECTION_ID, Template};
    use crate::wire::{Question, Request, Response};

    fn template(minimum: f64) -> Template {
        let questions = IndexMap::from([(
            "query".to_owned(),
            serde_json::from_value::<Question>(
                json!({"type":"noul","instructions":"Which line reports an outage?"}),
            )
            .unwrap(),
        )]);
        let variants = Set::from_document(&json!({"variants":[{"id":"alt","instructions":"Which line reports checkout is currently unavailable?"}]}), &questions).unwrap();
        Template {
            model: "probability-arithmetic".into(),
            state: Value::Null,
            question: json!("Which line reports an outage?"),
            presence_criteria: None,
            descriptions: BTreeMap::new(),
            companions: IndexMap::new(),
            ensemble: Some(
                Ensemble::from_query(&variants, "query", Probability::new(minimum).unwrap())
                    .unwrap(),
            ),
        }
    }
    fn options() -> Options {
        Options {
            take: NonZeroUsize::MIN,
            max_rounds: NonZeroUsize::new(64).unwrap(),
        }
    }
    fn response(request: &Request) -> Response {
        // Pure probability/codec arithmetic, never an inference replacement.
        let answers = request.questions.iter().map(|(id, question)| {
            let alternate = id.ends_with("/alt");
            let answer = match question {
                Question::Noul { .. } => json!({"type":"noul","noul":if alternate {0.4} else {0.9}}),
                Question::Choice { criteria, .. } => {
                    if criteria.len() == 2 {
                        let candidate = criteria.keys().find(|key|key.as_str() != "none").unwrap();
                        json!({"type":"choice","choice":candidate,"confidence":0.9,"probabilities":{candidate:0.9,"none":0.1}})
                    } else if alternate {
                        json!({"type":"choice","choice":"L2","confidence":0.85,"probabilities":{"L1":0.1,"L2":0.85,"none":0.05}})
                    } else {
                        json!({"type":"choice","choice":"L1","confidence":0.8,"probabilities":{"L1":0.8,"L2":0.1,"none":0.1}})
                    }
                }
                Question::Score { .. } => {
                    let legend: IndexMap<_, _> = question.options().into_iter().collect();
                    if alternate { json!({"type":"score","score":0.4,"confidence":0.6,"legend":legend,"probabilities":{"0":0.6,"1":0.4,"2":0.0}}) }
                    else { json!({"type":"score","score":2.0,"confidence":1.0,"legend":legend,"probabilities":{"0":0.0,"1":0.0,"2":1.0}}) }
                }
            };
            (id.clone(), answer)
        }).collect();
        Response {
            model: request.model.clone(),
            answers,
            usage: crate::wire::Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        }
    }

    #[test]
    fn selection_means_disagreement_and_external_winner_preserve_original_members() {
        let document = Document::new("first\nsecond\n", None);
        let candidates = document.lines(false).unwrap();
        let limits = Limits::default();
        let planner = Planner::new(template(1.0), &candidates, None, &limits, options()).unwrap();
        let policy = Policy {
            on_uncertain: UncertainAction::Keep,
            ..Policy::default()
        };
        let mut session = Session::new(planner, policy).unwrap();
        let round = session.first().unwrap();
        let bracket = &round.brackets()[0];
        let raw = response(bracket.request());
        let original = serde_json::to_value(&raw).unwrap();
        let decision = session.assess_response(bracket, &raw).unwrap();
        assert_eq!(decision.status(), Status::Uncertain);
        assert!(decision.presence().uncertain && decision.choice().uncertain);
        assert_eq!(decision.choice().selected, Selected::Choice("L2".into()));
        assert!((decision.ranking().probabilities()["L2"] - 0.475).abs() < 1e-12);
        assert_eq!(decision.ensembles()[PRESENCE_ID]["agreement"], 0.5);
        assert_eq!(
            decision.ensembles()[SELECTION_ID]["members"][0]["answer"],
            raw.answers[SELECTION_ID]
        );
        assert!(
            policy
                .assess(
                    bracket,
                    0.9,
                    BTreeMap::from([("L1".into(), 0.8), ("L2".into(), 0.1), ("none".into(), 0.1)])
                )
                .is_err()
        );
        let overrides = IndexMap::from([
            (PRESENCE_ID.into(), Selected::Boolean(true)),
            (SELECTION_ID.into(), Selected::Choice("L1".into())),
        ]);
        let external = session
            .assess_response_with_overrides(bracket, &raw, &overrides)
            .unwrap();
        assert_eq!(external.status(), Status::Selected);
        assert_eq!(external.ranking().selection_override(), Some("L1"));
        assert!((external.ranking().probabilities()["L1"] - 0.45).abs() < 1e-12);
        let Step::Final { outcome } = session.advance(&round, &[decision]).unwrap() else {
            panic!("unexpected additional round")
        };
        assert_eq!(outcome.tournament.selected[0].id.to_string(), "L2");
        assert_eq!(serde_json::to_value(&raw).unwrap(), original);
    }

    #[test]
    fn every_choice_head_limits_brackets_and_candidate_options_stay_aligned() {
        let document = Document::new("a\nb\nc\nd\ne\nf\n", None);
        let candidates = document.lines(false).unwrap();
        let mut limits = Limits {
            max_options: 3,
            max_total_options: 10,
            ..Limits::default()
        };
        let planner = Planner::new(template(1.0), &candidates, None, &limits, options()).unwrap();
        let round = planner.first().unwrap();
        assert_eq!(round.brackets().len(), 3);
        for bracket in round.brackets() {
            assert_eq!(bracket.candidate_ids().len(), 2);
            limits.validate_request(bracket.request()).unwrap();
            assert_eq!(
                bracket.request().questions[SELECTION_ID].options(),
                bracket.request().questions["ensemble:selection/alt"].options()
            );
            assert_eq!(
                serde_json::to_value(bracket).unwrap()["ensemble_min_agreement"],
                1.0
            );
        }
        limits.max_total_options = 8;
        assert_eq!(
            Planner::new(template(1.0), &candidates, None, &limits, options())
                .err()
                .unwrap()
                .exit_code(),
            65
        );
        limits.max_total_options = 10;
        limits.max_questions = 3;
        assert_eq!(
            Planner::new(template(1.0), &candidates, None, &limits, options())
                .err()
                .unwrap()
                .exit_code(),
            65
        );
    }

    #[test]
    fn agreement_changes_policy_bindings_without_changing_the_encoded_request() {
        let document = Document::new("first\nsecond\n", None);
        let candidates = document.lines(false).unwrap();
        let limits = Limits::default();
        let strict = Session::new(
            Planner::new(template(1.0), &candidates, None, &limits, options()).unwrap(),
            Policy::default(),
        )
        .unwrap();
        let mut relaxed = Session::new(
            Planner::new(template(0.5), &candidates, None, &limits, options()).unwrap(),
            Policy::default(),
        )
        .unwrap();
        let first = strict.first().unwrap();
        let second = relaxed.first().unwrap();
        assert_eq!(
            serde_json::to_value(first.brackets()[0].request()).unwrap(),
            serde_json::to_value(second.brackets()[0].request()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(first.brackets()[0].counts()).unwrap(),
            serde_json::to_value(second.brackets()[0].counts()).unwrap()
        );
        let raw = response(second.brackets()[0].request());
        let certain = relaxed
            .assess_response(&second.brackets()[0], &raw)
            .unwrap();
        assert_eq!(certain.status(), Status::Selected);
        let foreign = strict.assess_response(&first.brackets()[0], &raw).unwrap();
        assert!(relaxed.advance(&second, &[foreign]).is_err());
        assert!(relaxed.advance(&second, &[certain]).is_ok());
    }

    #[test]
    fn independent_score_disagreement_accepts_external_scores_after_resolved_presence() {
        let document = Document::new("first\n", None);
        let candidates = document.lines(false).unwrap();
        let plan = crate::independent::Plan::new(
            template(1.0),
            &candidates,
            crate::independent::default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        let raw = response(plan.items()[0].request());
        let original = serde_json::to_value(&raw).unwrap();
        let policy = Policy::default();
        let mut overrides = IndexMap::from([(PRESENCE_ID.into(), Selected::Boolean(true))]);
        let uncertain = plan
            .assess_response_with_overrides(0, &raw, policy, &overrides)
            .unwrap();
        assert_eq!(uncertain.status, Status::Uncertain);
        assert!((uncertain.ranking_score - 1.2).abs() < 1e-12);
        assert_eq!(uncertain.ensemble.as_ref().unwrap()["agreement"], 0.5);
        overrides.insert(crate::independent::SCORE_ID.into(), Selected::Score(0.5));
        let external = plan
            .assess_response_with_overrides(0, &raw, policy, &overrides)
            .unwrap();
        assert_eq!(external.status, Status::Selected);
        assert_eq!(external.ranking_score, 0.5);
        overrides.shift_remove(PRESENCE_ID);
        assert!(
            plan.assess_response_with_overrides(0, &raw, policy, &overrides)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&raw).unwrap(), original);
    }

    #[test]
    fn failed_independent_choice_stays_uncertain_when_the_mean_none_option_wins() {
        let document = Document::new("first\n", None);
        let candidates = document.lines(false).unwrap();
        let plan = crate::independent::Plan::new(
            template(1.0),
            &candidates,
            crate::independent::default_criteria(),
            None,
            &Limits::default(),
        )
        .unwrap();
        let mut raw = response(plan.items()[0].request());
        raw.answers[SELECTION_ID] = json!({"type":"choice","choice":"L1","confidence":0.6,"probabilities":{"L1":0.6,"none":0.4}});
        raw.answers["ensemble:selection/alt"] = json!({"type":"choice","choice":"none","confidence":0.9,"probabilities":{"L1":0.1,"none":0.9}});
        let original = serde_json::to_value(&raw).unwrap();
        let overrides = IndexMap::from([(PRESENCE_ID.into(), Selected::Boolean(true))]);
        let mut decision = plan
            .assess_response_with_overrides(0, &raw, Policy::default(), &overrides)
            .unwrap();
        assert_eq!(decision.status, Status::Absent);
        assert!(decision.gate.choice().uncertain);
        decision.mark_failed_fallback(SELECTION_ID).unwrap();
        assert_eq!(decision.status, Status::Uncertain);
        assert_eq!(decision.gate.status(), Status::Uncertain);
        let outcome = plan
            .reduce(&[decision], Policy::default(), NonZeroUsize::MIN)
            .unwrap();
        assert!(outcome.selected.is_empty());
        assert_eq!(outcome.uncertain_candidates, 1);
        assert_eq!(serde_json::to_value(&raw).unwrap(), original);
    }
}
