use std::num::NonZeroUsize;

use loupe_core::calibration_pin;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::truncate::Mode;
use serde_json::Value;

use crate::args::{Classify, Eval};

pub(crate) fn evaluation(original: &Eval, document: &Value) -> Result<Eval> {
    let mut options = original.clone();
    let Some(pin) = calibration_pin::read(document)? else {
        return Ok(options);
    };
    let explicit = options.focus.is_some()
        || options.split_text
        || options.combine.is_some()
        || options.truncate.is_some()
        || options.truncate_field.is_some();
    if !explicit {
        options.focus = positive(&pin.view, "focus")?;
        options.split_text = pin.view.get("split") == Some(&Value::Bool(true));
        if let Some((mode, path)) = truncation(&pin.view)? {
            if path.is_empty() {
                options.truncate = Some(mode);
            } else {
                options.truncate_field = Some(path);
            }
        }
    }
    if options.focus.is_some() || options.split_text {
        if options.overlap.is_none() {
            options.overlap = number(&pin.view, "overlap")?;
        }
        if options.max_chunks.is_none() {
            options.max_chunks = positive(&pin.view, "max_chunks")?;
        }
    }
    if options.question_id_source.is_none() {
        options.question_id_source = pin
            .view
            .get("question_id_source")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| invalid("calibrated question ID source must be a string"))?
                    .parse()
            })
            .transpose()?;
    }
    compatible(
        options.focus.is_some(),
        options.split_text,
        options.truncate.is_some() || options.truncate_field.is_some(),
    )?;
    Ok(options)
}

pub(crate) fn options(original: &Classify, document: &Value) -> Result<Classify> {
    let mut options = original.clone();
    let Some(pin) = calibration_pin::read(document)? else {
        return Ok(options);
    };
    let explicit = options.focus.is_some()
        || options.split
        || options.combine.is_some()
        || options.truncate.is_some()
        || options.truncate_field.is_some();
    if !explicit {
        options.common.focus = positive(&pin.view, "focus")?;
        options.common.split = pin.view.get("split") == Some(&Value::Bool(true));
        if let Some((mode, path)) = truncation(&pin.view)? {
            if path.is_empty() {
                options.truncate = Some(mode);
            } else {
                options.common.truncate_field = Some(path);
            }
        }
    }
    if options.focus.is_some() || options.split || pin.view.get("split") == Some(&Value::Bool(true))
    {
        if options.overlap.is_none() {
            options.common.overlap = number(&pin.view, "overlap")?;
        }
        if options.max_chunks.is_none() {
            options.common.max_chunks = positive(&pin.view, "max_chunks")?;
        }
    }
    compatible(
        options.focus.is_some(),
        options.split,
        options.truncate.is_some() || options.truncate_field.is_some(),
    )?;
    Ok(options)
}

fn compatible(focus: bool, split: bool, truncate: bool) -> Result<()> {
    if focus && (split || truncate) || split && truncate {
        return Err(invalid(
            "calibrated view has incompatible focus/split/truncation controls",
        ));
    }
    Ok(())
}

fn truncation(view: &Value) -> Result<Option<(Mode, String)>> {
    let Some(value) = view.get("truncate").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let mode = value
        .as_str()
        .ok_or_else(|| invalid("calibrated truncate mode must be a string"))?
        .parse::<Mode>()?;
    let path = view.get("path").and_then(Value::as_str).unwrap_or("");
    if path.is_empty() {
        Ok(Some((mode, String::new())))
    } else if mode == Mode::Head {
        Ok(Some((mode, path.to_owned())))
    } else {
        Err(invalid("calibrated field truncation must retain the head"))
    }
}

fn number(view: &Value, key: &str) -> Result<Option<usize>> {
    view.get(key)
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    invalid("calibrated view count is outside the supported integer range")
                })
        })
        .transpose()
}
fn positive(view: &Value, key: &str) -> Result<Option<NonZeroUsize>> {
    number(view, key)?
        .map(|value| {
            NonZeroUsize::new(value)
                .ok_or_else(|| invalid("calibrated view count must be positive"))
        })
        .transpose()
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::args::{Cli, Command};
    use crate::run::calibrated::options;
    use clap::Parser;
    use serde_json::{Value, json};

    fn metadata(view: Value) -> Value {
        // Pure pin/grammar codec fixture, with no model or discovery claim.
        json!({"policy":{"calibration":{"schema_version":1,"model":format!("fixture/model@{};compute=bf16;quantization=nf4;server={}","1".repeat(40),"2".repeat(64)),"local_encoder":"fixture","server_encoder":"fixture","preset_version":1,"preset_blake3":"0".repeat(64),"field":"decision","view":view,"source":{"partition":"calibrate"}}}})
    }

    #[test]
    fn recorded_view_defaults_restore_focus_and_chunks_without_overriding_explicit_controls() {
        let Command::Keep(original) = Cli::try_parse_from(["loupe", "keep", "Question?"])
            .unwrap()
            .command
        else {
            panic!("keep grammar");
        };
        let document = metadata(
            json!({"declared":"Unit","split":false,"focus":3,"overlap":0,"max_chunks":7,"path":""}),
        );
        let effective = options(&original, &document).unwrap();
        assert_eq!(effective.focus.unwrap().get(), 3);
        assert_eq!(effective.overlap, Some(0));
        assert_eq!(effective.max_chunks.unwrap().get(), 7);
        let Command::Keep(original) = Cli::try_parse_from([
            "loupe",
            "keep",
            "Question?",
            "--focus",
            "2",
            "--overlap",
            "128",
        ])
        .unwrap()
        .command
        else {
            panic!("keep grammar");
        };
        let effective = options(&original, &document).unwrap();
        assert_eq!(effective.focus.unwrap().get(), 2);
        assert_eq!(effective.overlap, Some(128));
        assert_eq!(effective.max_chunks.unwrap().get(), 7);
        assert!(
            options(
                &original,
                &metadata(json!({"declared":"Unit","split":false,"max_chunks":0}))
            )
            .is_err()
        );
    }

    #[test]
    fn recorded_truncation_and_split_defaults_preserve_explicit_view_choice() {
        let Command::Keep(original) = Cli::try_parse_from(["loupe", "keep", "Question?"])
            .unwrap()
            .command
        else {
            panic!("keep grammar");
        };
        let tail = options(
            &original,
            &metadata(json!({"declared":"Unit","split":false,"truncate":"tail","path":""})),
        )
        .unwrap();
        assert_eq!(tail.truncate, Some(loupe_core::truncate::Mode::Tail));
        for path in ["/text", "/unit", "/content"] {
            let field = options(
                &original,
                &metadata(json!({"declared":"Unit","split":false,"truncate":"head","path":path})),
            )
            .unwrap();
            assert_eq!(field.truncate_field.as_deref(), Some(path));
            assert!(field.truncate.is_none());
        }
        let split = options(
            &original,
            &metadata(json!({"declared":"Unit","split":true,"overlap":128,"max_chunks":9})),
        )
        .unwrap();
        assert!(split.split);
        assert_eq!(split.overlap, Some(128));
        let Command::Keep(original) =
            Cli::try_parse_from(["loupe", "keep", "Question?", "--split", "--combine", "any"])
                .unwrap()
                .command
        else {
            panic!("keep grammar");
        };
        let effective = options(
            &original,
            &metadata(json!({"declared":"Unit","split":false,"focus":3})),
        )
        .unwrap();
        assert!(effective.split);
        assert!(effective.focus.is_none());
    }
}
