use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;

use crate::config::Limits;
use crate::encoder::{ClefEncoder, Counts};
use crate::error::{Error, ErrorKind, Result};
use crate::wire::Request;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Head,
    Tail,
    Middle,
}
impl FromStr for Mode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "head" => Ok(Self::Head),
            "tail" => Ok(Self::Tail),
            "middle" => Ok(Self::Middle),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "truncate mode must be head, tail or middle",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub byte_start: usize,
    pub byte_end: usize,
    pub token_start: usize,
    pub token_end: usize,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub operation: &'static str,
    pub mode: Mode,
    pub path: String,
    pub coordinate_space: &'static str,
    pub original_counts: Counts,
    pub retained: Vec<Range>,
    pub separator: &'static str,
}
#[derive(Debug)]
pub struct Planned {
    pub request: Request,
    pub counts: Counts,
    pub view: View,
}

/// Head/tail retain that end. Middle removes the middle and separates the ends.
pub fn plan(
    encoder: &ClefEncoder,
    template: &Request,
    path: &str,
    mode: Mode,
    limits: &Limits,
) -> Result<Planned> {
    limits.validate_request(template)?;
    let text = template
        .state
        .pointer(path)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "truncate target must be an existing string field",
            )
        })?;
    let original_counts = encoder.encode(template)?.1;
    let cuts = encoder.text_cuts(text)?;
    let total = cuts.len().saturating_sub(1);
    let make = |keep: usize| -> Result<(Request, Counts, Vec<Range>, &'static str)> {
        let mut retained = Vec::new();
        let mut value = String::new();
        let mut separator = "";
        let mut add = |start: usize, end: usize| -> Result<()> {
            let start = cuts
                .get(start)
                .ok_or_else(|| Error::new(ErrorKind::Input, "truncate start out of range"))?;
            let end = cuts
                .get(end)
                .ok_or_else(|| Error::new(ErrorKind::Input, "truncate end out of range"))?;
            value.push_str(
                text.get(start.byte..end.byte)
                    .ok_or_else(|| Error::new(ErrorKind::Input, "invalid UTF-8 truncate range"))?,
            );
            if start.byte < end.byte {
                retained.push(Range {
                    byte_start: start.byte,
                    byte_end: end.byte,
                    token_start: start.token,
                    token_end: end.token,
                });
            }
            Ok(())
        };
        match mode {
            Mode::Head => add(0, keep)?,
            Mode::Tail => add(total - keep, total)?,
            Mode::Middle if keep == total => add(0, total)?,
            Mode::Middle => {
                add(0, keep.div_ceil(2))?;
                if keep >= 2 {
                    separator = "\n[…]\n";
                    value.push_str(separator);
                    let start = cuts
                        .get(total - keep / 2)
                        .ok_or_else(|| Error::new(ErrorKind::Input, "truncate tail missing"))?;
                    let end = cuts
                        .last()
                        .ok_or_else(|| Error::new(ErrorKind::Input, "truncate end missing"))?;
                    value.push_str(text.get(start.byte..end.byte).ok_or_else(|| {
                        Error::new(ErrorKind::Input, "invalid UTF-8 truncate tail")
                    })?);
                    retained.push(Range {
                        byte_start: start.byte,
                        byte_end: end.byte,
                        token_start: start.token,
                        token_end: end.token,
                    });
                }
            }
        }
        let mut request = template.clone();
        *request
            .state
            .pointer_mut(path)
            .ok_or_else(|| Error::new(ErrorKind::Input, "truncate field disappeared"))? =
            Value::String(value);
        let counts = encoder.encode(&request)?.1;
        Ok((request, counts, retained, separator))
    };
    let mut best = total;
    if original_counts.preflight(limits).is_err() {
        make(0)?.1.preflight(limits).map_err(|error| {
            Error::new(
                error.kind,
                format!("schema/other fields do not fit with an empty truncate target: {error}"),
            )
        })?;
        let mut low = 0;
        let mut high = total;
        best = 0;
        while low <= high {
            let middle = low + (high - low) / 2;
            let (_, counts, _, _) = make(middle)?;
            if counts.preflight(limits).is_ok() {
                best = middle;
                low = middle + 1;
            } else if middle == 0 {
                break;
            } else {
                high = middle - 1;
            }
        }
    }
    let (request, counts, retained, separator) = make(best)?;
    counts.preflight(limits)?;
    Ok(Planned {
        request,
        counts,
        view: View {
            operation: "truncate",
            mode,
            path: path.to_owned(),
            coordinate_space: "original decoded target string UTF-8 bytes and tokens",
            original_counts,
            retained,
            separator,
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::config::{Config, Limits, Settings};
    use crate::encoder::ClefEncoder;
    use crate::truncate::{Mode, plan};
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
            state: json!({"body":{"message":text,"number":18446744073709551615u64},"other":"untouched"}),
            model: "clef-flash".to_owned(),
            questions: IndexMap::from([(
                "decision".to_owned(),
                Question::Noul {
                    instructions: None,
                    criteria: None,
                },
            )]),
        }
    }
    fn limits() -> Limits {
        Limits {
            max_length: 700,
            max_state_tokens: 500,
            ..Limits::default()
        }
    }
    fn verify(encoder: &ClefEncoder, text: &str, mode: Mode) {
        let template = request(text);
        let planned = plan(encoder, &template, "/body/message", mode, &limits()).unwrap();
        planned.counts.preflight(&limits()).unwrap();
        assert_eq!(
            planned.counts.input_tokens,
            encoder.encode(&planned.request).unwrap().1.input_tokens
        );
        assert_eq!(
            planned.request.state["body"]["number"],
            template.state["body"]["number"]
        );
        assert_eq!(planned.request.state["other"], template.state["other"]);
        let ranges = &planned.view.retained;
        let reconstructed = ranges
            .iter()
            .map(|range| {
                assert!(
                    text.is_char_boundary(range.byte_start)
                        && text.is_char_boundary(range.byte_end)
                );
                &text[range.byte_start..range.byte_end]
            })
            .collect::<Vec<_>>()
            .join(planned.view.separator);
        assert_eq!(planned.request.state["body"]["message"], reconstructed);
        assert!(planned.view.original_counts.input_tokens >= planned.counts.input_tokens);
        if !ranges.is_empty() {
            match mode {
                Mode::Head => assert_eq!(ranges[0].byte_start, 0),
                Mode::Tail => assert_eq!(ranges.last().unwrap().byte_end, text.len()),
                Mode::Middle => {
                    assert_eq!(ranges[0].byte_start, 0);
                    assert_eq!(ranges.last().unwrap().byte_end, text.len());
                }
            }
        }
    }

    #[test]
    fn all_views_preserve_typed_fields_and_reconstruct_unicode_source_ranges() {
        let encoder = encoder();
        let text = "東京🙂 café event.\n".repeat(400);
        for mode in [Mode::Head, Mode::Tail, Mode::Middle] {
            verify(&encoder, &text, mode);
        }
        let view = plan(
            &encoder,
            &request(&text),
            "/body/message",
            Mode::Middle,
            &limits(),
        )
        .unwrap();
        assert_eq!(view.view.retained.len(), 2);
        assert_eq!(view.view.separator, "\n[…]\n");
        assert!(view.view.retained[0].byte_end < view.view.retained[1].byte_start);
    }

    #[test]
    fn fit_targets_and_irreducible_other_fields_have_explicit_contracts() {
        let encoder = encoder();
        let template = request("short");
        for mode in [Mode::Head, Mode::Tail, Mode::Middle] {
            let planned = plan(&encoder, &template, "/body/message", mode, &limits()).unwrap();
            assert_eq!(planned.request.state, template.state);
            assert_eq!(planned.view.separator, "");
        }
        for path in ["/missing", "/body/number", "/body"] {
            assert_eq!(
                plan(&encoder, &template, path, Mode::Head, &limits())
                    .unwrap_err()
                    .exit_code(),
                64
            );
        }
        let mut oversized = template.clone();
        oversized.state["other"] = json!(" event".repeat(1000));
        let error = plan(&encoder, &oversized, "/body/message", Mode::Head, &limits()).unwrap_err();
        assert_eq!(error.exit_code(), 65);
        assert!(error.to_string().contains("other fields"));
        assert_eq!("invalid".parse::<Mode>().unwrap_err().exit_code(), 64);
    }

    #[test]
    fn middle_keeps_a_unicode_target_that_already_fits() {
        let encoder = encoder();
        let template = request(&"🙂".repeat(464));
        encoder
            .encode(&template)
            .unwrap()
            .1
            .preflight(&limits())
            .unwrap();
        let planned = plan(
            &encoder,
            &template,
            "/body/message",
            Mode::Middle,
            &limits(),
        )
        .unwrap();
        assert_eq!(planned.request.state, template.state);
        assert_eq!(planned.view.separator, "");
    }

    #[test]
    fn generated_unicode_views_fit_and_retain_only_original_spans() {
        let encoder = encoder();
        let strategy = (
            prop::collection::vec(
                prop_oneof![
                    Just('🙂'),
                    Just('東'),
                    Just('é'),
                    Just(' '),
                    Just('\n'),
                    Just('a')
                ],
                4..32,
            ),
            100usize..200,
        );
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 12,
            ..ProptestConfig::default()
        });
        runner
            .run(&strategy, |(chars, repeats)| {
                let text = chars.into_iter().collect::<String>().repeat(repeats);
                for mode in [Mode::Head, Mode::Tail, Mode::Middle] {
                    verify(&encoder, &text, mode);
                }
                Ok(())
            })
            .unwrap();
    }
}
