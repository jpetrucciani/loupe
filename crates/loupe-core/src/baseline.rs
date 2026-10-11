use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{Mode, OFlags, open, openat};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::suites::Gate;
use crate::task_metrics::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ratification {
    pub by: String,
    pub at: String,
    pub justification: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub suite: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default = "one")]
    pub minimum_support: NonZeroUsize,
    pub gate: Gate,
}

fn one() -> NonZeroUsize {
    NonZeroUsize::MIN
}

/// Explicit human-authored floors over a pinned, published evaluator report.
/// The report, rather than a model alias in this file, supplies execution identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub report: PathBuf,
    pub report_blake3: String,
    pub ratification: Ratification,
    pub checks: Vec<Check>,
}

pub struct Loaded {
    pub record: Record,
    pub content_blake3: String,
    pub report: Vec<u8>,
}

/// Read a ratification and its sealed report without creating files or following links.
pub fn load(path: &Path) -> Result<Loaded> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| invalid("baseline file name missing"))?;
    let directory = File::from(
        open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding baseline directory", error.into()))?,
    );
    let file = File::from(
        openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("opening baseline record", error.into()))?,
    );
    let metadata = file
        .metadata()
        .map_err(|error| Error::io("checking baseline record", error))?;
    const CAP: u64 = 256 * 1024;
    if !metadata.is_file() || metadata.len() > CAP {
        return Err(invalid("baseline must be a bounded regular file"));
    }
    let mut bytes = Vec::new();
    file.take(CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io("reading baseline record", error))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(invalid("baseline file changed during reading"));
    }
    let document = crate::presets::parse_document(path, &bytes)?;
    let record: Record = serde_json::from_value(document)
        .map_err(|error| invalid(&format!("invalid baseline record: {error}")))?;
    record.validate()?;
    let report = crate::report_store::read_relative(&directory, &record.report)?;
    if blake3::hash(&report).to_hex().as_str() != record.report_blake3 {
        return Err(invalid(
            "baseline report differs from the human-ratified digest",
        ));
    }
    Ok(Loaded {
        record,
        content_blake3: blake3::hash(&bytes).to_hex().to_string(),
        report,
    })
}

impl Record {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.report.as_os_str().is_empty()
            || self
                .report
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || self.report_blake3.len() != 64
            || !self
                .report_blake3
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.ratification.by.trim().is_empty()
            || self.ratification.justification.trim().is_empty()
            || chrono::DateTime::parse_from_rfc3339(&self.ratification.at).is_err()
            || self.checks.is_empty()
        {
            return Err(invalid(
                "baseline requires a confined pinned report, human ratification, RFC 3339 date, written justification and explicit checks",
            ));
        }
        let mut identities = BTreeSet::new();
        for check in &self.checks {
            if check.suite.trim().is_empty()
                || check
                    .field
                    .as_ref()
                    .is_some_and(|field| field.trim().is_empty())
                || !check.gate.validate()
                || check.gate.minimum.is_none() && check.gate.maximum.is_none()
                || !identities.insert((&check.suite, &check.field, check.gate.metric))
            {
                return Err(invalid(
                    "baseline checks require unique suite/field/metric targets and an explicit absolute floor or ceiling; regression bounds alone cannot ratify the initial run",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Constraint {
    Minimum,
    Maximum,
    MaxDrop,
    MaxIncrease,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bound {
    pub constraint: Constraint,
    pub limit: f64,
    pub observed: Option<f64>,
    pub passed: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    CurrentUndefined,
    BaselineUndefined,
    InsufficientCurrentSupport,
    InsufficientBaselineSupport,
    NonFiniteDifference,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub current: Value,
    pub baseline: Value,
    pub minimum_support: usize,
    pub bounds: Vec<Bound>,
    pub reasons: Vec<Reason>,
    pub passed: bool,
}

pub fn assess(check: &Check, current: &Value, baseline: &Value) -> Result<Assessment> {
    if !check.gate.validate() {
        return Err(invalid("invalid gate bounds"));
    }
    let mut reasons = Vec::new();
    let value = |metric: &Value| {
        metric
            .value
            .filter(|value| value.is_finite() && metric.undefined.is_none())
    };
    let actual = value(current);
    let reference = value(baseline);
    if actual.is_none() {
        reasons.push(Reason::CurrentUndefined);
    }
    if reference.is_none() {
        reasons.push(Reason::BaselineUndefined);
    }
    if current.support < check.minimum_support.get() {
        reasons.push(Reason::InsufficientCurrentSupport);
    }
    if baseline.support < check.minimum_support.get() {
        reasons.push(Reason::InsufficientBaselineSupport);
    }
    let mut bounds = Vec::new();
    for (constraint, limit) in [
        (Constraint::Minimum, check.gate.minimum),
        (Constraint::Maximum, check.gate.maximum),
        (Constraint::MaxDrop, check.gate.max_drop),
        (Constraint::MaxIncrease, check.gate.max_increase),
    ] {
        let Some(limit) = limit else {
            continue;
        };
        let observed = match constraint {
            Constraint::Minimum | Constraint::Maximum => actual,
            Constraint::MaxDrop => reference
                .zip(actual)
                .map(|(reference, actual)| reference - actual),
            Constraint::MaxIncrease => reference
                .zip(actual)
                .map(|(reference, actual)| actual - reference),
        };
        if observed.is_some_and(|value| !value.is_finite()) {
            reasons.push(Reason::NonFiniteDifference);
        }
        let observed = observed.filter(|value| value.is_finite());
        let passed = observed.map(|observed| match constraint {
            Constraint::Minimum => observed >= limit,
            _ => observed <= limit,
        });
        bounds.push(Bound {
            constraint,
            limit,
            observed,
            passed,
        });
    }
    let passed = reasons.is_empty() && bounds.iter().all(|bound| bound.passed == Some(true));
    Ok(Assessment {
        current: current.clone(),
        baseline: baseline.clone(),
        minimum_support: check.minimum_support.get(),
        bounds,
        reasons,
        passed,
    })
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::baseline::{Check, Constraint, Reason, Record, assess};
    use crate::suites::{Gate, Metric};
    use crate::task_metrics::{Undefined, Value};
    use std::num::NonZeroUsize;

    fn check() -> Check {
        Check {
            suite: "rubric".into(),
            field: Some("decision".into()),
            minimum_support: NonZeroUsize::new(10).unwrap(),
            gate: Gate {
                metric: Metric::Accuracy,
                minimum: Some(0.8),
                maximum: Some(1.0),
                max_drop: Some(0.1),
                max_increase: Some(0.2),
            },
        }
    }
    fn number(value: f64, support: usize) -> Value {
        Value {
            value: Some(value),
            support,
            undefined: None,
        }
    }

    #[test]
    fn numerical_absolute_and_regression_bounds_are_inclusive_and_improvements_are_signed() {
        let check = check();
        let baseline = number(0.9, 20);
        let accepted = assess(&check, &number(0.8, 10), &baseline).unwrap();
        assert!(accepted.passed);
        assert_eq!(accepted.bounds[2].constraint, Constraint::MaxDrop);
        assert!(assess(&check, &number(0.9, 10), &baseline).unwrap().passed);
        assert!(!assess(&check, &number(0.7, 10), &baseline).unwrap().passed);
        let improvement = assess(&check, &number(1.0, 10), &baseline).unwrap();
        assert!(improvement.passed);
        assert!(improvement.bounds[2].observed.unwrap() < 0.0);
        let mut latency = check.clone();
        latency.gate.minimum = None;
        latency.gate.maximum = Some(200.0);
        latency.gate.max_drop = None;
        latency.gate.max_increase = Some(10.0);
        assert!(
            assess(&latency, &number(110.0, 10), &number(100.0, 10))
                .unwrap()
                .passed
        );
        assert!(
            !assess(&latency, &number(111.0, 10), &number(100.0, 10))
                .unwrap()
                .passed
        );
    }

    #[test]
    fn undefined_insufficient_and_nonfinite_values_never_pass_or_become_zero() {
        let check = check();
        let baseline = number(0.9, 20);
        let missing = Value {
            value: None,
            support: 0,
            undefined: Some(Undefined::NoSamples),
        };
        let report = assess(&check, &missing, &baseline).unwrap();
        assert!(!report.passed);
        assert!(report.reasons.contains(&Reason::CurrentUndefined));
        assert!(
            report
                .bounds
                .iter()
                .all(|bound| bound.observed.is_none() && bound.passed.is_none())
        );
        assert!(!assess(&check, &number(1.0, 9), &baseline).unwrap().passed);
        assert!(!assess(&check, &number(1.0, 10), &missing).unwrap().passed);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                !assess(&check, &number(value, 10), &baseline)
                    .unwrap()
                    .passed
            );
        }
        let report = assess(&check, &number(f64::MAX, 10), &number(-f64::MAX, 10)).unwrap();
        assert!(report.reasons.contains(&Reason::NonFiniteDifference));
        let mut invalid = check;
        invalid.gate.max_drop = Some(-0.1);
        assert!(assess(&invalid, &baseline, &baseline).is_err());
    }

    #[test]
    fn ratification_requires_explicit_floors_and_pins_and_round_trips_without_inventing_identity() {
        let document = serde_json::json!({"schema_version":1,"report":"reports/measured","report_blake3":"a".repeat(64),"ratification":{"by":"human","at":"2026-10-06T22:00:00Z","justification":"Written floor chosen after inspecting the held-out report."},"checks":[{"suite":"rubric","field":"decision","minimum_support":10,"gate":{"metric":"accuracy","minimum":0.8,"max_drop":0.1}}]});
        let record: Record = serde_json::from_value(document.clone()).unwrap();
        record.validate().unwrap();
        let text = toml::to_string(&record).unwrap();
        toml::from_str::<Record>(&text).unwrap().validate().unwrap();
        for (field, value) in [
            ("report", serde_json::json!("../outside")),
            ("report_blake3", serde_json::json!("alias")),
            ("schema_version", serde_json::json!(2)),
        ] {
            let mut changed = document.clone();
            changed[field] = value;
            assert!(
                serde_json::from_value::<Record>(changed)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for field in ["by", "at", "justification"] {
            let mut changed = document.clone();
            changed["ratification"][field] = serde_json::json!(" ");
            assert!(
                serde_json::from_value::<Record>(changed)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut changed = document.clone();
        changed["checks"][0]["gate"]
            .as_object_mut()
            .unwrap()
            .remove("minimum");
        assert!(
            serde_json::from_value::<Record>(changed)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut changed = document.clone();
        changed["checks"][0]["minimum_support"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Record>(changed).is_err());
        let mut duplicate = record.clone();
        duplicate.checks.push(duplicate.checks[0].clone());
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn real_file_loader_pins_sealed_bytes_and_refuses_links_changed_content_and_special_files() {
        use crate::baseline::{Ratification, load};
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        // Filesystem/codec bytes only, not a simulated evaluator/backend response.
        let bytes = b"{\"codec_bytes\":true}\n";
        let store = crate::report_store::Store::open(&root.join("reports")).unwrap();
        let bundle = store.publish(bytes, b"Codec filesystem fixture\n").unwrap();
        let record = Record {
            schema_version: 1,
            report: bundle.strip_prefix(root).unwrap().to_owned(),
            report_blake3: blake3::hash(bytes).to_hex().to_string(),
            ratification: Ratification {
                by: "codec fixture".into(),
                at: "2026-10-06T22:00:00Z".into(),
                justification: "Codec fixture; no production human ratification claimed.".into(),
            },
            checks: vec![check()],
        };
        let path = root.join("baseline.toml");
        let text = toml::to_string(&record).unwrap();
        std::fs::write(&path, &text).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.report, bytes);
        assert_eq!(
            loaded.content_blake3,
            blake3::hash(text.as_bytes()).to_hex().as_str()
        );
        symlink(&path, root.join("alias.toml")).unwrap();
        assert!(load(&root.join("alias.toml")).is_err());
        assert!(
            std::process::Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(root.join("fifo.toml"))
                .status()
                .unwrap()
                .success()
        );
        assert!(load(&root.join("fifo.toml")).is_err());
        assert!(load(&root.join("reports")).is_err());
        let mut changed = record.clone();
        changed.report_blake3 = "0".repeat(64);
        std::fs::write(&path, toml::to_string(&changed).unwrap()).unwrap();
        assert!(load(&path).is_err());
        std::fs::write(&path, &text).unwrap();
        std::fs::write(bundle.join("report.md"), b"Changed sealed bytes\n").unwrap();
        assert!(load(&path).is_err());
        std::fs::write(&path, " ".repeat(256 * 1024 + 1)).unwrap();
        assert!(load(&path).is_err());
    }
}
