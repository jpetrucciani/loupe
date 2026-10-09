use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags, linkat, openat, unlinkat};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::wire::{Request, Response};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Task {
    Noul,
    Choice,
    Score,
    Retrieval,
    Pair,
    Join,
    Parity,
    Mixed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Label {
    Noul(bool),
    Level(u32),
    Interval([u32; 2]),
    Choice(String),
    Ids(Vec<String>),
    Fields(BTreeMap<String, Label>),
}
impl Label {
    fn validate(&self, task: Task) -> Result<()> {
        let valid = match (task, self) {
            (Task::Noul | Task::Pair, Self::Noul(_)) => true,
            (Task::Choice, Self::Choice(value)) => text(value),
            (Task::Score, Self::Level(_)) => true,
            (Task::Score, Self::Interval([low, high])) => low <= high,
            (Task::Retrieval, Self::Ids(ids)) => {
                ids.iter().all(|id| text(id))
                    && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
            }
            (Task::Join, Self::Fields(fields)) => {
                !fields.is_empty() && fields.iter().all(|(id, label)| {
                    text(id) && matches!(label, Self::Ids(ids) if ids.iter().all(|id| text(id)) && ids.iter().collect::<BTreeSet<_>>().len() == ids.len())
                })
            }
            (Task::Parity | Task::Mixed, Self::Fields(fields)) => {
                !fields.is_empty()
                    && fields.iter().all(|(id, label)| {
                        text(id)
                            && match label {
                                Self::Noul(_) | Self::Level(_) => true,
                                Self::Interval([low, high]) => low <= high,
                                Self::Choice(value) => text(value),
                                Self::Ids(ids) => {
                                    ids.iter().all(|id| text(id))
                                        && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
                                }
                                Self::Fields(_) => false,
                            }
                    })
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("case label does not match its declared task"))
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Input {
    Unit { unit: Value },
    Document { doc: String },
    Pair { left: Value, right: Value },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Sampling {
    Uncertainty,
    UniformAudit,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub request: Request,
    pub response: Response,
    pub encoder_identity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "RawCase")]
pub struct Case {
    pub id: String,
    #[serde(flatten)]
    pub input: Input,
    pub label: Label,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    pub tags: Vec<String>,
    pub source: String,
    pub reviewed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Sampling>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_spans: Vec<Span>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub id: crate::candidates::Id,
    pub text: String,
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_start: u64,
    pub line_end: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    id: String,
    #[serde(default, deserialize_with = "present")]
    unit: Option<Value>,
    #[serde(default)]
    doc: Option<String>,
    #[serde(default, deserialize_with = "present")]
    left: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    right: Option<Value>,
    label: Label,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    candidates: Option<String>,
    #[serde(default)]
    context: Option<Value>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "handwritten")]
    source: String,
    #[serde(default)]
    reviewed: bool,
    #[serde(default)]
    sampling: Option<Sampling>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    reference: Option<Reference>,
    #[serde(default)]
    expected_spans: Vec<Span>,
}
fn present<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
fn handwritten() -> String {
    "handwritten".to_owned()
}
impl TryFrom<RawCase> for Case {
    type Error = Error;
    fn try_from(raw: RawCase) -> Result<Self> {
        let input = match (raw.unit, raw.doc, raw.left, raw.right) {
            (Some(unit), None, None, None) => Input::Unit { unit },
            (None, Some(doc), None, None) if doc.starts_with('@') => Input::Document { doc },
            (None, None, Some(left), Some(right)) => Input::Pair { left, right },
            _ => return Err(invalid("case requires exactly unit, @doc, or left/right")),
        };
        if !text(&raw.id)
            || !text(&raw.source)
            || raw.tags.iter().any(|tag| !text(tag))
            || (raw.sampling.is_some() && raw.source != "review")
        {
            return Err(invalid("invalid case ID/source/tags/review sampling"));
        }
        Ok(Self {
            id: raw.id,
            input,
            label: raw.label,
            query: raw.query,
            candidates: raw.candidates,
            context: raw.context,
            tags: raw.tags,
            source: raw.source,
            reviewed: raw.reviewed,
            sampling: raw.sampling,
            notes: raw.notes,
            reference: raw.reference,
            expected_spans: raw.expected_spans,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub path: PathBuf,
    pub revision: String,
    pub blake3: String,
}
impl Pin {
    /// Capture an explicitly versioned local dataset through confined regular-file reads.
    pub fn capture(root: &Path, path: &Path, revision: &str) -> Result<Self> {
        if !relative(path) || !text(revision) {
            return Err(invalid("dataset pin requires a confined path and revision"));
        }
        let descriptor = rustix::fs::open(
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding dataset directory", error.into()))?;
        let bytes = read_regular(&std::fs::File::from(descriptor), path)?;
        Ok(Self {
            path: path.to_owned(),
            revision: revision.to_owned(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
        })
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub license: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SplitConfig {
    pub seed: u64,
    pub calibrate_percent: u8,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Metric {
    Accuracy,
    MacroF1,
    Auroc,
    F1,
    Brier,
    Ece,
    Coverage,
    SelectiveAccuracy,
    Mae,
    WithinOneAccuracy,
    ProbabilityMaxError,
    ProbabilityMeanError,
    ArgmaxAgreement,
    LatencyP50Ms,
    LatencyP95Ms,
    UnitsPerSecond,
    BlockingRecall,
    AssignmentAccuracy,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub metric: Metric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_drop: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_increase: Option<f64>,
}
impl Gate {
    pub(crate) fn validate(&self) -> bool {
        [self.minimum, self.maximum, self.max_drop, self.max_increase]
            .into_iter()
            .flatten()
            .all(f64::is_finite)
            && [self.minimum, self.maximum, self.max_drop, self.max_increase]
                .into_iter()
                .any(|bound| bound.is_some())
            && !matches!((self.minimum,self.maximum),(Some(low),Some(high)) if low>high)
            && self.max_drop.is_none_or(|value| value >= 0.0)
            && self.max_increase.is_none_or(|value| value >= 0.0)
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub revision: String,
    pub preset: String,
    pub task: Task,
    pub cases: Pin,
    pub splits: PathBuf,
    pub split: SplitConfig,
    pub provenance: Provenance,
    #[serde(default)]
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub gates: Vec<Gate>,
    #[serde(default)]
    pub fixtures: Vec<Pin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<Join>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionMode {
    #[default]
    Find,
    Pick,
    Independent,
    Extract,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub mode: SelectionMode,
    pub take: std::num::NonZeroUsize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<std::num::NonZeroUsize>,
    #[serde(default)]
    pub context_lines: usize,
    #[serde(default)]
    pub keep_blank: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalize: Option<crate::normalize::Options>,
}

/// A complete labeled join catalog is one independently split scenario.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub fields: Vec<String>,
    #[serde(default)]
    pub blocking: crate::blocking::Mode,
    pub max_candidates: std::num::NonZeroUsize,
    pub max_pairs: std::num::NonZeroUsize,
    pub cardinalities: Vec<crate::matching::Cardinality>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Splits {
    pub schema_version: u32,
    pub suite_id: String,
    pub suite_revision: String,
    pub dataset_revision: String,
    pub cases_blake3: String,
    pub config: SplitConfig,
    pub calibrate: Vec<String>,
    pub test: Vec<String>,
    pub captured: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Calibrate,
    Test,
    Captured,
}

pub struct Suite {
    pub manifest: Manifest,
    pub cases: Vec<Case>,
    pub positions: Vec<u64>,
    pub locations: Vec<crate::input::Location>,
    pub root_path: PathBuf,
    root: std::fs::File,
    fixtures: BTreeMap<PathBuf, Pin>,
}
impl Suite {
    pub fn load(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .map_err(|error| Error::io("resolving suite manifest", error))?;
        let root = path
            .parent()
            .ok_or_else(|| invalid("suite manifest has no parent"))?
            .to_owned();
        let content = std::fs::read_to_string(&path)
            .map_err(|error| Error::io("reading suite manifest", error))?;
        let manifest: Manifest = toml::from_str(&content)
            .map_err(|error| invalid(format!("suite manifest: {error}")))?;
        Self::open(manifest, &root)
    }
    pub fn open(manifest: Manifest, root: &Path) -> Result<Self> {
        if manifest.schema_version != 1
            || [
                &manifest.id,
                &manifest.revision,
                &manifest.preset,
                &manifest.provenance.source,
                &manifest.provenance.license,
            ]
            .into_iter()
            .any(|value| !text(value))
            || !(1..=99).contains(&manifest.split.calibrate_percent)
            || !relative(&manifest.splits)
            || manifest.metrics.iter().collect::<BTreeSet<_>>().len() != manifest.metrics.len()
            || manifest.gates.iter().any(|gate| !gate.validate())
            || (manifest.task == Task::Join) != manifest.join.is_some()
            || (manifest.task != Task::Retrieval && manifest.selection.is_some())
        {
            return Err(invalid(
                "unsupported/invalid suite version, identity, provenance or split settings",
            ));
        }
        if let Some(join) = &manifest.join {
            if join.fields.iter().any(|field| field.contains(',')) {
                return Err(invalid(
                    "join fields are separate scalar paths, without commas",
                ));
            }
            crate::blocking::Fields::parse(&join.fields.join(","))?;
            if join.cardinalities.is_empty()
                || join
                    .cardinalities
                    .iter()
                    .enumerate()
                    .any(|(index, value)| join.cardinalities[..index].contains(value))
            {
                return Err(invalid("join cardinalities must be nonempty and distinct"));
            }
        }
        let root_path = root
            .canonicalize()
            .map_err(|error| Error::io("resolving suite directory", error))?;
        let root = std::fs::File::open(&root_path)
            .map_err(|error| Error::io("opening suite directory", error))?;
        let bytes = pinned(&root, &manifest.cases)?;
        let content = std::str::from_utf8(&bytes)
            .map_err(|error| invalid(format!("cases are not UTF-8: {error}")))?;
        let mut cases = Vec::new();
        let mut positions = Vec::new();
        let mut locations = Vec::new();
        let mut byte = 0u64;
        let mut ids = BTreeSet::new();
        for (line, value) in content.split_inclusive('\n').enumerate() {
            let start = byte;
            byte = byte
                .checked_add(
                    u64::try_from(value.len())
                        .map_err(|_| invalid("case source offset overflow"))?,
                )
                .ok_or_else(|| invalid("case source offset overflow"))?;
            if value.trim().is_empty() {
                continue;
            }
            let case: Case = serde_json::from_str(value)
                .map_err(|error| invalid(format!("case line {}: {error}", line + 1)))?;
            case.label.validate(manifest.task)?;
            if !case.expected_spans.is_empty() && manifest.task != Task::Retrieval {
                return Err(invalid("span expectations apply only to retrieval suites"));
            }
            if !ids.insert(case.id.clone()) {
                return Err(invalid(format!("duplicate case ID {}", case.id)));
            }
            if manifest.task == Task::Pair && !matches!(case.input, Input::Pair { .. }) {
                return Err(invalid("pair tasks require left/right input"));
            }
            if let Some(reference) = &case.reference {
                if manifest.task != Task::Parity || !text(&reference.encoder_identity) {
                    return Err(invalid(
                        "reference responses belong to parity cases and require encoder identity",
                    ));
                }
                reference.request.validate()?;
                reference.response.validate(&reference.request)?;
            } else if manifest.task == Task::Parity {
                return Err(invalid(
                    "parity cases require a pinned reference request/response",
                ));
            }
            let position = u64::try_from(line)
                .ok()
                .and_then(|line| line.checked_add(1))
                .ok_or_else(|| invalid("case source index overflow"))?;
            positions.push(position);
            locations.push(crate::input::Location {
                source: Some(root_path.join(&manifest.cases.path)),
                byte_start: start,
                byte_end: byte,
                line_start: position,
                line_end: position,
            });
            cases.push(case);
        }
        if cases.is_empty() {
            return Err(invalid("suite has no cases"));
        }
        let mut fixtures = BTreeMap::new();
        for pin in &manifest.fixtures {
            pinned(&root, pin)?;
            if fixtures.insert(pin.path.clone(), pin.clone()).is_some() {
                return Err(invalid("duplicate pinned fixture path"));
            }
        }
        for case in &cases {
            if let Input::Document { doc } = &case.input {
                let path = Path::new(
                    doc.strip_prefix('@')
                        .ok_or_else(|| invalid("document is not a fixture reference"))?,
                );
                if !relative(path) || !fixtures.contains_key(path) {
                    return Err(invalid(format!(
                        "case {} references an unpinned fixture",
                        case.id
                    )));
                }
            }
        }
        Ok(Self {
            manifest,
            cases,
            positions,
            locations,
            root_path,
            root,
            fixtures,
        })
    }
    pub fn state(&self, case: &Case) -> Result<Value> {
        match &case.input {
            Input::Unit { unit } => Ok(unit.clone()),
            Input::Pair { left, right } => Ok(serde_json::json!({"left":left,"right":right})),
            Input::Document { doc } => {
                let path = Path::new(
                    doc.strip_prefix('@')
                        .ok_or_else(|| invalid("document is not a fixture reference"))?,
                );
                let pin = self
                    .fixtures
                    .get(path)
                    .ok_or_else(|| invalid("document fixture is not pinned"))?;
                String::from_utf8(pinned(&self.root, pin)?)
                    .map(Value::String)
                    .map_err(|error| invalid(format!("document fixture is not UTF-8: {error}")))
            }
        }
    }
    pub fn planned_splits(&self) -> Result<Splits> {
        let mut strata = BTreeMap::<String, Vec<&Case>>::new();
        let mut captured = Vec::new();
        for case in &self.cases {
            if case.source == "review" {
                captured.push(case.id.clone());
            } else {
                let label = serde_json::to_string(&case.label).map_err(json_error)?;
                strata.entry(label).or_default().push(case);
            }
        }
        let mut calibrate = Vec::new();
        let mut test = Vec::new();
        for (label, mut cases) in strata {
            let rank = |case: &Case| {
                let mut hash = blake3::Hasher::new_derive_key("loupe evaluation split v1");
                hash.update(&self.manifest.split.seed.to_le_bytes());
                hash.update(&(label.len() as u64).to_le_bytes());
                hash.update(label.as_bytes());
                hash.update(case.id.as_bytes());
                hash.finalize()
            };
            cases.sort_by(|a, b| {
                rank(a)
                    .as_bytes()
                    .cmp(rank(b).as_bytes())
                    .then(a.id.cmp(&b.id))
            });
            let count = if cases.len() == 1 {
                1
            } else {
                (cases.len() / 100 * usize::from(self.manifest.split.calibrate_percent)
                    + cases.len() % 100 * usize::from(self.manifest.split.calibrate_percent) / 100)
                    .clamp(1, cases.len() - 1)
            };
            for (index, case) in cases.into_iter().enumerate() {
                if index < count {
                    calibrate.push(case.id.clone());
                } else {
                    test.push(case.id.clone());
                }
            }
        }
        calibrate.sort();
        test.sort();
        captured.sort();
        Ok(Splits {
            schema_version: 1,
            suite_id: self.manifest.id.clone(),
            suite_revision: self.manifest.revision.clone(),
            dataset_revision: self.manifest.cases.revision.clone(),
            cases_blake3: self.manifest.cases.blake3.clone(),
            config: self.manifest.split,
            calibrate,
            test,
            captured,
        })
    }
    pub fn load_splits(&self) -> Result<Splits> {
        let (directory, name) = self.split_directory()?;
        let bytes = read_split(&directory, &name)?.ok_or_else(|| {
            invalid("suite split is missing; create it explicitly before evaluation")
        })?;
        self.validate_splits(&bytes)
    }
    pub fn ensure_splits(&self) -> Result<Splits> {
        let (directory, name) = self.split_directory()?;
        directory.try_lock().map_err(|error| {
            Error::new(
                ErrorKind::Io,
                format!("suite split directory is already owned: {error}"),
            )
        })?;
        let outcome = self.ensure_splits_locked(&directory, &name);
        // A concurrently spawned child can briefly inherit the open file
        // description, so dropping our descriptor need not release its lock.
        let release = directory
            .unlock()
            .map_err(|error| Error::io("releasing split publication lock", error));
        let splits = outcome?;
        release?;
        Ok(splits)
    }
    fn ensure_splits_locked(
        &self,
        directory: &std::fs::File,
        name: &std::ffi::OsStr,
    ) -> Result<Splits> {
        if let Some(bytes) = read_split(directory, name)? {
            return self.validate_splits(&bytes);
        }
        let splits = self.planned_splits()?;
        let mut bytes = serde_json::to_vec_pretty(&splits).map_err(json_error)?;
        bytes.push(b'\n');
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| invalid(format!("split publication clock: {error}")))?
            .as_nanos();
        let temporary = format!(".loupe-splits-{}-{nonce}", std::process::id());
        let descriptor = openat(
            directory,
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| Error::io("creating private split publication", error.into()))?;
        let mut file = std::fs::File::from(descriptor);
        let mut publish = || -> Result<()> {
            file.write_all(&bytes)
                .map_err(|error| Error::io("writing split publication", error))?;
            file.sync_all()
                .map_err(|error| Error::io("syncing split publication", error))?;
            linkat(directory, &temporary, directory, name, AtFlags::empty())
                .map_err(|error| Error::io("publishing split without replacement", error.into()))?;
            directory
                .sync_all()
                .map_err(|error| Error::io("syncing split directory", error))
        };
        let outcome = publish();
        let cleanup = unlinkat(directory, &temporary, AtFlags::empty())
            .map_err(|error| Error::io("removing private split publication", error.into()));
        outcome?;
        cleanup?;
        directory
            .sync_all()
            .map_err(|error| Error::io("syncing split cleanup", error))?;
        Ok(splits)
    }
    pub fn cases_for<'a>(&'a self, splits: &Splits, split: Split) -> Result<Vec<&'a Case>> {
        if splits != &self.planned_splits()? {
            return Err(invalid("split identity or fixed assignments changed"));
        }
        let ids = match split {
            Split::Calibrate => &splits.calibrate,
            Split::Test => &splits.test,
            Split::Captured => &splits.captured,
        };
        let ids: BTreeSet<_> = ids.iter().collect();
        Ok(self
            .cases
            .iter()
            .filter(|case| ids.contains(&case.id))
            .collect())
    }
    fn validate_splits(&self, bytes: &[u8]) -> Result<Splits> {
        let splits: Splits = serde_json::from_slice(bytes)
            .map_err(|error| invalid(format!("persisted splits: {error}")))?;
        if splits != self.planned_splits()? {
            return Err(invalid(
                "persisted split identity/seed/assignments do not match this pinned suite",
            ));
        }
        Ok(splits)
    }
    fn split_directory(&self) -> Result<(std::fs::File, std::ffi::OsString)> {
        within(&self.root, &self.manifest.splits)
    }
}

fn read_split(directory: &std::fs::File, name: &std::ffi::OsStr) -> Result<Option<Vec<u8>>> {
    let descriptor = match openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(descriptor) => descriptor,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(Error::io("opening persisted splits", error.into())),
    };
    let mut file = std::fs::File::from(descriptor);
    if !file
        .metadata()
        .map_err(|error| Error::io("checking split file", error))?
        .is_file()
    {
        return Err(invalid("persisted splits must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| Error::io("reading persisted splits", error))?;
    Ok(Some(bytes))
}
fn pinned(root: &std::fs::File, pin: &Pin) -> Result<Vec<u8>> {
    if !relative(&pin.path)
        || !text(&pin.revision)
        || pin.blake3.len() != 64
        || !pin
            .blake3
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(invalid(
            "dataset/fixture requires a confined relative path, revision and lowercase BLAKE3 digest",
        ));
    }
    let bytes = read_regular(root, &pin.path)?;
    if blake3::hash(&bytes).to_hex().as_str() != pin.blake3 {
        return Err(invalid(format!(
            "pinned fixture digest changed: {}",
            pin.path.display()
        )));
    }
    Ok(bytes)
}
fn read_regular(root: &std::fs::File, path: &Path) -> Result<Vec<u8>> {
    let (directory, name) = within(root, path)?;
    let descriptor = openat(
        &directory,
        &name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        invalid(format!(
            "opening pinned fixture {}: {error}",
            path.display()
        ))
    })?;
    let mut file = std::fs::File::from(descriptor);
    if !file
        .metadata()
        .map_err(|error| Error::io("checking pinned fixture", error))?
        .is_file()
    {
        return Err(invalid("pinned fixture must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| Error::io("reading pinned fixture", error))?;
    Ok(bytes)
}
fn within(root: &std::fs::File, path: &Path) -> Result<(std::fs::File, std::ffi::OsString)> {
    if !relative(path) {
        return Err(invalid("suite paths must be confined relative components"));
    }
    let mut directory = std::fs::File::from(
        openat(
            root,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding suite directory", error.into()))?,
    );
    let mut components = path.components().peekable();
    while let Some(Component::Normal(name)) = components.next() {
        if components.peek().is_none() {
            return Ok((directory, name.to_owned()));
        }
        let descriptor = openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| invalid(format!("opening confined suite directory: {error}")))?;
        directory = std::fs::File::from(descriptor);
    }
    Err(invalid("suite path has no filename"))
}
fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
fn text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}
fn json_error(error: serde_json::Error) -> Error {
    invalid(format!("suite JSON: {error}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::suites::{
        Case, Input, Label, Manifest, Pin, Provenance, Sampling, Split, SplitConfig, Suite, Task,
    };
    use serde_json::json;
    use std::path::Path;

    fn write_suite(
        root: &Path,
        task: Task,
        cases: &[serde_json::Value],
        fixtures: Vec<Pin>,
    ) -> std::path::PathBuf {
        let bytes = cases
            .iter()
            .map(|case| serde_json::to_string(case).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(root.join("cases.jsonl"), &bytes).unwrap();
        let manifest = Manifest {
            schema_version: 1,
            id: "example".to_owned(),
            revision: "suite-v1".to_owned(),
            preset: "real-failure".to_owned(),
            task,
            cases: Pin {
                path: "cases.jsonl".into(),
                revision: "dataset-v1".to_owned(),
                blake3: blake3::hash(bytes.as_bytes()).to_hex().to_string(),
            },
            splits: "splits.json".into(),
            split: SplitConfig {
                seed: 42,
                calibrate_percent: 50,
            },
            provenance: Provenance {
                source: "handwritten test cases".to_owned(),
                license: "MIT".to_owned(),
            },
            metrics: vec![],
            gates: vec![],
            fixtures,
            join: None,
            selection: None,
        };
        let path = root.join("suite.toml");
        std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
        path
    }
    #[test]
    fn explicit_dataset_pins_reject_fifos_symlinks_and_traversal_without_blocking() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let bytes = b"{\"id\":\"actual\",\"unit\":null,\"label\":true}\n";
        std::fs::write(root.join("cases.jsonl"), bytes).unwrap();
        let pin = Pin::capture(root, Path::new("cases.jsonl"), "reviewed-v1").unwrap();
        assert_eq!(pin.blake3, blake3::hash(bytes).to_hex().as_str());
        let held = std::fs::File::open(root).unwrap();
        rustix::fs::mknodat(
            &held,
            "blocked",
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        std::os::unix::fs::symlink("cases.jsonl", root.join("alias")).unwrap();
        for path in ["blocked", "alias", "../outside", "."] {
            assert_eq!(
                Pin::capture(root, Path::new(path), "reviewed-v1")
                    .unwrap_err()
                    .exit_code(),
                65
            );
        }
        assert!(Pin::capture(root, Path::new("cases.jsonl"), "").is_err());
    }
    #[test]
    fn labels_and_input_shapes_round_trip_without_losing_null_or_score_intervals() {
        for value in [
            json!({"id":"null","unit":null,"label":false}),
            json!({"id":"pair","left":{"name":"a"},"right":{"name":"b"},"label":true}),
            json!({"id":"score","unit":"x","label":[1,3]}),
            json!({"id":"retrieval","doc":"@fixture.txt","label":[]}),
            json!({"id":"mixed","unit":"x","label":{"binary":true,"rank":2,"route":"none"}}),
        ] {
            let case: Case = serde_json::from_value(value).unwrap();
            let replay: Case =
                serde_json::from_value(serde_json::to_value(&case).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(&case).unwrap(),
                serde_json::to_value(replay).unwrap()
            );
        }
        for value in [
            json!({"id":"x","unit":"x","doc":"@file","label":true}),
            json!({"id":"x","left":"x","label":true}),
            json!({"id":"x","unit":"x","label":true,"typo":1}),
            json!({"id":"x","unit":"x","label":true,"sampling":"uniform-audit"}),
        ] {
            assert!(serde_json::from_value::<Case>(value).is_err());
        }
        assert!(Label::Interval([3, 1]).validate(Task::Score).is_err());
        assert!(
            Label::Ids(vec!["L1".to_owned(), "L1".to_owned()])
                .validate(Task::Retrieval)
                .is_err()
        );
    }
    #[test]
    fn fixed_seed_splits_are_stratified_complete_and_never_put_review_captures_in_test() {
        let root = tempfile::tempdir().unwrap();
        let mut cases = (0..20).map(|index| json!({"id":format!("case-{index:02}"),"unit":format!("input-{index}"),"label":index%2==0,"reviewed":true})).collect::<Vec<_>>();
        cases.push(json!({"id":"review","unit":"captured","label":true,"source":"review","sampling":"uncertainty"}));
        cases.push(json!({"id":"audit","unit":"uniform","label":false,"source":"review","sampling":"uniform-audit"}));
        let path = write_suite(root.path(), Task::Noul, &cases, vec![]);
        let mut suite = Suite::load(&path).unwrap();
        let split = suite.planned_splits().unwrap();
        assert_eq!(split.calibrate.len(), 10);
        assert_eq!(split.test.len(), 10);
        assert_eq!(split.captured, ["audit", "review"]);
        for partition in [Split::Calibrate, Split::Test] {
            let selected = suite.cases_for(&split, partition).unwrap();
            assert_eq!(
                selected
                    .iter()
                    .filter(|case| case.label == Label::Noul(true))
                    .count(),
                5
            );
            assert!(selected.iter().all(|case| case.source != "review"));
        }
        assert_eq!(
            suite
                .cases_for(&split, Split::Captured)
                .unwrap()
                .iter()
                .filter(|case| case.sampling == Some(Sampling::UniformAudit))
                .count(),
            1
        );
        suite.cases.reverse();
        assert_eq!(suite.planned_splits().unwrap(), split);
        suite.manifest.split.seed = 43;
        assert_ne!(suite.planned_splits().unwrap().calibrate, split.calibrate);
    }
    #[test]
    fn singleton_strata_are_calibration_only_and_existing_splits_are_validated_without_replacement()
    {
        let root = tempfile::tempdir().unwrap();
        let path = write_suite(
            root.path(),
            Task::Choice,
            &[
                json!({"id":"one","unit":"x","label":"one"}),
                json!({"id":"two","unit":"y","label":"two"}),
            ],
            vec![],
        );
        let suite = Suite::load(&path).unwrap();
        assert!(suite.load_splits().is_err());
        let split = suite.ensure_splits().unwrap();
        assert_eq!(split.calibrate, ["one", "two"]);
        assert!(split.test.is_empty());
        let artifact = root.path().join("splits.json");
        let original = std::fs::read(&artifact).unwrap();
        assert_eq!(suite.ensure_splits().unwrap(), split);
        assert_eq!(suite.load_splits().unwrap(), split);
        assert_eq!(std::fs::read(&artifact).unwrap(), original);
        let mut corrupt = serde_json::to_value(&split).unwrap();
        corrupt["test"] = json!(["one"]);
        let corrupt = serde_json::to_vec(&corrupt).unwrap();
        std::fs::write(&artifact, &corrupt).unwrap();
        assert_eq!(suite.ensure_splits().unwrap_err().exit_code(), 65);
        assert_eq!(std::fs::read(&artifact).unwrap(), corrupt);
        std::fs::write(&artifact, &original).unwrap();
        assert_eq!(suite.ensure_splits().unwrap(), split);
        assert!(!root.path().read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".loupe-splits-")
        }));
    }
    #[test]
    fn fixtures_are_confined_pinned_and_rechecked_when_materialized() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let fixture = root.path().join("document.txt");
        std::fs::write(&fixture, "東京🙂\r\n").unwrap();
        let pin = Pin {
            path: "document.txt".into(),
            revision: "doc-v1".to_owned(),
            blake3: blake3::hash("東京🙂\r\n".as_bytes()).to_hex().to_string(),
        };
        let path = write_suite(
            root.path(),
            Task::Retrieval,
            &[json!({"id":"doc","doc":"@document.txt","label":["L1"]})],
            vec![pin],
        );
        let suite = Suite::load(&path).unwrap();
        assert_eq!(suite.state(&suite.cases[0]).unwrap(), json!("東京🙂\r\n"));
        std::fs::write(&fixture, "changed").unwrap();
        assert_eq!(suite.state(&suite.cases[0]).unwrap_err().exit_code(), 65);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("document.txt"), "東京🙂\r\n").unwrap();
        std::fs::remove_file(&fixture).unwrap();
        symlink(outside.path().join("document.txt"), &fixture).unwrap();
        assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
        let mut manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest.fixtures.clear();
        std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
        assert!(matches!(suite.cases[0].input, Input::Document { .. }));
    }
    #[test]
    fn split_symlinks_and_changed_dataset_revisions_are_rejected_without_writes() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let path = write_suite(
            root.path(),
            Task::Noul,
            &[json!({"id":"one","unit":"x","label":true})],
            vec![],
        );
        let suite = Suite::load(&path).unwrap();
        suite.ensure_splits().unwrap();
        let artifact = root.path().join("splits.json");
        assert_eq!(
            std::fs::metadata(&artifact).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let original = std::fs::read(&artifact).unwrap();
        let mut manifest = suite.manifest.clone();
        manifest.cases.revision = "dataset-v2".to_owned();
        std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(
            Suite::load(&path)
                .unwrap()
                .ensure_splits()
                .unwrap_err()
                .exit_code(),
            65
        );
        assert_eq!(std::fs::read(&artifact).unwrap(), original);
        std::fs::rename(&artifact, root.path().join("original.json")).unwrap();
        symlink("original.json", &artifact).unwrap();
        assert!(suite.ensure_splits().is_err());
        assert_eq!(std::fs::read(&artifact).unwrap(), original);
    }

    #[test]
    fn malformed_cases_and_changed_pins_fail_before_creating_splits() {
        let root = tempfile::tempdir().unwrap();
        for cases in [
            vec![],
            vec![json!({"id":"one","unit":"x","label":"true"})],
            vec![
                json!({"id":"one","unit":"x","label":true}),
                json!({"id":"one","unit":"y","label":false}),
            ],
        ] {
            let path = write_suite(root.path(), Task::Noul, &cases, vec![]);
            assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
            assert!(!root.path().join("splits.json").exists());
        }
        let path = write_suite(
            root.path(),
            Task::Noul,
            &[json!({"id":"one","unit":"x","label":true})],
            vec![],
        );
        let mut manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for percent in [0, 100] {
            manifest.split.calibrate_percent = percent;
            std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
            assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
        }
        manifest.split.calibrate_percent = 50;
        manifest.schema_version = 2;
        std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
        manifest.schema_version = 1;
        std::fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
        std::fs::write(
            root.path().join("cases.jsonl"),
            "{\"id\":\"changed\",\"unit\":\"x\",\"label\":true}\n",
        )
        .unwrap();
        assert_eq!(Suite::load(&path).err().unwrap().exit_code(), 65);
        assert!(!root.path().join("splits.json").exists());
    }

    #[test]
    fn publication_lock_is_scoped_and_held_root_survives_path_replacement() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("suite");
        std::fs::create_dir(&root).unwrap();
        let path = write_suite(
            &root,
            Task::Noul,
            &[json!({"id":"one","unit":"x","label":true})],
            vec![],
        );
        let suite = Suite::load(&path).unwrap();
        let owner = std::fs::File::open(&root).unwrap();
        owner.try_lock().unwrap();
        assert!(suite.ensure_splits().is_err());
        assert!(!root.join("splits.json").exists());
        owner.unlock().unwrap();
        suite.ensure_splits().unwrap();
        let another = Suite::load(&path).unwrap();
        assert_eq!(
            another.ensure_splits().unwrap(),
            suite.load_splits().unwrap()
        );
        let held = scratch.path().join("held");
        std::fs::rename(&root, &held).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("splits.json"), "unrelated data\n").unwrap();
        assert_eq!(
            suite.ensure_splits().unwrap(),
            another.load_splits().unwrap()
        );
        assert_eq!(
            std::fs::read(root.join("splits.json")).unwrap(),
            b"unrelated data\n"
        );
    }
}
