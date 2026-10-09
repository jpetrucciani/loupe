use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::wire::Question;

pub const BUILTINS: &[&str] = &[
    "real-failure",
    "severity",
    "secret",
    "pii",
    "breaking-change",
    "conventional-commit",
    "semver-bump",
    "actionable",
    "urgent",
    "sentiment",
    "toxicity",
    "question",
    "spam",
    "language",
    "code-lang",
    "dupe-of",
    "relevant",
    "answers",
];

pub fn builtin(name: &str) -> Result<Value> {
    let value = match name {
        "real-failure" => {
            json!({"type":"noul","instructions":"Does this describe an actual unresolved failure, rather than an expected event, successful retry, recovered error or informational message?"})
        }
        "secret" => {
            json!({"type":"noul","instructions":"Does this contain an actual credential, secret key, access token or credential-bearing connection string? Exclude placeholders and public identifiers."})
        }
        "pii" => {
            json!({"type":"noul","instructions":"Does this contain personal data identifying a person, including contact information, addresses or identity numbers?"})
        }
        "breaking-change" => {
            json!({"type":"noul","instructions":"Does this diff change a public interface incompatibly for existing consumers?"})
        }
        "urgent" => {
            json!({"type":"noul","instructions":"Does this explicitly require time-sensitive action?"})
        }
        "question" => {
            json!({"type":"noul","instructions":"Is this asking for something or requiring a reply?"})
        }
        "spam" => {
            json!({"type":"noul","instructions":"Is this unsolicited bulk, promotional or spam content?"})
        }
        "severity" => {
            json!({"type":"choice","instructions":"How should an on-call engineer treat this event?","criteria":{"page":"Service down, data loss or user impact; act now","ticket":"Real unresolved defect, but not urgent","noise":"Expected, transient, recovered, successfully retried or informational"}})
        }
        "conventional-commit" => {
            json!({"type":"choice","instructions":"Classify this commit subject using conventional commit types.","criteria":{"feat":"New feature","fix":"Bug fix","perf":"Performance improvement","refactor":"Internal restructuring","docs":"Documentation","test":"Tests","build":"Build tooling or dependencies","ci":"Continuous integration","chore":"Other maintenance"}})
        }
        "semver-bump" => {
            json!({"type":"choice","instructions":"Which semantic version bump does this change require?","criteria":{"major":"Incompatible public interface change","minor":"Compatible new functionality","patch":"Compatible bug fix","none":"No release-impacting change"}})
        }
        "actionable" => {
            json!({"type":"score","instructions":"How actionable is this review comment?","criteria":["No concrete action","Some useful suggestion, vague","Clear concrete change to make"]})
        }
        "sentiment" => {
            json!({"type":"score","instructions":"Rate the sentiment of this text.","criteria":["Very negative","Negative","Neutral","Positive","Very positive"]})
        }
        "toxicity" => {
            json!({"type":"score","instructions":"Rate abusive or toxic content.","criteria":["None","Mild","Moderate","Severe"]})
        }
        "language" => {
            json!({"type":"choice","instructions":"Which natural language is used?","criteria":{"en":"English","es":"Spanish","fr":"French","de":"German","pt":"Portuguese","zh":"Chinese","ja":"Japanese","other":"Another language or insufficient evidence"}})
        }
        "code-lang" => {
            json!({"type":"choice","instructions":"Which programming or configuration language is this?","criteria":{"rust":"Rust","python":"Python","javascript":"JavaScript","typescript":"TypeScript","go":"Go","nix":"Nix","shell":"Shell","json":"JSON","toml":"TOML","yaml":"YAML","other":"Other or not code"}})
        }
        "dupe-of" => {
            json!({"type":"noul","instructions":"Does this describe the same issue or entity as {{arg.ref}}?"})
        }
        "relevant" => {
            json!({"type":"score","instructions":"How relevant is this to {{arg.query}}?","criteria":["Unrelated","Somewhat related","Directly relevant"]})
        }
        "answers" => json!({"type":"noul","instructions":"Does this unit answer {{arg.query}}?"}),
        _ => {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("unknown built-in preset {name}"),
            ));
        }
    };
    Ok(value)
}

pub fn read_document(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path)
        .map_err(|error| Error::io(&format!("reading question file {}", path.display()), error))?;
    parse_document(path, &bytes)
}

pub fn parse_document(path: &Path, bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| Error::new(ErrorKind::Usage, format!("{}: {error}", path.display())))?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        serde_json::from_str(text)
            .map_err(|error| Error::new(ErrorKind::Usage, format!("{}: {error}", path.display())))
    } else {
        let value: toml::Value = toml::from_str(text).map_err(|error| {
            Error::new(ErrorKind::Usage, format!("{}: {error}", path.display()))
        })?;
        toml_to_json(value)
    }
}

fn toml_to_json(value: toml::Value) -> Result<Value> {
    match value {
        toml::Value::String(value) => Ok(Value::String(value)),
        toml::Value::Integer(value) => Ok(Value::Number(value.into())),
        toml::Value::Float(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Usage,
                    "preset values cannot contain NaN or infinity",
                )
            }),
        toml::Value::Boolean(value) => Ok(Value::Bool(value)),
        toml::Value::Datetime(value) => Ok(Value::String(value.to_string())),
        toml::Value::Array(values) => values
            .into_iter()
            .map(toml_to_json)
            .collect::<Result<_>>()
            .map(Value::Array),
        toml::Value::Table(values) => values
            .into_iter()
            .map(|(key, value)| Ok((key, toml_to_json(value)?)))
            .collect::<Result<_>>()
            .map(Value::Object),
    }
}

pub fn to_toml(value: &Value) -> Result<String> {
    fn convert(value: &Value) -> Result<toml::Value> {
        match value {
            Value::Null => Err(Error::new(
                ErrorKind::Usage,
                "TOML cannot represent null; use a JSON preset",
            )),
            Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
            Value::String(value) => Ok(toml::Value::String(value.clone())),
            Value::Number(value) => {
                if value.to_string().contains(['.', 'e', 'E']) {
                    value
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .map(toml::Value::Float)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorKind::Usage,
                                "preset float cannot be represented in TOML",
                            )
                        })
                } else {
                    value.as_i64().map(toml::Value::Integer).ok_or_else(|| {
                        Error::new(
                            ErrorKind::Usage,
                            "TOML integers require signed 64-bit values; use a JSON preset",
                        )
                    })
                }
            }
            Value::Array(values) => values
                .iter()
                .map(convert)
                .collect::<Result<_>>()
                .map(toml::Value::Array),
            Value::Object(values) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), convert(value)?)))
                .collect::<Result<_>>()
                .map(toml::Value::Table),
        }
    }
    toml::to_string_pretty(&convert(value)?)
        .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))
}

pub fn question(document: &Value) -> Result<Question> {
    let object = document
        .as_object()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "preset must be an object"))?;
    let fields = ["type", "instructions", "criteria"]
        .into_iter()
        .filter_map(|field| {
            object
                .get(field)
                .map(|value| (field.to_owned(), value.clone()))
        })
        .collect();
    let question: Question = serde_json::from_value(Value::Object(fields))
        .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
    question.validate()?;
    Ok(question)
}

pub fn add_context(questions: &mut indexmap::IndexMap<String, Question>, context: Value) {
    for (id, question) in questions {
        let instructions = match question {
            Question::Noul { instructions, .. }
            | Question::Choice { instructions, .. }
            | Question::Score { instructions, .. } => instructions,
        };
        *instructions = Some(
            json!({"question":instructions.clone().unwrap_or_else(||json!(id)),"context":context}),
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn toml_preserves_numbers_and_dates_without_json_private_wrappers() {
        use crate::presets::{parse_document, to_toml};
        let original = serde_json::json!({"version":1,"float":1e16,"negative_zero":-0.0,"nested":[2,0.5,"東京"]});
        let text = to_toml(&original).unwrap();
        let roundtrip =
            parse_document(std::path::Path::new("preset.toml"), text.as_bytes()).unwrap();
        assert_eq!(original, roundtrip);
        assert_eq!(roundtrip["version"].as_u64(), Some(1));
        assert!(
            roundtrip["negative_zero"]
                .as_f64()
                .unwrap()
                .is_sign_negative()
        );
        let dated = parse_document(
            std::path::Path::new("preset.toml"),
            b"date=2026-10-05T12:34:56Z",
        )
        .unwrap();
        assert_eq!(dated["date"], "2026-10-05T12:34:56Z");
        let oversized: serde_json::Value =
            serde_json::from_str("{\"large\":1267650600228229401496703205376}").unwrap();
        assert!(to_toml(&oversized).is_err());
        for text in [b"n=nan".as_slice(), b"n=inf", b"n=-inf"] {
            assert!(parse_document(std::path::Path::new("preset.toml"), text).is_err());
        }
    }
    #[test]
    fn every_builtin_has_a_valid_question_and_initial_score_rubric() {
        for name in crate::presets::BUILTINS {
            let document = crate::presets::builtin(name).unwrap();
            let question = crate::presets::question(&document).unwrap();
            if let crate::wire::Question::Score { criteria, .. } = question {
                assert!((2..=10).contains(&criteria.len()));
            }
        }
    }
}
