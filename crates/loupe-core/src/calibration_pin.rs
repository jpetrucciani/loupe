use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::calibration::Parameters;
use crate::error::{Error, ErrorKind, Result};
use crate::identity::Model;
use crate::pack::Mode as PackMode;
use crate::policy::QuestionKind;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ensemble {
    #[serde(deserialize_with = "deserialize_probability")]
    pub min_agreement: crate::policy::Probability,
}

fn deserialize_probability<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<crate::policy::Probability, D::Error> {
    crate::policy::Probability::new(f64::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub schema_version: u32,
    pub model: Model,
    pub local_encoder: String,
    pub server_encoder: String,
    pub preset_version: u64,
    pub preset_blake3: String,
    pub field: String,
    pub view: Value,
    pub source: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<Ensemble>,
}

pub fn document(
    original: &Value,
    parameters: &Parameters,
    kind: QuestionKind,
    mut pin: Pin,
) -> Result<Value> {
    parameters.policy()?;
    let mut document = original.clone();
    let object = document
        .as_object_mut()
        .ok_or_else(|| invalid("calibrated preset must be an object"))?;
    let version = object
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .checked_add(1)
        .ok_or_else(|| invalid("preset version overflow"))?;
    object.insert("version".to_owned(), json!(version));
    let policy = object
        .entry("policy")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("preset policy must be a table"))?;
    policy.remove("calibration");
    policy.remove("splittable");
    policy.remove("combine");
    policy.insert("model_identity".to_owned(), json!(pin.model.as_str()));
    if let Some(ensemble) = &pin.ensemble {
        policy.insert("min_agreement".to_owned(), json!(ensemble.min_agreement));
    }
    if let Some(pack) = pin.view.get("declared").and_then(Value::as_str) {
        let pack = pack_name(pack)?;
        policy.insert("pack".to_owned(), json!(pack));
    }
    if pin.view.get("split") == Some(&Value::Bool(true)) {
        let combine = pin
            .view
            .get("combine")
            .ok_or_else(|| invalid("split calibration lacks combine modes"))?
            .clone();
        if !combine.is_object() || combine.as_object().is_none_or(serde_json::Map::is_empty) {
            return Err(invalid("split calibration requires declared reducers"));
        }
        policy.insert("splittable".to_owned(), Value::Bool(true));
        policy.insert("combine".to_owned(), combine);
    }
    let mixed = original.get("questions").is_some();
    let policy = if mixed {
        if original
            .get("questions")
            .and_then(|questions| questions.get(&pin.field))
            .is_none()
        {
            return Err(invalid(
                "calibration field must name a declared rubric question",
            ));
        }
        policy
            .entry("fields")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| invalid("policy.fields must be a table"))?
            .entry(pin.field.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| invalid("field policy must be a table"))?
    } else {
        policy
    };
    match kind {
        QuestionKind::Noul => {
            policy.insert("threshold".to_owned(), json!(parameters.threshold));
            policy.insert("margin".to_owned(), json!(parameters.margin));
        }
        QuestionKind::Choice => {
            policy.insert(
                "min_confidence".to_owned(),
                json!(parameters.min_confidence),
            );
            policy.insert("min_gap".to_owned(), json!(parameters.min_gap));
        }
        QuestionKind::Score => return Err(invalid("score calibration is unsupported")),
    }
    pin.preset_version = version;
    pin.preset_blake3 = fingerprint(&document)?;
    validate_pin(&pin)?;
    document
        .get_mut("policy")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("calibration policy missing"))?
        .insert(
            "calibration".to_owned(),
            serde_json::to_value(&pin).map_err(json_error)?,
        );
    validate(&document)?;
    Ok(document)
}

pub fn validate(document: &Value) -> Result<Option<Pin>> {
    let Some(pin) = read(document)? else {
        return Ok(None);
    };
    if document.get("version").and_then(Value::as_u64) != Some(pin.preset_version)
        || document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .and_then(Value::as_str)
            != Some(pin.model.as_str())
        || fingerprint(document)? != pin.preset_blake3
    {
        return Err(invalid(
            "calibrated preset content/version/model identity changed; recalibrate this preset",
        ));
    }
    if let Some(ensemble) = &pin.ensemble
        && crate::ensemble::minimum(document, None)? != ensemble.min_agreement
    {
        return Err(invalid(
            "calibrated ensemble agreement differs from the preset policy",
        ));
    }
    Ok(Some(pin))
}

/// Read an annotation after template expansion. Validate the original document
/// first, since expanded arguments intentionally differ from its fingerprint.
pub fn read(document: &Value) -> Result<Option<Pin>> {
    let Some(value) = document
        .get("policy")
        .and_then(|policy| policy.get("calibration"))
    else {
        return Ok(None);
    };
    let pin: Pin = serde_json::from_value(value.clone())
        .map_err(|error| invalid(format!("invalid calibration identity: {error}")))?;
    validate_pin(&pin)?;
    Ok(Some(pin))
}

impl Pin {
    pub fn check_ensemble(
        &self,
        enabled: bool,
        min_agreement: crate::policy::Probability,
    ) -> Result<()> {
        match (&self.ensemble, enabled) {
            (None, false) => Ok(()),
            (Some(expected), true) if expected.min_agreement == min_agreement => Ok(()),
            _ => Err(invalid(
                "ensemble mode or agreement gate differs from the calibrated policy; explicitly recalibrate this configuration",
            )),
        }
    }

    pub fn check_policy(
        &self,
        document: &Value,
        actual: crate::policy::Policy,
        kind: QuestionKind,
    ) -> Result<()> {
        let expected = crate::policy::Policy::from_field(document, &self.field, kind)?;
        let matches = match kind {
            QuestionKind::Noul => {
                actual.threshold == expected.threshold && actual.margin == expected.margin
            }
            QuestionKind::Choice => {
                actual.min_confidence == expected.min_confidence
                    && actual.min_gap == expected.min_gap
            }
            QuestionKind::Score => false,
        };
        if !matches {
            return Err(invalid(
                "decision parameters differ from the calibrated policy; recalibrate this policy",
            ));
        }
        Ok(())
    }

    pub fn check(
        &self,
        model: Option<&str>,
        local_encoder: &str,
        server_encoder: Option<&str>,
        view: &Value,
    ) -> Result<()> {
        let current = model.ok_or_else(|| {
            Error::new(
                ErrorKind::Unavailable,
                "calibrated policy requires fresh complete model/encoder discovery",
            )
        })?;
        if Model::parse(current)? != self.model
            || local_encoder != self.local_encoder
            || server_encoder != Some(self.server_encoder.as_str())
            || view != &self.view
        {
            return Err(invalid(
                "model, encoder or execution view differs from the calibrated policy; recalibrate or select the matching profile/preset",
            ));
        }
        Ok(())
    }
}

pub fn fingerprint(document: &Value) -> Result<String> {
    let mut document = document.clone();
    if let Some(policy) = document.get_mut("policy").and_then(Value::as_object_mut) {
        policy.remove("calibration");
    }
    // JSON/TOML table formatting may reorder unrelated keys; field order still
    // changes the joint prompt and must remain part of the calibrated identity.
    if let Some(questions) = document.get_mut("questions")
        && let Some(object) = questions.as_object()
    {
        *questions = Value::Array(
            object
                .iter()
                .map(|(id, question)| json!([id, question]))
                .collect(),
        );
    }
    fn ordered(value: &Value) -> Result<Value> {
        match value {
            Value::Object(object) => {
                let entries = object
                    .iter()
                    .map(|(key, value)| ordered(value).map(|value| (key.clone(), value)))
                    .collect::<Result<BTreeMap<_, _>>>()?;
                Ok(Value::Object(entries.into_iter().collect()))
            }
            Value::Array(array) => array
                .iter()
                .map(ordered)
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            _ => Ok(value.clone()),
        }
    }
    let encoded = serde_json::to_vec(&ordered(&document)?).map_err(json_error)?;
    Ok(blake3::hash(&encoded).to_hex().to_string())
}

pub fn view(packing: &Value, limits: &Value) -> Result<Value> {
    let mut view = packing
        .as_object()
        .ok_or_else(|| invalid("calibration packing metadata must be an object"))?
        .clone();
    view.remove("source_indices");
    view.retain(|_, value| !value.is_null());
    view.insert("limits".to_owned(), limits.clone());
    Ok(Value::Object(view))
}

fn validate_pin(pin: &Pin) -> Result<()> {
    let expected = pin
        .ensemble
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(json_error)?;
    if expected.as_ref() != pin.source.get("ensemble") {
        return Err(invalid(
            "calibration ensemble identity differs from its fitting provenance",
        ));
    }
    if pin.schema_version != 1
        || pin.local_encoder.is_empty()
        || pin.server_encoder.is_empty()
        || pin.preset_version == 0
        || pin.field.is_empty()
        || pin.preset_blake3.len() != 64
        || !pin
            .preset_blake3
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !pin.view.is_object()
        || !pin.source.is_object()
        || pin
            .source
            .get("partition")
            .and_then(Value::as_str)
            .is_none_or(|partition| !matches!(partition, "calibrate" | "captured"))
    {
        return Err(invalid(
            "calibration identity requires model/encoder/preset/view and calibration/development provenance",
        ));
    }
    let pack = pin
        .view
        .get("declared")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("calibration packing declaration missing"))?;
    pack_name(pack)?;
    Ok(())
}

pub fn pack_name(value: &str) -> Result<String> {
    let value = match value {
        "Unit" => "unit".to_owned(),
        "Whole" => "whole".to_owned(),
        value if value.starts_with("Window(") && value.ends_with(')') => {
            format!("window:{}", &value[7..value.len() - 1])
        }
        _ => value.to_owned(),
    };
    if value.parse::<PackMode>()? == PackMode::Auto {
        return Err(invalid("calibration requires concrete packing"));
    }
    Ok(value)
}

fn json_error(error: serde_json::Error) -> Error {
    invalid(error.to_string())
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(unix)]
pub fn write(
    store: &crate::preset_store::Store,
    original: &crate::preset_store::Preset,
    document: &Value,
    limits: &crate::config::Limits,
    requested_model: &str,
) -> Result<std::path::PathBuf> {
    use crate::preset_store::Origin;
    use rustix::fs::{AtFlags, Mode, OFlags, linkat, open, openat, renameat, unlinkat};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::Component;

    original.require_trusted()?;
    if validate(document)?.is_none() {
        return Err(invalid("calibrated write requires a complete identity pin"));
    }
    let mut proposed = original.clone();
    proposed.document = document.clone();
    proposed.validate(limits, requested_model)?;
    let destination = original
        .origin
        .path()
        .map(std::path::Path::to_owned)
        .unwrap_or_else(|| {
            store
                .config_dir
                .join("presets")
                .join(format!("{}.toml", original.name))
        });
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("preset destination lacks a parent"))?;
    let builtin = matches!(original.origin, Origin::Builtin);
    if builtin {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::io("creating calibrated preset directory", error))?;
    }
    let directory = if original.origin.project() {
        let descriptor = open(
            &store.project_root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding calibrated project root", error.into()))?;
        let mut directory = File::from(descriptor);
        for component in parent
            .strip_prefix(&store.project_root)
            .map_err(|_| invalid("calibrated project preset escapes its root"))?
            .components()
        {
            let Component::Normal(name) = component else {
                return Err(invalid("invalid calibrated project path"));
            };
            directory = File::from(
                openat(
                    &directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| Error::io("holding calibrated project directory", error.into()))?,
            );
        }
        directory
    } else {
        File::from(
            open(
                parent,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| Error::io("holding calibrated preset directory", error.into()))?,
        )
    };
    let lock = File::from(
        openat(
            &directory,
            ".loupe-calibrate.lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| Error::io("opening calibrated preset lock", error.into()))?,
    );
    if !lock
        .metadata()
        .map_err(|error| Error::io("checking calibrated preset lock", error))?
        .is_file()
    {
        return Err(invalid("calibrated preset lock is not a regular file"));
    }
    lock.try_lock().map_err(|error| {
        Error::new(
            ErrorKind::Unavailable,
            format!("calibrated preset directory is busy: {error}"),
        )
    })?;
    let name = destination
        .file_name()
        .ok_or_else(|| invalid("preset destination lacks a filename"))?;
    let check = || -> Result<()> {
        if builtin {
            return Ok(());
        }
        let descriptor = openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("checking preset before calibration commit", error.into()))?;
        let mut file = File::from(descriptor);
        if !file
            .metadata()
            .map_err(|error| Error::io("checking preset file", error))?
            .is_file()
        {
            return Err(invalid("calibrated preset is not a regular file"));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| Error::io("reading preset before calibration commit", error))?;
        if blake3::hash(&bytes).to_hex().as_str() != original.content_hash {
            return Err(invalid(
                "preset changed during calibration; original bytes retained",
            ));
        }
        Ok(())
    };
    check()?;
    let bytes = if destination
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        serde_json::to_vec_pretty(document).map_err(json_error)?
    } else {
        crate::presets::to_toml(document)?.into_bytes()
    };
    let stamp = crate::report_store::Stamp::now().generated_at;
    let temporary = format!(
        ".loupe-calibrate-{}-{}.pending",
        std::process::id(),
        stamp.replace([':', '+'], "_")
    );
    let descriptor = openat(
        &directory,
        &temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| Error::io("creating calibrated preset staging file", error.into()))?;
    let mut file = File::from(descriptor);
    let commit = (|| {
        file.write_all(&bytes)
            .map_err(|error| Error::io("writing calibrated preset", error))?;
        file.sync_all()
            .map_err(|error| Error::io("syncing calibrated preset", error))?;
        check()?;
        if builtin {
            linkat(&directory, &temporary, &directory, name, AtFlags::empty())
                .map_err(|error| Error::io("publishing calibrated built-in copy", error.into()))?;
            unlinkat(&directory, &temporary, AtFlags::empty()).map_err(|error| {
                Error::io("removing calibrated preset staging file", error.into())
            })?;
        } else {
            renameat(&directory, &temporary, &directory, name)
                .map_err(|error| Error::io("committing calibrated preset", error.into()))?;
        }
        directory
            .sync_all()
            .map_err(|error| Error::io("syncing calibrated preset directory", error))
    })();
    if commit.is_err() {
        let _ = unlinkat(&directory, &temporary, AtFlags::empty());
    }
    commit?;
    if original.origin.project() || matches!(original.origin, Origin::External(_)) {
        proposed.content_hash = blake3::hash(&bytes).to_hex().to_string();
        store.trust(&proposed)?;
    }
    Ok(destination)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;

    use serde_json::{Value, json};

    use crate::calibration::Parameters;
    use crate::calibration_pin::{Pin, document, fingerprint, read, validate, view, write};
    use crate::config::Limits;
    use crate::identity::Model;
    use crate::policy::QuestionKind;
    use crate::preset_store::Store;

    fn model() -> Model {
        // A mathematical tuple fixture, not a discovery or real-server claim.
        Model::parse(&format!(
            "fixture/model@{};compute=bf16;quantization=nf4;server={}",
            "1".repeat(40),
            "2".repeat(64)
        ))
        .unwrap()
    }
    fn proposal(original: &Value) -> Value {
        document(original, &Parameters {threshold:0.65,margin:0.1,min_confidence:0.6,min_gap:0.02}, QuestionKind::Noul, Pin {
            schema_version:1, model:model(), local_encoder:"clef-v1;fixture".to_owned(), server_encoder:"clef-v1;fixture".to_owned(), preset_version:1,preset_blake3:"0".repeat(64),field:original.get("questions").and_then(Value::as_object).and_then(|questions| questions.keys().next()).map_or_else(|| "rubric".to_owned(), String::clone),
            view:view(&json!({"declared":"Unit","combine":{},"split":false,"focus":null,"truncate":null,"path":""}),&serde_json::to_value(Limits::default()).unwrap()).unwrap(),
            source:json!({"partition":"calibrate","suite":"fixture","cases":["a","b"],"targets":[{"minimum_precision":0.9}]}), ensemble:None,
        }).unwrap()
    }
    fn store(root: &std::path::Path) -> Store {
        std::fs::create_dir(root.join(".git")).unwrap();
        Store::new(
            root,
            &HashMap::from([(
                "XDG_CONFIG_HOME".to_owned(),
                root.join("xdg").to_str().unwrap().to_owned(),
            )]),
        )
        .unwrap()
    }

    #[test]
    fn ensemble_pins_bind_mode_agreement_provenance_round_trips_and_real_writes() {
        let original = json!({"version":1,"type":"noul","instructions":"Outage?","variants":[{"id":"direct","instructions":"Unresolved outage?"}]});
        let scalar = proposal(&original);
        let mut pin = validate(&scalar).unwrap().unwrap();
        assert!(scalar["policy"]["calibration"].get("ensemble").is_none());
        pin.check_ensemble(false, crate::policy::Probability::new(0.2).unwrap())
            .unwrap();
        assert!(
            pin.check_ensemble(true, crate::policy::Probability::new(1.0).unwrap())
                .is_err()
        );
        let gate = crate::policy::Probability::new(0.75).unwrap();
        pin.ensemble = Some(crate::calibration_pin::Ensemble {
            min_agreement: gate,
        });
        pin.source["ensemble"] = serde_json::to_value(&pin.ensemble).unwrap();
        let proposed = document(
            &original,
            &Parameters {
                threshold: 0.65,
                margin: 0.1,
                min_confidence: 0.6,
                min_gap: 0.02,
            },
            QuestionKind::Noul,
            pin,
        )
        .unwrap();
        assert_eq!(proposed["policy"]["min_agreement"], 0.75);
        let pin = validate(&proposed).unwrap().unwrap();
        pin.check_ensemble(true, gate).unwrap();
        assert!(pin.check_ensemble(false, gate).is_err());
        assert!(
            pin.check_ensemble(true, crate::policy::Probability::new(0.5).unwrap())
                .is_err()
        );
        for bytes in [
            serde_json::to_vec(&proposed).unwrap(),
            crate::presets::to_toml(&proposed).unwrap().into_bytes(),
        ] {
            let decoded = if bytes.first() == Some(&b'{') {
                serde_json::from_slice(&bytes).unwrap()
            } else {
                crate::presets::parse_document(std::path::Path::new("rubric.toml"), &bytes).unwrap()
            };
            assert_eq!(validate(&decoded).unwrap().unwrap().ensemble, pin.ensemble);
        }
        for change in [0.5, -0.1, 1.1] {
            let mut modified = proposed.clone();
            modified["policy"]["calibration"]["ensemble"]["min_agreement"] = json!(change);
            assert!(validate(&modified).is_err());
        }
        let mut modified = proposed.clone();
        modified["policy"]["calibration"]["ensemble"]["min_agreement"] = json!(0.5);
        modified["policy"]["calibration"]["source"]["ensemble"]["min_agreement"] = json!(0.5);
        assert!(validate(&modified).is_err());
        let root = tempfile::tempdir().unwrap();
        let store = store(root.path());
        let directory = root.path().join("xdg/loupe/presets");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("rubric.toml"),
            crate::presets::to_toml(&original).unwrap(),
        )
        .unwrap();
        let loaded = store.lookup("rubric").unwrap();
        let written = write(&store, &loaded, &proposed, &Limits::default(), "clef-flash").unwrap();
        assert_eq!(written, directory.join("rubric.toml"));
        let written = store.lookup("rubric").unwrap();
        assert_eq!(
            validate(&written.document).unwrap().unwrap().ensemble,
            pin.ensemble
        );
    }

    #[test]
    fn pin_survives_json_and_toml_and_binds_original_question_order() {
        let original = json!({"version":3,"questions":{"first":{"type":"noul","instructions":"{{arg.query}}"},"second":{"type":"noul","instructions":"Is this urgent?"}},"policy":{"margin":0.0}});
        let proposed = proposal(&original);
        assert_eq!(proposed["version"], 4);
        let pin = validate(&proposed).unwrap().unwrap();
        let decoded: Value =
            serde_json::from_slice(&serde_json::to_vec(&proposed).unwrap()).unwrap();
        validate(&decoded).unwrap();
        let text = crate::presets::to_toml(&proposed).unwrap();
        let decoded = serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap();
        validate(&decoded).unwrap();
        let mut changed = proposed.clone();
        changed["questions"]["first"]["instructions"] = json!("expanded query");
        assert!(validate(&changed).is_err());
        assert_eq!(
            read(&changed).unwrap().unwrap().preset_blake3,
            pin.preset_blake3
        );
        let mut reordered = proposed.clone();
        let entries = reordered["questions"]
            .as_object()
            .unwrap()
            .iter()
            .rev()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        reordered["questions"] = Value::Object(entries);
        assert_ne!(fingerprint(&reordered).unwrap(), pin.preset_blake3);
        assert!(validate(&reordered).is_err());
        for (key, value) in [
            ("version", json!(5)),
            ("context", json!("new jointly decided context")),
        ] {
            let mut changed = proposed.clone();
            changed[key] = value;
            assert_eq!(validate(&changed).unwrap_err().exit_code(), 64);
        }
    }

    #[test]
    fn application_requires_fresh_model_encoder_and_exact_execution_view() {
        let proposed = proposal(&json!({"type":"noul","instructions":"Is it urgent?"}));
        let pin = validate(&proposed).unwrap().unwrap();
        let expected = crate::policy::Policy::from_document(&proposed, QuestionKind::Noul).unwrap();
        pin.check_policy(&proposed, expected, QuestionKind::Noul)
            .unwrap();
        assert!(
            pin.check_policy(
                &proposed,
                crate::policy::Policy::default(),
                QuestionKind::Noul
            )
            .is_err()
        );
        pin.check(
            Some(model().as_str()),
            "clef-v1;fixture",
            Some("clef-v1;fixture"),
            &pin.view,
        )
        .unwrap();
        assert_eq!(
            pin.check(None, "clef-v1;fixture", None, &pin.view)
                .unwrap_err()
                .exit_code(),
            69
        );
        assert_eq!(
            pin.check(
                Some(model().as_str()),
                "changed",
                Some("clef-v1;fixture"),
                &pin.view
            )
            .unwrap_err()
            .exit_code(),
            64
        );
        assert!(
            pin.check(Some(model().as_str()), "clef-v1;fixture", None, &pin.view)
                .is_err()
        );
        let mut changed = pin.view.clone();
        changed["declared"] = json!("Window(4)");
        assert!(
            pin.check(
                Some(model().as_str()),
                "clef-v1;fixture",
                Some("clef-v1;fixture"),
                &changed
            )
            .is_err()
        );
        let mut changed = proposed.clone();
        changed["policy"]["calibration"]["source"]["partition"] = json!("test");
        assert!(validate(&changed).is_err());
    }

    #[test]
    fn fitting_one_mixed_field_preserves_sibling_decision_policies() {
        use crate::policy::Policy;
        let original = json!({"version":1,"questions":{"first":{"type":"noul","instructions":"Urgent?"},"second":{"type":"noul","instructions":"Private?"},"topic":{"type":"choice","instructions":"Topic?","criteria":{"a":"Alpha","b":"Beta"}}}});
        let proposed = proposal(&original);
        let expected = Policy::from_field(&proposed, "first", QuestionKind::Noul).unwrap();
        assert_eq!(expected.threshold.get(), 0.65);
        assert_eq!(expected.margin, 0.1);
        assert_eq!(
            Policy::from_field(&proposed, "second", QuestionKind::Noul)
                .unwrap()
                .threshold
                .get(),
            0.5
        );
        assert_eq!(
            Policy::from_field(&proposed, "topic", QuestionKind::Choice)
                .unwrap()
                .min_confidence
                .get(),
            Policy::default().min_confidence.get()
        );
        crate::preset_store::Preset {
            name: "rubric".to_owned(),
            document: proposed,
            origin: crate::preset_store::Origin::Builtin,
            trusted: true,
            content_hash: String::new(),
        }
        .validate(&Limits::default(), "model")
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_project_write_updates_trust_and_preserves_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let path = store.create("rubric", "noul", true).unwrap();
        let original = store.lookup("rubric").unwrap();
        assert!(!original.trusted);
        assert!(
            write(
                &store,
                &original,
                &proposal(&original.document),
                &Limits::default(),
                "model"
            )
            .is_err()
        );
        store.trust(&original).unwrap();
        let original = store.lookup("rubric").unwrap();
        let proposed = proposal(&original.document);
        assert_eq!(
            write(&store, &original, &proposed, &Limits::default(), "model").unwrap(),
            path
        );
        let written = store.lookup("rubric").unwrap();
        assert!(written.trusted);
        validate(&written.document).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            std::fs::read_dir(directory.path().join(".loupe/presets"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".pending"))
        );
        assert!(write(&store, &original, &proposed, &Limits::default(), "model").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn writer_retains_changed_source_and_refuses_symlinks_or_busy_locks() {
        use std::fs::OpenOptions;
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let path = store.create("rubric", "noul", false).unwrap();
        let original = store.lookup("rubric").unwrap();
        let proposed = proposal(&original.document);
        let changed = "type='noul'\ninstructions='Concurrent edit'\n";
        std::fs::write(&path, changed).unwrap();
        assert!(write(&store, &original, &proposed, &Limits::default(), "model").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        let original = store.lookup("rubric").unwrap();
        let proposed = proposal(&original.document);
        let lock_path = path.parent().unwrap().join(".loupe-calibrate.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        lock.lock().unwrap();
        assert_eq!(
            write(&store, &original, &proposed, &Limits::default(), "model")
                .unwrap_err()
                .exit_code(),
            69
        );
        drop(lock);
        std::fs::remove_file(&path).unwrap();
        let outside = directory.path().join("outside.toml");
        std::fs::write(&outside, changed).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(write(&store, &original, &proposed, &Limits::default(), "model").is_err());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), changed);
    }

    #[cfg(unix)]
    #[test]
    fn builtin_copy_never_replaces_a_newly_created_user_preset() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let original = store.lookup("secret").unwrap();
        let proposed = proposal(&original.document);
        let path = write(&store, &original, &proposed, &Limits::default(), "model").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(write(&store, &original, &proposed, &Limits::default(), "model").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        validate(&store.lookup("secret").unwrap().document).unwrap();
    }
}
