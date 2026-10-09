use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::config::Limits;
use crate::encoder::{ClefEncoder, Counts, TextCut};
use crate::error::{Error, ErrorKind, Result};
use crate::wire::Request;

pub const FLOOR: usize = 512;

#[derive(Clone, Debug)]
pub struct Options {
    pub overlap: usize,
    pub max_chunks: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            overlap: 256,
            max_chunks: 64,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub byte_start: usize,
    pub byte_end: usize,
    pub token_start: usize,
    pub token_end: usize,
    pub line_start: u64,
    pub line_end: u64,
    pub text_tokens: usize,
    pub unique_tokens: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Chunk {
    pub range: Range,
    pub request: Request,
    pub counts: Counts,
}

pub fn plan(
    encoder: &ClefEncoder,
    template: &Request,
    path: &str,
    limits: &Limits,
    options: &Options,
) -> Result<Vec<Chunk>> {
    if options.max_chunks == 0 {
        return Err(Error::new(ErrorKind::Usage, "max-chunks must be positive"));
    }
    limits.validate_request(template)?;
    let text = template
        .state
        .pointer(path)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "splitting requires a text subject, never a structured record",
            )
        })?;
    let cuts = encoder.text_cuts(text)?;
    if cuts.len() == 1 {
        return Err(Error::new(
            ErrorKind::Input,
            "empty text cannot resolve an oversized schema/context",
        ));
    }
    let boundaries = structures(text);
    let make = |start: TextCut, end: TextCut| -> Result<(Request, Counts, usize)> {
        let fragment = text
            .get(start.byte..end.byte)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid UTF-8 chunk range"))?;
        let mut request = template.clone();
        let target = request.state.pointer_mut(path).ok_or_else(|| {
            Error::new(ErrorKind::Input, "text field disappeared while splitting")
        })?;
        *target = Value::String(fragment.to_owned());
        let counts = encoder.encode(&request)?.1;
        Ok((request, counts, encoder.text_tokens(fragment)?))
    };
    let empty = TextCut { byte: 0, token: 0 };
    make(empty, empty)?.1.preflight(limits).map_err(|error| {
        Error::new(
            error.kind,
            format!("schema/context does not fit before splitting: {error}"),
        )
    })?;
    let mut chunks = Vec::new();
    let mut start_index = 0usize;
    let mut covered = 0usize;
    while cuts
        .get(start_index)
        .is_some_and(|cut| cut.byte < text.len())
    {
        if chunks.len() == options.max_chunks {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "split would exceed max-chunks {}; no recursive re-splits are allowed",
                    options.max_chunks
                ),
            ));
        }
        let start = *cuts
            .get(start_index)
            .ok_or_else(|| Error::new(ErrorKind::Input, "chunk start out of range"))?;
        let mut low = start_index + 1;
        let mut high = cuts.len();
        let mut selected = None;
        while low < high {
            let middle = low + (high - low) / 2;
            let end = *cuts
                .get(middle)
                .ok_or_else(|| Error::new(ErrorKind::Input, "chunk cut out of range"))?;
            let (_, counts, _) = make(start, end)?;
            if counts.preflight(limits).is_ok() {
                selected = Some(middle);
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut end_index = selected.ok_or_else(|| {
            Error::new(
                ErrorKind::Input,
                "schema/context leaves no room for a text chunk",
            )
        })?;
        let largest = *cuts
            .get(end_index)
            .ok_or_else(|| Error::new(ErrorKind::Input, "chunk end out of range"))?;
        if largest.byte != text.len() {
            for category in &boundaries {
                let candidate = cuts
                    .iter()
                    .enumerate()
                    .take(end_index + 1)
                    .skip(start_index + 1)
                    .rev()
                    .find(|(_, cut)| category.contains(&cut.byte));
                if let Some((index, cut)) = candidate {
                    let (_, counts, tokens) = make(start, *cut)?;
                    if tokens >= FLOOR && counts.preflight(limits).is_ok() {
                        end_index = index;
                        break;
                    }
                }
            }
        }
        let end = *cuts
            .get(end_index)
            .ok_or_else(|| Error::new(ErrorKind::Input, "chunk end disappeared"))?;
        let (request, counts, tokens) = make(start, end)?;
        counts.preflight(limits)?;
        if end.byte < text.len() && tokens < FLOOR {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "schema/context leaves only {tokens} text tokens; nonfinal chunks require the {FLOOR}-token floor"
                ),
            ));
        }
        let unique = end
            .token
            .checked_sub(covered)
            .filter(|unique| *unique > 0)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Input,
                    "overlap prevents forward source-token progress",
                )
            })?;
        let before = text
            .get(..start.byte)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid chunk line start"))?;
        let through = text
            .get(..end.byte)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid chunk line end"))?;
        let line_start = 1 + before.bytes().filter(|byte| *byte == b'\n').count() as u64;
        let line_end = 1 + through.bytes().filter(|byte| *byte == b'\n').count() as u64
            - u64::from(through.ends_with('\n'));
        chunks.push(Chunk {
            range: Range {
                byte_start: start.byte,
                byte_end: end.byte,
                token_start: start.token,
                token_end: end.token,
                line_start,
                line_end,
                text_tokens: tokens,
                unique_tokens: unique,
            },
            request,
            counts,
        });
        covered = end.token;
        if end.byte == text.len() {
            break;
        }
        let target = end.token.checked_sub(options.overlap).ok_or_else(|| {
            Error::new(
                ErrorKind::Input,
                "overlap is larger than the admitted chunk",
            )
        })?;
        let next = cuts
            .iter()
            .enumerate()
            .take(end_index + 1)
            .rev()
            .find(|(_, cut)| cut.token <= target)
            .map(|(index, _)| index)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Input,
                    "cannot align overlap to original token boundaries",
                )
            })?;
        if next <= start_index {
            return Err(Error::new(
                ErrorKind::Input,
                "overlap/schema combination cannot advance; reduce overlap or schema/context",
            ));
        }
        start_index = next;
    }
    Ok(chunks)
}

fn structures(text: &str) -> [BTreeSet<usize>; 3] {
    let mut paragraphs = BTreeSet::new();
    let mut lines = BTreeSet::new();
    let mut sentences = BTreeSet::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        if line.ends_with('\n') {
            lines.insert(offset);
        }
        if line.trim().is_empty() {
            paragraphs.insert(offset);
        }
    }
    for (index, ch) in text.char_indices() {
        let end = index + ch.len_utf8();
        if matches!(ch, '.' | '!' | '?')
            && text
                .get(end..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(char::is_whitespace)
        {
            sentences.insert(end);
        }
    }
    [paragraphs, lines, sentences]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::config::{Config, Limits, Settings};
    use crate::encoder::ClefEncoder;
    use crate::split::{FLOOR, Options, plan};
    use crate::wire::{Question, Request};
    use indexmap::IndexMap;
    use proptest::prelude::*;
    use serde_json::json;

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
            max_length: 1200,
            max_state_tokens: 1000,
            ..Limits::default()
        }
    }
    fn verify(
        encoder: &ClefEncoder,
        request: &Request,
        path: &str,
        limits: &Limits,
        options: &Options,
    ) {
        let text = request.state.pointer(path).unwrap().as_str().unwrap();
        let offsets = encoder.text_offsets(text).unwrap();
        let chunks = plan(encoder, request, path, limits, options).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.len() <= options.max_chunks);
        assert_eq!(chunks.first().unwrap().range.byte_start, 0);
        assert_eq!(chunks.last().unwrap().range.byte_end, text.len());
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.range.unique_tokens)
                .sum::<usize>(),
            offsets.len()
        );
        let mut covered = 0;
        for (index, chunk) in chunks.iter().enumerate() {
            let range = &chunk.range;
            assert!(
                text.is_char_boundary(range.byte_start) && text.is_char_boundary(range.byte_end)
            );
            assert_eq!(
                chunk.request.state.pointer(path).unwrap(),
                &json!(&text[range.byte_start..range.byte_end])
            );
            assert_eq!(
                chunk.counts.input_tokens,
                encoder.encode(&chunk.request).unwrap().1.input_tokens
            );
            chunk.counts.preflight(limits).unwrap();
            assert_eq!(range.unique_tokens, range.token_end - covered);
            assert_eq!(
                range.text_tokens,
                encoder
                    .text_tokens(&text[range.byte_start..range.byte_end])
                    .unwrap()
            );
            if index + 1 < chunks.len() {
                assert!(range.text_tokens >= FLOOR);
            }
            if index > 0 {
                let previous = &chunks[index - 1].range;
                assert!(range.byte_start <= previous.byte_end);
                assert!(range.byte_start > previous.byte_start);
                assert!(previous.token_end - range.token_start >= options.overlap);
            }
            covered = range.token_end;
        }
    }

    #[test]
    fn utf8_offsets_are_bytes_and_all_chunks_fit_cover_and_preserve_context() {
        let encoder = encoder();
        let text = "東京🙂 café\u{301} event. ".repeat(400);
        let offsets = encoder.text_offsets(&text).unwrap();
        assert_eq!(offsets.last().unwrap().1, text.len());
        assert!(
            offsets
                .iter()
                .all(|(start, end)| start <= end && *end <= text.len())
        );
        let mut request = request(&text);
        request.state =
            json!({"unit":text,"previous_lines":[{"id":"L8","text":"immutable context"}]});
        verify(&encoder, &request, "/unit", &limits(), &Options::default());
        for chunk in plan(&encoder, &request, "/unit", &limits(), &Options::default()).unwrap() {
            assert_eq!(
                chunk.request.state["previous_lines"],
                request.state["previous_lines"]
            );
        }
    }

    #[test]
    fn structural_cuts_prefer_paragraphs_then_lines_then_sentences() {
        let encoder = encoder();
        for separator in ["\n\n", "\n", ". "] {
            let section = " event".repeat(600) + separator;
            let text = section.repeat(4);
            let request = request(&text);
            let chunks = plan(
                &encoder,
                &request,
                "",
                &limits(),
                &Options {
                    overlap: 0,
                    ..Options::default()
                },
            )
            .unwrap();
            let first = &chunks[0];
            let end = if separator == ". " {
                section.len() - 1
            } else {
                section.len()
            };
            assert_eq!(first.range.byte_end, end, "separator={separator:?}");
            assert!(first.range.text_tokens >= FLOOR);
            assert_eq!(first.range.line_start, 1);
            if separator == "\n\n" {
                assert_eq!(first.range.line_end, 2);
            }
        }
    }

    #[test]
    fn cap_floor_overlap_schema_and_structured_terminal_failures_do_not_recurse() {
        let encoder = encoder();
        let request = request(&" event".repeat(2500));
        for (limits, options, expected) in [
            (
                limits(),
                Options {
                    max_chunks: 1,
                    ..Options::default()
                },
                "max-chunks",
            ),
            (
                Limits {
                    max_state_tokens: 400,
                    ..limits()
                },
                Options::default(),
                "floor",
            ),
            (
                limits(),
                Options {
                    overlap: 2000,
                    ..Options::default()
                },
                "overlap",
            ),
            (
                Limits {
                    max_length: 1,
                    ..limits()
                },
                Options::default(),
                "schema/context",
            ),
        ] {
            let error = plan(&encoder, &request, "", &limits, &options).unwrap_err();
            assert_eq!(error.exit_code(), 65);
            assert!(error.to_string().contains(expected), "{error}");
        }
        assert_eq!(
            plan(
                &encoder,
                &request,
                "",
                &limits(),
                &Options {
                    max_chunks: 0,
                    ..Options::default()
                }
            )
            .unwrap_err()
            .exit_code(),
            64
        );
        let mut structured = request.clone();
        structured.state = json!({"text":"event"});
        assert_eq!(
            plan(&encoder, &structured, "", &limits(), &Options::default())
                .unwrap_err()
                .exit_code(),
            64
        );
    }

    #[test]
    fn generated_unicode_text_chunks_obey_encoded_limits_and_unique_coverage() {
        let encoder = encoder();
        let strategy = (
            prop::collection::vec(
                prop_oneof![
                    Just('🙂'),
                    Just('東'),
                    Just('é'),
                    Just(' '),
                    Just('\n'),
                    Just('.'),
                    Just('a')
                ],
                1..48,
            ),
            80usize..240,
            512usize..900,
            0usize..256,
        );
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 24,
            ..ProptestConfig::default()
        });
        runner
            .run(&strategy, |(chars, repeats, max_state, overlap)| {
                let text = chars.into_iter().collect::<String>().repeat(repeats);
                let request = request(&text);
                let limits = Limits {
                    max_state_tokens: max_state,
                    max_length: max_state + 400,
                    ..Limits::default()
                };
                let options = Options {
                    overlap,
                    max_chunks: 64,
                };
                match plan(&encoder, &request, "", &limits, &options) {
                    Ok(_) => verify(&encoder, &request, "", &limits, &options),
                    Err(error) => {
                        prop_assert_eq!(error.exit_code(), 65);
                        prop_assert!(
                            error.to_string().contains("floor")
                                || error.to_string().contains("overlap")
                                || error.to_string().contains("max-chunks"),
                            "{error}"
                        );
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}
