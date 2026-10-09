use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokenizers::Tokenizer;

use crate::config::{Limits, Profile};
use crate::error::{Error, ErrorKind, Result};
use crate::wire::Request;

pub const VERSION: &str = "clef-v1@17f0b0ad64efb65d273590632833508766b2aae6";
pub const SYSTEM_PROMPT: &str = "Read the complete state and schema. Decide every field jointly. Each answer must be exactly one of that field's allowed options.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EncodedQuestion {
    pub question_id: String,
    pub question_type: u8,
    pub question_span: (usize, usize),
    pub option_spans: Vec<(usize, usize)>,
    pub option_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EncodedRecord {
    pub input_ids: Vec<u32>,
    pub questions: Vec<EncodedQuestion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub state_tokens: usize,
    pub question_tokens: usize,
    pub input_tokens: usize,
    pub exact: bool,
}

impl Counts {
    pub fn preflight(&self, limits: &Limits) -> Result<()> {
        if self.input_tokens > limits.max_length || self.state_tokens > limits.max_state_tokens {
            return Err(Error::new(
                ErrorKind::Input,
                format!(
                    "input exceeds token limits: state={}, schema/fixed={}, total={} ({}); max_length={}, max_state_tokens={}. Splitting requires an explicit combine mode",
                    self.state_tokens,
                    self.question_tokens,
                    self.input_tokens,
                    if self.exact { "exact" } else { "estimate" },
                    limits.max_length,
                    limits.max_state_tokens
                ),
            ));
        }
        Ok(())
    }
}

pub struct ClefEncoder {
    tokenizer: Tokenizer,
    pub identity: String,
}

impl ClefEncoder {
    pub fn has_verbatim_boundary(&self, marker: &str) -> bool {
        if self.tokenizer.get_encode_special_tokens() {
            return false;
        }
        let tokens = self.tokenizer.get_added_tokens_decoder();
        tokens.values().any(|token| {
            token.content == marker
                && !token.normalized
                && !token.single_word
                && !token.lstrip
                && !token.rstrip
        }) && !tokens
            .values()
            .any(|token| token.content != marker && token.content.contains(marker))
    }

    pub fn text_offsets(&self, text: &str) -> Result<Vec<(usize, usize)>> {
        self.tokenizer
            .encode(text, false)
            .map(|encoded| encoded.get_offsets().to_vec())
            .map_err(|error| {
                Error::new(
                    ErrorKind::Input,
                    format!("tokenizing text offsets: {error}"),
                )
            })
    }

    pub fn text_tokens(&self, text: &str) -> Result<usize> {
        self.tokenizer
            .encode(text, false)
            .map(|encoded| encoded.len())
            .map_err(|error| Error::new(ErrorKind::Input, format!("tokenizing text: {error}")))
    }

    pub(crate) fn text_cuts(&self, text: &str) -> Result<Vec<TextCut>> {
        let offsets = self.text_offsets(text)?;
        let mut cuts = vec![TextCut { byte: 0, token: 0 }];
        for (index, (start, end)) in offsets.iter().enumerate() {
            if start > end || *end > text.len() {
                return Err(Error::new(
                    ErrorKind::Input,
                    "token offset exceeds UTF-8 input",
                ));
            }
            // Multiple byte tokens can share one character's source span.
            if offsets
                .get(index + 1)
                .is_some_and(|(next_start, next_end)| next_end == end || next_start < end)
            {
                continue;
            }
            if text.is_char_boundary(*end) && cuts.last().is_none_or(|cut| cut.byte < *end) {
                cuts.push(TextCut {
                    byte: *end,
                    token: index + 1,
                });
            }
        }
        if cuts.last().is_none_or(|cut| cut.byte != text.len()) {
            cuts.push(TextCut {
                byte: text.len(),
                token: offsets.len(),
            });
        }
        Ok(cuts)
    }
    pub fn load(path: &Path, revision: &str) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|error| Error::io(&format!("reading tokenizer {}", path.display()), error))?;
        let hash = blake3::hash(&bytes);
        if revision == crate::config::CLEF_REVISION
            && hash.to_hex().as_str()
                != "16066ad27076349236a282e9351f8bab5aa9b33803f12f7ab9dca401909d7273"
        {
            return Err(Error::new(
                ErrorKind::Config,
                "tokenizer content does not match the pinned Clef release",
            ));
        }
        let tokenizer = Tokenizer::from_bytes(&bytes).map_err(|error| {
            Error::new(ErrorKind::Config, format!("invalid tokenizer: {error}"))
        })?;
        Ok(Self {
            tokenizer,
            identity: format!("{VERSION};tokenizer={revision};blake3={}", hash),
        })
    }

    fn tokens(&self, fragment: &str) -> Result<Vec<u32>> {
        self.tokenizer
            .encode(fragment, false)
            .map(|encoded| encoded.get_ids().to_vec())
            .map_err(|error| Error::new(ErrorKind::Input, format!("tokenizing fragment: {error}")))
    }

    pub fn encode(&self, request: &Request) -> Result<(EncodedRecord, Counts)> {
        request.validate()?;
        let mut schema = self.tokens("\n\nSCHEMA FIELDS:\n")?;
        let mut questions = Vec::new();
        for (index, (id, question)) in request.questions.iter().enumerate() {
            schema.extend(self.tokens(&format!(
                "\nFIELD {}\nID: {id}\nTYPE: {}\nINSTRUCTION: ",
                index + 1,
                question.type_name()
            ))?);
            let start = schema.len();
            let instructions = match question.instructions() {
                None | Some(Value::Null) => id.clone(),
                Some(Value::String(s)) if s.is_empty() => id.clone(),
                Some(value) => render(value)?,
            };
            schema.extend(self.tokens(&instructions)?);
            let question_span = (start, schema.len());
            schema.extend(self.tokens("\nALLOWED OPTIONS:\n")?);
            let mut option_spans = Vec::new();
            let mut option_ids = Vec::new();
            for (option_index, (option_id, description)) in
                question.options().into_iter().enumerate()
            {
                schema.extend(self.tokens(&format!("OPTION {}: ", option_index + 1))?);
                let start = schema.len();
                let mut semantics = serde_json::Map::new();
                semantics.insert("option_id".to_owned(), Value::String(option_id.clone()));
                if !description.is_null() {
                    semantics.insert("description".to_owned(), description);
                }
                schema.extend(self.tokens(&render(&Value::Object(semantics))?)?);
                option_spans.push((start, schema.len()));
                option_ids.push(option_id);
                schema.extend(self.tokens("\n")?);
            }
            schema.extend(self.tokens("END FIELD\n")?);
            questions.push(EncodedQuestion {
                question_id: id.clone(),
                question_type: match question.type_name() {
                    "noul" => 0,
                    "choice" => 1,
                    _ => 2,
                },
                question_span,
                option_spans,
                option_ids,
            });
        }
        let mut input_ids = self.tokens(&format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\nSTATE:\n"
        ))?;
        let state = self.tokens(&render(&request.state)?)?;
        let suffix = self.tokens(
            "\n<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\nJOINT SCHEMA DECISIONS:",
        )?;
        let state_tokens = state.len();
        let question_tokens = input_ids.len() + schema.len() + suffix.len();
        input_ids.extend(state);
        let offset = input_ids.len();
        for question in &mut questions {
            question.question_span.0 += offset;
            question.question_span.1 += offset;
            for span in &mut question.option_spans {
                span.0 += offset;
                span.1 += offset;
            }
        }
        input_ids.extend(schema);
        input_ids.extend(suffix);
        let counts = Counts {
            state_tokens,
            question_tokens,
            input_tokens: input_ids.len(),
            exact: true,
        };
        Ok((
            EncodedRecord {
                input_ids,
                questions,
            },
            counts,
        ))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TextCut {
    pub byte: usize,
    pub token: usize,
}

// Python render passes strings through. Nested strings still use JSON escaping.
pub fn render(value: &Value) -> Result<String> {
    if let Value::String(text) = value {
        return Ok(text.clone());
    }
    let mut output = String::new();
    json_render(value, &mut output)?;
    Ok(output)
}

fn json_render(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::String(text) => output.push_str(
            &serde_json::to_string(text)
                .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?,
        ),
        Value::Number(number) => {
            let spelling = number.to_string();
            if spelling.contains(['.', 'e', 'E']) {
                let value = number
                    .as_f64()
                    .ok_or_else(|| Error::new(ErrorKind::Input, "invalid float"))?;
                output.push_str(&python_float(value)?);
            } else {
                output.push_str(if spelling == "-0" { "0" } else { &spelling });
            }
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                json_render(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                json_render(&Value::String(key.clone()), output)?;
                output.push(':');
                json_render(&values[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub fn python_float(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err(Error::new(
            ErrorKind::Input,
            "NaN and infinity cannot be encoded",
        ));
    }
    if value == 0.0 {
        return Ok(if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned());
    }
    let shortest = format!("{value:?}");
    let (negative, unsigned) = if let Some(unsigned) = shortest.strip_prefix('-') {
        ("-", unsigned)
    } else {
        ("", shortest.as_str())
    };
    let (mantissa, exponent) = if let Some((mantissa, exponent)) = unsigned.split_once('e') {
        (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| Error::new(ErrorKind::Input, "invalid floating point exponent"))?,
        )
    } else {
        (unsigned, 0)
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let first = digits.find(|ch| ch != '0').unwrap_or(0);
    let scientific_exponent = exponent
        + i32::try_from(point)
            .map_err(|_| Error::new(ErrorKind::Input, "float rendering overflow"))?
        - i32::try_from(first)
            .map_err(|_| Error::new(ErrorKind::Input, "float rendering overflow"))?
        - 1;
    digits.drain(..first);
    while digits.ends_with('0') && digits.len() > 1 {
        digits.pop();
    }
    if !(-4..16).contains(&scientific_exponent) {
        let coefficient = if digits.len() == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        return Ok(format!(
            "{negative}{coefficient}e{}{:#02}",
            if scientific_exponent < 0 { '-' } else { '+' },
            scientific_exponent.abs()
        ));
    }
    let decimal = scientific_exponent + 1;
    let fixed = if decimal <= 0 {
        format!(
            "0.{}{}",
            "0".repeat(
                usize::try_from(-decimal)
                    .map_err(|_| Error::new(ErrorKind::Input, "float rendering overflow"))?
            ),
            digits
        )
    } else {
        let decimal = usize::try_from(decimal)
            .map_err(|_| Error::new(ErrorKind::Input, "float rendering overflow"))?;
        if decimal >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(decimal - digits.len()))
        } else {
            format!("{}.{}", &digits[..decimal], &digits[decimal..])
        }
    };
    Ok(format!("{negative}{fixed}"))
}

pub fn estimate(request: &Request) -> Result<Counts> {
    let state_tokens = render(&request.state)?.len().div_ceil(3);
    let question_tokens = serde_json::to_vec(&request.questions)
        .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?
        .len()
        .div_ceil(3)
        + 80;
    Ok(Counts {
        state_tokens,
        question_tokens,
        input_tokens: state_tokens + question_tokens,
        exact: false,
    })
}

pub fn tokenizer_path(profile: &Profile) -> Result<std::path::PathBuf> {
    if let Some(repository) = profile.tokenizer.strip_prefix("hf:") {
        if profile.tokenizer_revision.len() != 40
            || !profile
                .tokenizer_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || repository.split('/').any(|s| {
                s.is_empty()
                    || s == "."
                    || s == ".."
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
            })
        {
            return Err(Error::new(
                ErrorKind::Config,
                "HF tokenizer requires a safe repository ID and pinned 40-character commit revision",
            ));
        }
        Ok(profile
            .cache_dir
            .join("tokenizers")
            .join(repository.replace('/', "--"))
            .join(&profile.tokenizer_revision)
            .join("tokenizer.json"))
    } else {
        Ok(std::path::PathBuf::from(&profile.tokenizer))
    }
}

pub fn load_profile(profile: &Profile) -> Result<Option<ClefEncoder>> {
    match profile.encoder.as_str() {
        "clef-v1" => {
            let path = tokenizer_path(profile)?;
            if !path.is_file() {
                return Err(Error::new(
                    ErrorKind::Config,
                    format!(
                        "tokenizer missing at {}; run loupe doctor --prefetch to prepare offline use",
                        path.display()
                    ),
                ));
            }
            ClefEncoder::load(&path, &profile.tokenizer_revision).map(Some)
        }
        "estimate" => Ok(None),
        name => Err(Error::new(
            ErrorKind::Config,
            format!("unknown encoder {name}; choose clef-v1 or estimate"),
        )),
    }
}

pub async fn prefetch(profile: &Profile) -> Result<std::path::PathBuf> {
    let path = tokenizer_path(profile)?;
    if path.is_file() {
        ClefEncoder::load(&path, &profile.tokenizer_revision)?;
        return Ok(path);
    }
    let repository = profile
        .tokenizer
        .strip_prefix("hf:")
        .ok_or_else(|| Error::new(ErrorKind::Config, "local tokenizer path does not exist"))?;
    let (owner, name) = hf_hub::split_id(repository);
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::Config, "tokenizer path has no parent"))?;
    let client = hf_hub::HFClient::builder()
        .endpoint("https://huggingface.co")
        .token("")
        .cache_dir(profile.cache_dir.join("hf"))
        .build()
        .map_err(|_| {
            Error::new(
                ErrorKind::Unavailable,
                "could not prepare HF download client",
            )
        })?;
    client
        .model(owner, name)
        .download_file()
        .filename("tokenizer.json")
        .revision(&profile.tokenizer_revision)
        .local_dir(parent)
        .send()
        .await
        .map_err(|_| {
            Error::new(
                ErrorKind::Unavailable,
                "could not download pinned tokenizer from Hugging Face",
            )
        })?;
    ClefEncoder::load(&path, &profile.tokenizer_revision)?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::encoder::{python_float, render};
    use serde_json::json;
    #[test]
    fn python_number_boundaries_negative_zero_and_nested_unicode() {
        for (value, expected) in [
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1e-4, "0.0001"),
            (1e-5, "1e-05"),
            (-0.0, "-0.0"),
            (1.2345e20, "1.2345e+20"),
        ] {
            assert_eq!(python_float(value).unwrap(), expected);
        }
        assert!(python_float(f64::NAN).is_err());
        assert_eq!(
            render(&json!({"z": "α😀", "a": [1e16, -0.0, "\n"]})).unwrap(),
            "{\"a\":[1e+16,-0.0,\"\\n\"],\"z\":\"α😀\"}"
        );
        assert_eq!(render(&json!("plain\n")).unwrap(), "plain\n");
    }

    #[test]
    fn all_python_numeric_goldens_render_identically() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../evals/golden/clef-v1/numbers.json"))
                .unwrap();
        for case in cases {
            assert_eq!(
                render(&case["value"]).unwrap(),
                case["rendered"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn full_token_ids_head_metadata_and_untruncated_counts_match_python() {
        let path = std::env::var_os("LOUPE_TEST_TOKENIZER")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(format!(
                    ".cache/loupe/tokenizers/Cloudflare--clef-flash/{}/tokenizer.json",
                    crate::config::CLEF_REVISION
                ))
            });
        let encoder =
            crate::encoder::ClefEncoder::load(&path, crate::config::CLEF_REVISION).unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../evals/golden/clef-v1/records.json"))
                .unwrap();
        for (index, case) in cases.into_iter().enumerate() {
            let request: crate::wire::Request =
                serde_json::from_value(case["request"].clone()).unwrap();
            let expected: crate::encoder::EncodedRecord =
                serde_json::from_value(case["encoded"].clone()).unwrap();
            let (actual, counts) = encoder.encode(&request).unwrap();
            assert_eq!(actual, expected, "golden record {index}");
            assert_eq!(
                counts.state_tokens as u64,
                case["state_tokens"].as_u64().unwrap()
            );
            assert_eq!(
                counts.input_tokens as u64,
                case["input_tokens"].as_u64().unwrap()
            );
            let tiny = crate::config::Limits {
                max_length: counts.input_tokens - 1,
                ..crate::config::Limits::default()
            };
            assert_eq!(counts.preflight(&tiny).unwrap_err().exit_code(), 65);
        }
    }
}
