use indexmap::IndexMap;
use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Destination, Policy as DecisionPolicy, Selected, UncertainAction};
use crate::tournament::{
    Bracket, Final, PRESENCE_ID, Planner, Ranking, Reduction, Round, SELECTION_ID,
};
use crate::wire::{Answer, Response};

#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub presence: DecisionPolicy,
    pub choice: DecisionPolicy,
    pub on_uncertain: UncertainAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Selected,
    Absent,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Candidate,
    PresenceNo,
    NoneWins,
    PresenceUncertain,
    ChoiceUncertain,
}

#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    status: Status,
    reason: Reason,
    presence_probability: f64,
    presence: Assessment,
    choice: Assessment,
    ranking: Ranking,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    overrides: IndexMap<String, Selected>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    ensembles: IndexMap<String, serde_json::Value>,
    #[serde(skip)]
    policy: [f64; 8],
    #[serde(skip)]
    action: UncertainAction,
}

impl Policy {
    fn validate(self) -> Result<()> {
        self.presence.validate()?;
        self.choice.validate()
    }

    fn binding(self) -> [f64; 8] {
        [
            self.presence.threshold.get(),
            self.presence.margin,
            self.presence.min_confidence.get(),
            self.presence.min_gap,
            self.choice.threshold.get(),
            self.choice.margin,
            self.choice.min_confidence.get(),
            self.choice.min_gap,
        ]
    }

    /// Pure policy arithmetic over one bracket's original probabilities.
    pub fn assess(
        self,
        bracket: &Bracket,
        presence_probability: f64,
        probabilities: BTreeMap<String, f64>,
    ) -> Result<Decision> {
        self.assess_with_overrides(
            bracket,
            presence_probability,
            probabilities,
            &IndexMap::new(),
        )
    }

    pub fn assess_with_overrides(
        self,
        bracket: &Bracket,
        presence_probability: f64,
        probabilities: BTreeMap<String, f64>,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        if bracket.ensemble().is_some() {
            return Err(usage(
                "ensemble selection requires complete member responses",
            ));
        }
        self.validate()?;
        let ranking = bracket.rank(probabilities)?;
        let presence = self
            .presence
            .noul(crate::policy::Probability::new(presence_probability)?)?;
        let choice = self.choice.choice(
            &ranking
                .probabilities()
                .iter()
                .map(|(key, value)| Ok((key.clone(), crate::policy::Probability::new(*value)?)))
                .collect::<Result<_>>()?,
        )?;
        self.resolve(
            bracket,
            Prepared {
                presence_probability,
                presence,
                choice,
                ranking,
                ensembles: IndexMap::new(),
            },
            overrides,
        )
    }

    fn resolve(
        self,
        bracket: &Bracket,
        prepared: Prepared,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        let Prepared {
            presence_probability,
            presence,
            choice,
            mut ranking,
            ensembles,
        } = prepared;
        let mut effective_presence = presence.clone();
        let mut effective_choice = choice.clone();
        for (id, selected) in overrides {
            let assessment = match (id.as_str(), selected) {
                (PRESENCE_ID, Selected::Boolean(_)) => &mut effective_presence,
                (SELECTION_ID, Selected::Choice(value))
                    if ranking.probabilities().contains_key(value) =>
                {
                    &mut effective_choice
                }
                _ => return Err(usage("invalid selection override field, type or option")),
            };
            if !assessment.uncertain {
                return Err(usage(
                    "selection overrides require an uncertain model field",
                ));
            }
            assessment.selected = selected.clone();
            assessment.uncertain = false;
        }
        if let Some(Selected::Choice(value)) = overrides.get(SELECTION_ID) {
            if effective_presence.uncertain
                || effective_presence.selected != Selected::Boolean(true)
            {
                return Err(usage(
                    "external Choice requires a resolved positive presence gate",
                ));
            }
            ranking = bracket.resolve_choice(&ranking, value)?;
        }
        let (status, reason) = if effective_presence.uncertain {
            (Status::Uncertain, Reason::PresenceUncertain)
        } else if effective_presence.selected == Selected::Boolean(false) {
            (Status::Absent, Reason::PresenceNo)
        } else if ranking.none_wins() {
            (Status::Absent, Reason::NoneWins)
        } else if effective_choice.uncertain {
            (Status::Uncertain, Reason::ChoiceUncertain)
        } else {
            (Status::Selected, Reason::Candidate)
        };
        Ok(Decision {
            status,
            reason,
            presence_probability,
            presence,
            choice,
            ranking,
            overrides: overrides.clone(),
            ensembles,
            policy: self.binding(),
            action: self.on_uncertain,
        })
    }

    pub fn assess_response(self, bracket: &Bracket, response: &Response) -> Result<Decision> {
        self.assess_response_with_overrides(bracket, response, &IndexMap::new())
    }

    pub fn assess_response_with_overrides(
        self,
        bracket: &Bracket,
        response: &Response,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        let presence = crate::ensemble::field(
            bracket.request(),
            response,
            PRESENCE_ID,
            self.presence,
            bracket.ensemble(),
        )?;
        let choice = crate::ensemble::field(
            bracket.request(),
            response,
            SELECTION_ID,
            self.choice,
            bracket.ensemble(),
        )?;
        self.assess_fields(bracket, presence, choice, overrides)
    }

    pub(crate) fn assess_fields(
        self,
        bracket: &Bracket,
        presence: crate::ensemble::Field,
        choice: crate::ensemble::Field,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        self.validate()?;
        let Answer::Noul { noul } = presence.answer else {
            return Err(protocol("selection response lacks its presence Noul"));
        };
        let Answer::Choice { probabilities, .. } = choice.answer else {
            return Err(protocol("selection response lacks its Choice"));
        };
        let ranking = bracket.rank(probabilities)?;
        let ensembles = [
            (PRESENCE_ID, presence.ensemble),
            (SELECTION_ID, choice.ensemble),
        ]
        .into_iter()
        .filter_map(|(id, value)| value.map(|value| (id.to_owned(), value)))
        .collect();
        self.resolve(
            bracket,
            Prepared {
                presence_probability: noul,
                presence: presence.assessment,
                choice: choice.assessment,
                ranking,
                ensembles,
            },
            overrides,
        )
    }
}

struct Prepared {
    presence_probability: f64,
    presence: Assessment,
    choice: Assessment,
    ranking: Ranking,
    ensembles: IndexMap<String, serde_json::Value>,
}

impl Decision {
    pub fn status(&self) -> Status {
        self.status
    }
    pub fn reason(&self) -> Reason {
        self.reason
    }
    pub fn presence(&self) -> &Assessment {
        &self.presence
    }
    pub fn choice(&self) -> &Assessment {
        &self.choice
    }
    pub fn ranking(&self) -> &Ranking {
        &self.ranking
    }

    pub fn overrides(&self) -> &IndexMap<String, Selected> {
        &self.overrides
    }
    pub fn ensembles(&self) -> &IndexMap<String, serde_json::Value> {
        &self.ensembles
    }

    pub fn mark_failed_fallback(&mut self, field: &str) -> Result<()> {
        if self.overrides.contains_key(field) {
            return Err(usage(
                "resolved selection field cannot also have failed fallback",
            ));
        }
        self.reason = match field {
            PRESENCE_ID if self.presence.uncertain => Reason::PresenceUncertain,
            SELECTION_ID if self.choice.uncertain => {
                let presence = self
                    .overrides
                    .get(PRESENCE_ID)
                    .unwrap_or(&self.presence.selected);
                if (self.presence.uncertain && !self.overrides.contains_key(PRESENCE_ID))
                    || presence != &Selected::Boolean(true)
                {
                    return Err(usage(
                        "failed Choice fallback requires a resolved positive gate",
                    ));
                }
                Reason::ChoiceUncertain
            }
            _ => {
                return Err(usage(
                    "failed selection fallback requires an uncertain field",
                ));
            }
        };
        self.status = Status::Uncertain;
        Ok(())
    }

    pub fn destination(&self) -> Result<Destination> {
        match self.status {
            Status::Absent => Ok(Destination::Omit),
            Status::Selected => Ok(Destination::Stdout),
            Status::Uncertain => {
                let mut assessment = self.choice.clone();
                assessment.uncertain = true;
                let destination = assessment.destination(self.action)?;
                // Keeping uncertainty never turns a winning none into a span.
                Ok(
                    if destination == Destination::Stdout && self.ranking.none_wins() {
                        Destination::Omit
                    } else {
                        destination
                    },
                )
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub tournament: Final,
    pub status: Status,
    pub destination: Destination,
    pub exit_code: u8,
    pub uncertain_brackets: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "stage", rename_all = "lowercase")]
pub enum Step {
    Next { round: Round },
    Final { outcome: Outcome },
}

/// The caller owns submission, diagnostics and uncertainty-file writes.
/// This session applies the same policy to every bracket before reduction.
pub struct Session<'a> {
    planner: Planner<'a>,
    policy: Policy,
    expected_round: Option<usize>,
    uncertain_brackets: usize,
}

impl<'a> Session<'a> {
    pub fn new(planner: Planner<'a>, policy: Policy) -> Result<Self> {
        policy.validate()?;
        Ok(Self {
            planner,
            policy,
            expected_round: Some(1),
            uncertain_brackets: 0,
        })
    }

    pub fn first(&self) -> Result<Round> {
        if self.expected_round != Some(1) {
            return Err(usage("selection session has already advanced"));
        }
        self.planner.first()
    }

    pub fn assess_response(&self, bracket: &Bracket, response: &Response) -> Result<Decision> {
        self.policy.assess_response(bracket, response)
    }

    pub fn assess_response_with_overrides(
        &self,
        bracket: &Bracket,
        response: &Response,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        self.policy
            .assess_response_with_overrides(bracket, response, overrides)
    }

    pub fn advance(&mut self, round: &Round, decisions: &[Decision]) -> Result<Step> {
        if self.expected_round != Some(round.number())
            || decisions.iter().any(|decision| {
                decision.policy != self.policy.binding()
                    || decision.action != self.policy.on_uncertain
            })
        {
            return Err(usage(
                "selection round was replayed or decision policy changed",
            ));
        }
        let rankings = decisions
            .iter()
            .map(|decision| decision.ranking.clone())
            .collect::<Vec<_>>();
        self.planner.validate_rankings(round, &rankings)?;
        let destinations = decisions
            .iter()
            .map(Decision::destination)
            .collect::<Result<Vec<_>>>()?;
        let eligible = destinations
            .iter()
            .map(|destination| *destination == Destination::Stdout)
            .collect::<Vec<_>>();
        let reduction = self.planner.reduce_eligible(round, &rankings, &eligible)?;
        self.uncertain_brackets = self
            .uncertain_brackets
            .checked_add(
                decisions
                    .iter()
                    .filter(|decision| decision.status == Status::Uncertain)
                    .count(),
            )
            .ok_or_else(|| usage("selection uncertainty count overflow"))?;
        match reduction {
            Reduction::Next { round } => {
                self.expected_round = Some(round.number());
                Ok(Step::Next { round })
            }
            Reduction::Final { result } => {
                self.expected_round = None;
                let emitted = !result.selected.is_empty();
                let status = if decisions
                    .iter()
                    .any(|decision| decision.status == Status::Uncertain)
                    || (!emitted && self.uncertain_brackets > 0)
                {
                    Status::Uncertain
                } else if emitted {
                    Status::Selected
                } else {
                    Status::Absent
                };
                let destination = if emitted {
                    Destination::Stdout
                } else if status == Status::Uncertain
                    && self.policy.on_uncertain == UncertainAction::Stderr
                {
                    Destination::Stderr
                } else {
                    Destination::Omit
                };
                Ok(Step::Final {
                    outcome: Outcome {
                        tournament: result,
                        status,
                        destination,
                        exit_code: u8::from(!emitted),
                        uncertain_brackets: self.uncertain_brackets,
                    },
                })
            }
        }
    }
}

fn protocol(message: &str) -> Error {
    Error::new(ErrorKind::Unavailable, message)
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::num::NonZeroUsize;

    use indexmap::IndexMap;
    use serde_json::json;

    use crate::candidates::Document;
    use crate::config::{Config, Limits, Settings};
    use crate::policy::{Destination, Probability, Selected, UncertainAction};
    use crate::selection::{Policy, Reason, Session, Status, Step};
    use crate::tournament::{Bracket, NONE, Options, PRESENCE_ID, Planner, SELECTION_ID, Template};

    fn template() -> Template {
        Template {
            model: "clef-flash".to_owned(),
            state: serde_json::Value::Null,
            question: json!("Which line describes a current checkout outage?"),
            presence_criteria: None,
            descriptions: BTreeMap::new(),
            companions: IndexMap::new(),
            ensemble: None,
        }
    }

    fn options(rounds: usize) -> Options {
        Options {
            take: NonZeroUsize::new(1).unwrap(),
            max_rounds: NonZeroUsize::new(rounds).unwrap(),
        }
    }

    fn distribution(bracket: &Bracket, winner: &str, probability: f64) -> BTreeMap<String, f64> {
        let other = (1.0 - probability) / bracket.candidate_ids().len() as f64;
        bracket
            .candidate_ids()
            .iter()
            .map(ToString::to_string)
            .chain(std::iter::once(NONE.to_owned()))
            .map(|id| {
                let value = if id == winner { probability } else { other };
                (id, value)
            })
            .collect()
    }

    #[test]
    fn external_choice_keeps_one_winner_without_changing_model_probabilities() {
        let document = Document::new("first\nsecond\nthird", None);
        let catalog = document.lines(false).unwrap();
        let planner = Planner::new(
            template(),
            &catalog,
            None,
            &Limits::default(),
            Options {
                take: NonZeroUsize::new(2).unwrap(),
                ..options(64)
            },
        )
        .unwrap();
        let mut policy = Policy::default();
        policy.presence.margin = 1.0;
        policy.choice.min_confidence = Probability::new(1.0).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let round = session.first().unwrap();
        let bracket = &round.brackets()[0];
        let probabilities = distribution(bracket, "L1", 0.7);
        let overrides = IndexMap::from([
            (PRESENCE_ID.into(), Selected::Boolean(true)),
            (SELECTION_ID.into(), Selected::Choice("L2".into())),
        ]);
        let decision = policy
            .assess_with_overrides(bracket, 0.5, probabilities.clone(), &overrides)
            .unwrap();
        assert!(decision.presence().uncertain && decision.choice().uncertain);
        assert_eq!(decision.status(), Status::Selected);
        assert_eq!(decision.ranking().probabilities(), &probabilities);
        let Step::Final { outcome } = session.advance(&round, &[decision]).unwrap() else {
            panic!("expected final");
        };
        assert_eq!(outcome.tournament.selected.len(), 1);
        assert_eq!(outcome.tournament.selected[0].id.to_string(), "L2");
        assert_eq!(
            outcome.tournament.selected[0].probability,
            probabilities["L2"]
        );
        for invalid in [
            IndexMap::from([(SELECTION_ID.into(), Selected::Choice("L2".into()))]),
            IndexMap::from([
                (PRESENCE_ID.into(), Selected::Boolean(false)),
                (SELECTION_ID.into(), Selected::Choice("L2".into())),
            ]),
            IndexMap::from([
                (PRESENCE_ID.into(), Selected::Boolean(true)),
                (SELECTION_ID.into(), Selected::Choice("L9".into())),
            ]),
            IndexMap::from([(PRESENCE_ID.into(), Selected::Score(1.0))]),
        ] {
            assert!(
                policy
                    .assess_with_overrides(bracket, 0.5, probabilities.clone(), &invalid)
                    .is_err()
            );
        }
        let none = policy
            .assess_with_overrides(
                bracket,
                0.5,
                probabilities,
                &IndexMap::from([
                    (PRESENCE_ID.into(), Selected::Boolean(true)),
                    (SELECTION_ID.into(), Selected::Choice(NONE.into())),
                ]),
            )
            .unwrap();
        assert_eq!(none.status(), Status::Absent);
        assert_eq!(none.destination().unwrap(), Destination::Omit);
    }

    #[test]
    fn external_winners_progress_through_overflow_brackets_in_source_order() {
        let document = Document::new("a\nb\nc\nd\ne\nf", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
        let mut policy = Policy::default();
        policy.presence.margin = 1.0;
        policy.choice.min_confidence = Probability::new(1.0).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let mut round = session.first().unwrap();
        loop {
            let decisions = round
                .brackets()
                .iter()
                .map(|bracket| {
                    let id = bracket.candidate_ids()[0].to_string();
                    policy
                        .assess_with_overrides(
                            bracket,
                            0.5,
                            distribution(bracket, &id, 0.7),
                            &IndexMap::from([
                                (PRESENCE_ID.into(), Selected::Boolean(true)),
                                (SELECTION_ID.into(), Selected::Choice(id)),
                            ]),
                        )
                        .unwrap()
                })
                .collect::<Vec<_>>();
            match session.advance(&round, &decisions).unwrap() {
                Step::Next { round: next } => round = next,
                Step::Final { outcome } => {
                    assert_eq!(outcome.tournament.selected[0].id.to_string(), "L1");
                    assert_eq!(outcome.tournament.round, 3);
                    break;
                }
            }
        }
    }

    #[test]
    fn failed_choice_fallback_remains_uncertain_when_model_none_had_won() {
        let document = Document::new("first", None);
        let catalog = document.lines(false).unwrap();
        let planner =
            Planner::new(template(), &catalog, None, &Limits::default(), options(64)).unwrap();
        let round = planner.first().unwrap();
        let bracket = &round.brackets()[0];
        let mut policy = Policy::default();
        policy.choice.min_confidence = Probability::new(0.99).unwrap();
        let probabilities = distribution(bracket, NONE, 0.8);
        let mut decision = policy.assess(bracket, 1.0, probabilities.clone()).unwrap();
        assert_eq!(decision.status(), Status::Absent);
        decision.mark_failed_fallback(SELECTION_ID).unwrap();
        assert_eq!(decision.status(), Status::Uncertain);
        assert_eq!(decision.reason(), Reason::ChoiceUncertain);
        assert_eq!(decision.ranking().probabilities(), &probabilities);
        assert_eq!(decision.destination().unwrap(), Destination::Omit);
        assert!(decision.mark_failed_fallback(PRESENCE_ID).is_err());
    }

    #[test]
    fn certain_presence_no_and_none_override_choice_confidence_without_relabeling() {
        let doc = Document::new("Checkout is broken", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits::default();
        let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
        let round = planner.first().unwrap();
        let bracket = &round.brackets()[0];
        let mut policy = Policy::default();
        policy.choice.min_confidence = Probability::new(0.99).unwrap();
        for (presence, winner, reason) in [
            (0.0, "L1", Reason::PresenceNo),
            (1.0, NONE, Reason::NoneWins),
        ] {
            let probabilities = distribution(bracket, winner, 0.8);
            let decision = policy
                .assess(bracket, presence, probabilities.clone())
                .unwrap();
            assert_eq!(decision.status(), Status::Absent);
            assert_eq!(decision.reason(), reason);
            assert!(decision.choice().uncertain);
            assert_eq!(decision.ranking().probabilities(), &probabilities);
            assert_eq!(decision.destination().unwrap(), Destination::Omit);
        }
        policy.choice.min_confidence = Probability::new(0.0).unwrap();
        let equality = policy
            .assess(bracket, 0.5, distribution(bracket, "L1", 0.8))
            .unwrap();
        assert_eq!(equality.status(), Status::Selected);
        assert_eq!(equality.presence().confidence.get(), 0.5);
        policy.presence.margin = 0.125;
        assert_eq!(
            policy
                .assess(bracket, 0.375, distribution(bracket, "L1", 0.8))
                .unwrap()
                .status(),
            Status::Absent
        );
        assert_eq!(
            policy
                .assess(bracket, 0.625, distribution(bracket, "L1", 0.8))
                .unwrap()
                .status(),
            Status::Selected
        );
    }

    #[test]
    fn uncertain_presence_obeys_every_action_and_never_invents_a_span_for_none() {
        let doc = Document::new("one", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits::default();
        for (action, destination, code) in [
            (UncertainAction::Keep, Some(Destination::Stdout), 0),
            (UncertainAction::Drop, Some(Destination::Omit), 1),
            (UncertainAction::Stderr, Some(Destination::Stderr), 1),
            (UncertainAction::Fail, None, 3),
        ] {
            let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
            let mut policy = Policy {
                on_uncertain: action,
                ..Policy::default()
            };
            policy.presence.margin = 0.2;
            let mut session = Session::new(planner, policy).unwrap();
            let round = session.first().unwrap();
            let bracket = &round.brackets()[0];
            let decision = policy
                .assess(bracket, 0.5, distribution(bracket, "L1", 0.9))
                .unwrap();
            assert_eq!(decision.reason(), Reason::PresenceUncertain);
            let step = session.advance(&round, &[decision]);
            if let Some(destination) = destination {
                let Step::Final { outcome } = step.unwrap() else {
                    panic!("expected final")
                };
                assert_eq!(outcome.status, Status::Uncertain);
                assert_eq!(outcome.destination, destination);
                assert_eq!(outcome.exit_code, code);
                assert_eq!(outcome.uncertain_brackets, 1);
                assert_eq!(outcome.tournament.selected.len(), usize::from(code == 0));
            } else {
                assert_eq!(step.unwrap_err().exit_code(), code);
            }
            policy.on_uncertain = UncertainAction::Keep;
            let decision = policy
                .assess(bracket, 0.5, distribution(bracket, NONE, 0.9))
                .unwrap();
            assert_eq!(decision.status(), Status::Uncertain);
            assert_eq!(decision.destination().unwrap(), Destination::Omit);
            assert!(decision.ranking().none_wins());
        }
    }

    #[test]
    fn choice_ties_confidence_and_gap_reuse_the_shared_policy() {
        let doc = Document::new("one\ntwo", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits::default();
        let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
        let round = planner.first().unwrap();
        let bracket = &round.brackets()[0];
        let tied = BTreeMap::from([
            ("L2".to_owned(), 0.4),
            ("L1".to_owned(), 0.4),
            (NONE.to_owned(), 0.2),
        ]);
        let decision = Policy::default()
            .assess(bracket, 1.0, tied.clone())
            .unwrap();
        assert_eq!(decision.status(), Status::Uncertain);
        assert_eq!(decision.reason(), Reason::ChoiceUncertain);
        assert_eq!(decision.ranking().ranked()[0].id.to_string(), "L1");
        assert_eq!(decision.ranking().probabilities(), &tied);
        let mut policy = Policy::default();
        policy.choice.min_confidence = Probability::new(0.9).unwrap();
        assert_eq!(
            policy
                .assess(bracket, 1.0, distribution(bracket, "L2", 0.8))
                .unwrap()
                .status(),
            Status::Uncertain
        );
        policy.choice.min_confidence = Probability::new(0.8).unwrap();
        assert_eq!(
            policy
                .assess(bracket, 1.0, distribution(bracket, "L2", 0.8))
                .unwrap()
                .status(),
            Status::Selected
        );
        policy.choice.min_gap = 0.8;
        assert_eq!(
            policy
                .assess(bracket, 1.0, distribution(bracket, "L2", 0.8))
                .unwrap()
                .status(),
            Status::Uncertain
        );
    }

    #[test]
    fn presence_prunes_preliminary_winners_and_final_scores_keep_original_ids() {
        let doc = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
        let policy = Policy::default();
        let mut session = Session::new(planner, policy).unwrap();
        let first = session.first().unwrap();
        let decisions = first
            .brackets()
            .iter()
            .enumerate()
            .map(|(index, bracket)| {
                policy
                    .assess(
                        bracket,
                        if index == 0 { 0.0 } else { 1.0 },
                        distribution(bracket, &bracket.candidate_ids()[0].to_string(), 0.8),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let Step::Next { round } = session.advance(&first, &decisions).unwrap() else {
            panic!("expected next round")
        };
        assert_eq!(
            round.brackets()[0].candidate_ids(),
            &["L3".parse().unwrap()]
        );
        assert!(session.first().is_err());
        let decision = policy
            .assess(
                &round.brackets()[0],
                1.0,
                distribution(&round.brackets()[0], "L3", 0.6),
            )
            .unwrap();
        let Step::Final { outcome } = session
            .advance(&round, std::slice::from_ref(&decision))
            .unwrap()
        else {
            panic!("expected final")
        };
        assert_eq!(outcome.status, Status::Selected);
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.tournament.selected[0].id.to_string(), "L3");
        assert_eq!(outcome.tournament.selected[0].probability, 0.6);
        assert_eq!(
            outcome.tournament.probability_scope,
            "final-surviving-bracket"
        );
        assert_eq!(
            session
                .advance(&round, &[decision])
                .unwrap_err()
                .exit_code(),
            64
        );
    }

    #[test]
    fn discarded_uncertainty_is_preserved_when_every_bracket_finishes_without_survivors() {
        let doc = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let mut policy = Policy::default();
        policy.presence.margin = 0.2;
        let planner = Planner::new(template(), &catalog, None, &limits, options(1)).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let first = session.first().unwrap();
        let decisions = first
            .brackets()
            .iter()
            .enumerate()
            .map(|(index, bracket)| {
                policy
                    .assess(
                        bracket,
                        if index == 0 { 0.5 } else { 0.0 },
                        distribution(bracket, &bracket.candidate_ids()[0].to_string(), 0.8),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let Step::Final { outcome } = session.advance(&first, &decisions).unwrap() else {
            panic!("no extra request")
        };
        assert_eq!(outcome.status, Status::Uncertain);
        assert_eq!(outcome.uncertain_brackets, 1);
        assert_eq!(outcome.exit_code, 1);
        assert_eq!(
            outcome.tournament.probability_scope,
            "no-surviving-candidates"
        );
        assert!(outcome.tournament.ranking.is_none());
        assert!(outcome.tournament.selected.is_empty());
        assert_eq!(decisions[0].reason(), Reason::PresenceUncertain);
        assert_eq!(decisions[1].reason(), Reason::PresenceNo);
    }

    #[test]
    fn changed_policies_mismatched_brackets_empty_input_and_serialized_evidence_are_checked() {
        let doc = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = doc.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let policy = Policy::default();
        let planner = Planner::new(template(), &catalog, None, &limits, options(64)).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let first = session.first().unwrap();
        let mut decisions = first
            .brackets()
            .iter()
            .map(|bracket| {
                policy
                    .assess(bracket, 0.0, distribution(bracket, NONE, 0.9))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        decisions.reverse();
        assert_eq!(
            session.advance(&first, &decisions).unwrap_err().exit_code(),
            64
        );
        decisions.reverse();
        let mut other = policy;
        other.choice.min_gap = 0.1;
        let changed = other
            .assess(
                &first.brackets()[0],
                0.0,
                distribution(&first.brackets()[0], NONE, 0.9),
            )
            .unwrap();
        let original = decisions[0].clone();
        decisions[0] = changed;
        assert_eq!(
            session.advance(&first, &decisions).unwrap_err().exit_code(),
            64
        );
        decisions[0] = original;
        let Step::Final { outcome } = session.advance(&first, &decisions).unwrap() else {
            panic!("expected absent")
        };
        assert_eq!(outcome.status, Status::Absent);
        let serialized = serde_json::to_value(&decisions[0]).unwrap();
        assert_eq!(serialized["presence_probability"], 0.0);
        assert_eq!(serialized["presence"]["confidence"], 1.0);
        assert_eq!(serialized["reason"], "presence_no");
        assert!(serialized.get("policy").is_none());
        let empty = Document::new("", None);
        let catalog = empty.lines(false).unwrap();
        let planner = Planner::new(template(), &catalog, None, &limits, options(1)).unwrap();
        let mut session = Session::new(planner, policy).unwrap();
        let first = session.first().unwrap();
        assert!(first.brackets().is_empty());
        let Step::Final { outcome } = session.advance(&first, &[]).unwrap() else {
            panic!("empty input finishes")
        };
        assert_eq!(outcome.status, Status::Absent);
        assert_eq!(outcome.exit_code, 1);
    }

    #[tokio::test]
    #[ignore = "requires real server model; no fake HTTP/model backend"]
    async fn real_server_presence_policy_selects_positive_and_rejects_absent_answers() {
        use crate::backend::Backend;
        use crate::budget::Budget;

        let environment = HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = Config::default()
            .resolve(
                None,
                &Settings {
                    base_url: Some(
                        std::env::var("LOUPE_TEST_URL")
                            .unwrap_or_else(|_| "http://127.0.0.1:8015".to_owned()),
                    ),
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER").ok(),
                    model: Some("clef-flash".to_owned()),
                    max_options: Some(4),
                    ..Settings::default()
                },
                &environment,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = Backend::new(&profile).await.unwrap();
        let discovery = backend.discover(&profile.limits).await.unwrap();
        let budget = Budget::new(None);
        let deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(60));
        let mut captures = Vec::new();
        for (name, text, expected) in [
            (
                "positive",
                "Blue skies and sunshine.\nCheckout works normally.\nCheckout is unavailable and customers cannot pay.\nA football team won its match.\nThe archive contains old logs.\nTraffic is moving smoothly.\nThe database backup completed.\nA new coffee shop opened.\nClouds will clear tonight.\nBirds sing outside.\nThe garden needs water.",
                Some("L3"),
            ),
            (
                "absent",
                "Blue skies and sunshine.\nThe garden needs water.\nA football team won its match.",
                None,
            ),
        ] {
            let document = Document::new(text, None);
            let catalog = document.lines(false).unwrap();
            let planner = Planner::new(
                template(),
                &catalog,
                Some(&encoder),
                &discovery.limits,
                options(64),
            )
            .unwrap();
            let mut policy = Policy::default();
            policy.presence.margin = 0.15;
            policy.choice.min_confidence = Probability::new(0.6).unwrap();
            policy.choice.min_gap = 0.1;
            let mut session = Session::new(planner, policy).unwrap();
            let mut round = session.first().unwrap();
            let mut rounds = Vec::new();
            let outcome = loop {
                let mut evaluations = Vec::new();
                let mut decisions = Vec::new();
                for bracket in round.brackets() {
                    let evaluation = backend
                        .evaluate(bracket.request(), bracket.counts(), &budget, true, deadline)
                        .await
                        .unwrap();
                    assert!(!evaluation.cached);
                    assert_eq!(
                        evaluation.response.usage.input_tokens,
                        bracket.counts().input_tokens as u64
                    );
                    let decision = session
                        .assess_response(bracket, &evaluation.response)
                        .unwrap();
                    eprintln!(
                        "selection {name} round {} bracket {}: {:?} {:?}",
                        round.number(),
                        bracket.index(),
                        decision.status(),
                        decision.reason()
                    );
                    // Apply every uncertainty action to the actual positive response,
                    // retaining the raw distribution instead of simulating a backend.
                    if decision.status() == Status::Selected {
                        for action in [
                            UncertainAction::Keep,
                            UncertainAction::Drop,
                            UncertainAction::Stderr,
                            UncertainAction::Fail,
                        ] {
                            let mut gated = policy;
                            gated.choice.min_confidence = Probability::new(1.0).unwrap();
                            gated.on_uncertain = action;
                            let uncertain = gated
                                .assess_response(bracket, &evaluation.response)
                                .unwrap();
                            assert_eq!(uncertain.status(), Status::Uncertain);
                            match action {
                                UncertainAction::Keep => assert_eq!(
                                    uncertain.destination().unwrap(),
                                    Destination::Stdout
                                ),
                                UncertainAction::Drop => {
                                    assert_eq!(uncertain.destination().unwrap(), Destination::Omit)
                                }
                                UncertainAction::Stderr => assert_eq!(
                                    uncertain.destination().unwrap(),
                                    Destination::Stderr
                                ),
                                UncertainAction::Fail => {
                                    assert_eq!(uncertain.destination().unwrap_err().exit_code(), 3)
                                }
                            }
                        }
                    }
                    decisions.push(decision);
                    evaluations.push(evaluation);
                }
                rounds.push(json!({"round":round,"evaluations":evaluations,"decisions":decisions}));
                match session.advance(&round, &decisions).unwrap() {
                    Step::Next { round: next } => round = next,
                    Step::Final { outcome } => break outcome,
                }
            };
            if let Some(id) = expected {
                assert_eq!(outcome.status, Status::Selected);
                assert_eq!(outcome.exit_code, 0);
                assert_eq!(outcome.tournament.selected[0].id.to_string(), id);
            } else {
                assert_eq!(outcome.status, Status::Absent);
                assert_eq!(outcome.exit_code, 1);
                assert!(outcome.tournament.selected.is_empty());
            }
            captures.push(json!({"name":name,"catalog":catalog,"rounds":rounds,"outcome":outcome}));
        }
        let usage = budget.usage().unwrap();
        assert_eq!(
            usage.spent_input,
            usage.reported_input + usage.unknown_input
        );
        assert_eq!(usage.cache_hits, 0);
        if let Ok(path) = std::env::var("LOUPE_SELECTION_EVIDENCE") {
            let evidence = json!({"schema_version":1,"captured_at":chrono::Utc::now().to_rfc3339(),"endpoint":format!("{}{}",profile.base_url,profile.path),"encoder":encoder.identity,"discovery":discovery,"captures":captures,"token_usage":usage,"proof_boundary":"Actual joint presence/Choice responses over synthetic positive and absent documents with exact request counts and local uncertainty actions. Core policy tests, not complete selection CLI, held-out quality or authoritative cache identity qualification."});
            std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        }
        eprintln!(
            "selection: submitted={} reported={} unknown={} calls={}",
            usage.spent_input, usage.reported_input, usage.unknown_input, usage.http_attempts
        );
    }
}
