use std::collections::HashMap;
use std::io::{self, Write};

use loupe_core::backend::Backend;
use loupe_core::budget::Budget;
use loupe_core::config::Profile;
use loupe_core::encoder;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::policy::{Policy, Selected};
use loupe_core::preset_store::{Origin, Store};
use loupe_core::wire::Request;
use loupe_core::{presets, templates};
use serde_json::{Value, json};

use crate::args::{PresetType, Presets};

pub async fn execute(command: &Presets, store: &Store, profile: &Profile) -> Result<u8> {
    match command {
        Presets::List { json: as_json } => {
            let presets = store
                .names()?
                .into_iter()
                .map(|name| store.lookup(&name))
                .collect::<Result<Vec<_>>>()?;
            let mut output = io::stdout().lock();
            if *as_json {
                write_json(&mut output, &presets)?;
            } else {
                for preset in presets {
                    writeln!(
                        output,
                        "{}\t{}\t{}\t{}\t{}",
                        preset.name,
                        preset
                            .document
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("invalid"),
                        preset
                            .document
                            .get("version")
                            .and_then(Value::as_u64)
                            .unwrap_or(1),
                        if preset.trusted {
                            "trusted"
                        } else {
                            "untrusted"
                        },
                        serde_json::to_string(&preset.origin).map_err(json_error)?
                    )
                    .map_err(|error| Error::io("writing preset list", error))?;
                }
            }
        }
        Presets::Show { name } => {
            write_json(&mut io::stdout().lock(), &store.lookup(name)?)?;
        }
        Presets::New {
            name,
            r#type,
            project,
        } => {
            let path = store.create(
                name,
                match r#type {
                    PresetType::Noul => "noul",
                    PresetType::Choice => "choice",
                    PresetType::Score => "score",
                },
                *project,
            )?;
            writeln!(io::stdout().lock(), "{}", path.display())
                .map_err(|error| Error::io("writing preset path", error))?;
        }
        Presets::Edit { name } => {
            let editor = store
                .environment()
                .get("VISUAL")
                .or_else(|| store.environment().get("EDITOR"))
                .filter(|command| !command.trim().is_empty())
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Config,
                        "set VISUAL or EDITOR before editing a preset",
                    )
                })?;
            let origin = store.resolve_origin(name)?;
            let path = if matches!(origin, Origin::Builtin) {
                let preset = store.lookup(name)?;
                let path = store.create(
                    name,
                    preset
                        .document
                        .get("type")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::new(ErrorKind::Usage, "built-in type missing"))?,
                    false,
                )?;
                let text = presets::to_toml(&preset.document)?;
                loupe_core::preset_store::atomic_write(&path, text.as_bytes())?;
                path
            } else {
                origin
                    .path()
                    .ok_or_else(|| Error::new(ErrorKind::Usage, "preset has no editable path"))?
                    .to_owned()
            };
            let status = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(format!("exec {editor} \"$1\""))
                .arg("loupe-editor")
                .arg(&path)
                .kill_on_drop(true)
                .status()
                .await
                .map_err(|error| Error::io("launching preset editor", error))?;
            if !status.success() {
                return Err(Error::new(
                    ErrorKind::Io,
                    format!("preset editor exited {status}"),
                ));
            }
            let preset = store.lookup(name)?;
            write_json(
                &mut io::stdout().lock(),
                &json!({"path":path,"trusted":preset.trusted,"content_hash":preset.content_hash}),
            )?;
        }
        Presets::Trust { name, file } => {
            let choices = if let Some(path) = file {
                vec![store.explicit(path)?]
            } else if let Some(name) = name {
                vec![store.lookup(name)?]
            } else {
                store
                    .names()?
                    .into_iter()
                    .map(|name| store.lookup(&name))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|preset| preset.origin.project())
                    .collect()
            };
            for preset in choices {
                preset.validate(&profile.limits, &profile.model)?;
                if !preset.trusted {
                    store.trust(&preset)?;
                }
                write_json(
                    &mut io::stdout().lock(),
                    &json!({"name":preset.name,"trusted":true,"content_hash":preset.content_hash}),
                )?;
            }
        }
        Presets::Lint {
            name,
            live,
            arg,
            json,
        } => {
            return lint(name.as_deref(), *live, arg, *json, store, profile).await;
        }
    }
    Ok(0)
}

async fn lint(
    name: Option<&str>,
    live: bool,
    args: &[String],
    as_json: bool,
    store: &Store,
    profile: &Profile,
) -> Result<u8> {
    let names = match name {
        Some(name) => vec![name.to_owned()],
        None => store.names()?,
    };
    let args = args
        .iter()
        .map(|arg| {
            arg.split_once('=')
                .filter(|(name, _)| !name.is_empty())
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .ok_or_else(|| Error::new(ErrorKind::Usage, "--arg requires NAME=VALUE"))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let presets = names
        .iter()
        .map(|name| store.lookup(name))
        .collect::<Result<Vec<_>>>()?;
    let mut reports = Vec::new();
    let mut documents = Vec::new();
    // Complete the local trust/template checks before credentials or network access.
    for preset in &presets {
        let local = preset
            .validate(&profile.limits, &profile.model)
            .and_then(|_| templates::lint(preset));
        let mut report = json!({"name":preset.name,"origin":preset.origin,"trusted":preset.trusted,"valid":local.is_ok(),"policy_identity":"unverified"});
        if let Err(error) = local {
            report["error"] = json!(error.to_string());
            documents.push(None);
        } else if live {
            preset.require_trusted()?;
            let document = templates::expand(preset, store, &args)?;
            validate_examples(&document)?;
            documents.push(Some(document));
        } else {
            documents.push(None);
        }
        reports.push(report);
    }
    let mut backend = if documents.iter().any(Option::is_some) {
        Some(Backend::new(profile).await?)
    } else {
        None
    };
    let mut limits = profile.limits.clone();
    let mut identity = None;
    let mut server_encoder = None;
    let encoder = if backend.is_some() {
        encoder::load_profile(profile)?.map(std::sync::Arc::new)
    } else {
        None
    };
    if let Some(live_backend) = backend.take() {
        let discovery = live_backend.discover(&limits).await?;
        limits = discovery.limits.clone();
        identity = discovery.full_model_identity.clone();
        server_encoder = discovery
            .identity
            .as_ref()
            .map(|identity| identity.encoder_identity.clone());
        backend = Some(
            live_backend
                .with_cache(
                    &discovery,
                    encoder.clone(),
                    &profile.cache_dir,
                    loupe_core::cache_session::Mode::Disabled,
                )
                .await?
                .0,
        );
    }
    let budget = Budget::new(None);
    for ((preset, document), report) in presets.iter().zip(documents).zip(&mut reports) {
        let Some(document) = document else {
            continue;
        };
        let effective = loupe_core::preset_store::Preset {
            document: document.clone(),
            ..preset.clone()
        };
        if let Err(error) = effective
            .validate_rendered(&limits, &profile.model)
            .and_then(|_| effective.check_policy_identity(identity.as_deref()))
            .and_then(|_| {
                if let Some(pin) = loupe_core::calibration_pin::validate(&preset.document)? {
                    pin.check_ensemble(false, loupe_core::ensemble::minimum(&document, None)?)?;
                    let view = loupe_core::calibration_pin::view(
                        &json!({"declared":"Unit","combine":{},"split":false,"path":""}),
                        &serde_json::to_value(&limits)
                            .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?,
                    )?;
                    pin.check(
                        identity.as_deref(),
                        encoder
                            .as_ref()
                            .map_or("estimate-v1", |encoder| encoder.identity.as_str()),
                        server_encoder.as_deref(),
                        &view,
                    )?;
                }
                Ok(())
            })
        {
            report["valid"] = json!(false);
            report["error"] = json!(error.to_string());
            continue;
        }
        report["policy_identity"] = json!(if effective.policy_identity().is_some() {
            "verified"
        } else {
            "not_required"
        });
        let question = presets::question(&document)?;
        let policy = Policy::from_document(&document, question.kind())?;
        let examples = validate_examples(&document)?;
        let backend = backend
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "live lint backend missing"))?;
        let mut results = Vec::new();
        for (state, expected) in examples {
            let mut questions = indexmap::IndexMap::from([(preset.name.clone(), question.clone())]);
            loupe_core::factors::Set::from_document(&document)?.append(&mut questions)?;
            loupe_core::signals::Set::from_document(&document)?.append(&mut questions)?;
            if let Some(context) = document.get("context") {
                presets::add_context(&mut questions, context.clone());
            }
            let request = Request {
                state,
                model: profile.model.clone(),
                questions,
            };
            limits.validate_request(&request)?;
            let counts = if let Some(encoder) = &encoder {
                encoder.encode(&request)?.1
            } else {
                encoder::estimate(&request)?
            };
            counts.preflight(&limits)?;
            let response = backend
                .evaluate(&request, &counts, &budget, true, None)
                .await?
                .response;
            if let Some(identity) = &identity
                && response.model != *identity
            {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "model identity changed during live lint",
                ));
            }
            let answer = response
                .validate(&request)?
                .into_iter()
                .next()
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "example has no answer"))?
                .1;
            let assessment = answer.assessment(&policy)?;
            let passed = if assessment.uncertain {
                expected.as_str() == Some("uncertain")
            } else {
                match &assessment.selected {
                    Selected::Boolean(yes) => {
                        expected.as_bool() == Some(*yes)
                            || expected.as_str() == Some(if *yes { "yes" } else { "no" })
                    }
                    Selected::Choice(label) => expected.as_str() == Some(label.as_str()),
                    Selected::Score(score) => expected
                        .as_f64()
                        .is_some_and(|expected| (*score - expected).abs() <= 0.5),
                }
            };
            results.push(json!({"unit":request.state,"expect":expected,"passed":passed,"uncertain":assessment.uncertain,"response":response}));
        }
        report["examples_passed"] = json!(results.iter().all(|result| result["passed"] == true));
        report["examples"] = json!(results);
    }
    let failed = reports.iter().any(|report| {
        report["valid"] != true || report.get("examples_passed") == Some(&Value::Bool(false))
    });
    let mut output = io::stdout().lock();
    if as_json {
        write_json(&mut output, &reports)?;
    } else {
        for report in reports {
            let status = if report["valid"] != true {
                "invalid"
            } else if report.get("examples_passed") == Some(&Value::Bool(false)) {
                "examples_failed"
            } else {
                "valid"
            };
            writeln!(
                output,
                "{}\t{}\t{}",
                report["name"].as_str().unwrap_or("unknown"),
                status,
                report.get("error").and_then(Value::as_str).unwrap_or("")
            )
            .map_err(|error| Error::io("writing lint report", error))?;
        }
    }
    Ok(if failed { 64 } else { 0 })
}

fn validate_examples(document: &Value) -> Result<Vec<(Value, Value)>> {
    let question = presets::question(document)?;
    let examples = document
        .get("examples")
        .and_then(Value::as_array)
        .filter(|examples| !examples.is_empty())
        .ok_or_else(|| Error::new(ErrorKind::Usage, "live lint requires at least one example"))?;
    examples
        .iter()
        .map(|example| {
            let state = example
                .get("unit")
                .ok_or_else(|| Error::new(ErrorKind::Usage, "example missing unit"))?
                .clone();
            let expected = example
                .get("expect")
                .ok_or_else(|| Error::new(ErrorKind::Usage, "example missing expect"))?
                .clone();
            let valid = expected.as_str() == Some("uncertain")
                || match &question {
                    loupe_core::wire::Question::Noul { .. } => {
                        expected.is_boolean() || matches!(expected.as_str(), Some("yes" | "no"))
                    }
                    loupe_core::wire::Question::Choice { criteria, .. } => expected
                        .as_str()
                        .is_some_and(|label| criteria.contains_key(label)),
                    loupe_core::wire::Question::Score { criteria, .. } => expected
                        .as_u64()
                        .is_some_and(|level| level < criteria.len() as u64),
                };
            if !valid {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "example expectation does not match the question type/options",
                ));
            }
            Ok((state, expected))
        })
        .collect()
}

fn json_error(error: serde_json::Error) -> Error {
    if let Some(kind) = error.io_error_kind() {
        Error::io("writing preset JSON", io::Error::new(kind, error))
    } else {
        Error::new(ErrorKind::Io, error.to_string())
    }
}
fn write_json(writer: &mut impl Write, value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut *writer, value).map_err(json_error)?;
    writer
        .write_all(b"\n")
        .map_err(|error| Error::io("writing preset output", error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::preset_commands::validate_examples;
    use serde_json::json;

    #[test]
    fn score_example_labels_use_zero_based_api_levels() {
        let mut document = json!({"type":"score","criteria":["Low","High"],"examples":[{"unit":"Example","expect":0}]});
        assert!(validate_examples(&document).is_ok());
        document["examples"][0]["expect"] = json!(1);
        assert!(validate_examples(&document).is_ok());
        for value in [json!(2), json!(-1), json!(0.5)] {
            document["examples"][0]["expect"] = value;
            assert_eq!(validate_examples(&document).unwrap_err().exit_code(), 64);
        }
    }
}
