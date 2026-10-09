use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Name(String);

impl Name {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 255
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "route filenames must contain 1–255 characters from [a-z0-9_-]",
            ));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Name {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}
impl From<Name> for String {
    fn from(value: Name) -> String {
        value.0
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Labels {
    names: BTreeMap<String, Name>,
}
impl Labels {
    pub fn new<'a>(criteria: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let mut names = BTreeMap::new();
        let mut owners = BTreeMap::new();
        let mut declared = BTreeSet::new();
        for label in criteria {
            if !declared.insert(label.to_owned()) {
                return Err(Error::new(ErrorKind::Usage, "duplicate route criterion"));
            }
            Self::insert(&mut names, &mut owners, label)?;
        }
        if declared.is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "route requires declared choice criteria",
            ));
        }
        for reserved in ["none", "uncertain"] {
            if !names.contains_key(reserved) {
                Self::insert(&mut names, &mut owners, reserved)?;
            }
        }
        Ok(Self { names })
    }
    fn insert(
        names: &mut BTreeMap<String, Name>,
        owners: &mut BTreeMap<Name, String>,
        label: &str,
    ) -> Result<()> {
        let sanitized: String = label
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else if character == '_' {
                    '_'
                } else {
                    '-'
                }
            })
            .collect();
        let name = Name::parse(&sanitized)?;
        if let Some(other) = owners.insert(name.clone(), label.to_owned()) {
            return Err(Error::new(
                ErrorKind::Usage,
                format!(
                    "route labels {other:?} and {label:?} collide as {:?}",
                    name.as_str()
                ),
            ));
        }
        names.insert(label.to_owned(), name);
        Ok(())
    }
    pub fn resolve(&self, label: &str) -> Result<&Name> {
        self.names.get(label).ok_or_else(|| {
            Error::new(
                ErrorKind::Unavailable,
                format!("undeclared route label {label:?}"),
            )
        })
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Name)> {
        self.names
            .iter()
            .map(|(label, name)| (label.as_str(), name))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::route_labels::{Labels, Name};

    #[test]
    fn only_declared_labels_and_reserved_buckets_resolve_to_flat_safe_names() {
        let labels = Labels::new(["PAGE NOW", "ticket", "../../escape", "東京🙂"]).unwrap();
        assert_eq!(labels.resolve("PAGE NOW").unwrap().as_str(), "page-now");
        assert_eq!(
            labels.resolve("../../escape").unwrap().as_str(),
            "------escape"
        );
        assert_eq!(labels.resolve("東京🙂").unwrap().as_str(), "---");
        assert_eq!(labels.resolve("none").unwrap().as_str(), "none");
        assert_eq!(labels.resolve("uncertain").unwrap().as_str(), "uncertain");
        assert_eq!(labels.resolve("injected").unwrap_err().exit_code(), 69);
        for (_, name) in labels.iter() {
            assert_eq!(std::path::Path::new(name.as_str()).components().count(), 1);
            assert!(!name.as_str().contains('/'));
        }
        for bad in [
            "",
            "../label",
            ".loupe-route.jsonl",
            "UPPER",
            "bad space",
            "foo/bar",
        ] {
            assert!(Name::parse(bad).is_err(), "{bad}");
        }
        assert!(Name::parse(&"a".repeat(256)).is_err());
        assert!(serde_json::from_str::<Name>(r#""../escape""#).is_err());
    }
    #[test]
    fn sanitized_and_reserved_collisions_fail_before_files_or_commands_are_created() {
        for keys in [
            vec!["PAGE", "page"],
            vec!["a/b", "a-b"],
            vec!["東京", "大阪"],
            vec!["NONE"],
            vec!["Uncertain"],
            vec!["x", "x"],
            vec![],
        ] {
            assert_eq!(Labels::new(keys).unwrap_err().exit_code(), 64);
        }
        let labels = Labels::new(["none", "uncertain", "valid"]).unwrap();
        assert_eq!(labels.iter().count(), 3);
    }
}
