use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Model(String);

impl Model {
    pub fn parse(value: &str) -> Result<Self> {
        let mut parts = value.split(';');
        let (name, revision) = parts
            .next()
            .and_then(|part| part.rsplit_once('@'))
            .ok_or_else(invalid)?;
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.'))
            || !hex(revision, 40)
        {
            return Err(invalid());
        }
        let mut compute = None;
        let mut quantization = None;
        let mut server = None;
        for part in parts {
            let (key, value) = part.split_once('=').ok_or_else(invalid)?;
            let target = match key {
                "compute" => &mut compute,
                "quantization" => &mut quantization,
                "server" => &mut server,
                _ => return Err(invalid()),
            };
            if target.replace(value).is_some() {
                return Err(invalid());
            }
        }
        let compute = compute
            .filter(|dtype| matches!(*dtype, "bf16" | "fp16" | "fp32" | "fp64" | "fp8"))
            .ok_or_else(invalid)?;
        let quantization = quantization
            .filter(|quant| {
                !quant.is_empty()
                    && quant.len() <= 64
                    && *quant != "unknown"
                    && quant
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
            })
            .ok_or_else(invalid)?;
        let server = server.filter(|build| hex(build, 64)).ok_or_else(invalid)?;
        Ok(Self(format!(
            "{name}@{revision};compute={compute};quantization={quantization};server={server}"
        )))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Model {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}
impl From<Model> for String {
    fn from(value: Model) -> Self {
        value.0
    }
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn invalid() -> Error {
    Error::new(
        ErrorKind::Unavailable,
        "model identity requires name@immutable HF revision, compute dtype, quantization and a SHA-256 server-build identity",
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct Discovery {
    pub model: Model,
    pub encoder_identity: String,
}
#[derive(Deserialize)]
struct Document {
    schema_version: u64,
    requested_model: String,
    model: Model,
    encoder_identity: String,
}
impl Discovery {
    pub fn parse(value: serde_json::Value, requested_model: &str) -> Result<Self> {
        let document: Document = serde_json::from_value(value).map_err(|error| {
            Error::new(
                ErrorKind::Unavailable,
                format!("invalid identity metadata: {error}"),
            )
        })?;
        if document.schema_version != 1
            || document.requested_model != requested_model
            || document.encoder_identity.is_empty()
            || document.encoder_identity.len() > 4096
            || document.encoder_identity.chars().any(char::is_control)
        {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "identity metadata version/requested model/encoder does not match the run",
            ));
        }
        Ok(Self {
            model: document.model,
            encoder_identity: document.encoder_identity,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::identity::{Discovery, Model};
    use serde_json::json;
    fn model() -> String {
        format!(
            "clef-flash@{};compute=bf16;quantization=nf4;server={}",
            "a".repeat(40),
            "b".repeat(64)
        )
    }
    #[test]
    fn complete_identities_normalize_order_and_refuse_aliases_versions_and_unknown_precision() {
        let canonical = Model::parse(&model()).unwrap();
        let reordered = format!(
            "clef-flash@{};server={};quantization=nf4;compute=bf16",
            "a".repeat(40),
            "b".repeat(64)
        );
        assert_eq!(Model::parse(&reordered).unwrap(), canonical);
        for invalid in [
            "clef-flash".to_owned(),
            model().replace(&"a".repeat(40), "main"),
            model().replace(&"b".repeat(64), "v0.1.0"),
            model().replace("bf16", "unknown"),
            model() + ";compute=bf16",
            model().replace("nf4", "unknown"),
        ] {
            assert!(Model::parse(&invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn metadata_requires_the_fresh_versioned_requested_model_contract() {
        let valid = json!({"schema_version":1,"requested_model":"clef-flash","model":model(),"encoder_identity":"clef-v1;immutable-tokenizer"});
        assert!(Discovery::parse(valid.clone(), "clef-flash").is_ok());
        assert!(Discovery::parse(valid.clone(), "other-model").is_err());
        for (key, value) in [
            ("schema_version", json!(2)),
            ("encoder_identity", json!("")),
            ("model", json!("clef-flash")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(Discovery::parse(invalid, "clef-flash").is_err());
        }
    }
}
