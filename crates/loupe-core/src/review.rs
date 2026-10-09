//! Durable review tasks and captured labels. Queue contents never execute commands.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use rustix::fs::{Mode, OFlags, mkdirat, open, openat};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::policy::Selected;
use crate::suites::{Case, Input, Label, Reference, Sampling};
use crate::wire::{Question, Request, Response};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub run: String,
    pub preset: String,
    pub unit: Unit,
    pub request: Request,
    pub response: Response,
    pub field: String,
    pub encoder_identity: String,
    pub sampling: Sampling,
}

impl Task {
    pub fn seal(mut self) -> Result<Self> {
        self.id = self.digest()?;
        self.validate()?;
        Ok(self)
    }
    fn digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(&(
            &self.run,
            &self.preset,
            &self.unit,
            &self.request,
            &self.response,
            &self.field,
            &self.encoder_identity,
            self.sampling,
        ))
        .map_err(json_error)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
    pub fn validate(&self) -> Result<()> {
        if self.id != self.digest()?
            || self.run.len() != 64
            || self.encoder_identity.is_empty()
            || !name(&self.preset)
        {
            return Err(invalid("invalid review task identity"));
        }
        self.response.validate(&self.request)?;
        self.question()?;
        Ok(())
    }
    pub fn question(&self) -> Result<&Question> {
        self.request
            .questions
            .get(&self.field)
            .ok_or_else(|| invalid("review task lacks its question"))
    }
    pub fn validate_label(&self, label: &Selected) -> Result<()> {
        let valid = match (self.question()?, label) {
            (Question::Noul { .. }, Selected::Boolean(_)) => true,
            (Question::Choice { criteria, .. }, Selected::Choice(label)) => {
                criteria.contains_key(label)
            }
            (Question::Score { criteria, .. }, Selected::Score(level)) => {
                level.is_finite()
                    && *level >= 0.0
                    && *level < criteria.len() as f64
                    && level.fract() == 0.0
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("review label is not a declared option"))
        }
    }
    pub fn capture(&self, label: Selected) -> Result<Case> {
        self.validate_label(&label)?;
        let label = match label {
            Selected::Boolean(value) => Label::Noul(value),
            Selected::Choice(value) => Label::Choice(value),
            Selected::Score(value) => Label::Level(value as u32),
        };
        Ok(Case{expected_spans:Vec::new(),id:self.id.clone(),input:Input::Unit{unit:self.unit.state.clone()},label,query:None,candidates:None,context:None,tags:vec!["captured".into(),format!("preset={}",self.preset)],source:"review".into(),reviewed:true,sampling:Some(self.sampling),notes:Some(json!({"run":self.run,"source_location":self.unit.location,"index":self.unit.index,"question_field":self.field}).to_string()),reference:Some(Reference{request:self.request.clone(),response:self.response.clone(),encoder_identity:self.encoder_identity.clone()})})
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Task { task: Box<Task> },
    Decision { id: String, label: Option<Selected> },
    Handled { id: String },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u32,
    record: Record,
    checksum: String,
}
impl Frame {
    fn checksum(record: &Record) -> Result<String> {
        Ok(
            blake3::hash(&serde_json::to_vec(&(1u32, record)).map_err(json_error)?)
                .to_hex()
                .to_string(),
        )
    }
}

#[derive(Clone)]
pub struct Pending {
    pub task: Task,
    pub decision: Option<Option<Selected>>,
    pub handled: bool,
}
pub struct Queue {
    file: File,
    entries: BTreeMap<String, Pending>,
    order: Vec<String>,
}
impl Queue {
    pub fn open(path: &Path, create: bool) -> Result<Self> {
        let mut file = locked_file(path, create)?;
        let bytes = read(&mut file)?;
        let committed = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |position| position + 1);
        let mut queue = Self {
            file,
            entries: BTreeMap::new(),
            order: Vec::new(),
        };
        for line in bytes[..committed]
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let frame: Frame = serde_json::from_slice(line).map_err(json_error)?;
            if frame.version != 1 || frame.checksum != Frame::checksum(&frame.record)? {
                return Err(invalid("review queue checksum/version mismatch"));
            }
            queue.apply(frame.record)?;
        }
        if committed != bytes.len() {
            queue
                .file
                .set_len(committed as u64)
                .map_err(|error| Error::io("repairing interrupted review append", error))?;
            queue.file.sync_data().map_err(io_error)?;
        }
        queue.file.seek(SeekFrom::End(0)).map_err(io_error)?;
        Ok(queue)
    }
    pub fn pending(&self) -> impl Iterator<Item = &Pending> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|pending| !pending.handled)
    }
    pub fn get(&self, id: &str) -> Option<&Pending> {
        self.entries.get(id)
    }
    pub fn insert(&mut self, task: Task) -> Result<()> {
        task.validate()?;
        if self.entries.contains_key(&task.id) {
            return Ok(());
        }
        self.append(Record::Task {
            task: Box::new(task),
        })
    }
    pub fn decide(&mut self, id: &str, label: Option<Selected>) -> Result<()> {
        if let Some(pending) = self.entries.get(id)
            && let Some(saved) = &pending.decision
        {
            if saved == &label {
                return Ok(());
            }
            return Err(invalid("review decision conflicts with a durable decision"));
        }
        self.append(Record::Decision {
            id: id.to_owned(),
            label,
        })
    }
    pub fn handled(&mut self, id: &str) -> Result<()> {
        if self.entries.get(id).is_some_and(|pending| pending.handled) {
            return Ok(());
        }
        self.append(Record::Handled { id: id.to_owned() })
    }
    fn apply(&mut self, record: Record) -> Result<()> {
        match record {
            Record::Task { task } => {
                task.validate()?;
                if self.entries.contains_key(&task.id) {
                    return Err(invalid("duplicate review task"));
                }
                self.order.push(task.id.clone());
                self.entries.insert(
                    task.id.clone(),
                    Pending {
                        task: *task,
                        decision: None,
                        handled: false,
                    },
                );
            }
            Record::Decision { id, label } => {
                let pending = self
                    .entries
                    .get_mut(&id)
                    .ok_or_else(|| invalid("review decision has no task"))?;
                if pending.decision.is_some() {
                    return Err(invalid("duplicate review decision"));
                }
                if let Some(label) = &label {
                    pending.task.validate_label(label)?;
                }
                pending.decision = Some(label);
            }
            Record::Handled { id } => {
                let pending = self
                    .entries
                    .get_mut(&id)
                    .ok_or_else(|| invalid("review receipt has no task"))?;
                if pending.decision.is_none() || pending.handled {
                    return Err(invalid("invalid review receipt"));
                }
                pending.handled = true;
            }
        }
        Ok(())
    }
    fn append(&mut self, record: Record) -> Result<()> {
        // Validate on a temporary entry before making the frame durable.
        match &record {
            Record::Decision { id, label } => {
                let entry = self
                    .entries
                    .get(id)
                    .ok_or_else(|| invalid("review decision has no task"))?;
                if entry.decision.is_some() {
                    return Err(invalid("duplicate review decision"));
                }
                if let Some(label) = label {
                    entry.task.validate_label(label)?;
                }
            }
            Record::Handled { id } => {
                if !self
                    .entries
                    .get(id)
                    .is_some_and(|entry| entry.decision.is_some() && !entry.handled)
                {
                    return Err(invalid("review receipt has no decision"));
                }
            }
            Record::Task { .. } => {}
        }
        let checksum = Frame::checksum(&record)?;
        append(
            &mut self.file,
            &Frame {
                version: 1,
                record: record.clone(),
                checksum,
            },
        )?;
        self.apply(record)
    }
}

pub struct Captures {
    file: File,
    labels: BTreeMap<String, Label>,
}
impl Captures {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = locked_file(path, true)?;
        let bytes = read(&mut file)?;
        let committed = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |position| position + 1);
        let mut labels = BTreeMap::new();
        for line in bytes[..committed]
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let case: Case = serde_json::from_slice(line).map_err(json_error)?;
            if case.source != "review"
                || !case.reviewed
                || case.sampling.is_none()
                || labels.insert(case.id, case.label).is_some()
            {
                return Err(invalid(
                    "capture file contains invalid/duplicate review cases",
                ));
            }
        }
        if committed != bytes.len() {
            file.set_len(committed as u64).map_err(io_error)?;
            file.sync_data().map_err(io_error)?;
        }
        file.seek(SeekFrom::End(0)).map_err(io_error)?;
        Ok(Self { file, labels })
    }
    pub fn existing(&self, task: &Task) -> Result<Option<Selected>> {
        self.labels
            .get(&task.id)
            .map(|label| {
                let selected = match label {
                    Label::Noul(value) => Selected::Boolean(*value),
                    Label::Choice(value) => Selected::Choice(value.clone()),
                    Label::Level(value) => Selected::Score(f64::from(*value)),
                    _ => return Err(invalid("capture label is not scalar")),
                };
                task.validate_label(&selected)?;
                Ok(selected)
            })
            .transpose()
    }
    pub fn insert(&mut self, task: &Task, label: Selected) -> Result<()> {
        let case = task.capture(label)?;
        if let Some(saved) = self.labels.get(&case.id) {
            if saved == &case.label {
                return Ok(());
            }
            return Err(invalid("capture label conflicts with durable label"));
        }
        append(&mut self.file, &case)?;
        self.labels.insert(case.id, case.label);
        Ok(())
    }
}

pub fn seed() -> Result<String> {
    let mut entropy = [0u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut entropy))
        .map_err(|error| Error::io("reading review sampling seed", error))?;
    Ok(blake3::hash(&entropy).to_hex().to_string())
}
pub fn sampling(seed: &str, index: u64, uncertain: bool, rate: f64) -> Result<Option<Sampling>> {
    if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
        return Err(Error::new(
            ErrorKind::Usage,
            "audit rate must be between 0 and 100%",
        ));
    }
    let digest = blake3::hash(&serde_json::to_vec(&(seed, index)).map_err(json_error)?);
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest.as_bytes()[..8]);
    // Use 53 random bits, with an exact uniform grid of representable fractions.
    let draw = (u64::from_le_bytes(first) >> 11) as f64 / (1u64 << 53) as f64;
    Ok(if draw < rate {
        Some(Sampling::UniformAudit)
    } else if uncertain {
        Some(Sampling::Uncertainty)
    } else {
        None
    })
}
pub fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn locked_file(path: &Path, create: bool) -> Result<File> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(io_error)?.join(path)
    };
    let mut directory = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| io_error(error.into()))?,
    );
    let parent = absolute
        .parent()
        .ok_or_else(|| invalid("review file needs a parent"))?;
    for component in parent.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                if create {
                    match mkdirat(&directory, part, Mode::from_raw_mode(0o700)) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(io_error(error.into())),
                    }
                }
                directory = File::from(
                    openat(
                        &directory,
                        part,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|error| io_error(error.into()))?,
                );
            }
            _ => return Err(invalid("review paths cannot contain parent traversal")),
        }
    }
    let leaf = absolute
        .file_name()
        .ok_or_else(|| invalid("review file needs a name"))?;
    let flags = OFlags::RDWR
        | OFlags::APPEND
        | OFlags::NOFOLLOW
        | OFlags::NONBLOCK
        | OFlags::CLOEXEC
        | if create {
            OFlags::CREATE
        } else {
            OFlags::empty()
        };
    let file = File::from(
        openat(&directory, leaf, flags, Mode::from_raw_mode(0o600))
            .map_err(|error| io_error(error.into()))?,
    );
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_BYTES {
        return Err(invalid(
            "review state must be a single-link regular file of at most 64 MiB",
        ));
    }
    file.try_lock()
        .map_err(|_| Error::new(ErrorKind::Io, "review state is busy or cannot be locked"))?;
    if create {
        directory.sync_all().map_err(io_error)?;
    }
    Ok(file)
}
fn read(file: &mut File) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("review state exceeds 64 MiB"));
    }
    Ok(bytes)
}
fn append(file: &mut File, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(json_error)?;
    bytes.push(b'\n');
    if file
        .metadata()
        .map_err(io_error)?
        .len()
        .saturating_add(bytes.len() as u64)
        > MAX_BYTES
    {
        return Err(invalid(
            "review state exceeds 64 MiB; select a new queue/capture file",
        ));
    }
    file.write_all(&bytes).map_err(io_error)?;
    file.sync_data().map_err(io_error)
}
fn json_error(error: serde_json::Error) -> Error {
    invalid(format!("review JSON: {error}"))
}
fn invalid(value: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, value)
}
fn io_error(error: std::io::Error) -> Error {
    Error::io("review state", error)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::policy::Selected;
    use crate::review::{Captures, Queue, Task, sampling};
    use crate::suites::Sampling;
    use serde_json::Value;
    use std::io::Write;
    #[test]
    fn genuine_response_tasks_and_captured_labels_survive_torn_appends_without_duplicates() {
        let evidence: Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let mut unit = crate::input::Reader::new(
            std::io::Cursor::new("Checkout is down.\n"),
            crate::input::InputFormat::Lines,
            None,
            false,
        )
        .next_unit()
        .unwrap()
        .unwrap();
        unit.state = evidence["request"]["state"].clone();
        let task = Task {
            id: String::new(),
            run: "a".repeat(64),
            preset: "outage".into(),
            unit,
            request: serde_json::from_value(evidence["request"].clone()).unwrap(),
            response: serde_json::from_value(evidence["response"].clone()).unwrap(),
            field: "outage".into(),
            encoder_identity: "test-real-reference".into(),
            sampling: Sampling::UniformAudit,
        }
        .seal()
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let queue_path = root.path().join("queue.jsonl");
        let labels_path = root.path().join("labels.jsonl");
        let mut queue = Queue::open(&queue_path, true).unwrap();
        queue.insert(task.clone()).unwrap();
        assert!(Queue::open(&queue_path, false).is_err());
        drop(queue);
        let mut labels = Captures::open(&labels_path).unwrap();
        labels.insert(&task, Selected::Boolean(true)).unwrap();
        labels.insert(&task, Selected::Boolean(true)).unwrap();
        drop(labels);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&queue_path)
            .unwrap()
            .write_all(b"{torn")
            .unwrap();
        let mut queue = Queue::open(&queue_path, false).unwrap();
        let mut labels = Captures::open(&labels_path).unwrap();
        assert_eq!(
            labels.existing(&task).unwrap(),
            Some(Selected::Boolean(true))
        );
        queue
            .decide(&task.id, labels.existing(&task).unwrap())
            .unwrap();
        queue.handled(&task.id).unwrap();
        drop(queue);
        assert_eq!(
            Queue::open(&queue_path, false).unwrap().pending().count(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(labels_path)
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(labels.insert(&task, Selected::Boolean(false)).is_err());
    }
    #[test]
    fn audit_sampling_is_independent_of_uncertainty_and_covers_both_buckets() {
        let mut audits = 0;
        for index in 0..10000 {
            let certain = sampling("fixed", index, false, 0.02).unwrap();
            let uncertain = sampling("fixed", index, true, 0.02).unwrap();
            if certain == Some(Sampling::UniformAudit) {
                audits += 1;
                assert_eq!(uncertain, certain);
            } else {
                assert_eq!(uncertain, Some(Sampling::Uncertainty));
            }
        }
        assert!((150..250).contains(&audits));
        assert_eq!(
            sampling("fixed", 0, true, 1.0).unwrap(),
            Some(Sampling::UniformAudit)
        );
        assert!(sampling("fixed", 0, false, f64::NAN).is_err());
    }
}
