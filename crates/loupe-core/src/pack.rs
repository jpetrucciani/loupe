use std::num::NonZeroUsize;
use std::str::FromStr;

use indexmap::IndexMap;
use serde_json::json;

use crate::config::Limits;
use crate::encoder::{self, ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::wire::{Question, Request};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Unit,
    Whole,
    Auto,
    Window(NonZeroUsize),
}

impl FromStr for Mode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "unit" => Ok(Self::Unit),
            "whole" => Ok(Self::Whole),
            "auto" => Ok(Self::Auto),
            "window" => "16"
                .parse::<NonZeroUsize>()
                .map(Self::Window)
                .map_err(|error| Error::new(ErrorKind::Usage, error.to_string())),
            other => other
                .strip_prefix("window:")
                .unwrap_or(other)
                .parse::<NonZeroUsize>()
                .map(Self::Window)
                .map_err(|_| {
                    Error::new(
                        ErrorKind::Usage,
                        "pack mode must be unit, whole, auto, window or window:N with positive N",
                    )
                }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Lines,
    Records,
    Files,
    Units,
}
impl FromStr for SourceKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "lines" | "Lines" => Ok(Self::Lines),
            "records" | "Records" => Ok(Self::Records),
            "files" | "Files" => Ok(Self::Files),
            "units" | "Units" => Ok(Self::Units),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "question ID source must be lines, records, files or units",
            )),
        }
    }
}
impl SourceKind {
    pub fn id(self, index: u64) -> String {
        let prefix = match self {
            Self::Lines => 'L',
            Self::Records => 'R',
            Self::Files => 'F',
            Self::Units => 'U',
        };
        format!("{prefix}{index}")
    }
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub index: u64,
    pub source_id: String,
    pub questions: IndexMap<String, String>,
}

pub struct Packed {
    pub request: Request,
    pub counts: Counts,
    pub bindings: Vec<Binding>,
}

pub struct Window<'a> {
    pub encoder: Option<&'a ClefEncoder>,
    pub limits: &'a Limits,
    pub questions: &'a IndexMap<String, Question>,
    pub model: &'a str,
    pub contextual: bool,
    pub source: SourceKind,
}

impl Window<'_> {
    pub fn single(&self, unit: &Unit) -> Result<Packed> {
        self.build(std::slice::from_ref(unit))
    }
    pub fn max_units(&self, requested: NonZeroUsize) -> Result<usize> {
        if self.questions.is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "window packing requires questions",
            ));
        }
        let options = self
            .questions
            .values()
            .try_fold(0usize, |total, question| {
                total
                    .checked_add(question.options().len())
                    .ok_or_else(|| Error::new(ErrorKind::Input, "window option count overflow"))
            })?;
        if options == 0 {
            return Err(Error::new(
                ErrorKind::Usage,
                "window packing requires allowed options",
            ));
        }
        Ok(requested
            .get()
            .min((self.limits.max_questions / self.questions.len()).max(1))
            .min((self.limits.max_total_options / options).max(1)))
    }

    pub fn prefix(&self, units: &[Unit], requested: NonZeroUsize) -> Result<Packed> {
        if units.is_empty() {
            return Err(Error::new(ErrorKind::Usage, "cannot pack an empty window"));
        }
        let mut length = units.len().min(self.max_units(requested)?);
        loop {
            let prefix = units.get(..length).ok_or_else(|| {
                Error::new(ErrorKind::Unavailable, "invalid window prefix length")
            })?;
            let packed = self.build(prefix)?;
            match self
                .limits
                .validate_request(&packed.request)
                .and_then(|_| packed.counts.preflight(self.limits))
            {
                Ok(()) => return Ok(packed),
                Err(error) if error.kind == ErrorKind::Input && length > 1 => {
                    length = (length / 2).max(1)
                }
                Err(error) => {
                    let condition = if length == 1 {
                        "window reached size 1"
                    } else {
                        "window cannot be admitted"
                    };
                    return Err(Error::new(
                        error.kind,
                        format!(
                            "unit {}: {condition}: {error}",
                            units
                                .first()
                                .ok_or_else(|| Error::new(
                                    ErrorKind::Unavailable,
                                    "window unit disappeared"
                                ))?
                                .index
                        ),
                    ));
                }
            }
        }
    }

    fn build(&self, units: &[Unit]) -> Result<Packed> {
        let mut state = serde_json::Map::new();
        let mut questions = IndexMap::new();
        let mut bindings = Vec::new();
        for unit in units {
            let source_id = self.source.id(unit.index);
            if state
                .insert(source_id.clone(), unit.state.clone())
                .is_some()
            {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "window source IDs must be distinct",
                ));
            }
            let mut ids = IndexMap::new();
            for (base_id, question) in self.questions {
                let id = if self.questions.len() == 1 {
                    source_id.clone()
                } else {
                    format!("{source_id}/{base_id}")
                };
                let question = bind(question, base_id, Some(&source_id), self.contextual);
                if questions.insert(id.clone(), question).is_some() {
                    return Err(Error::new(ErrorKind::Usage, "window question ID collision"));
                }
                ids.insert(base_id.clone(), id);
            }
            bindings.push(Binding {
                index: unit.index,
                source_id,
                questions: ids,
            });
        }
        let request = Request {
            model: self.model.to_owned(),
            state: json!({"units":state}),
            questions,
        };
        let counts = count(self.encoder, &request)?;
        Ok(Packed {
            request,
            counts,
            bindings,
        })
    }
}

pub fn count(encoder: Option<&ClefEncoder>, request: &Request) -> Result<Counts> {
    match encoder {
        Some(encoder) => encoder.encode(request).map(|(_, counts)| counts),
        None => encoder::estimate(request),
    }
}

pub fn bind(
    question: &Question,
    base_id: &str,
    source_id: Option<&str>,
    contextual: bool,
) -> Question {
    if source_id.is_none() && !contextual {
        return question.clone();
    }
    let mut question = question.clone();
    let instructions = match &mut question {
        Question::Noul { instructions, .. }
        | Question::Choice { instructions, .. }
        | Question::Score { instructions, .. } => instructions,
    };
    let original = instructions
        .as_ref()
        .filter(|value| !value.is_null() && value.as_str() != Some(""))
        .cloned()
        .unwrap_or_else(|| json!(base_id));
    let path = source_id.map(|id| format!("units.{id}"));
    *instructions = Some(if contextual {
        json!({"question":original,"unit":path.as_ref().map(|path|format!("`{path}.unit`")).unwrap_or_else(||"`unit`".to_owned()),
            "context":path.as_ref().map(|path|format!("`{path}.previous_lines`")).unwrap_or_else(||"`previous_lines`".to_owned()),
            "scope":"Decide only about unit; previous_lines are read-only context."})
    } else {
        json!({"question":original,"unit":path.as_ref().map(|path|format!("`{path}`")),"scope":"Decide only about this unit, using its own reference data."})
    });
    question
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::config::{Config, Limits, Settings};
    use crate::encoder::ClefEncoder;
    use crate::input::{Location, Unit};
    use crate::pack::{Mode, SourceKind, Window, bind};
    use crate::wire::Question;
    use indexmap::IndexMap;
    use proptest::prelude::*;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    fn encoder() -> ClefEncoder {
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = Config::default()
            .resolve(
                None,
                &Settings {
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER").ok(),
                    ..Settings::default()
                },
                &env,
            )
            .unwrap();
        crate::encoder::load_profile(&profile).unwrap().unwrap()
    }
    fn unit(index: u64, state: Value) -> Unit {
        let text = state
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| state.to_string());
        Unit {
            index,
            subject: state.clone(),
            state,
            text: text.clone(),
            raw: text.into_bytes(),
            header: None,
            location: Location {
                source: None,
                byte_start: 0,
                byte_end: 0,
                line_start: index,
                line_end: index,
            },
        }
    }
    fn questions(count: usize, options: usize) -> IndexMap<String, Question> {
        (0..count).map(|n| {
            let question=match n%3 {
                0=>Question::Noul {instructions:Some(json!("Does this report an active failure?")),criteria:None},
                1=>serde_json::from_value(json!({"type":"choice","instructions":"Choose the appropriate label","criteria":(0..options).map(|i|(format!("c{i}"),json!(format!("Option {i}")))).collect::<serde_json::Map<_,_>>()})).unwrap(),
                _=>Question::Score {instructions:Some(json!("Rate urgency")),criteria:(0..options).map(|i|json!(format!("Level {i}"))).collect()},
            };
            (format!("q{n}"),question)
        }).collect()
    }

    #[test]
    fn grammar_context_binding_and_missing_instructions_keep_source_meaning() {
        assert_eq!("unit".parse::<Mode>().unwrap(), Mode::Unit);
        assert_eq!(
            "window".parse::<Mode>().unwrap(),
            Mode::Window(NonZeroUsize::new(16).unwrap())
        );
        assert_eq!(
            "4".parse::<Mode>().unwrap(),
            "window:4".parse::<Mode>().unwrap()
        );
        for value in ["0", "window:0", "window:-1", "unknown", "window:overflow"] {
            assert_eq!(value.parse::<Mode>().unwrap_err().exit_code(), 64);
        }
        for instructions in [None, Some(Value::Null), Some(json!(""))] {
            let question = Question::Noul {
                instructions,
                criteria: None,
            };
            let bound = bind(&question, "meaningful-question", Some("L7"), true);
            assert_eq!(
                bound.instructions().unwrap()["question"],
                "meaningful-question"
            );
            assert_eq!(bound.instructions().unwrap()["unit"], "`units.L7.unit`");
            assert_eq!(
                bound.instructions().unwrap()["context"],
                "`units.L7.previous_lines`"
            );
        }
    }

    #[test]
    fn token_overflow_halves_until_fit_without_losing_remaining_units_or_renumbering() {
        let encoder = encoder();
        let questions = questions(1, 2);
        let units = (0..4)
            .map(|n| unit(1 + n * 2, json!(" event".repeat(100))))
            .collect::<Vec<_>>();
        let limits = Limits::default();
        let reference = Window {
            encoder: Some(&encoder),
            limits: &limits,
            questions: &questions,
            model: "clef-flash",
            contextual: false,
            source: SourceKind::Lines,
        };
        let two = reference
            .prefix(&units, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let limits = Limits {
            max_length: two.counts.input_tokens,
            ..limits
        };
        let window = Window {
            limits: &limits,
            ..reference
        };
        let first = window
            .prefix(&units, NonZeroUsize::new(4).unwrap())
            .unwrap();
        assert_eq!(
            first
                .bindings
                .iter()
                .map(|binding| binding.index)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        let second = window
            .prefix(&units[2..], NonZeroUsize::new(4).unwrap())
            .unwrap();
        assert_eq!(
            second
                .bindings
                .iter()
                .map(|binding| binding.source_id.as_str())
                .collect::<Vec<_>>(),
            ["L5", "L7"]
        );
        assert!(first.request.questions.contains_key("L3"));
        assert!(second.request.questions.contains_key("L7"));
        let limits = Limits {
            max_length: 1,
            ..limits
        };
        let window = Window {
            limits: &limits,
            ..window
        };
        let error = window
            .prefix(&units, NonZeroUsize::new(4).unwrap())
            .err()
            .unwrap();
        assert_eq!(error.exit_code(), 65);
        assert!(error.to_string().contains("size 1"));
        assert!(error.to_string().contains("total="));
    }

    #[test]
    fn every_admitted_window_obeys_all_limits_and_covers_an_exact_ordered_prefix() {
        let encoder = encoder();
        let strategy = (
            prop::collection::vec(prop::collection::vec(any::<char>(), 0..36), 1..18),
            1usize..4,
            2usize..8,
            1usize..32,
            100usize..2200,
            1usize..700,
            1usize..24,
            1usize..9,
            1usize..100,
            any::<bool>(),
            any::<bool>(),
        );
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 96,
            ..ProptestConfig::default()
        });
        runner.run(&strategy,|(values,question_count,options,requested,max_length,max_state,max_questions,max_options,max_total_options,contextual,structured)| {
            let questions=questions(question_count,options);
            let units=values.into_iter().enumerate().map(|(n,chars)| {
                let text=chars.into_iter().collect::<String>();
                let state=if contextual {json!({"unit":text,"previous_lines":[{"id":"L0","text":"read-only context"}]})}
                    else if structured {json!({"msg":text,"ordinal":n})} else {json!(text)};
                unit(1+(n as u64)*2,state)
            }).collect::<Vec<_>>();
            let limits=Limits {max_length,max_state_tokens:max_state,max_questions,max_options,max_total_options,max_batch:4,..Limits::default()};
            let window=Window {encoder:Some(&encoder),limits:&limits,questions:&questions,model:"clef-flash",contextual,source:SourceKind::Lines};
            let requested=NonZeroUsize::new(requested).unwrap();
            let mut cursor=0;
            let mut seen=Vec::new();
            while cursor<units.len() {
                match window.prefix(&units[cursor..],requested) {
                    Ok(packed)=> {
                        prop_assert!(!packed.bindings.is_empty());
                        prop_assert!(packed.bindings.len()<=requested.get());
                        prop_assert!(packed.request.questions.len()<=max_questions);
                        prop_assert!(packed.counts.input_tokens<=max_length);
                        prop_assert!(packed.counts.state_tokens<=max_state);
                        prop_assert!(packed.request.questions.values().all(|q|q.options().len()<=max_options));
                        prop_assert!(packed.request.questions.values().map(|q|q.options().len()).sum::<usize>()<=max_total_options);
                        let (encoded,recount)=encoder.encode(&packed.request).unwrap();
                        prop_assert_eq!(encoded.input_ids.len(),packed.counts.input_tokens);
                        prop_assert_eq!(encoded.questions.len(),packed.request.questions.len());
                        prop_assert_eq!(recount.input_tokens,packed.counts.input_tokens);
                        for (offset,binding) in packed.bindings.iter().enumerate() {
                            let original=&units[cursor+offset];
                            prop_assert_eq!(binding.index,original.index);
                            prop_assert_eq!(&packed.request.state["units"][&binding.source_id],&original.state);
                            prop_assert_eq!(binding.questions.len(),questions.len());
                            for id in binding.questions.values() {prop_assert!(packed.request.questions.contains_key(id));}
                            seen.push(binding.index);
                        }
                        cursor+=packed.bindings.len();
                    },
                    Err(error)=> {
                        prop_assert_eq!(error.exit_code(),65);
                        let single=window.build(&units[cursor..cursor+1]).unwrap();
                        prop_assert!(limits.validate_request(&single.request).and_then(|_|single.counts.preflight(&limits)).is_err());
                        break;
                    }
                }
            }
            prop_assert_eq!(seen,units.iter().take(cursor).map(|unit|unit.index).collect::<Vec<_>>());
            Ok(())
        }).unwrap();
    }
}
