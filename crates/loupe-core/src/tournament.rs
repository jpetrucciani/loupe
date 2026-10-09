use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::candidates::{Candidate, Id};
use crate::config::Limits;
use crate::encoder::{self, ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::policy::Probability;
use crate::wire::{Question, Request};

pub const SELECTION_ID: &str = "selection";
pub const PRESENCE_ID: &str = "selection-present";
pub const NONE: &str = "none";
pub const STATE_SEPARATOR: &str = "<|file_sep|>";

/// Original containing-line evidence, shared by CLI, MCP and evaluation.
pub fn descriptions(
    document: &crate::candidates::Document<'_>,
    catalog: &[Candidate<'_>],
    parsed: bool,
    context_lines: usize,
) -> Result<BTreeMap<Id, Value>> {
    if !parsed && context_lines == 0 {
        return Ok(BTreeMap::new());
    }
    let mut options = crate::records::Options::new(crate::records::Format::Lines);
    options.keep_blank = true;
    options.with_context = context_lines;
    let source = catalog
        .first()
        .and_then(|candidate| candidate.location.source.clone());
    let mut reader = crate::records::Source::new(
        std::io::Cursor::new(document.text().as_bytes()),
        options,
        source,
    )?;
    let mut candidates = catalog.iter().peekable();
    let mut result = BTreeMap::new();
    while let Some(line) = reader.next_unit()? {
        if parsed {
            while let Some(candidate) = candidates
                .next_if(|candidate| candidate.location.line_start == line.location.line_start)
            {
                result.insert(candidate.id,json!({"verbatim":candidate.verbatim,"source_line":line.state,"location":candidate.location}));
            }
        } else {
            let id = format!("L{}", line.location.line_start).parse()?;
            if catalog
                .binary_search_by_key(&id, |candidate| candidate.id)
                .is_ok()
            {
                result.insert(id, line.state);
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize)]
pub struct Template {
    pub model: String,
    pub state: Value,
    pub question: Value,
    pub presence_criteria: Option<IndexMap<String, Value>>,
    pub descriptions: BTreeMap<Id, Value>,
    pub companions: IndexMap<String, Question>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<crate::selection_ensemble::Ensemble>,
}

impl Template {
    fn request(&self, candidates: &[&Candidate<'_>]) -> Result<Request> {
        if self.question.is_null() || self.question.as_str() == Some("") {
            return Err(usage("selection requires a nonempty question"));
        }
        if self.companions.contains_key(SELECTION_ID) || self.companions.contains_key(PRESENCE_ID) {
            return Err(usage(
                "companions cannot replace selection or selection-present",
            ));
        }
        let mut criteria = IndexMap::new();
        let mut state = format!("REFERENCE CONTEXT:\n{}", encoder::render(&self.state)?);
        for candidate in candidates {
            let description = self
                .descriptions
                .get(&candidate.id)
                .cloned()
                .unwrap_or_else(|| json!(candidate.verbatim));
            state.push_str(STATE_SEPARATOR);
            state.push_str(&encoder::render(
                &json!({"id":candidate.id,"evidence":description}),
            )?);
            if criteria
                .insert(candidate.id.to_string(), description)
                .is_some()
            {
                return Err(usage("selection candidate IDs must be unique"));
            }
        }
        criteria.insert(
            NONE.to_owned(),
            json!("None of these candidates answers or matches the question."),
        );
        let mut questions = IndexMap::from([
            (
                PRESENCE_ID.to_owned(),
                Question::Noul {
                    instructions: Some(
                        json!({"question":self.question,"task":"Does at least one candidate option in the selection field answer or match this question? Consider only the listed candidates, using state as reference context."}),
                    ),
                    criteria: self.presence_criteria.clone(),
                },
            ),
            (
                SELECTION_ID.to_owned(),
                Question::Choice {
                    instructions: Some(self.question.clone()),
                    criteria,
                },
            ),
        ]);
        questions.extend(self.companions.clone());
        let mut request = Request {
            model: self.model.clone(),
            state: Value::String(state),
            questions,
        };
        if let Some(ensemble) = &self.ensemble {
            ensemble.expand(&mut request)?;
        }
        Ok(request)
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Options {
    pub take: NonZeroUsize,
    pub max_rounds: NonZeroUsize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Bracket {
    index: usize,
    candidate_ids: Vec<Id>,
    request: Request,
    question_order: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ensemble_min_agreement: Option<Probability>,
    counts: Counts,
    #[serde(skip)]
    binding: String,
    #[serde(skip)]
    ensemble: Option<crate::ensemble::Group>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Round {
    number: usize,
    capacity: usize,
    final_round: bool,
    brackets: Vec<Bracket>,
    #[serde(skip)]
    planner: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Scored {
    pub id: Id,
    pub probability: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Ranking {
    ranked: Vec<Scored>,
    probabilities: BTreeMap<String, f64>,
    none_wins: bool,
    tied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_override: Option<String>,
    #[serde(skip)]
    binding: String,
}

impl Bracket {
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn candidate_ids(&self) -> &[Id] {
        &self.candidate_ids
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn ensemble(&self) -> Option<&crate::ensemble::Group> {
        self.ensemble.as_ref()
    }
    /// Reduce a probability distribution; callers validate backend protocol
    /// before passing its values into this arithmetic operation.
    pub fn rank(&self, probabilities: BTreeMap<String, f64>) -> Result<Ranking> {
        let expected = self
            .candidate_ids
            .iter()
            .map(ToString::to_string)
            .chain(std::iter::once(NONE.to_owned()))
            .collect::<BTreeSet<_>>();
        if probabilities.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(protocol(
                "ranking keys differ from this bracket's candidates and none",
            ));
        }
        let mut sum = 0.0;
        for probability in probabilities.values() {
            Probability::new(*probability).map_err(|error| protocol(error.to_string()))?;
            sum += probability;
        }
        if (sum - 1.0).abs() > probabilities.len() as f64 * 0.000051 + 1e-8 {
            return Err(protocol("ranking probabilities do not sum to one"));
        }
        let mut all = probabilities.iter().collect::<Vec<_>>();
        all.sort_by(|left, right| {
            if left.1 == right.1 {
                left.0.cmp(right.0)
            } else {
                right.1.total_cmp(left.1)
            }
        });
        let winner = all.first().ok_or_else(|| protocol("empty ranking"))?;
        let none_wins = winner.0 == NONE;
        let tied = all.get(1).is_some_and(|next| next.1 == winner.1);
        let ranked = all
            .into_iter()
            .filter(|(id, _)| id.as_str() != NONE)
            .map(|(id, probability)| {
                Ok(Scored {
                    id: id.parse().map_err(|_| protocol("invalid candidate ID"))?,
                    probability: *probability,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Ranking {
            ranked,
            probabilities,
            none_wins,
            tied,
            selection_override: None,
            binding: self.binding.clone(),
        })
    }

    pub(crate) fn resolve_choice(&self, ranking: &Ranking, selected: &str) -> Result<Ranking> {
        if ranking.binding != self.binding || !ranking.probabilities.contains_key(selected) {
            return Err(usage("external selection is outside this bracket"));
        }
        let mut resolved = ranking.clone();
        resolved.selection_override = Some(selected.to_owned());
        Ok(resolved)
    }
}

impl Round {
    pub fn number(&self) -> usize {
        self.number
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn is_final(&self) -> bool {
        self.final_round
    }
    pub fn brackets(&self) -> &[Bracket] {
        &self.brackets
    }
}

impl Ranking {
    pub fn selection_override(&self) -> Option<&str> {
        self.selection_override.as_deref()
    }

    fn selected(&self, take: usize) -> Vec<Scored> {
        if let Some(selected) = &self.selection_override {
            return self
                .ranked
                .iter()
                .find(|candidate| candidate.id.to_string() == *selected)
                .cloned()
                .into_iter()
                .collect();
        }
        self.ranked.iter().take(take).cloned().collect()
    }
    pub fn ranked(&self) -> &[Scored] {
        &self.ranked
    }
    pub fn probabilities(&self) -> &BTreeMap<String, f64> {
        &self.probabilities
    }
    pub fn none_wins(&self) -> bool {
        self.selection_override
            .as_deref()
            .map_or(self.none_wins, |selected| selected == NONE)
    }
    pub fn tied(&self) -> bool {
        self.tied
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Final {
    pub round: usize,
    pub original_candidates: usize,
    pub surviving_candidates: Vec<Id>,
    pub selected: Vec<Scored>,
    pub probability_scope: &'static str,
    pub ranking: Option<Ranking>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "stage", rename_all = "lowercase")]
pub enum Reduction {
    Next { round: Round },
    Final { result: Final },
}

pub struct Planner<'a> {
    template: Template,
    catalog: &'a [Candidate<'a>],
    lookup: BTreeMap<Id, usize>,
    encoder: Option<&'a ClefEncoder>,
    limits: Limits,
    options: Options,
    capacity: usize,
    identity: String,
}

impl<'a> Planner<'a> {
    pub fn new(
        template: Template,
        catalog: &'a [Candidate<'a>],
        encoder: Option<&'a ClefEncoder>,
        limits: &Limits,
        options: Options,
    ) -> Result<Self> {
        let mut lookup = BTreeMap::new();
        for (index, candidate) in catalog.iter().enumerate() {
            if lookup.insert(candidate.id, index).is_some() {
                return Err(usage("selection candidate IDs must be unique"));
            }
        }
        if template
            .descriptions
            .keys()
            .any(|id| !lookup.contains_key(id))
        {
            return Err(usage(
                "candidate descriptions must name IDs in this catalog",
            ));
        }
        let base = template.request(&[])?;
        limits.validate_request(&base)?;
        let base_counts = count(&base, encoder)?;
        base_counts.preflight(limits)?;
        let base_options = base
            .questions
            .values()
            .try_fold(0usize, |total, question| {
                total
                    .checked_add(question.options().len())
                    .ok_or_else(|| input("option count overflow"))
            })?;
        let choice_heads = template
            .ensemble
            .as_ref()
            .map_or(1, |ensemble| ensemble.choice_heads());
        let cap = limits
            .max_options
            .saturating_sub(1)
            .min(limits.max_total_options.saturating_sub(base_options) / choice_heads)
            .min(catalog.len());
        if catalog.len() <= cap {
            let candidates = catalog.iter().collect::<Vec<_>>();
            if count(&template.request(&candidates)?, encoder)?
                .preflight(limits)
                .is_ok()
            {
                let identity = planner_identity(&template, catalog, encoder, limits, options)?;
                return Ok(Self {
                    template,
                    catalog,
                    lookup,
                    encoder,
                    limits: limits.clone(),
                    options,
                    capacity: catalog.len(),
                    identity,
                });
            }
        }
        if encoder.is_some_and(|encoder| !encoder.has_verbatim_boundary(STATE_SEPARATOR)) {
            return Err(usage(
                "candidate-state tournaments require the tokenizer's registered verbatim file separator",
            ));
        }
        let mut expenses = Vec::with_capacity(catalog.len());
        let mut state_expenses = Vec::with_capacity(catalog.len());
        for (index, candidate) in catalog.iter().enumerate() {
            let request = template.request(&[candidate])?;
            limits.validate_request(&request).map_err(|error| {
                input(format!(
                    "candidate {} cannot fit alone: {error}",
                    candidate.id
                ))
            })?;
            let counts = count(&request, encoder)?;
            counts.preflight(limits).map_err(|error| {
                input(format!(
                    "candidate {} cannot fit alone: {error}",
                    candidate.id
                ))
            })?;
            let expense = if encoder.is_some() {
                counts
                    .input_tokens
                    .checked_sub(base_counts.input_tokens)
                    .ok_or_else(|| input("candidate encoder count decreased"))?
            } else {
                // Rank by unrounded bytes; integer token estimates can tie
                // descriptions whose actual serialized sizes differ.
                serde_json::to_vec(&request.questions)
                    .map_err(json_error)?
                    .len()
                    .checked_add(encoder::render(&request.state)?.len())
                    .ok_or_else(|| input("candidate byte count overflow"))?
            };
            expenses.push((index, expense));
            let state_expense = if encoder.is_some() {
                counts.state_tokens
            } else {
                encoder::render(&request.state)?.len()
            };
            state_expenses.push((index, state_expense));
        }
        expenses.sort_unstable_by(|left, right| {
            right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0))
        });
        state_expenses.sort_unstable_by(|left, right| {
            right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0))
        });
        let mut low = 0usize;
        let mut high = cap;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            let candidates = expenses
                .iter()
                .take(middle)
                .filter_map(|(index, _)| catalog.get(*index))
                .collect::<Vec<_>>();
            let request = template.request(&candidates)?;
            limits.validate_request(&request)?;
            let state_candidates = state_expenses
                .iter()
                .take(middle)
                .filter_map(|(index, _)| catalog.get(*index))
                .collect::<Vec<_>>();
            let worst_state = count(&template.request(&state_candidates)?, encoder)?.state_tokens;
            let mut upper = count(&request, encoder)?;
            if encoder.is_none() {
                let question_bytes = serde_json::to_vec(&request.questions)
                    .map_err(json_error)?
                    .len();
                let fixed = upper
                    .question_tokens
                    .checked_sub(question_bytes.div_ceil(3))
                    .ok_or_else(|| input("invalid token estimator"))?;
                upper.input_tokens = question_bytes
                    .checked_add(encoder::render(&request.state)?.len())
                    .and_then(|bytes| bytes.div_ceil(3).checked_add(1))
                    .and_then(|tokens| tokens.checked_add(fixed))
                    .ok_or_else(|| input("token estimate overflow"))?;
            }
            if upper.preflight(limits).is_ok() && worst_state <= limits.max_state_tokens {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        if !catalog.is_empty() && low == 0 {
            return Err(input(
                "no candidate fits with reserved none and presence fields",
            ));
        }
        if catalog.len() > low && low <= options.take.get() {
            return Err(input(format!(
                "tournament cannot reduce {} candidates: capacity={low}, top-k={}; increase the request limits or lower top-k",
                catalog.len(),
                options.take
            )));
        }
        let identity = planner_identity(&template, catalog, encoder, limits, options)?;
        Ok(Self {
            template,
            catalog,
            lookup,
            encoder,
            limits: limits.clone(),
            options,
            capacity: low,
            identity,
        })
    }

    pub fn first(&self) -> Result<Round> {
        self.round(
            1,
            self.catalog.iter().map(|candidate| candidate.id).collect(),
        )
    }

    pub fn reduce(&self, round: &Round, rankings: &[Ranking]) -> Result<Reduction> {
        self.reduce_eligible(round, rankings, &vec![true; rankings.len()])
    }

    pub(crate) fn validate_rankings(&self, round: &Round, rankings: &[Ranking]) -> Result<()> {
        if round.planner != self.identity
            || round.capacity != self.capacity
            || rankings.len() != round.brackets.len()
        {
            return Err(usage(
                "tournament round/rankings belong to a different plan or are incomplete",
            ));
        }
        for (bracket, ranking) in round.brackets.iter().zip(rankings) {
            if bracket.binding != ranking.binding {
                return Err(usage("ranking belongs to a different round or bracket"));
            }
        }
        Ok(())
    }

    pub(crate) fn reduce_eligible(
        &self,
        round: &Round,
        rankings: &[Ranking],
        eligible: &[bool],
    ) -> Result<Reduction> {
        self.validate_rankings(round, rankings)?;
        if eligible.len() != rankings.len() {
            return Err(usage("selection eligibility does not cover every bracket"));
        }
        if round.final_round {
            let ranking = rankings.first().cloned();
            let selected = ranking
                .as_ref()
                .filter(|ranking| !ranking.none_wins() && eligible.first() == Some(&true))
                .map(|ranking| ranking.selected(self.options.take.get()))
                .unwrap_or_default();
            return Ok(Reduction::Final {
                result: Final {
                    round: round.number,
                    original_candidates: self.catalog.len(),
                    surviving_candidates: round
                        .brackets
                        .first()
                        .map(|bracket| bracket.candidate_ids.clone())
                        .unwrap_or_default(),
                    selected,
                    probability_scope: if round.number == 1 {
                        "all-candidates"
                    } else {
                        "final-surviving-bracket"
                    },
                    ranking,
                },
            });
        }
        let previous = round
            .brackets
            .iter()
            .map(|bracket| bracket.candidate_ids.len())
            .sum::<usize>();
        let mut survivors = rankings
            .iter()
            .zip(eligible)
            .filter(|(ranking, eligible)| !ranking.none_wins() && **eligible)
            .map(|(ranking, _)| ranking)
            .flat_map(|ranking| {
                ranking
                    .selected(self.options.take.get())
                    .into_iter()
                    .map(|candidate| candidate.id)
            })
            .collect::<Vec<_>>();
        survivors.sort_unstable_by_key(|id| self.lookup.get(id).copied());
        if survivors.len() >= previous {
            return Err(input(
                "tournament made no progress; no further requests will be sent",
            ));
        }
        if survivors.is_empty() {
            return Ok(Reduction::Final {
                result: Final {
                    round: round.number,
                    original_candidates: self.catalog.len(),
                    surviving_candidates: Vec::new(),
                    selected: Vec::new(),
                    probability_scope: "no-surviving-candidates",
                    ranking: None,
                },
            });
        }
        let next = round
            .number
            .checked_add(1)
            .ok_or_else(|| input("tournament round overflow"))?;
        Ok(Reduction::Next {
            round: self.round(next, survivors)?,
        })
    }

    fn round(&self, number: usize, survivors: Vec<Id>) -> Result<Round> {
        if number > self.options.max_rounds.get() {
            return Err(input(format!(
                "tournament reached max-rounds={} with {} surviving candidates",
                self.options.max_rounds,
                survivors.len()
            )));
        }
        let final_round = survivors.len() <= self.capacity;
        let mut brackets = Vec::new();
        for (index, ids) in survivors.chunks(self.capacity.max(1)).enumerate() {
            let candidates = ids
                .iter()
                .map(|id| {
                    self.lookup
                        .get(id)
                        .and_then(|index| self.catalog.get(*index))
                        .ok_or_else(|| usage("tournament candidate is absent from its catalog"))
                })
                .collect::<Result<Vec<_>>>()?;
            let request = self.template.request(&candidates)?;
            self.limits.validate_request(&request)?;
            let counts = count(&request, self.encoder)?;
            counts.preflight(&self.limits)?;
            let question_order = request.questions.keys().cloned().collect::<Vec<_>>();
            let ensemble = self
                .template
                .ensemble
                .as_ref()
                .map(|ensemble| ensemble.group(&request))
                .transpose()?;
            let ensemble_min_agreement = ensemble.as_ref().map(|ensemble| ensemble.min_agreement);
            let binding = hash(
                &json!({"planner":self.identity,"round":number,"bracket":index,"candidates":ids,"request":request,"question_order":question_order}),
            )?;
            brackets.push(Bracket {
                index,
                candidate_ids: ids.to_vec(),
                request,
                question_order,
                ensemble_min_agreement,
                counts,
                binding,
                ensemble,
            });
        }
        Ok(Round {
            number,
            capacity: self.capacity,
            final_round,
            brackets,
            planner: self.identity.clone(),
        })
    }
}

fn count(request: &Request, encoder: Option<&ClefEncoder>) -> Result<Counts> {
    match encoder {
        Some(encoder) => encoder.encode(request).map(|(_, counts)| counts),
        None => encoder::estimate(request),
    }
}
fn planner_identity(
    template: &Template,
    catalog: &[Candidate<'_>],
    encoder: Option<&ClefEncoder>,
    limits: &Limits,
    options: Options,
) -> Result<String> {
    hash(
        &json!({"template":template,"companion_order":template.companions.keys().collect::<Vec<_>>(),"catalog":catalog,"encoder":encoder.map(|encoder|encoder.identity.as_str()).unwrap_or("estimate-v1"),"limits":limits,"options":options}),
    )
}
fn hash(value: &Value) -> Result<String> {
    Ok(
        blake3::hash(&serde_json::to_vec(value).map_err(json_error)?)
            .to_hex()
            .to_string(),
    )
}
fn json_error(error: serde_json::Error) -> Error {
    input(error.to_string())
}
fn usage(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn input(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}
fn protocol(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Unavailable, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use indexmap::IndexMap;
    use serde_json::{Value, json};

    use crate::candidates::Document;
    use crate::config::{Config, Limits, Settings};
    use crate::encoder::ClefEncoder;
    use crate::tournament::{
        NONE, Options, PRESENCE_ID, Planner, Reduction, SELECTION_ID, Template,
    };
    use crate::wire::Question;

    fn template() -> Template {
        Template {
            model: "clef-flash".to_owned(),
            state: Value::Null,
            question: json!("Which candidate reports checkout is unavailable?"),
            presence_criteria: None,
            descriptions: BTreeMap::new(),
            companions: IndexMap::new(),
            ensemble: None,
        }
    }
    fn options(take: usize, max_rounds: usize) -> Options {
        Options {
            take: NonZeroUsize::new(take).unwrap(),
            max_rounds: NonZeroUsize::new(max_rounds).unwrap(),
        }
    }
    fn encoder() -> ClefEncoder {
        let environment =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = Config::default()
            .resolve(
                None,
                &Settings {
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER").ok(),
                    ..Settings::default()
                },
                &environment,
            )
            .unwrap();
        crate::encoder::load_profile(&profile).unwrap().unwrap()
    }
    fn probabilities(values: &[(&str, f64)]) -> BTreeMap<String, f64> {
        values
            .iter()
            .map(|(id, value)| (id.to_string(), *value))
            .collect()
    }

    #[test]
    fn companion_order_is_explicit_and_binds_requests_with_equal_counts() {
        let document = Document::new("candidate", None);
        let catalog = document.lines(false).unwrap();
        let mut query = template();
        for id in ["z_signal", "a_signal"] {
            query.companions.insert(
                id.into(),
                Question::Noul {
                    instructions: Some(json!(id)),
                    criteria: None,
                },
            );
        }
        let first = Planner::new(
            query.clone(),
            &catalog,
            None,
            &Limits::default(),
            options(1, 64),
        )
        .unwrap();
        let first_round = first.first().unwrap();
        query.companions.swap_indices(0, 1);
        let second =
            Planner::new(query, &catalog, None, &Limits::default(), options(1, 64)).unwrap();
        let second_round = second.first().unwrap();
        let a = &first_round.brackets()[0];
        let b = &second_round.brackets()[0];
        assert_eq!(a.counts().input_tokens, b.counts().input_tokens);
        assert_eq!(
            serde_json::to_value(&a.request().questions).unwrap(),
            serde_json::to_value(&b.request().questions).unwrap()
        );
        assert_ne!(a.question_order, b.question_order);
        assert_eq!(
            a.question_order,
            [PRESENCE_ID, SELECTION_ID, "z_signal", "a_signal"]
        );
        let ranking = a.rank(probabilities(&[("L1", 0.8), (NONE, 0.2)])).unwrap();
        assert!(second.reduce(&second_round, &[ranking]).is_err());
    }

    #[test]
    fn reserved_none_presence_and_companions_count_toward_every_option_limit() {
        let text = (0..300)
            .map(|index| format!("candidate {index}\n"))
            .collect::<String>();
        let document = Document::new(&text, None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_length: 100_000,
            max_options: 255,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(1, 64)).unwrap();
        let round = planner.first().unwrap();
        assert_eq!(round.capacity(), 254);
        assert_eq!(round.brackets().len(), 2);
        for bracket in round.brackets() {
            assert_eq!(bracket.request().questions.len(), 2);
            assert_eq!(bracket.request().questions[PRESENCE_ID].options().len(), 2);
            assert_eq!(
                bracket.request().questions[SELECTION_ID].options().len(),
                bracket.candidate_ids().len() + 1
            );
            limits.validate_request(bracket.request()).unwrap();
            bracket.counts().preflight(&limits).unwrap();
            assert!(!bracket.counts().exact);
        }
        let mut template = template();
        template.companions.insert(
            "signal".to_owned(),
            Question::Noul {
                instructions: Some(json!("Does this mention payments?")),
                criteria: None,
            },
        );
        let limits = Limits {
            max_total_options: 8,
            ..limits
        };
        let planner = Planner::new(template, &catalog, None, &limits, options(1, 64)).unwrap();
        assert_eq!(planner.first().unwrap().capacity(), 3);
        let limits = Limits {
            max_questions: 1,
            ..limits
        };
        assert_eq!(
            Planner::new(
                crate::tournament::tests::template(),
                &catalog,
                None,
                &limits,
                options(1, 64)
            )
            .err()
            .unwrap()
            .exit_code(),
            65
        );
    }

    #[test]
    fn exact_uniform_capacity_fits_every_subset_and_uses_the_actual_reference_encoder() {
        let encoder = encoder();
        let text = [
            "短い",
            "a modest sentence",
            "many repeated words ".repeat(10).as_str(),
            "other repeated words ".repeat(17).as_str(),
            "東京\"escaped\"",
            "small",
        ]
        .join("\n");
        let document = Document::new(&text, None);
        let catalog = document.lines(false).unwrap();
        let template = template();
        let base = template.request(&[]).unwrap();
        let two = template.request(&[&catalog[2], &catalog[3]]).unwrap();
        let bound = encoder.encode(&two).unwrap().1.input_tokens;
        assert!(bound > encoder.encode(&base).unwrap().1.input_tokens);
        let limits = Limits {
            max_length: bound,
            ..Limits::default()
        };
        let planner = Planner::new(
            template.clone(),
            &catalog,
            Some(&encoder),
            &limits,
            options(1, 64),
        )
        .unwrap();
        assert_eq!(planner.first().unwrap().capacity(), 2);
        for mask in 0_u64..(1 << catalog.len()) {
            let subset = catalog
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, candidate)| candidate)
                .collect::<Vec<_>>();
            if subset.len() <= 2 {
                let request = template.request(&subset).unwrap();
                let counts = encoder.encode(&request).unwrap().1;
                counts.preflight(&limits).unwrap();
            }
        }
        for bracket in planner.first().unwrap().brackets() {
            let (encoded, counts) = encoder.encode(bracket.request()).unwrap();
            assert_eq!(counts.input_tokens, bracket.counts().input_tokens);
            assert_eq!(encoded.input_ids.len(), bracket.counts().input_tokens);
            assert!(counts.exact);
        }
    }

    #[test]
    fn top_k_is_carried_per_bracket_and_final_probabilities_only_describe_survivors() {
        let document = Document::new("first\nsecond\nthird\nfourth\nfifth", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 4,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(1, 64)).unwrap();
        let first = planner.first().unwrap();
        let rankings = vec![
            first.brackets()[0]
                .rank(probabilities(&[
                    ("L1", 0.15),
                    ("L2", 0.15),
                    ("L3", 0.6),
                    (NONE, 0.1),
                ]))
                .unwrap(),
            first.brackets()[1]
                .rank(probabilities(&[("L4", 0.1), ("L5", 0.8), (NONE, 0.1)]))
                .unwrap(),
        ];
        let Reduction::Next { round } = planner.reduce(&first, &rankings).unwrap() else {
            panic!("expected a final comparison round")
        };
        assert_eq!(
            round.brackets()[0]
                .candidate_ids()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["L3", "L5"]
        );
        assert!(round.is_final());
        let ranking = round.brackets()[0]
            .rank(probabilities(&[("L3", 0.4), ("L5", 0.35), (NONE, 0.25)]))
            .unwrap();
        let Reduction::Final { result } = planner.reduce(&round, &[ranking]).unwrap() else {
            panic!("expected final output")
        };
        assert_eq!(result.original_candidates, 5);
        assert_eq!(result.probability_scope, "final-surviving-bracket");
        assert_eq!(result.selected[0].id.to_string(), "L3");
        assert_eq!(result.selected[0].probability, 0.4);
        assert!(result.ranking.unwrap().probabilities().get("L1").is_none());
    }

    #[test]
    fn top_two_carry_through_all_rounds_without_renumbering_or_global_score_comparison() {
        let document = Document::new("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 4,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(2, 64)).unwrap();
        let mut round = planner.first().unwrap();
        let mut sizes = Vec::new();
        let result = loop {
            sizes.push(
                round
                    .brackets()
                    .iter()
                    .map(|bracket| bracket.candidate_ids().len())
                    .sum::<usize>(),
            );
            let rankings = round
                .brackets()
                .iter()
                .map(|bracket| {
                    let denominator =
                        (1 + (1..=bracket.candidate_ids().len()).sum::<usize>()) as f64;
                    let values = bracket
                        .candidate_ids()
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (id.to_string(), (index + 1) as f64 / denominator))
                        .chain(std::iter::once((NONE.to_owned(), 1.0 / denominator)))
                        .collect();
                    bracket.rank(values).unwrap()
                })
                .collect::<Vec<_>>();
            match planner.reduce(&round, &rankings).unwrap() {
                Reduction::Next { round: next } => round = next,
                Reduction::Final { result } => break result,
            }
        };
        assert_eq!(sizes, [8, 6, 4, 3]);
        assert_eq!(result.round, 4);
        assert_eq!(
            result
                .selected
                .iter()
                .map(|candidate| candidate.id.to_string())
                .collect::<Vec<_>>(),
            ["L8", "L7"]
        );
        assert_eq!(
            result
                .surviving_candidates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["L5", "L7", "L8"]
        );
        assert_eq!(result.selected[0].probability, 3.0 / 7.0);
        assert_eq!(result.selected[1].probability, 2.0 / 7.0);
    }

    #[test]
    fn nonprogress_oversized_singletons_and_round_caps_are_terminal() {
        let document = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        assert_eq!(
            Planner::new(template(), &catalog, None, &limits, options(2, 64))
                .err()
                .unwrap()
                .exit_code(),
            65
        );
        let planner = Planner::new(template(), &catalog, None, &limits, options(1, 1)).unwrap();
        let first = planner.first().unwrap();
        let rankings = first
            .brackets()
            .iter()
            .map(|bracket| {
                let values = bracket
                    .candidate_ids()
                    .iter()
                    .enumerate()
                    .map(|(index, id)| (id.to_string(), if index == 0 { 0.7 } else { 0.2 }))
                    .chain(std::iter::once((NONE.to_owned(), 0.1)))
                    .collect();
                bracket.rank(values).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            planner.reduce(&first, &rankings).unwrap_err().exit_code(),
            65
        );
        let encoder = encoder();
        let limits = Limits {
            max_length: 1,
            ..Limits::default()
        };
        assert_eq!(
            Planner::new(
                template(),
                &catalog,
                Some(&encoder),
                &limits,
                options(1, 64)
            )
            .err()
            .unwrap()
            .exit_code(),
            65
        );
        let limits = Limits {
            max_state_tokens: 1,
            ..Limits::default()
        };
        let mut context = template();
        context.state = json!("long reference context ".repeat(20));
        assert_eq!(
            Planner::new(context, &catalog, Some(&encoder), &limits, options(1, 64))
                .err()
                .unwrap()
                .exit_code(),
            65
        );
    }

    #[test]
    fn rankings_bind_to_brackets_validate_distributions_and_keep_sorted_key_ties() {
        let document = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(1, 64)).unwrap();
        let first = planner.first().unwrap();
        let tied = first.brackets()[0]
            .rank(probabilities(&[("L1", 0.5), ("L2", 0.0), (NONE, 0.5)]))
            .unwrap();
        assert!(tied.tied());
        assert!(!tied.none_wins());
        assert_eq!(tied.ranked()[0].id.to_string(), "L1");
        assert_eq!(
            planner
                .reduce(&first, &[tied.clone(), tied])
                .unwrap_err()
                .exit_code(),
            64
        );
        for values in [
            probabilities(&[("L1", 0.5), (NONE, 0.5)]),
            probabilities(&[("L1", 0.9), ("L2", 0.9), (NONE, 0.9)]),
            probabilities(&[("L1", f64::NAN), ("L2", 0.5), (NONE, 0.5)]),
        ] {
            assert_eq!(
                first.brackets()[0].rank(values).unwrap_err().exit_code(),
                69
            );
        }
        let mut changed = template();
        changed.state = json!("different context");
        let other = Planner::new(changed, &catalog, None, &limits, options(1, 64)).unwrap();
        assert_eq!(other.reduce(&first, &[]).unwrap_err().exit_code(), 64);
    }

    #[test]
    fn empty_catalog_and_none_winners_finish_without_fabricating_candidates() {
        let document = Document::new("", None);
        let catalog = document.lines(false).unwrap();
        let planner = Planner::new(
            template(),
            &catalog,
            None,
            &Limits::default(),
            options(1, 64),
        )
        .unwrap();
        let first = planner.first().unwrap();
        assert!(first.is_final());
        assert!(first.brackets().is_empty());
        let Reduction::Final { result } = planner.reduce(&first, &[]).unwrap() else {
            panic!("empty selection should finish")
        };
        assert!(result.selected.is_empty());
        assert!(result.ranking.is_none());
        let document = Document::new("one\ntwo", None);
        let catalog = document.lines(false).unwrap();
        let planner = Planner::new(
            template(),
            &catalog,
            None,
            &Limits::default(),
            options(1, 64),
        )
        .unwrap();
        let first = planner.first().unwrap();
        let ranking = first.brackets()[0]
            .rank(probabilities(&[("L1", 0.05), ("L2", 0.05), (NONE, 0.9)]))
            .unwrap();
        let Reduction::Final { result } = planner.reduce(&first, &[ranking]).unwrap() else {
            panic!("expected final output")
        };
        assert!(result.selected.is_empty());
        assert!(result.ranking.unwrap().none_wins());
    }

    #[test]
    fn every_absent_bracket_finishes_at_the_round_limit_without_an_extra_request() {
        let document = Document::new("one\ntwo\nthree\nfour", None);
        let catalog = document.lines(false).unwrap();
        let limits = Limits {
            max_options: 3,
            ..Limits::default()
        };
        let planner = Planner::new(template(), &catalog, None, &limits, options(1, 1)).unwrap();
        let first = planner.first().unwrap();
        let rankings = first
            .brackets()
            .iter()
            .map(|bracket| {
                bracket
                    .rank(
                        bracket
                            .candidate_ids()
                            .iter()
                            .map(|id| (id.to_string(), 0.1))
                            .chain(std::iter::once((NONE.to_owned(), 0.8)))
                            .collect(),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let Reduction::Final { result } = planner.reduce(&first, &rankings).unwrap() else {
            panic!("absence must finish without another request")
        };
        assert_eq!(result.round, 1);
        assert_eq!(result.probability_scope, "no-surviving-candidates");
        assert!(result.selected.is_empty());
        assert!(result.ranking.is_none());
    }

    #[test]
    fn contextual_descriptions_are_counted_without_changing_verbatim_provenance() {
        let encoder = encoder();
        let document = Document::new("v1.0.0\nv2.0.0\nv3.0.0", None);
        let catalog = document.lines(false).unwrap();
        let mut template = template();
        template.descriptions.insert(catalog[1].id,json!({"verbatim":catalog[1].verbatim,"context":"This version introduced the regression."}));
        let planner = Planner::new(
            template.clone(),
            &catalog,
            Some(&encoder),
            &Limits::default(),
            options(1, 64),
        )
        .unwrap();
        let round = planner.first().unwrap();
        let Question::Choice { criteria, .. } =
            &round.brackets()[0].request().questions[SELECTION_ID]
        else {
            panic!("expected Choice")
        };
        assert_eq!(
            criteria["L2"]["context"],
            "This version introduced the regression."
        );
        assert_eq!(catalog[1].verbatim, "v2.0.0");
        assert_eq!(catalog[1].location.byte_start, 7);
        template
            .descriptions
            .insert("L99".parse().unwrap(), json!("unknown"));
        assert_eq!(
            Planner::new(
                template,
                &catalog,
                Some(&encoder),
                &Limits::default(),
                options(1, 64)
            )
            .err()
            .unwrap()
            .exit_code(),
            64
        );
    }

    #[test]
    fn candidate_state_boundaries_are_additive_through_unicode_normalization_and_special_tokens() {
        use crate::tournament::STATE_SEPARATOR;
        let encoder = encoder();
        assert!(encoder.has_verbatim_boundary(STATE_SEPARATOR));
        assert!(!encoder.has_verbatim_boundary("ordinary text"));
        let text = "a\u{030a}\n\u{0958}\n東京\n<|im_start|> embedded marker\nemoji 👩‍👩‍👦";
        let document = Document::new(text, None);
        let catalog = document.lines(false).unwrap();
        let query = template();
        let base = query.request(&[]).unwrap();
        let base_counts = encoder.encode(&base).unwrap().1;
        let additions = catalog
            .iter()
            .map(|candidate| {
                encoder
                    .encode(&query.request(&[candidate]).unwrap())
                    .unwrap()
                    .1
                    .state_tokens
                    - base_counts.state_tokens
            })
            .collect::<Vec<_>>();
        for mask in 0_u64..(1 << catalog.len()) {
            let candidates = catalog
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, candidate)| candidate)
                .collect::<Vec<_>>();
            let request = query.request(&candidates).unwrap();
            let counts = encoder.encode(&request).unwrap().1;
            let expected = base_counts.state_tokens
                + additions
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .map(|(_, value)| *value)
                    .sum::<usize>();
            assert_eq!(counts.state_tokens, expected);
        }
        let mut largest = additions.clone();
        largest.sort_unstable_by(|left, right| right.cmp(left));
        let limits = Limits {
            max_state_tokens: base_counts.state_tokens + largest[0] + largest[1],
            ..Limits::default()
        };
        let planned =
            Planner::new(query, &catalog, Some(&encoder), &limits, options(1, 64)).unwrap();
        assert_eq!(planned.first().unwrap().capacity(), 2);
        for bracket in planned.first().unwrap().brackets() {
            bracket.counts().preflight(&limits).unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "requires real server model; no fake HTTP/model backend"]
    async fn real_server_selection_prompt_probe_records_each_actual_form() {
        use crate::backend::Backend;
        use crate::budget::Budget;
        use crate::wire::Answer;

        let environment =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
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
                    ..Settings::default()
                },
                &environment,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = Backend::new(&profile).await.unwrap();
        let deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(60));
        let text = "Checkout is unavailable and customers cannot pay.";
        let document_text = format!("Blue skies and sunshine.\nCheckout works normally.\n{text}");
        let document = Document::new(&document_text, None);
        let catalog = document.lines(false).unwrap();
        let query = template();
        let production = query.request(&[&catalog[2]]).unwrap();
        let mut original = production.clone();
        original.state = Value::Null;
        if let Question::Choice { instructions, .. } =
            original.questions.get_mut(SELECTION_ID).unwrap()
        {
            *instructions = Some(
                json!({"question":query.question,"task":"Choose the listed candidate that best answers or matches this question. Choose none when no listed candidate does. State supplies reference context."}),
            );
        }
        let mut forms = vec![("structured-query", original.clone())];
        let mut raw = original.clone();
        if let Question::Choice { instructions, .. } = raw.questions.get_mut(SELECTION_ID).unwrap()
        {
            *instructions = Some(query.question.clone());
        }
        forms.push(("raw-query", raw.clone()));
        let mut state = raw.clone();
        state.state = json!({"candidates":{"L3":text},"context":null});
        forms.push(("raw-query-candidate-state", state));
        let mut context = raw;
        context.state = json!(text);
        forms.push(("raw-query-text-state", context));
        forms.push(("raw-query-separated-candidate-state", production));
        let budget = Budget::new(None);
        let mut captures = Vec::new();
        for (name, request) in forms {
            let counts = encoder.encode(&request).unwrap().1;
            counts.preflight(&profile.limits).unwrap();
            let evaluation = backend
                .evaluate(&request, &counts, &budget, true, deadline)
                .await
                .unwrap();
            let answers = evaluation.response.validate(&request).unwrap();
            let Answer::Choice { probabilities, .. } = &answers[SELECTION_ID] else {
                panic!("expected Choice")
            };
            eprintln!("prompt {name}: {probabilities:?}");
            captures.push(
                json!({"name":name,"request":request,"counts":counts,"evaluation":evaluation}),
            );
        }
        let usage = budget.usage().unwrap();
        assert_eq!(
            usage.spent_input,
            usage.reported_input + usage.unknown_input
        );
        if let Ok(path) = std::env::var("LOUPE_PROMPT_EVIDENCE") {
            let evidence = json!({"schema_version":1,"captured_at":chrono::Utc::now().to_rfc3339(),"endpoint":format!("{}{}",profile.base_url,profile.path),"encoder":encoder.identity,"captures":captures,"token_usage":usage,"proof_boundary":"Five actual model prompt forms on one synthetic positive line. Diagnostic comparison, not a held-out retrieval gate or cache identity qualification."});
            std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "requires real server model; no fake HTTP/model backend"]
    async fn real_server_tournament_counts_every_joint_request_and_preserves_final_distribution() {
        use crate::backend::Backend;
        use crate::budget::Budget;
        use crate::wire::Answer;

        let environment =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
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
                    concurrency: Some(2),
                    ..Settings::default()
                },
                &environment,
            )
            .unwrap();
        let encoder = crate::encoder::load_profile(&profile).unwrap().unwrap();
        let backend = Backend::new(&profile).await.unwrap();
        let discovery = backend.discover(&profile.limits).await.unwrap();
        let document = Document::new(
            "Blue skies and sunshine.\nCheckout works normally.\nCheckout is unavailable and customers cannot pay.\nA football team won its match.\nThe archive contains old logs.\nTraffic is moving smoothly.\nThe database backup completed.\nA new coffee shop opened.\nClouds will clear tonight.\nBirds sing outside.\nThe garden needs water.",
            None,
        );
        let catalog = document.lines(false).unwrap();
        let mut question = template();
        question.model = profile.model.clone();
        let planner = Planner::new(
            question,
            &catalog,
            Some(&encoder),
            &discovery.limits,
            options(1, 64),
        )
        .unwrap();
        let mut round = planner.first().unwrap();
        assert!(!round.is_final());
        let budget = Budget::new(None);
        let deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(60));
        let mut captures = Vec::new();
        let mut observed_model = None;
        let result = loop {
            let mut evaluations = Vec::new();
            let mut rankings = Vec::new();
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
                let model = crate::identity::Model::parse(&evaluation.response.model)
                    .map(|model| model.as_str().to_owned())
                    .unwrap_or_else(|_| evaluation.response.model.clone());
                if let Some(expected) = &observed_model {
                    assert_eq!(&model, expected);
                } else {
                    observed_model = Some(model);
                }
                let answers = evaluation.response.validate(bracket.request()).unwrap();
                let Answer::Choice { probabilities, .. } = &answers[SELECTION_ID] else {
                    panic!("selection response must be Choice")
                };
                rankings.push(bracket.rank(probabilities.clone()).unwrap());
                evaluations.push(evaluation);
            }
            captures.push(json!({"round":round,"evaluations":evaluations,"rankings":rankings}));
            match planner.reduce(&round, &rankings).unwrap() {
                Reduction::Next { round: next } => round = next,
                Reduction::Final { result } => break result,
            }
        };
        assert_ne!(result.probability_scope, "all-candidates");
        assert_eq!(
            result
                .selected
                .iter()
                .map(|candidate| candidate.id.to_string())
                .collect::<Vec<_>>(),
            ["L3"]
        );
        for candidate in &result.selected {
            assert!(result.surviving_candidates.contains(&candidate.id));
            assert_eq!(
                result.ranking.as_ref().unwrap().probabilities()[&candidate.id.to_string()],
                candidate.probability
            );
        }
        let usage = budget.usage().unwrap();
        assert!(usage.attempts > 0);
        assert_eq!(
            usage.spent_input,
            usage.reported_input + usage.unknown_input
        );
        assert_eq!(usage.cache_hits, 0);
        if let Ok(path) = std::env::var("LOUPE_TOURNAMENT_EVIDENCE") {
            let source = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tournament.rs"),
            )
            .unwrap();
            let evidence = json!({"schema_version":1,"captured_at":chrono::Utc::now().to_rfc3339(),"endpoint":format!("{}{}",profile.base_url,profile.path),"source_blake3":blake3::hash(&source).to_hex().to_string(),"encoder":encoder.identity,"discovery":discovery,"catalog":catalog,"captures":captures,"result":result,"token_usage":usage,"proof_boundary":"Real backend requests with exact local counts and raw responses. Functional synthetic candidates, not held-out retrieval accuracy. Presence policy is not applied by this budgeting/reduction test; full identity/cache eligibility and selection CLI remain separate tasks."});
            std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        }
        eprintln!(
            "tournament: rounds={}, submitted={} reported={} unknown={} calls={}",
            captures.len(),
            usage.spent_input,
            usage.reported_input,
            usage.unknown_input,
            usage.http_attempts
        );
    }
}
