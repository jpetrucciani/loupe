//! Pair-local joint decisions and streaming greedy cardinality assignment.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Limits;
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::pack;
use crate::policy::{Assessment, Policy, Probability, Selected, UncertainAction};
use crate::wire::{Answer, Question, Request, Response};

pub const GATE_ID: &str = "same_entity";
pub const SCORE_ID: &str = "similarity";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum Cardinality {
    #[default]
    #[serde(rename = "1:1")]
    OneToOne,
    #[serde(rename = "1:n")]
    OneToMany,
    #[serde(rename = "n:m")]
    ManyToMany,
}
impl FromStr for Cardinality {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "1:1" => Ok(Self::OneToOne),
            "1:n" => Ok(Self::OneToMany),
            "n:m" => Ok(Self::ManyToMany),
            _ => Err(usage("cardinality must be 1:1, 1:n or n:m")),
        }
    }
}
#[derive(Clone)]
pub struct Template {
    model: String,
    questions: IndexMap<String, Question>,
    policies: IndexMap<String, Policy>,
    ensemble: Option<crate::ensemble::Group>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    left_index: u64,
    right_index: u64,
    request: Request,
    counts: Counts,
    #[serde(skip)]
    binding: String,
}
impl Plan {
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn right_index(&self) -> u64 {
        self.right_index
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    left_index: u64,
    right_index: u64,
    pub gate: Assessment,
    pub score: Assessment,
    pub ranking_score: f64,
    pub probabilities: BTreeMap<String, f64>,
    pub companions: IndexMap<String, Assessment>,
    /// External selections; gate/score assessments and probabilities remain raw.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub overrides: IndexMap<String, Selected>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub ensembles: IndexMap<String, Value>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub derived: IndexMap<String, Assessment>,
    pub uncertain: bool,
    pub eligible: bool,
    #[serde(skip)]
    binding: String,
}
impl Decision {
    pub fn left_index(&self) -> u64 {
        self.left_index
    }
    pub fn right_index(&self) -> u64 {
        self.right_index
    }
    pub fn assessment(&self, field: &str) -> Result<&Assessment> {
        if let Some(assessment) = self.derived.get(field) {
            return Ok(assessment);
        }
        match field {
            GATE_ID => Ok(&self.gate),
            SCORE_ID => Ok(&self.score),
            _ => self
                .companions
                .get(field)
                .ok_or_else(|| usage("pair field is missing")),
        }
    }
    pub fn validate_plan(&self, plan: &Plan) -> Result<()> {
        if self.left_index != plan.left_index
            || self.right_index != plan.right_index
            || self.binding != plan.binding
        {
            return Err(usage("pair decision belongs to a different request"));
        }
        Ok(())
    }
}
impl Template {
    pub fn new(
        model: String,
        questions: IndexMap<String, Question>,
        policies: IndexMap<String, Policy>,
    ) -> Result<Self> {
        let Some(Question::Score { criteria, .. }) = questions.get(SCORE_ID) else {
            return Err(usage("pair rubric requires a similarity Score"));
        };
        if criteria.len() < 2 {
            return Err(usage("pair similarity needs at least two ordered levels"));
        }
        if !matches!(questions.get(GATE_ID), Some(Question::Noul { .. })) {
            return Err(usage("pair rubric requires a same_entity Noul"));
        }
        if questions
            .iter()
            .any(|(id, q)| id != SCORE_ID && !matches!(q, Question::Noul { .. }))
        {
            return Err(usage("pair companion fields must be Nouls"));
        }
        if questions.keys().ne(policies.keys()) {
            return Err(usage("pair policies must match complete question order"));
        }
        for question in questions.values() {
            question.validate()?;
        }
        for policy in policies.values() {
            policy.validate()?;
        }
        Request {
            model: model.clone(),
            state: Value::Null,
            questions: questions.clone(),
        }
        .validate()?;
        Ok(Self {
            model,
            questions,
            policies,
            ensemble: None,
        })
    }
    pub fn questions(&self) -> &IndexMap<String, Question> {
        &self.questions
    }
    pub fn policies(&self) -> &IndexMap<String, Policy> {
        &self.policies
    }
    pub fn ensemble(&self) -> Option<&crate::ensemble::Group> {
        self.ensemble.as_ref()
    }
    pub fn with_ensemble(
        mut self,
        set: crate::ensemble::Set,
        min_agreement: Probability,
    ) -> Result<Self> {
        if set.is_empty() {
            return Err(usage("join ensemble requires named pair variants"));
        }
        set.append(&mut self.questions)?;
        let set = set.materialize(&self.questions)?;
        for id in self.questions.keys() {
            if let Some(base) = set.base_for(id) {
                let policy = *self
                    .policies
                    .get(base)
                    .ok_or_else(|| usage("pair variant policy is missing"))?;
                self.policies.insert(id.clone(), policy);
            }
        }
        self.ensemble = Some(crate::ensemble::Group { set, min_agreement });
        Ok(self)
    }
    pub fn add_context(&mut self, context: Value) -> Result<()> {
        crate::presets::add_context(&mut self.questions, context);
        if let Some(group) = &mut self.ensemble {
            group.set = group.set.clone().materialize(&self.questions)?;
        }
        Ok(())
    }
    fn binding(&self, request: &Request) -> Result<String> {
        let raw = binding(request)?;
        if let Some(group) = &self.ensemble {
            return Ok(blake3::hash(
                &serde_json::to_vec(&json!({"request":raw,"min_agreement":group.min_agreement}))
                    .map_err(|error| usage(&error.to_string()))?,
            )
            .to_hex()
            .to_string());
        }
        Ok(raw)
    }
    pub fn plan(
        &self,
        left: &Unit,
        right: &Unit,
        encoder: Option<&ClefEncoder>,
        limits: &Limits,
    ) -> Result<Plan> {
        if left.index == 0 || right.index == 0 {
            return Err(usage("pair source indices start at one"));
        }
        let request = Request {
            model: self.model.clone(),
            state: json!({"left":{"id":format!("L{}",left.index),"record":left.state},"right":{"id":format!("R{}",right.index),"record":right.state}}),
            questions: self.questions.clone(),
        };
        limits.validate_request(&request)?;
        let counts = pack::count(encoder, &request)?;
        counts.preflight(limits)?;
        let binding = self.binding(&request)?;
        Ok(Plan {
            left_index: left.index,
            right_index: right.index,
            request,
            counts,
            binding,
        })
    }
    pub fn assess(
        &self,
        plan: &Plan,
        response: &Response,
        action: UncertainAction,
    ) -> Result<Decision> {
        self.assess_with_overrides(plan, response, action, &IndexMap::new())
    }

    pub fn assess_with_overrides(
        &self,
        plan: &Plan,
        response: &Response,
        action: UncertainAction,
        overrides: &IndexMap<String, Selected>,
    ) -> Result<Decision> {
        let expected = Request {
            model: self.model.clone(),
            state: plan.request.state.clone(),
            questions: self.questions.clone(),
        };
        if self.binding(plan.request())? != plan.binding || self.binding(&expected)? != plan.binding
        {
            return Err(usage("pair request or rubric binding changed"));
        }
        let answers = response.validate(plan.request())?;
        let Some(gate_answer @ Answer::Noul { .. }) = answers.get(GATE_ID) else {
            return Err(protocol("missing pair gate"));
        };
        let Some(score_answer @ Answer::Score { probabilities, .. }) = answers.get(SCORE_ID) else {
            return Err(protocol("missing pair score"));
        };
        let gate = gate_answer.assessment(
            self.policies
                .get(GATE_ID)
                .ok_or_else(|| usage("pair gate policy missing"))?,
        )?;
        let score = score_answer.assessment(
            self.policies
                .get(SCORE_ID)
                .ok_or_else(|| usage("pair score policy missing"))?,
        )?;
        let Some(Question::Score { criteria, .. }) = plan.request.questions.get(SCORE_ID) else {
            return Err(protocol("missing score rubric"));
        };
        let mut mass = 0.0;
        let mut weighted = 0.0;
        for (level, probability) in probabilities {
            let level = level
                .parse::<usize>()
                .map_err(|_| protocol("invalid score level"))?;
            mass += probability;
            weighted += level as f64 * probability;
        }
        if mass <= 0.0 {
            return Err(protocol("pair score has zero probability mass"));
        }
        let mut ensembles = IndexMap::new();
        let mut derived = IndexMap::new();
        if let Some(group) = &self.ensemble {
            for (id, policy) in &self.policies {
                if group.set.has_primary(id) {
                    let value =
                        crate::ensemble::field(plan.request(), response, id, *policy, Some(group))?;
                    if let Some(report) = value.ensemble {
                        ensembles.insert(id.clone(), report);
                    }
                    derived.insert(id.clone(), value.assessment);
                }
            }
        }
        let mut effective_gate = derived.get(GATE_ID).unwrap_or(&gate).clone();
        let mut effective_score = derived.get(SCORE_ID).unwrap_or(&score).clone();
        let mean_ranking = if let Some(value) = derived.get(SCORE_ID) {
            let Selected::Score(value) = &value.selected else {
                return Err(protocol("derived similarity has the wrong type"));
            };
            value / criteria.len().saturating_sub(1) as f64
        } else {
            weighted / mass / criteria.len().saturating_sub(1) as f64
        };
        for (id, selected) in overrides {
            let assessment = match (id.as_str(), selected) {
                (GATE_ID, Selected::Boolean(_)) => &mut effective_gate,
                (SCORE_ID, Selected::Score(value))
                    if value.is_finite()
                        && (0.0..=criteria.len().saturating_sub(1) as f64).contains(value) =>
                {
                    &mut effective_score
                }
                _ => return Err(usage("invalid pair override field, type or level")),
            };
            if !assessment.uncertain {
                return Err(usage("pair overrides require an uncertain model field"));
            }
            assessment.selected = selected.clone();
            assessment.uncertain = false;
        }
        if overrides.contains_key(SCORE_ID)
            && (effective_gate.uncertain || effective_gate.selected != Selected::Boolean(true))
        {
            return Err(usage(
                "pair score override requires a resolved positive gate",
            ));
        }
        let ranking_score = match overrides.get(SCORE_ID) {
            Some(Selected::Score(value)) => value / criteria.len().saturating_sub(1) as f64,
            _ => mean_ranking.clamp(0.0, 1.0),
        };
        Probability::new(ranking_score)?;
        let certain_no = !effective_gate.uncertain
            && matches!(effective_gate.selected, Selected::Boolean(false));
        let uncertain = effective_gate.uncertain || !certain_no && effective_score.uncertain;
        if uncertain && action == UncertainAction::Fail {
            return Err(Error::new(
                ErrorKind::Uncertain,
                "pair decision is uncertain",
            ));
        }
        let eligible = !certain_no && (!uncertain || action == UncertainAction::Keep);
        let mut companions = IndexMap::new();
        for (id, answer) in &answers {
            if id == GATE_ID
                || id == SCORE_ID
                || self
                    .ensemble
                    .as_ref()
                    .is_some_and(|group| group.set.contains(id))
            {
                continue;
            }
            companions.insert(
                id.clone(),
                answer.assessment(
                    self.policies
                        .get(id)
                        .ok_or_else(|| usage("pair companion policy missing"))?,
                )?,
            );
        }
        Ok(Decision {
            left_index: plan.left_index,
            right_index: plan.right_index,
            gate,
            score,
            ranking_score,
            probabilities: probabilities.clone(),
            companions,
            overrides: overrides.clone(),
            ensembles,
            derived,
            uncertain,
            eligible,
            binding: plan.binding.clone(),
        })
    }
}

pub struct Assignment {
    cardinality: Cardinality,
    known: BTreeSet<u64>,
    used: BTreeSet<u64>,
    last_left: u64,
}
impl Assignment {
    pub fn new(cardinality: Cardinality, right: impl Iterator<Item = u64>) -> Result<Self> {
        let mut known = BTreeSet::new();
        for index in right {
            if index == 0 || !known.insert(index) {
                return Err(usage("right indices must be distinct positive integers"));
            }
        }
        Ok(Self {
            cardinality,
            known,
            used: BTreeSet::new(),
            last_left: 0,
        })
    }
    pub fn excluded(&self) -> Option<&BTreeSet<u64>> {
        (self.cardinality != Cardinality::ManyToMany).then_some(&self.used)
    }
    pub fn unmatched(&self) -> impl Iterator<Item = u64> + '_ {
        self.known.difference(&self.used).copied()
    }
    /// Called once per arriving left record, including records with no matches.
    pub fn assign(&mut self, left: u64, decisions: &[Decision]) -> Result<Vec<usize>> {
        if left <= self.last_left {
            return Err(usage(
                "left assignment must follow strictly increasing input indices",
            ));
        }
        let mut seen = BTreeSet::new();
        for decision in decisions {
            if decision.left_index != left
                || !self.known.contains(&decision.right_index)
                || !seen.insert(decision.right_index)
            {
                return Err(usage(
                    "pair decisions contain foreign or duplicate source IDs",
                ));
            }
        }
        let mut ranked = decisions
            .iter()
            .enumerate()
            .filter(|(_, decision)| {
                decision.eligible
                    && (self.cardinality == Cardinality::ManyToMany
                        || !self.used.contains(&decision.right_index))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(_, left), (_, right)| {
            right
                .ranking_score
                .total_cmp(&left.ranking_score)
                .then_with(|| left.right_index.cmp(&right.right_index))
        });
        if self.cardinality == Cardinality::OneToOne {
            ranked.truncate(1);
        }
        let selected = ranked
            .into_iter()
            .map(|(index, decision)| {
                self.used.insert(decision.right_index);
                index
            })
            .collect();
        self.last_left = left;
        Ok(selected)
    }
}
fn binding(request: &Request) -> Result<String> {
    let bytes = serde_json::to_vec(request)
        .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
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
    use crate::config::Limits;
    use crate::input::Unit;
    use crate::matching::{Assignment, Cardinality, Decision, GATE_ID, SCORE_ID, Template};
    use crate::policy::{Policy, Probability, Selected, UncertainAction};
    use crate::records::{Format, Options, Source};
    use crate::wire::{Question, Response};
    use indexmap::IndexMap;
    use serde_json::{Value, json};
    use std::io::Cursor;

    fn unit(index: u64, value: Value) -> Unit {
        let mut unit = Source::new(
            Cursor::new(format!("{value}\n")),
            Options::new(Format::Jsonl),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap();
        unit.index = index;
        unit
    }
    fn fixture(capture: usize) -> (Template, Unit, Unit, Response) {
        let evidence: Value =
            serde_json::from_str(include_str!("../testdata/captured-blocking.json")).unwrap();
        let source = evidence["captures"][capture]["input"].as_str().unwrap();
        let (left, right) = source
            .strip_prefix("LEFT record:\n")
            .unwrap()
            .split_once("\nRIGHT record:\n")
            .unwrap();
        let questions: IndexMap<String, Question> =
            serde_json::from_value(evidence["question_source"]["questions"].clone()).unwrap();
        let policies = questions
            .keys()
            .map(|id| (id.clone(), Policy::default()))
            .collect();
        (
            Template::new("clef-flash".into(), questions, policies).unwrap(),
            unit(1, serde_json::from_str(left).unwrap()),
            unit(1, serde_json::from_str(right).unwrap()),
            serde_json::from_value(evidence["captures"][capture]["response"].clone()).unwrap(),
        )
    }
    fn positive() -> Decision {
        let (template, left, right, response) = fixture(3);
        let plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        template
            .assess(&plan, &response, UncertainAction::Drop)
            .unwrap()
    }

    #[test]
    fn arithmetic_pair_ensembles_keep_raw_diagnostics_and_bind_mean_gates_ranking_and_overrides() {
        // Pure arithmetic over codec shapes, no inference replacement.
        let (base, left, right, mut raw) = fixture(3);
        let set = crate::ensemble::Set::from_document(
            &json!({"variants":[{"id":"alt","field":GATE_ID},{"id":"alt","field":SCORE_ID}]}),
            base.questions(),
        )
        .unwrap();
        let mut template = base
            .with_ensemble(set, Probability::new(1.0).unwrap())
            .unwrap();
        let plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        raw.answers[GATE_ID] = json!({"type":"noul","noul":0.9});
        raw.answers.insert(
            format!("ensemble:{GATE_ID}/alt"),
            json!({"type":"noul","noul":0.3}),
        );
        let legend: IndexMap<_, _> = template.questions()[SCORE_ID]
            .options()
            .into_iter()
            .collect();
        raw.answers[SCORE_ID] = json!({"type":"score","score":2.0,"confidence":1.0,"legend":legend,"probabilities":{"0":0.0,"1":0.0,"2":1.0}});
        raw.answers.insert(format!("ensemble:{SCORE_ID}/alt"),json!({"type":"score","score":0.4,"confidence":0.6,"legend":legend,"probabilities":{"0":0.6,"1":0.4,"2":0.0}}));
        let original = serde_json::to_value(&raw).unwrap();
        let decision = template.assess(&plan, &raw, UncertainAction::Drop).unwrap();
        assert!(decision.uncertain && !decision.eligible);
        assert_eq!(decision.score.selected, Selected::Score(2.0));
        assert!((decision.ranking_score - 0.6).abs() < 1e-12);
        assert_eq!(decision.ensembles[GATE_ID]["agreement"], 0.5);
        assert_eq!(
            decision.ensembles[SCORE_ID]["members"][0]["answer"],
            raw.answers[SCORE_ID]
        );
        assert_eq!(decision.probabilities["2"], 1.0);
        let external = template
            .assess_with_overrides(
                &plan,
                &raw,
                UncertainAction::Drop,
                &IndexMap::from([
                    (GATE_ID.into(), Selected::Boolean(true)),
                    (SCORE_ID.into(), Selected::Score(0.5)),
                ]),
            )
            .unwrap();
        assert!(external.eligible && !external.uncertain);
        assert_eq!(external.ranking_score, 0.25);
        template.ensemble.as_mut().unwrap().min_agreement = Probability::new(0.5).unwrap();
        assert!(template.assess(&plan, &raw, UncertainAction::Drop).is_err());
        let relaxed_plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        let relaxed = template
            .assess(&relaxed_plan, &raw, UncertainAction::Drop)
            .unwrap();
        assert!(relaxed.eligible && !relaxed.uncertain);
        assert_eq!(serde_json::to_value(&raw).unwrap(), original);
    }
    fn pair(left: u64, right: u64, score: f64) -> Decision {
        let mut decision = positive();
        decision.left_index = left;
        decision.right_index = right;
        decision.ranking_score = score;
        decision
    }

    #[test]
    fn actual_saved_distributions_supply_gate_companions_and_normalized_expected_score() {
        for capture in 3..8 {
            let (template, left, right, response) = fixture(capture);
            let plan = template
                .plan(&left, &right, None, &Limits::default())
                .unwrap();
            let decision = template
                .assess(&plan, &response, UncertainAction::Drop)
                .unwrap();
            decision.validate_plan(&plan).unwrap();
            let probabilities = &response.answers[SCORE_ID]["probabilities"];
            let p0 = probabilities["0"].as_f64().unwrap();
            let p1 = probabilities["1"].as_f64().unwrap();
            let p2 = probabilities["2"].as_f64().unwrap();
            assert!(
                (decision.ranking_score - (p1 + 2.0 * p2) / (p0 + p1 + p2) / 2.0).abs() < 1e-12
            );
            let p = response.answers[GATE_ID]["noul"].as_f64().unwrap();
            assert_eq!(decision.gate.confidence.get(), p.max(1.0 - p));
            assert_eq!(decision.eligible, p >= 0.5);
            assert_eq!(decision.companions.len(), 2);
            assert_eq!(plan.request.state["left"]["id"], json!("L1"));
            assert!(
                decision
                    .validate_plan(
                        &template
                            .plan(
                                &unit(2, left.state.clone()),
                                &right,
                                None,
                                &Limits::default()
                            )
                            .unwrap()
                    )
                    .is_err()
            );
        }
    }
    #[test]
    fn gate_boundaries_uncertainty_actions_and_certain_no_are_explicit() {
        let (mut template, left, right, response) = fixture(3);
        let plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        let p = response.answers[GATE_ID]["noul"].as_f64().unwrap();
        template.policies.get_mut(GATE_ID).unwrap().threshold = Probability::new(p).unwrap();
        let tied = template
            .assess(&plan, &response, UncertainAction::Drop)
            .unwrap();
        assert_eq!(tied.gate.selected, Selected::Boolean(true));
        assert!(tied.eligible);
        template.policies.get_mut(GATE_ID).unwrap().margin = 0.01;
        for action in [
            UncertainAction::Drop,
            UncertainAction::Stderr,
            UncertainAction::Keep,
        ] {
            let decision = template.assess(&plan, &response, action).unwrap();
            assert!(decision.uncertain);
            assert_eq!(decision.eligible, action == UncertainAction::Keep);
        }
        assert_eq!(
            template
                .assess(&plan, &response, UncertainAction::Fail)
                .unwrap_err()
                .exit_code(),
            3
        );
        let (mut template, left, right, response) = fixture(4);
        template.policies.get_mut(SCORE_ID).unwrap().min_confidence =
            Probability::new(1.0).unwrap();
        let decision = template
            .assess(
                &template
                    .plan(&left, &right, None, &Limits::default())
                    .unwrap(),
                &response,
                UncertainAction::Fail,
            )
            .unwrap();
        assert!(!decision.eligible);
        assert!(!decision.uncertain);
        assert!(decision.score.uncertain);
    }
    #[test]
    fn external_pair_decisions_preserve_distributions_and_drive_gate_and_rank() {
        let (mut template, left, right, response) = fixture(3);
        let original = serde_json::to_value(&response).unwrap();
        template.policies.get_mut(GATE_ID).unwrap().margin = 1.0;
        template.policies.get_mut(SCORE_ID).unwrap().min_confidence =
            Probability::new(1.0).unwrap();
        let plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        let mut overrides = IndexMap::from([(GATE_ID.into(), Selected::Boolean(false))]);
        let rejected = template
            .assess_with_overrides(&plan, &response, UncertainAction::Fail, &overrides)
            .unwrap();
        assert!(!rejected.eligible);
        assert!(!rejected.uncertain);
        assert!(rejected.gate.uncertain && rejected.score.uncertain);
        overrides.insert(GATE_ID.into(), Selected::Boolean(true));
        let pending = template
            .assess_with_overrides(&plan, &response, UncertainAction::Drop, &overrides)
            .unwrap();
        assert!(pending.uncertain);
        assert!(!pending.eligible);
        overrides.insert(SCORE_ID.into(), Selected::Score(1.5));
        let resolved = template
            .assess_with_overrides(&plan, &response, UncertainAction::Fail, &overrides)
            .unwrap();
        assert!(resolved.eligible);
        assert!(!resolved.uncertain);
        assert_eq!(resolved.ranking_score, 0.75);
        assert_eq!(resolved.probabilities, rejected.probabilities);
        assert_eq!(resolved.overrides, overrides);
        assert_eq!(serde_json::to_value(&response).unwrap(), original);
        resolved.validate_plan(&plan).unwrap();

        for invalid in [
            IndexMap::from([(GATE_ID.into(), Selected::Score(1.0))]),
            IndexMap::from([("name_agrees".into(), Selected::Boolean(true))]),
            IndexMap::from([(SCORE_ID.into(), Selected::Score(1.5))]),
            IndexMap::from([
                (GATE_ID.into(), Selected::Boolean(false)),
                (SCORE_ID.into(), Selected::Score(1.0)),
            ]),
            IndexMap::from([
                (GATE_ID.into(), Selected::Boolean(true)),
                (SCORE_ID.into(), Selected::Score(3.0)),
            ]),
            IndexMap::from([
                (GATE_ID.into(), Selected::Boolean(true)),
                (SCORE_ID.into(), Selected::Score(f64::NAN)),
            ]),
        ] {
            assert!(
                template
                    .assess_with_overrides(&plan, &response, UncertainAction::Drop, &invalid)
                    .is_err()
            );
        }
        template.policies.get_mut(GATE_ID).unwrap().margin = 0.0;
        assert!(
            template
                .assess_with_overrides(&plan, &response, UncertainAction::Drop, &overrides)
                .is_err()
        );
    }

    #[test]
    fn pair_schema_request_binding_and_budget_limits_fail_before_inference() {
        let (template, left, right, mut response) = fixture(3);
        let plan = template
            .plan(&left, &right, None, &Limits::default())
            .unwrap();
        response.answers.shift_remove("name_agrees");
        assert!(
            template
                .assess(&plan, &response, UncertainAction::Drop)
                .is_err()
        );
        let limits = Limits {
            max_questions: 1,
            ..Limits::default()
        };
        assert!(template.plan(&left, &right, None, &limits).is_err());
        let mut wrong = template.clone();
        wrong.model = "other-model".into();
        assert!(
            wrong
                .assess(&plan, &fixture(3).3, UncertainAction::Drop)
                .is_err()
        );
        let mut questions = template.questions.clone();
        questions.insert(
            SCORE_ID.into(),
            Question::Score {
                instructions: None,
                criteria: vec![json!("only")],
            },
        );
        assert!(Template::new("m".into(), questions, template.policies.clone()).is_err());
    }
    #[test]
    fn per_arriving_left_greedy_assignment_differs_from_global_score_order() {
        let mut assignment = Assignment::new(Cardinality::OneToOne, [1, 2].into_iter()).unwrap();
        assert_eq!(
            assignment
                .assign(1, &[pair(1, 1, 0.9), pair(1, 2, 0.8)])
                .unwrap(),
            vec![0]
        );
        assert_eq!(
            assignment
                .assign(2, &[pair(2, 1, 0.99), pair(2, 2, 0.1)])
                .unwrap(),
            vec![1]
        );
        assert!(assignment.unmatched().next().is_none());
        let mut reversed = Assignment::new(Cardinality::OneToOne, [1, 2].into_iter()).unwrap();
        assert_eq!(
            reversed
                .assign(1, &[pair(1, 1, 0.99), pair(1, 2, 0.1)])
                .unwrap(),
            vec![0]
        );
        assert_eq!(
            reversed
                .assign(2, &[pair(2, 1, 0.9), pair(2, 2, 0.8)])
                .unwrap(),
            vec![1]
        );
        assert_eq!(
            assignment.excluded().unwrap(),
            &[1, 2].into_iter().collect()
        );
    }
    #[test]
    fn all_cardinalities_ties_unmatched_and_invalid_sources_obey_constraints() {
        for cardinality in [
            Cardinality::OneToOne,
            Cardinality::OneToMany,
            Cardinality::ManyToMany,
        ] {
            let mut assignment = Assignment::new(cardinality, [1, 2, 3].into_iter()).unwrap();
            let first = assignment
                .assign(1, &[pair(1, 2, 0.7), pair(1, 1, 0.7)])
                .unwrap();
            assert_eq!(
                first,
                if cardinality == Cardinality::OneToOne {
                    vec![1]
                } else {
                    vec![1, 0]
                }
            );
            let second = assignment
                .assign(2, &[pair(2, 1, 0.9), pair(2, 2, 0.8)])
                .unwrap();
            assert_eq!(
                second,
                match cardinality {
                    Cardinality::OneToOne => vec![1],
                    Cardinality::OneToMany => vec![],
                    Cardinality::ManyToMany => vec![0, 1],
                }
            );
            assert_eq!(assignment.unmatched().collect::<Vec<_>>(), vec![3]);
            assert!(assignment.assign(2, &[]).is_err());
            assert!(assignment.assign(3, &[pair(3, 9, 0.9)]).is_err());
            assert!(
                assignment
                    .assign(3, &[pair(3, 3, 0.9), pair(3, 3, 0.9)])
                    .is_err()
            );
            assert!(assignment.assign(3, &[]).unwrap().is_empty());
        }
    }
}
