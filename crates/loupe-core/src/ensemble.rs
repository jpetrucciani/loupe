//! Equal-weight summaries of correlated questions from one encoded request.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Policy, Probability, QuestionKind};
use crate::wire::{Answer, Question, Request, Response};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: String,
    field: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(alias = "question")]
    instructions: Option<Value>,
    criteria: Option<Value>,
}

#[derive(Clone, Debug)]
struct Variant {
    id: String,
    primary: String,
    field: String,
    question: Question,
}

#[derive(Clone, Debug, Default)]
pub struct Set {
    variants: Vec<Variant>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Vote {
    Boolean(bool),
    Choice(String),
    Level(usize),
}

#[derive(Clone, Debug)]
pub struct Group {
    pub set: Set,
    pub min_agreement: Probability,
}

pub struct Field {
    pub answer: Answer,
    pub assessment: Assessment,
    pub ensemble: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Member<'a> {
    pub field: String,
    pub answer: &'a Value,
    pub probabilities: BTreeMap<String, Probability>,
    pub vote: Option<Vote>,
}

#[derive(Debug, Serialize)]
pub struct Report<'a> {
    pub method: &'static str,
    pub independent: bool,
    pub calibrated: bool,
    pub model: &'a str,
    pub members: Vec<Member<'a>>,
    pub mean_probabilities: BTreeMap<String, Probability>,
    pub mean_noul: Option<Probability>,
    pub mean_score: Option<f64>,
    pub winning_option: Option<Vote>,
    pub agreement: Probability,
    pub min_agreement: Probability,
    pub agreement_uncertain: bool,
    pub assessment: Assessment,
}

impl Report<'_> {
    /// A derived distribution for metrics, separate from every raw API answer.
    pub fn mean_answer(&self, question: &Question) -> Result<Answer> {
        let probabilities = self
            .mean_probabilities
            .iter()
            .map(|(key, probability)| (key.clone(), probability.get()))
            .collect();
        let answer = match question {
            Question::Noul { .. } => Answer::Noul {
                noul: self
                    .mean_noul
                    .ok_or_else(|| protocol("mean Noul is missing"))?
                    .get(),
            },
            Question::Choice { .. } => {
                let crate::policy::Selected::Choice(choice) = &self.assessment.selected else {
                    return Err(protocol("mean Choice selection is missing"));
                };
                Answer::Choice {
                    choice: choice.clone(),
                    probabilities,
                    confidence: self.assessment.confidence.get(),
                }
            }
            Question::Score { .. } => Answer::Score {
                score: self
                    .mean_score
                    .ok_or_else(|| protocol("mean Score is missing"))?,
                probabilities,
                confidence: self.assessment.confidence.get(),
                legend: question.options().into_iter().collect(),
            },
        };
        answer.validate(question)?;
        Ok(answer)
    }
}

pub fn field(
    request: &Request,
    response: &Response,
    id: &str,
    policy: Policy,
    group: Option<&Group>,
) -> Result<Field> {
    if let Some(group) = group.filter(|group| group.set.has_primary(id)) {
        let report = group
            .set
            .report(id, id, request, response, policy, group.min_agreement)?;
        let question = request
            .questions
            .get(id)
            .ok_or_else(|| usage("ensemble primary is missing"))?;
        return Ok(Field {
            answer: report.mean_answer(question)?,
            assessment: report.assessment.clone(),
            ensemble: Some(
                serde_json::to_value(report).map_err(|error| usage(&error.to_string()))?,
            ),
        });
    }
    let answer = response
        .validate(request)?
        .shift_remove(id)
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "classification field is missing"))?;
    Ok(Field {
        assessment: answer.assessment(&policy)?,
        answer,
        ensemble: None,
    })
}

impl Set {
    pub fn from_document(document: &Value, questions: &IndexMap<String, Question>) -> Result<Self> {
        let Some(definitions) = document.get("variants") else {
            return Ok(Self::default());
        };
        let definitions: Vec<Definition> = serde_json::from_value(definitions.clone())
            .map_err(|error| usage(&format!("variants: {error}")))?;
        if definitions.is_empty() {
            return Err(usage("variants must contain at least one named question"));
        }
        let factors = crate::factors::Set::from_document(document)?;
        let mut seen = BTreeSet::new();
        let mut variants = Vec::with_capacity(definitions.len());
        for definition in definitions {
            if definition.id.len() > 64
                || !definition
                    .id
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_lowercase)
                || !definition.id.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
            {
                return Err(usage("variant IDs use [a-z][a-z0-9_-]*, at most 64 bytes"));
            }
            let primary = match definition.field {
                Some(field) => field,
                None => {
                    let mut primary = questions.keys().filter(|id| !factors.contains(id));
                    let field = primary
                        .next()
                        .ok_or_else(|| usage("variant primary is missing"))?;
                    if primary.next().is_some() {
                        return Err(usage("variants in a multi-question preset require field"));
                    }
                    field.clone()
                }
            };
            let base = questions
                .get(&primary)
                .ok_or_else(|| usage("variant field is not a declared question"))?;
            base.validate()?;
            let field = format!("ensemble:{primary}/{}", definition.id);
            if !seen.insert(field.clone()) || questions.contains_key(&field) {
                return Err(usage("duplicate or colliding ensemble question ID"));
            }
            let mut value = serde_json::to_value(base).map_err(json_error)?;
            if let Some(kind) = definition.kind {
                value["type"] = json!(kind);
            }
            if let Some(instructions) = definition.instructions {
                value["instructions"] = instructions;
            } else if matches!(base.instructions(), None | Some(Value::Null))
                || matches!(base.instructions(), Some(Value::String(text)) if text.is_empty())
            {
                // Preserve the original question-ID fallback despite the variant's new ID.
                value["instructions"] = json!(primary);
            }
            if let Some(criteria) = definition.criteria {
                value["criteria"] = criteria;
            }
            let question: Question = serde_json::from_value(value).map_err(json_error)?;
            question.validate()?;
            aligned(base, &question)?;
            variants.push(Variant {
                id: definition.id,
                primary,
                field,
                question,
            });
        }
        Ok(Self { variants })
    }

    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }
    pub fn has_primary(&self, primary: &str) -> bool {
        self.variants
            .iter()
            .any(|variant| variant.primary == primary)
    }

    pub fn variants(&self, primary: &str) -> impl Iterator<Item = (&str, &Question)> {
        self.variants
            .iter()
            .filter(move |variant| variant.primary == primary)
            .map(|variant| (variant.id.as_str(), &variant.question))
    }

    pub fn contains(&self, field: &str) -> bool {
        self.variants.iter().any(|variant| variant.field == field)
    }

    pub fn base_for(&self, field: &str) -> Option<&str> {
        self.variants
            .iter()
            .find(|variant| variant.field == field)
            .map(|variant| variant.primary.as_str())
    }

    pub fn primary(&self, questions: &IndexMap<String, Question>) -> IndexMap<String, Question> {
        questions
            .iter()
            .filter(|(id, _)| !self.contains(id))
            .map(|(id, question)| (id.clone(), question.clone()))
            .collect()
    }

    pub fn append(&self, questions: &mut IndexMap<String, Question>) -> Result<()> {
        if self
            .variants
            .iter()
            .any(|variant| questions.contains_key(&variant.field))
        {
            return Err(usage("ensemble fields collide with the request schema"));
        }
        questions.extend(
            self.variants
                .iter()
                .map(|variant| (variant.field.clone(), variant.question.clone())),
        );
        Ok(())
    }

    /// Retain the fully expanded instructions/context used by downstream views.
    pub fn materialize(mut self, questions: &IndexMap<String, Question>) -> Result<Self> {
        for variant in &mut self.variants {
            let primary = questions
                .get(&variant.primary)
                .ok_or_else(|| usage("ensemble primary is missing"))?;
            let question = questions
                .get(&variant.field)
                .ok_or_else(|| usage("ensemble member is missing"))?;
            aligned(primary, question)?;
            variant.question = question.clone();
        }
        Ok(self)
    }

    pub fn extend_combine(&self, modes: &mut IndexMap<String, crate::combine::Mode>) -> Result<()> {
        for variant in &self.variants {
            let mode = *modes
                .get(&variant.primary)
                .ok_or_else(|| usage("variant primary has no combine mode"))?;
            if modes.insert(variant.field.clone(), mode).is_some() {
                return Err(usage("variant combine mode is already declared"));
            }
        }
        Ok(())
    }

    pub fn fields(&self, primary: &str, bound_primary: &str) -> Result<Vec<String>> {
        let prefix = if primary == bound_primary {
            None
        } else {
            Some(
                bound_primary
                    .strip_suffix(&format!("/{primary}"))
                    .filter(|prefix| !prefix.is_empty())
                    .ok_or_else(|| protocol("invalid ensemble window binding"))?,
            )
        };
        let mut fields = vec![bound_primary.to_owned()];
        fields.extend(
            self.variants
                .iter()
                .filter(|variant| variant.primary == primary)
                .map(|variant| {
                    prefix.map_or_else(
                        || variant.field.clone(),
                        |prefix| format!("{prefix}/{}", variant.field),
                    )
                }),
        );
        if fields.len() < 2 {
            return Err(usage("primary question has no ensemble variants"));
        }
        Ok(fields)
    }

    pub fn report<'a>(
        &self,
        primary: &str,
        bound_primary: &str,
        request: &Request,
        response: &'a Response,
        policy: Policy,
        min_agreement: Probability,
    ) -> Result<Report<'a>> {
        let parsed = response.validate(request)?;
        let question = request
            .questions
            .get(bound_primary)
            .ok_or_else(|| protocol("ensemble primary question is missing"))?;
        let fields = self.fields(primary, bound_primary)?;
        let mut members = Vec::with_capacity(fields.len());
        for field in fields {
            let member_question = request
                .questions
                .get(&field)
                .ok_or_else(|| protocol("ensemble member question is missing"))?;
            aligned(question, member_question).map_err(|error| protocol(&error.to_string()))?;
            let answer = parsed
                .get(&field)
                .ok_or_else(|| protocol("ensemble member answer is missing"))?;
            let probabilities = distribution(answer)?;
            members.push(Member {
                answer: response
                    .answers
                    .get(&field)
                    .ok_or_else(|| protocol("ensemble raw answer is missing"))?,
                vote: vote(question, &probabilities)?,
                field,
                probabilities,
            });
        }
        let sources = members
            .iter()
            .map(|member| &member.probabilities)
            .collect::<Vec<_>>();
        let mean_probabilities = mean(&sources)?;
        let winning_option = vote(question, &mean_probabilities)?;
        let agrees = members
            .iter()
            .filter(|member| winning_option.is_some() && member.vote == winning_option)
            .count();
        let agreement = Probability::new(agrees as f64 / members.len() as f64)?;
        let mean_noul = if question.kind() == QuestionKind::Noul {
            Some(
                *mean_probabilities
                    .get("true")
                    .ok_or_else(|| protocol("mean Noul probability is missing"))?,
            )
        } else {
            None
        };
        let mean_score = if question.kind() == QuestionKind::Score {
            Some(mean_probabilities.iter().try_fold(0.0, |total, (key, p)| {
                Ok::<_, Error>(
                    total
                        + key
                            .parse::<usize>()
                            .map_err(|_| protocol("Score option is not an index"))?
                            as f64
                            * p.get(),
                )
            })?)
        } else {
            None
        };
        let mut assessment = match question.kind() {
            QuestionKind::Noul => {
                policy.noul(mean_noul.ok_or_else(|| protocol("mean Noul is missing"))?)?
            }
            QuestionKind::Choice => policy.choice(&mean_probabilities)?,
            QuestionKind::Score => policy.score(
                mean_score.ok_or_else(|| protocol("mean Score is missing"))?,
                &mean_probabilities,
            )?,
        };
        let agreement_uncertain = winning_option.is_none() || agreement.get() < min_agreement.get();
        assessment.uncertain |= agreement_uncertain;
        Ok(Report {
            method: "equal_mean_probabilities",
            independent: false,
            calibrated: false,
            model: &response.model,
            members,
            mean_probabilities,
            mean_noul,
            mean_score,
            winning_option,
            agreement,
            min_agreement,
            agreement_uncertain,
            assessment,
        })
    }
}

fn aligned(base: &Question, variant: &Question) -> Result<()> {
    if base.kind() != variant.kind()
        || base
            .options()
            .iter()
            .map(|(key, _)| key)
            .ne(variant.options().iter().map(|(key, _)| key))
    {
        return Err(usage(
            "ensemble variants must retain the question type and option keys/level indices",
        ));
    }
    Ok(())
}

fn distribution(answer: &Answer) -> Result<BTreeMap<String, Probability>> {
    match answer {
        Answer::Noul { noul } => Ok(BTreeMap::from([
            ("true".to_owned(), Probability::new(*noul)?),
            ("false".to_owned(), Probability::new(1.0 - *noul)?),
        ])),
        Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
            let mass: f64 = probabilities.values().sum();
            if !mass.is_finite() || mass <= 0.0 {
                return Err(protocol(
                    "ensemble probability mass must be positive and finite",
                ));
            }
            probabilities
                .iter()
                .map(|(key, value)| Ok((key.clone(), Probability::new(value / mass)?)))
                .collect()
        }
    }
}

fn mean(sources: &[&BTreeMap<String, Probability>]) -> Result<BTreeMap<String, Probability>> {
    let first = sources
        .first()
        .ok_or_else(|| protocol("ensemble has no members"))?;
    if sources.iter().any(|source| source.keys().ne(first.keys())) {
        return Err(protocol("ensemble probability keys differ"));
    }
    first
        .keys()
        .map(|key| {
            let total = sources.iter().try_fold(0.0, |sum, source| {
                Ok::<_, Error>(
                    sum + source
                        .get(key)
                        .ok_or_else(|| protocol("ensemble option is missing"))?
                        .get(),
                )
            })?;
            Ok((key.clone(), Probability::new(total / sources.len() as f64)?))
        })
        .collect()
}

fn vote(
    question: &Question,
    probabilities: &BTreeMap<String, Probability>,
) -> Result<Option<Vote>> {
    if question.kind() == QuestionKind::Noul {
        return Ok(Some(Vote::Boolean(
            probabilities
                .get("true")
                .ok_or_else(|| protocol("Noul probability is missing"))?
                .get()
                >= 0.5,
        )));
    }
    let mut ranked = probabilities.iter().collect::<Vec<_>>();
    ranked.sort_by(|(ka, pa), (kb, pb)| pb.get().total_cmp(&pa.get()).then_with(|| ka.cmp(kb)));
    let (key, probability) = ranked
        .first()
        .ok_or_else(|| protocol("ensemble distribution is empty"))?;
    if ranked
        .get(1)
        .is_some_and(|(_, second)| second.get() == probability.get())
    {
        return Ok(None);
    }
    match question.kind() {
        QuestionKind::Choice => Ok(Some(Vote::Choice((*key).clone()))),
        QuestionKind::Score => Ok(Some(Vote::Level(
            key.parse()
                .map_err(|_| protocol("Score option is not an index"))?,
        ))),
        QuestionKind::Noul => Err(protocol("unexpected Noul vote")),
    }
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

pub fn minimum(document: &Value, override_value: Option<f64>) -> Result<Probability> {
    let configured = document
        .get("policy")
        .and_then(|policy| policy.get("min_agreement"))
        .map(|value| {
            value
                .as_f64()
                .ok_or_else(|| usage("min_agreement must be a number"))
        })
        .transpose()?;
    Probability::new(override_value.or(configured).unwrap_or(1.0))
        .map_err(|error| usage(&error.to_string()))
}
fn protocol(message: &str) -> Error {
    Error::new(ErrorKind::Unavailable, message)
}
fn json_error(error: serde_json::Error) -> Error {
    usage(&error.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    use crate::ensemble::{Set, Vote};
    use crate::policy::{Policy, Probability, Selected};
    use crate::wire::{Question, Request, Response};

    #[test]
    fn implicit_variants_ignore_only_declared_factors() {
        let document = json!({"variants":[{"id":"alt"}],"factors":[{"id":"impact","weight":1}]});
        let mut questions = IndexMap::from([(
            "main".to_owned(),
            Question::Noul {
                instructions: None,
                criteria: None,
            },
        )]);
        crate::factors::Set::from_document(&document)
            .unwrap()
            .append(&mut questions)
            .unwrap();
        let set = Set::from_document(&document, &questions).unwrap();
        assert!(set.has_primary("main"));
        assert!(!set.has_primary("factor:impact"));
        let document = json!({"variants":[{"id":"alt"}]});
        let mut questions = IndexMap::from([(
            "factor:source".to_owned(),
            Question::Noul {
                instructions: None,
                criteria: None,
            },
        )]);
        assert!(
            Set::from_document(&document, &questions)
                .unwrap()
                .has_primary("factor:source")
        );
        questions.insert(
            "second".to_owned(),
            Question::Noul {
                instructions: None,
                criteria: None,
            },
        );
        assert!(Set::from_document(&document, &questions).is_err());
    }

    #[test]
    fn materialized_variants_keep_expanded_context_and_partial_groups_keep_other_fields_raw() {
        let (set, mut request) = request(
            json!({"type":"noul","instructions":"Question"}),
            json!([{"id":"alt"}]),
        );
        request.questions.insert(
            "other".into(),
            serde_json::from_value(json!({"type":"noul","instructions":"Other"})).unwrap(),
        );
        crate::presets::add_context(&mut request.questions, json!("shared facts"));
        let set = set.materialize(&request.questions).unwrap();
        assert_eq!(
            set.variants("primary")
                .next()
                .unwrap()
                .1
                .instructions()
                .unwrap()["context"],
            "shared facts"
        );
        let response = response(
            &request,
            vec![
                json!({"type":"noul","noul":0.8}),
                json!({"type":"noul","noul":0.4}),
                json!({"type":"noul","noul":0.2}),
            ],
        );
        let group = crate::ensemble::Group {
            set: set.clone(),
            min_agreement: Probability::new(1.0).unwrap(),
        };
        let raw = crate::ensemble::field(
            &request,
            &response,
            "other",
            Policy::default(),
            Some(&group),
        )
        .unwrap();
        assert!(raw.ensemble.is_none());
        assert_eq!(raw.assessment.selected, Selected::Boolean(false));
        request.questions.shift_remove("ensemble:primary/alt");
        assert!(set.materialize(&request.questions).is_err());
    }

    #[tokio::test]
    async fn disagreement_reaches_real_fallback_with_mean_metadata_and_unchanged_raw_answers() {
        // Pure probability/codec fixture and a real child process, no inference backend.
        let (set, mut request) = request(
            json!({"type":"noul","instructions":"Math fixture"}),
            json!([{"id":"direct"}]),
        );
        let unit = crate::records::Source::new(
            std::io::Cursor::new(b"Arithmetic fixture\r\n"),
            crate::records::Options::new(crate::records::Format::Lines),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap();
        request.state = unit.state.clone();
        let response = response(
            &request,
            vec![
                json!({"type":"noul","noul":0.99}),
                json!({"type":"noul","noul":0.4}),
            ],
        );
        let original = serde_json::to_value(&response).unwrap();
        let policy = Policy::default();
        assert!(
            !response.validate(&request).unwrap()["primary"]
                .assessment(&policy)
                .unwrap()
                .uncertain
        );
        let input = crate::fallback::Input {
            unit: &unit,
            request: &request,
            response: &response,
            field: "primary",
            policy,
        };
        let ensemble = crate::fallback::Ensemble {
            set: &set,
            primary: "primary",
            min_agreement: Probability::new(1.0).unwrap(),
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("calls");
        let command = format!(
            "printf once >> '{}'; cat >&2; printf '%s\\n' \"$LOUPE_ENSEMBLE_JSON\" \"$LOUPE_ANSWER_JSON\" \"$LOUPE_DECISION_SOURCE\" \"$LOUPE_CONFIDENCE\" >&2; printf no",
            marker.display()
        );
        let expires = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        assert!(
            crate::fallback::execute(&command, &input, expires)
                .await
                .is_err()
        );
        assert!(!marker.exists());
        let result = crate::fallback::execute_ensemble(&command, &input, &ensemble, expires)
            .await
            .unwrap();
        let crate::fallback::Execution::Resolved { selected, outcome } = result else {
            panic!("real fallback failed")
        };
        assert_eq!(selected, Selected::Boolean(false));
        assert!(outcome.stderr.bytes.starts_with(&unit.raw));
        let lines = std::str::from_utf8(&outcome.stderr.bytes[unit.raw.len()..])
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        let report: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(report["agreement"], 0.5);
        assert_eq!(report["assessment"]["uncertain"], true);
        assert_eq!(
            serde_json::from_str::<Value>(lines[1]).unwrap(),
            response.answers["primary"]
        );
        assert_eq!(lines[2], "ensemble");
        assert!((lines[3].parse::<f64>().unwrap() - 0.695).abs() < 1e-12);
        assert_eq!(serde_json::to_value(&response).unwrap(), original);
        let relaxed = crate::fallback::Ensemble {
            min_agreement: Probability::new(0.5).unwrap(),
            ..ensemble
        };
        assert!(
            crate::fallback::execute_ensemble(&command, &input, &relaxed, expires)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(marker).unwrap(), b"once");
    }

    #[test]
    fn agreement_settings_and_inherited_reducers_are_validated() {
        assert_eq!(
            crate::ensemble::minimum(&json!({}), None).unwrap().get(),
            1.0
        );
        assert_eq!(
            crate::ensemble::minimum(&json!({"policy":{"min_agreement":0.8}}), None)
                .unwrap()
                .get(),
            0.8
        );
        assert_eq!(
            crate::ensemble::minimum(&json!({"policy":{"min_agreement":0.8}}), Some(0.6))
                .unwrap()
                .get(),
            0.6
        );
        for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert_eq!(
                crate::ensemble::minimum(&json!({}), Some(value))
                    .unwrap_err()
                    .exit_code(),
                64
            );
        }
        assert!(
            crate::ensemble::minimum(&json!({"policy":{"min_agreement":"all"}}), None).is_err()
        );
        let (set, request) = request(json!({"type":"noul"}), json!([{"id":"direct"}]));
        let mut modes = crate::combine::resolve(
            &set.primary(&request.questions),
            Some(&json!({"primary":"any"})),
            None,
        )
        .unwrap();
        set.extend_combine(&mut modes).unwrap();
        assert_eq!(modes["ensemble:primary/direct"], crate::combine::Mode::Any);
        assert!(set.extend_combine(&mut modes).is_err());
        for instructions in [Value::Null, json!("")] {
            let (_, request) = crate::ensemble::tests::request(
                json!({"type":"noul","instructions":instructions}),
                json!([{"id":"direct"}]),
            );
            assert_eq!(
                request.questions["ensemble:primary/direct"].instructions(),
                Some(&json!("primary"))
            );
        }
    }

    fn request(question: Value, variants: Value) -> (Set, Request) {
        let mut questions = IndexMap::from([(
            "primary".to_owned(),
            serde_json::from_value::<Question>(question).unwrap(),
        )]);
        let set = Set::from_document(&json!({"variants":variants}), &questions).unwrap();
        set.append(&mut questions).unwrap();
        (
            set,
            Request {
                model: "mathematical-fixture".into(),
                state: json!("arithmetic codec fixture; no backend"),
                questions,
            },
        )
    }

    fn response(request: &Request, values: Vec<Value>) -> Response {
        Response {
            model: request.model.clone(),
            answers: request.questions.keys().cloned().zip(values).collect(),
            usage: crate::wire::Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        }
    }

    #[test]
    fn schemas_preserve_ids_fallbacks_options_and_window_binding() {
        let (set, request) = request(
            json!({"type":"noul"}),
            json!([{"id":"direct"},{"id":"countercheck","instructions":"Is the proposition true?"}]),
        );
        assert_eq!(
            request
                .questions
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "primary",
                "ensemble:primary/direct",
                "ensemble:primary/countercheck"
            ]
        );
        assert_eq!(
            request.questions["ensemble:primary/direct"].instructions(),
            Some(&json!("primary"))
        );
        assert_eq!(
            set.fields("primary", "L7/primary").unwrap(),
            [
                "L7/primary",
                "L7/ensemble:primary/direct",
                "L7/ensemble:primary/countercheck"
            ]
        );
        assert!(set.fields("primary", "L7/other").is_err());
        assert_eq!(set.primary(&request.questions).len(), 1);
        let original = set.primary(&request.questions);
        for definitions in [
            json!([]),
            json!([{"id":"Bad"}]),
            json!([{"id":"a"},{"id":"a"}]),
            json!([{"id":"a","type":"score","criteria":["one"]}]),
            json!([{"id":"a","weight":2}]),
            json!([{"id":"a","field":"missing"}]),
        ] {
            assert!(Set::from_document(&json!({"variants":definitions}), &original).is_err());
        }
        let choice = IndexMap::from([(
            "x".to_owned(),
            serde_json::from_value(json!({"type":"choice","criteria":{"a":null,"b":null}}))
                .unwrap(),
        )]);
        assert!(
            Set::from_document(
                &json!({"variants":[{"id":"a","criteria":{"a":null,"c":null}}]}),
                &choice
            )
            .is_err()
        );
        let mut multi = original.clone();
        multi.insert("second".into(), original["primary"].clone());
        assert!(Set::from_document(&json!({"variants":[{"id":"direct"}]}), &multi).is_err());
        assert!(
            Set::from_document(
                &json!({"variants":[{"id":"direct","field":"second"}]}),
                &multi
            )
            .is_ok()
        );
    }

    #[test]
    fn noul_mean_votes_and_explicit_agreement_gate_preserve_raw_answers() {
        let (set, request) = request(
            json!({"type":"noul","instructions":"Question"}),
            json!([{"id":"first"},{"id":"second"}]),
        );
        let response = response(
            &request,
            vec![
                json!({"type":"noul","noul":0.9}),
                json!({"type":"noul","noul":0.7}),
                json!({"type":"noul","noul":0.1}),
            ],
        );
        let raw = serde_json::to_value(&response).unwrap();
        let report = set
            .report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(1.0).unwrap(),
            )
            .unwrap();
        assert!((report.mean_noul.unwrap().get() - 1.7 / 3.0).abs() < 1e-12);
        assert_eq!(report.agreement.get(), 2.0 / 3.0);
        assert_eq!(report.winning_option, Some(Vote::Boolean(true)));
        assert_eq!(report.assessment.selected, Selected::Boolean(true));
        assert!(report.assessment.uncertain);
        assert!(!report.independent);
        assert!(!report.calibrated);
        let relaxed = set
            .report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(0.6).unwrap(),
            )
            .unwrap();
        assert!(!relaxed.assessment.uncertain);
        assert_eq!(serde_json::to_value(&response).unwrap(), raw);
    }

    #[test]
    fn rounded_choice_mass_is_normalized_before_averaging_and_ties_are_ambiguous() {
        let (set, request) = request(
            json!({"type":"choice","criteria":{"a":"A","b":"B"}}),
            json!([{"id":"alternate","criteria":{"b":"Alternate B","a":"Alternate A"}}]),
        );
        let response = response(
            &request,
            vec![
                json!({"type":"choice","choice":"a","confidence":0.6,"probabilities":{"a":0.6,"b":0.3999}}),
                json!({"type":"choice","choice":"b","confidence":0.7,"probabilities":{"a":0.3,"b":0.7}}),
            ],
        );
        let report = set
            .report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(1.0).unwrap(),
            )
            .unwrap();
        assert!((report.mean_probabilities["a"].get() - (0.6 / 0.9999 + 0.3) / 2.0).abs() < 1e-12);
        assert_eq!(report.assessment.selected, Selected::Choice("b".into()));
        assert_eq!(report.agreement.get(), 0.5);
        assert!(report.assessment.uncertain);
        let tied = crate::ensemble::tests::response(
            &request,
            vec![
                json!({"type":"choice","choice":"a","confidence":0.5,"probabilities":{"a":0.5,"b":0.5}});
                2
            ],
        );
        let report = set
            .report(
                "primary",
                "primary",
                &request,
                &tied,
                Policy::default(),
                Probability::new(1.0).unwrap(),
            )
            .unwrap();
        assert_eq!(report.winning_option, None);
        assert_eq!(report.agreement.get(), 0.0);
        assert!(report.members.iter().all(|member| member.vote.is_none()));
    }

    #[test]
    fn score_uses_mean_distribution_expectation_and_modal_option_agreement() {
        let (set, request) = request(
            json!({"type":"score","criteria":["none","some","full"]}),
            json!([{"id":"alternate","criteria":["absent","partial","complete"]}]),
        );
        let legend = json!({"0":"none","1":"some","2":"full"});
        let response = response(
            &request,
            vec![
                json!({"type":"score","score":2.0,"confidence":1.0,"legend":legend,"probabilities":{"0":0.0,"1":0.0,"2":1.0}}),
                json!({"type":"score","score":0.4,"confidence":0.6,"legend":legend,"probabilities":{"0":0.6,"1":0.4,"2":0.0}}),
            ],
        );
        let report = set
            .report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(1.0).unwrap(),
            )
            .unwrap();
        assert_eq!(report.mean_score, Some(1.2));
        assert_eq!(report.assessment.selected, Selected::Score(1.2));
        assert_eq!(report.winning_option, Some(Vote::Level(2)));
        assert_eq!(report.agreement.get(), 0.5);
        assert!(report.assessment.uncertain);
        let wrong = IndexMap::from([("primary".to_owned(), request.questions["primary"].clone())]);
        assert!(
            Set::from_document(
                &json!({"variants":[{"id":"short","criteria":["none","full"]}]}),
                &wrong
            )
            .is_err()
        );
    }

    #[test]
    fn missing_misaligned_and_cross_unit_members_fail_validation() {
        let (set, mut request) = request(json!({"type":"noul"}), json!([{"id":"direct"}]));
        let mut response = response(&request, vec![json!({"type":"noul","noul":0.8}); 2]);
        response.answers.shift_remove("ensemble:primary/direct");
        assert!(
            set.report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(1.0).unwrap()
            )
            .is_err()
        );
        request.questions.shift_remove("ensemble:primary/direct");
        assert!(
            set.report(
                "primary",
                "primary",
                &request,
                &response,
                Policy::default(),
                Probability::new(1.0).unwrap()
            )
            .is_err()
        );
    }
}
