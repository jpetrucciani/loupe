use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path};

use crate::error::{Error, ErrorKind, Result};
use crate::preset_store::{Preset, Store};

pub fn expand(preset: &Preset, store: &Store, args: &HashMap<String, String>) -> Result<Value> {
    expand_with_reader(preset, store, args, &|path| {
        std::fs::read_to_string(path)
            .map_err(|error| Error::io("reading preset file template", error))
    })
}

/// Alternate readers retain the project-root rule and can impose a narrower scope.
pub fn expand_with_reader(
    preset: &Preset,
    store: &Store,
    args: &HashMap<String, String>,
    read: &impl Fn(&Path) -> Result<String>,
) -> Result<Value> {
    preset.require_trusted()?;
    let allowed = permissions(preset)?;
    let mut document = preset.document.clone();
    transform_document(&mut document, &mut |field| {
        if let Some(key) = field.strip_prefix("arg.").filter(|key| !key.is_empty()) {
            args.get(key)
                .cloned()
                .ok_or_else(|| Error::new(ErrorKind::Usage, format!("missing --arg {key}=VALUE")))
        } else if let Some(key) = field.strip_prefix("env.") {
            if !allowed.contains(key) {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!("environment template {key} is not allowed by permissions.env"),
                ));
            }
            store.environment().get(key).cloned().ok_or_else(|| {
                Error::new(
                    ErrorKind::Usage,
                    format!("allowed environment variable {key} is unset"),
                )
            })
        } else if let Some(path) = field.strip_prefix("file:") {
            let path = file_path(path)?;
            let path = store
                .project_root
                .join(path)
                .canonicalize()
                .map_err(|error| Error::io("resolving preset file template", error))?;
            if !path.starts_with(&store.project_root) {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "file template resolves outside the project root",
                ));
            }
            read(&path)
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!("unknown template field {field}"),
            ))
        }
    })?;
    Ok(document)
}

pub fn lint(preset: &Preset) -> Result<()> {
    let allowed = permissions(preset)?;
    let mut document = preset.document.clone();
    transform_document(&mut document, &mut |field| {
        if field
            .strip_prefix("arg.")
            .is_some_and(|key| !key.is_empty())
        {
            Ok(String::new())
        } else if let Some(key) = field.strip_prefix("env.") {
            if !allowed.contains(key) {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!("environment template {key} lacks an allowlist grant"),
                ));
            }
            Ok(String::new())
        } else if let Some(path) = field.strip_prefix("file:") {
            file_path(path)?;
            Ok(String::new())
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!("unknown template field {field}"),
            ))
        }
    })
}

fn permissions(preset: &Preset) -> Result<BTreeSet<String>> {
    let env = preset
        .document
        .get("permissions")
        .and_then(|value| value.get("env"));
    match env {
        None => Ok(BTreeSet::new()),
        Some(value) => value
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::Usage, "permissions.env must be a list"))?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    Error::new(ErrorKind::Usage, "env permission must be a variable name")
                })
            })
            .collect(),
    }
}

fn file_path(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        Err(Error::new(
            ErrorKind::Usage,
            "file templates require a relative path confined to the project root",
        ))
    } else {
        Ok(path)
    }
}

fn transform_document(
    document: &mut Value,
    lookup: &mut impl FnMut(&str) -> Result<String>,
) -> Result<()> {
    let calibration = document
        .get_mut("policy")
        .and_then(Value::as_object_mut)
        .and_then(|policy| policy.remove("calibration"));
    let result = transform(document, lookup);
    if let Some(calibration) = calibration
        && let Some(policy) = document.get_mut("policy").and_then(Value::as_object_mut)
    {
        policy.insert("calibration".to_owned(), calibration);
    }
    result
}

fn transform(value: &mut Value, lookup: &mut impl FnMut(&str) -> Result<String>) -> Result<()> {
    match value {
        Value::String(text) => {
            let mut output = String::new();
            let mut remaining = text.as_str();
            while let Some(start) = remaining.find("{{") {
                output.push_str(&remaining[..start]);
                let end = remaining[start + 2..]
                    .find("}}")
                    .ok_or_else(|| Error::new(ErrorKind::Usage, "unterminated preset template"))?
                    + start
                    + 2;
                output.push_str(&lookup(remaining[start + 2..end].trim())?);
                remaining = &remaining[end + 2..];
            }
            output.push_str(remaining);
            *text = output;
        }
        Value::Array(values) => {
            for value in values {
                transform(value, lookup)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                transform(value, lookup)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::preset_store::Store;
    use crate::templates::{expand, lint};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn calibration_annotations_are_opaque_to_template_expansion() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(
            dir.path(),
            &HashMap::from([(
                "XDG_CONFIG_HOME".to_owned(),
                dir.path().join("xdg").to_str().unwrap().to_owned(),
            )]),
        )
        .unwrap();
        let mut preset = store.lookup("secret").unwrap();
        preset.document["instructions"] = json!("{{arg.query}}");
        preset.document["policy"]["calibration"] =
            json!({"source":"{{file:/missing}} {{env.SECRET}} {{unterminated"});
        lint(&preset).unwrap();
        let document = expand(
            &preset,
            &store,
            &HashMap::from([("query".to_owned(), "Is this private?".to_owned())]),
        )
        .unwrap();
        assert_eq!(document["instructions"], "Is this private?");
        assert_eq!(
            document["policy"]["calibration"],
            preset.document["policy"]["calibration"]
        );
    }

    #[test]
    fn expansion_checks_trust_permissions_and_literal_substitution_on_real_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("reference.txt"), "reference {{env.SECRET}}").unwrap();
        let env = HashMap::from([
            (
                "XDG_CONFIG_HOME".to_owned(),
                dir.path().join("xdg").to_str().unwrap().to_owned(),
            ),
            (
                "ALLOWED".to_owned(),
                "context {{file:forbidden}}".to_owned(),
            ),
            ("SECRET".to_owned(), "private".to_owned()),
        ]);
        let store = Store::new(dir.path(), &env).unwrap();
        store.create("rubric", "noul", true).unwrap();
        let mut preset = store.lookup("rubric").unwrap();
        preset.document["permissions"] = json!({"env":["ALLOWED"]});
        preset.document["instructions"] =
            json!({"arg":"{{arg.name}}","env":"{{env.ALLOWED}}","file":"{{file:reference.txt}}"});
        lint(&preset).unwrap();
        let args = HashMap::from([("name".to_owned(), "{{env.SECRET}}".to_owned())]);
        assert_eq!(expand(&preset, &store, &args).unwrap_err().exit_code(), 64);
        preset.trusted = true;
        let document = expand(&preset, &store, &args).unwrap();
        assert_eq!(document["instructions"]["arg"], "{{env.SECRET}}");
        assert_eq!(
            document["instructions"]["env"],
            "context {{file:forbidden}}"
        );
        assert_eq!(document["instructions"]["file"], "reference {{env.SECRET}}");
        assert!(expand(&preset, &store, &HashMap::new()).is_err());
        for text in [
            "{{env.SECRET}}",
            "{{env.UNSET}}",
            "{{arg.}}",
            "{{file:../outside}}",
            "{{file:/etc/passwd}}",
            "{{unknown.name}}",
            "{{arg.name",
        ] {
            preset.document["instructions"] = json!(text);
            assert!(expand(&preset, &store, &args).is_err(), "{text}");
            assert!(lint(&preset).is_err(), "{text}");
        }
        preset.document["permissions"] = json!({"env":["UNSET"]});
        preset.document["instructions"] = json!("{{env.UNSET}}");
        lint(&preset).unwrap();
        assert_eq!(expand(&preset, &store, &args).unwrap_err().exit_code(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resolution_confines_file_templates_to_the_actual_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(outside.path().join("private.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        let env = HashMap::from([(
            "XDG_CONFIG_HOME".to_owned(),
            dir.path().join("xdg").to_str().unwrap().to_owned(),
        )]);
        let store = Store::new(dir.path(), &env).unwrap();
        let mut preset = store.lookup("secret").unwrap();
        preset.document["instructions"] = json!("{{file:escape/private.txt}}");
        lint(&preset).unwrap();
        assert_eq!(
            expand(&preset, &store, &HashMap::new())
                .unwrap_err()
                .exit_code(),
            64
        );
        std::fs::write(dir.path().join("allowed.txt"), "inside").unwrap();
        std::os::unix::fs::symlink(dir.path().join("allowed.txt"), dir.path().join("safe"))
            .unwrap();
        preset.document["instructions"] = json!("{{file:safe}}");
        assert_eq!(
            expand(&preset, &store, &HashMap::new()).unwrap()["instructions"],
            "inside"
        );
    }
}
