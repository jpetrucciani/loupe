use std::num::NonZeroUsize;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::config::Limits;
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::split::{self, Chunk};
use crate::wire::{Answer, Question, Request, Response};

pub const RELEVANCE_ID: &str = "focus-relevance";
pub const SEPARATOR: &str = "\n[…]\n";

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub operation: &'static str,
    pub path: String,
    pub coordinate_space: &'static str,
    pub k: NonZeroUsize,
    pub original_counts: Counts,
    pub relevance: Vec<Chunk>,
    pub final_template: Request,
    pub final_schema_counts: Counts,
    pub final_status: &'static str,
    #[serde(skip)]
    text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub byte_start: usize,
    pub byte_end: usize,
    pub token_start: usize,
    pub token_end: usize,
    pub line_start: u64,
    pub line_end: u64,
}

#[derive(Debug, Serialize)]
pub struct Selection {
    /// One-based chunk numbers in document order.
    pub selected_chunks: Vec<usize>,
    pub ranges: Vec<Range>,
    pub separator: &'static str,
    pub lossy: bool,
    pub request: Request,
    pub counts: Counts,
}

pub fn plan(
    encoder: &ClefEncoder,
    original: &Request,
    path: &str,
    k: NonZeroUsize,
    limits: &Limits,
    options: &split::Options,
) -> Result<Plan> {
    limits.validate_request(original)?;
    if options.max_chunks == 0 {
        return Err(Error::new(ErrorKind::Usage, "max-chunks must be positive"));
    }
    let text = original
        .state
        .pointer(path)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "focus requires a text subject, never a structured record",
            )
        })?;
    let original_counts = encoder.encode(original)?.1;
    let mut final_template = original.clone();
    for (id, question) in &mut final_template.questions {
        let instructions = question
            .instructions()
            .filter(|value| !value.is_null() && value.as_str() != Some(""))
            .cloned()
            .unwrap_or_else(|| json!(id));
        let focused = Some(
            json!({"question":instructions,"scope":{"operation":"focus","path":path,"instruction":"Answer over the selected document excerpts at this state path, using the supplied reference and context. Gaps marked […] contain unavailable evidence."}}),
        );
        match question {
            Question::Noul { instructions, .. }
            | Question::Choice { instructions, .. }
            | Question::Score { instructions, .. } => *instructions = focused,
        }
    }
    replace(&mut final_template, path, String::new())?;
    let final_schema_counts = encoder.encode(&final_template)?.1;
    final_schema_counts.preflight(limits).map_err(|error| {
        Error::new(
            error.kind,
            format!("focus final schema/context does not fit: {error}"),
        )
    })?;
    let mut relevance_template = original.clone();
    relevance_template.questions = IndexMap::from([(
        RELEVANCE_ID.to_owned(),
        Question::Score {
            instructions: Some(
                json!({"task":"Score the usefulness of this excerpt for answering any of the supplied questions. Direct evidence for yes, no, or an absent answer is relevant. Judge only the target text, using the other state as reference/context.","target":path,"questions":original.questions}),
            ),
            criteria: vec![
                json!("No relevant information"),
                json!("Related information without direct evidence"),
                json!("Direct evidence answering at least one question"),
            ],
        },
    )]);
    let relevance = if text.is_empty() {
        limits.validate_request(&relevance_template)?;
        let counts = encoder.encode(&relevance_template)?.1;
        counts.preflight(limits)?;
        vec![Chunk {
            range: split::Range {
                byte_start: 0,
                byte_end: 0,
                token_start: 0,
                token_end: 0,
                line_start: 1,
                line_end: 1,
                text_tokens: 0,
                unique_tokens: 0,
            },
            request: relevance_template,
            counts,
        }]
    } else {
        split::plan(encoder, &relevance_template, path, limits, options)?
    };
    Ok(Plan {
        operation: "focus",
        path: path.to_owned(),
        coordinate_space: "original target string UTF-8 bytes and tokens",
        k,
        original_counts,
        relevance,
        final_template,
        final_schema_counts,
        final_status: "conditional on real relevance scores; final evidence must be encoded and admitted separately",
        text: text.to_owned(),
    })
}

pub fn select(
    encoder: &ClefEncoder,
    plan: &Plan,
    responses: &[Response],
    limits: &Limits,
) -> Result<Selection> {
    if responses.len() != plan.relevance.len() || responses.is_empty() {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "focus relevance response count mismatch",
        ));
    }
    let mut ranking = Vec::new();
    let mut model = None;
    for (index, (response, chunk)) in responses.iter().zip(&plan.relevance).enumerate() {
        if model.is_some_and(|model| model != response.model) {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "model identity changed between focus relevance chunks",
            ));
        }
        model = Some(response.model.as_str());
        let answers = response.validate(&chunk.request)?;
        let score = match answers.get(RELEVANCE_ID) {
            Some(Answer::Score { score, .. }) => *score,
            _ => {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "focus relevance score missing",
                ));
            }
        };
        ranking.push((index, score));
    }
    ranking.sort_by(|(a, pa), (b, pb)| pb.total_cmp(pa).then_with(|| a.cmp(b)));
    let mut selected = ranking
        .into_iter()
        .take(plan.k.get())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    selected.sort_unstable();
    let mut ranges: Vec<Range> = Vec::new();
    for index in &selected {
        let source = &plan
            .relevance
            .get(*index)
            .ok_or_else(|| Error::new(ErrorKind::Input, "focus chunk index out of range"))?
            .range;
        if let Some(previous) = ranges.last_mut()
            && previous.byte_end >= source.byte_start
        {
            previous.byte_end = previous.byte_end.max(source.byte_end);
            previous.token_end = previous.token_end.max(source.token_end);
            previous.line_end = previous.line_end.max(source.line_end);
        } else {
            ranges.push(Range {
                byte_start: source.byte_start,
                byte_end: source.byte_end,
                token_start: source.token_start,
                token_end: source.token_end,
                line_start: source.line_start,
                line_end: source.line_end,
            });
        }
    }
    let fragments = ranges
        .iter()
        .map(|range| {
            plan.text
                .get(range.byte_start..range.byte_end)
                .ok_or_else(|| Error::new(ErrorKind::Input, "invalid UTF-8 focus evidence range"))
        })
        .collect::<Result<Vec<_>>>()?;
    let separator = if fragments.len() > 1 { SEPARATOR } else { "" };
    let evidence = fragments.join(separator);
    let lossy = ranges
        .iter()
        .map(|range| range.byte_end - range.byte_start)
        .sum::<usize>()
        < plan.text.len();
    let mut request = plan.final_template.clone();
    replace(&mut request, &plan.path, evidence)?;
    limits.validate_request(&request)?;
    let counts = encoder.encode(&request)?.1;
    counts.preflight(limits).map_err(|error|Error::new(error.kind,format!("focus selected evidence does not fit; reduce --focus K. Final request was not submitted: {error}")))?;
    Ok(Selection {
        selected_chunks: selected.into_iter().map(|index| index + 1).collect(),
        ranges,
        separator,
        lossy,
        request,
        counts,
    })
}

fn replace(request: &mut Request, path: &str, text: String) -> Result<()> {
    let target = request
        .state
        .pointer_mut(path)
        .ok_or_else(|| Error::new(ErrorKind::Input, "focus target disappeared"))?;
    *target = Value::String(text);
    Ok(())
}

/// Merge already selected one-unit window states and question bindings.
pub fn merge(requests: &[&Request]) -> Result<Request> {
    let first = requests
        .first()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "cannot merge an empty focus window"))?;
    let mut units = serde_json::Map::new();
    let mut questions = IndexMap::new();
    for request in requests {
        if request.model != first.model {
            return Err(Error::new(
                ErrorKind::Usage,
                "focus window request models differ",
            ));
        }
        let states = request
            .state
            .get("units")
            .and_then(Value::as_object)
            .filter(|states| states.len() == 1)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Input,
                    "focus merge requires one-unit window states",
                )
            })?;
        for (id, state) in states {
            if units.insert(id.clone(), state.clone()).is_some() {
                return Err(Error::new(
                    ErrorKind::Input,
                    "focus window source ID collision",
                ));
            }
        }
        for (id, question) in &request.questions {
            if questions.insert(id.clone(), question.clone()).is_some() {
                return Err(Error::new(
                    ErrorKind::Input,
                    "focus window question ID collision",
                ));
            }
        }
    }
    Ok(Request {
        model: first.model.clone(),
        state: json!({"units":units}),
        questions,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::config::{Config, Limits, Settings};
    use crate::encoder::ClefEncoder;
    use crate::focus::{Plan, RELEVANCE_ID, SEPARATOR, merge, plan, select};
    use crate::split;
    use crate::wire::{Question, Request, Response, Usage};
    use indexmap::IndexMap;
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
    fn request(text: &str) -> Request {
        Request {
            state: json!(text),
            model: "clef-flash".to_owned(),
            questions: IndexMap::from([(
                "present".to_owned(),
                Question::Noul {
                    instructions: Some(json!("Is checkout down?")),
                    criteria: None,
                },
            )]),
        }
    }
    fn limits() -> Limits {
        Limits {
            max_length: 1400,
            max_state_tokens: 1100,
            ..Limits::default()
        }
    }
    // Mathematical unit inputs for deterministic score-ranking and range tests.
    fn scores(plan: &Plan, levels: &[usize]) -> Vec<Response> {
        assert_eq!(levels.len(), plan.relevance.len());
        plan.relevance.iter().zip(levels).map(|(chunk,level)| {
            let legend=chunk.request.questions[RELEVANCE_ID].options().into_iter().collect::<std::collections::BTreeMap<_,_>>();
            Response {model:"verified-model".to_owned(),answers:IndexMap::from([(RELEVANCE_ID.to_owned(),json!({"type":"score","score":level,"legend":legend,"confidence":1.0,"probabilities":{"0":u8::from(*level==0),"1":u8::from(*level==1),"2":u8::from(*level==2)}}))]),usage:Usage {input_tokens:chunk.counts.input_tokens as u64,output_tokens:0}}
        }).collect()
    }
    fn planned(encoder: &ClefEncoder, k: usize) -> Plan {
        plan(
            encoder,
            &request(&" event".repeat(3500)),
            "",
            NonZeroUsize::new(k).unwrap(),
            &limits(),
            &split::Options::default(),
        )
        .unwrap()
    }
    fn larger() -> Limits {
        Limits {
            max_length: 5000,
            max_state_tokens: 4500,
            ..limits()
        }
    }

    #[test]
    fn plans_are_conditional_and_fit_or_empty_text_still_has_two_distinct_requests() {
        let encoder = encoder();
        for text in ["", "東京🙂 direct evidence"] {
            let original = request(text);
            let plan = plan(
                &encoder,
                &original,
                "",
                NonZeroUsize::new(3).unwrap(),
                &limits(),
                &split::Options::default(),
            )
            .unwrap();
            assert_eq!(plan.relevance.len(), 1);
            assert_eq!(plan.final_template.state, "");
            assert!(plan.final_status.contains("conditional"));
            assert_eq!(plan.relevance[0].request.questions.len(), 1);
            assert!(
                plan.relevance[0]
                    .request
                    .questions
                    .contains_key(RELEVANCE_ID)
            );
            let selection = select(&encoder, &plan, &scores(&plan, &[2]), &limits()).unwrap();
            assert_eq!(selection.selected_chunks, vec![1]);
            assert_eq!(selection.request.state, original.state);
            assert!(!selection.lossy);
            assert_eq!(selection.separator, "");
            assert!(
                selection.request.questions["present"]
                    .instructions()
                    .unwrap()["scope"]["operation"]
                    == "focus"
            );
            assert_eq!(
                selection.counts.input_tokens,
                encoder.encode(&selection.request).unwrap().1.input_tokens
            );
        }
    }

    #[test]
    fn relevance_selects_the_best_excerpt_and_preserves_original_ids_and_context() {
        let encoder = encoder();
        let mut original = request(&" event".repeat(3500));
        let text = original.state.clone();
        original.state = json!({"unit":text,"reference":{"stable":18446744073709551615u64}});
        let plan = plan(
            &encoder,
            &original,
            "/unit",
            NonZeroUsize::new(1).unwrap(),
            &limits(),
            &split::Options::default(),
        )
        .unwrap();
        let mut levels = vec![0; plan.relevance.len()];
        *levels.last_mut().unwrap() = 2;
        let selection = select(&encoder, &plan, &scores(&plan, &levels), &limits()).unwrap();
        assert_eq!(selection.selected_chunks, vec![levels.len()]);
        assert_eq!(
            selection.request.state["unit"],
            plan.relevance.last().unwrap().request.state["unit"]
        );
        assert_eq!(
            selection.request.state["reference"],
            original.state["reference"]
        );
        assert_eq!(
            selection.request.questions.keys().collect::<Vec<_>>(),
            vec!["present"]
        );
        assert!(selection.lossy);
    }

    #[test]
    fn ties_preserve_source_order_and_overlapping_chunks_merge_without_duplicate_text() {
        let encoder = encoder();
        let plan = planned(&encoder, 2);
        let mut levels = vec![0; plan.relevance.len()];
        levels[0] = 2;
        levels[1] = 2;
        let selection = select(&encoder, &plan, &scores(&plan, &levels), &larger()).unwrap();
        assert_eq!(selection.selected_chunks, vec![1, 2]);
        assert_eq!(selection.ranges.len(), 1);
        assert_eq!(
            selection.ranges[0].byte_end,
            plan.relevance[1].range.byte_end
        );
        assert_eq!(
            selection.request.state,
            json!(&plan.text[..selection.ranges[0].byte_end])
        );
        assert_eq!(selection.separator, "");
    }

    #[test]
    fn noncontiguous_selection_restores_document_order_and_marks_omitted_evidence() {
        let encoder = encoder();
        let plan = planned(&encoder, 2);
        let mut levels = vec![0; plan.relevance.len()];
        levels[0] = 1;
        *levels.last_mut().unwrap() = 2;
        let selection = select(&encoder, &plan, &scores(&plan, &levels), &larger()).unwrap();
        assert_eq!(selection.selected_chunks, vec![1, levels.len()]);
        assert_eq!(selection.ranges.len(), 2);
        assert_eq!(selection.separator, SEPARATOR);
        let reconstructed = selection
            .ranges
            .iter()
            .map(|range| &plan.text[range.byte_start..range.byte_end])
            .collect::<Vec<_>>()
            .join(SEPARATOR);
        assert_eq!(selection.request.state, reconstructed);
        assert!(selection.lossy);
    }

    #[test]
    fn final_evidence_is_checked_instead_of_reducing_the_requested_selection() {
        let encoder = encoder();
        let plan = planned(&encoder, 64);
        let responses = scores(&plan, &vec![2; plan.relevance.len()]);
        let error = select(&encoder, &plan, &responses, &limits()).unwrap_err();
        assert_eq!(error.exit_code(), 65);
        assert!(
            error
                .to_string()
                .contains("Final request was not submitted")
        );
        let complete = select(&encoder, &plan, &responses, &larger()).unwrap();
        assert!(!complete.lossy);
        assert_eq!(complete.request.state, plan.text);
    }

    #[test]
    fn malformed_identity_responses_and_terminal_schema_floor_caps_are_errors() {
        let encoder = encoder();
        let plan = planned(&encoder, 1);
        let mut responses = scores(&plan, &vec![0; plan.relevance.len()]);
        responses[1].model = "different-model".to_owned();
        assert_eq!(
            select(&encoder, &plan, &responses, &limits())
                .unwrap_err()
                .exit_code(),
            69
        );
        assert_eq!(
            select(&encoder, &plan, &[], &limits())
                .unwrap_err()
                .exit_code(),
            69
        );
        responses = scores(&plan, &vec![0; plan.relevance.len()]);
        responses[0].answers.clear();
        assert_eq!(
            select(&encoder, &plan, &responses, &limits())
                .unwrap_err()
                .exit_code(),
            69
        );
        for (limits, options) in [
            (
                Limits {
                    max_length: 1,
                    ..limits()
                },
                split::Options::default(),
            ),
            (
                Limits {
                    max_state_tokens: 400,
                    ..limits()
                },
                split::Options::default(),
            ),
            (
                limits(),
                split::Options {
                    max_chunks: 1,
                    ..split::Options::default()
                },
            ),
        ] {
            assert_eq!(
                crate::focus::plan(
                    &encoder,
                    &request(&" event".repeat(3500)),
                    "",
                    NonZeroUsize::new(1).unwrap(),
                    &limits,
                    &options
                )
                .unwrap_err()
                .exit_code(),
                65
            );
        }
        let mut structured = request("text");
        structured.state = json!({"message":"text"});
        assert_eq!(
            crate::focus::plan(
                &encoder,
                &structured,
                "",
                NonZeroUsize::new(1).unwrap(),
                &limits(),
                &split::Options::default()
            )
            .unwrap_err()
            .exit_code(),
            64
        );
    }

    #[test]
    fn window_merging_retains_source_states_question_order_and_detects_collisions() {
        let mut first = request("one");
        first.state = json!({"units":{"L7":{"unit":"one","previous_lines":["context"]}}});
        first.questions = IndexMap::from([("L7".to_owned(), first.questions["present"].clone())]);
        let mut second = first.clone();
        second.state = json!({"units":{"L9":"two"}});
        second.questions = IndexMap::from([("L9".to_owned(), second.questions["L7"].clone())]);
        let merged = merge(&[&first, &second]).unwrap();
        assert_eq!(
            merged.questions.keys().collect::<Vec<_>>(),
            vec!["L7", "L9"]
        );
        assert_eq!(merged.state["units"]["L7"], first.state["units"]["L7"]);
        assert_eq!(merged.state["units"]["L9"], "two");
        assert!(merge(&[&first, &first]).is_err());
        assert!(merge(&[]).is_err());
        second.model = "different".to_owned();
        assert!(merge(&[&first, &second]).is_err());
        let _: Value = serde_json::to_value(merged).unwrap();
    }
}
