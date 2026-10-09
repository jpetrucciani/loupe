//! Portable run journal. The JSONL file is authoritative; the index is disposable.
//! Callers persist admissions before enqueueing and results before ordered consumption.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::ops::Bound;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use rustix::fs::{Mode, OFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;

const VERSION: u64 = 1;
const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const UNITS: TableDefinition<u64, &[u8]> = TableDefinition::new("units");
pub const INDEX_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Emitted,
    Omitted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub index: u64,
    pub completed: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    Begin {
        identity: Value,
    },
    Admit {
        unit: Box<Unit>,
    },
    Plan {
        indices: Vec<u64>,
        provenance: Value,
    },
    Ready {
        index: u64,
        completed: Value,
    },
    ReadyMany {
        completions: Vec<Completion>,
    },
    Fallback {
        index: u64,
        execution: Value,
    },
    ReviewRun {
        seed: String,
    },
    Review {
        index: u64,
        decision: Value,
    },
    Failed {
        index: Option<u64>,
        code: u8,
        message: String,
    },
    Handled {
        index: u64,
        disposition: Disposition,
    },
    InputEnd {
        fingerprint: Value,
    },
    OutputCheckpoint {
        namespace: String,
        index: u64,
        emitted: u64,
    },
    Finish {
        emitted: u64,
        exit_code: u8,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u64,
    sequence: u64,
    previous: String,
    record: Record,
    checksum: String,
}
impl Frame {
    fn digest(sequence: u64, previous: &str, record: &Record) -> Result<String> {
        let bytes =
            serde_json::to_vec(&(VERSION, sequence, previous, record)).map_err(json_error)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    admit: u64,
    plan: Option<u64>,
    ready: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fallback: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review: Option<u64>,
    failed: Option<u64>,
    handled: Option<Disposition>,
}

pub struct Saved {
    pub unit: Unit,
    pub plan: Option<Value>,
    pub completed: Option<Value>,
    pub fallback: Option<Value>,
    pub review: Option<Value>,
    pub failure: Option<(u8, String)>,
    pub handled: Option<Disposition>,
}

pub struct Store {
    file: File,
    index: Database,
    identity: Option<Value>,
    sequence: u64,
    checksum: String,
    last_index: u64,
    input_end: Option<Value>,
    output_checkpoints: BTreeMap<String, (u64, u64)>,
    finished: Option<(u64, u8)>,
    poisoned: bool,
    review_seed: Option<String>,
    #[cfg(test)]
    crash_at: Option<&'static str>,
}

impl Drop for Store {
    fn drop(&mut self) {
        // Explicit unlock also handles a descriptor temporarily inherited before exec.
        let _ = self.file.unlock();
    }
}

impl Store {
    /// Refuses existing artifacts and aliases before any journal mutation.
    pub fn create(path: &Path, identity: Value, protected: &[(u64, u64)]) -> Result<Self> {
        if !identity.is_object() {
            return Err(Error::new(
                ErrorKind::Usage,
                "recovery identity must be an object",
            ));
        }
        let (file, directory) = open(path, true, protected)?;
        let mut store = Self::empty(file)?;
        store.append(Record::Begin { identity })?;
        directory
            .sync_all()
            .map_err(|error| Error::io("syncing recovery journal directory", error))?;
        Ok(store)
    }

    /// Validates the complete recorded chain and run identity before repairing a torn tail.
    pub fn resume(path: &Path, identity: &Value, protected: &[(u64, u64)]) -> Result<Self> {
        let (file, _) = open(path, false, protected)?;
        let mut store = Self::empty(file)?;
        let torn = store.scan()?;
        if store.identity.as_ref() != Some(identity) {
            return Err(Error::new(
                ErrorKind::Input,
                "recovery run identity changed or missing",
            ));
        }
        if let Some(length) = torn {
            store
                .file
                .set_len(length)
                .and_then(|_| store.file.sync_all())
                .map_err(|error| Error::io("repairing torn recovery journal tail", error))?;
        }
        Ok(store)
    }

    fn empty(file: File) -> Result<Self> {
        let index = Database::builder()
            .set_cache_size(INDEX_CACHE_BYTES)
            .create_file(anonymous_index()?)
            .map_err(db_error)?;
        let mut write = index.begin_write().map_err(db_error)?;
        write.set_durability(Durability::None).map_err(db_error)?;
        {
            let _ = write.open_table(UNITS).map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(Self {
            file,
            index,
            identity: None,
            sequence: 0,
            checksum: ZERO.to_owned(),
            last_index: 0,
            input_end: None,
            output_checkpoints: BTreeMap::new(),
            finished: None,
            poisoned: false,
            review_seed: None,
            #[cfg(test)]
            crash_at: None,
        })
    }

    pub fn last_index(&self) -> u64 {
        self.last_index
    }
    pub fn file_identity(&self) -> Result<(u64, u64)> {
        let metadata = self
            .file
            .metadata()
            .map_err(|error| Error::io("reading recovery identity", error))?;
        Ok((metadata.dev(), metadata.ino()))
    }
    pub fn disposition(&self, index: u64) -> Result<Option<Disposition>> {
        Ok(self.entry(index)?.handled)
    }
    pub fn input_end(&self) -> Option<&Value> {
        self.input_end.as_ref()
    }
    pub fn finished(&self) -> Option<(u64, u8)> {
        self.finished
    }

    pub fn admit(&mut self, unit: &Unit) -> Result<()> {
        self.append(Record::Admit {
            unit: Box::new(unit.clone()),
        })
    }
    pub fn plan(&mut self, indices: Vec<u64>, provenance: Value) -> Result<()> {
        self.append(Record::Plan {
            indices,
            provenance,
        })
    }
    pub fn ready(&mut self, index: u64, completed: Value) -> Result<()> {
        self.append(Record::Ready { index, completed })
    }
    pub fn ready_many(&mut self, completions: Vec<Completion>) -> Result<()> {
        self.append(Record::ReadyMany { completions })
    }
    pub fn fallback(&mut self, index: u64, execution: Value) -> Result<()> {
        self.append(Record::Fallback { index, execution })
    }
    pub fn review_seed(&mut self) -> Result<String> {
        if let Some(seed) = &self.review_seed {
            return Ok(seed.clone());
        }
        let seed = crate::review::seed()?;
        self.append(Record::ReviewRun { seed: seed.clone() })?;
        Ok(seed)
    }
    pub fn review(&mut self, index: u64, decision: Value) -> Result<()> {
        self.append(Record::Review { index, decision })
    }
    pub fn failed(&mut self, index: Option<u64>, code: u8, message: String) -> Result<()> {
        self.append(Record::Failed {
            index,
            code,
            message,
        })
    }
    pub fn handled(&mut self, index: u64, disposition: Disposition) -> Result<()> {
        let entry = self.entry(index)?;
        if entry.handled == Some(disposition) {
            return Ok(());
        }
        self.append(Record::Handled { index, disposition })
    }
    pub fn end_input(&mut self, fingerprint: Value) -> Result<()> {
        if self.input_end.as_ref() == Some(&fingerprint) {
            return Ok(());
        }
        self.append(Record::InputEnd { fingerprint })
    }
    /// A finished aggregate acknowledges its results after the caller flushes stdout.
    pub fn finish(&mut self, emitted: u64, exit_code: u8) -> Result<()> {
        if self.finished == Some((emitted, exit_code)) {
            return Ok(());
        }
        self.append(Record::Finish { emitted, exit_code })
    }
    pub fn output_checkpoint(&self, namespace: &str) -> Option<(u64, u64)> {
        self.output_checkpoints.get(namespace).copied()
    }
    pub fn checkpoint_output(&mut self, namespace: &str, index: u64, emitted: u64) -> Result<()> {
        if self.output_checkpoint(namespace) == Some((index, emitted)) {
            return Ok(());
        }
        self.append(Record::OutputCheckpoint {
            namespace: namespace.to_owned(),
            index,
            emitted,
        })
    }

    pub fn next_index(&self, after: u64) -> Result<Option<u64>> {
        let read = self.index.begin_read().map_err(db_error)?;
        let table = read.open_table(UNITS).map_err(db_error)?;
        let next = table
            .range((Bound::Excluded(after), Bound::Unbounded))
            .map_err(db_error)?
            .next();
        next.map(|entry| entry.map(|(key, _)| key.value()).map_err(db_error))
            .transpose()
    }
    pub fn saved(&mut self, index: u64) -> Result<Saved> {
        let entry = self.entry(index)?;
        let unit = match self.record(entry.admit)? {
            Record::Admit { unit } if unit.index == index => *unit,
            _ => return Err(invalid("recovery admission index is inconsistent")),
        };
        let plan = match entry.plan.map(|offset| self.record(offset)).transpose()? {
            Some(Record::Plan {
                indices,
                provenance,
            }) if indices.contains(&index) => Some(provenance),
            None => None,
            _ => return Err(invalid("recovery plan index is inconsistent")),
        };
        let completed = match entry.ready.map(|offset| self.record(offset)).transpose()? {
            Some(Record::Ready {
                index: recorded,
                completed,
            }) if recorded == index => Some(completed),
            Some(Record::ReadyMany { completions }) => Some(
                completions
                    .into_iter()
                    .find(|completion| completion.index == index)
                    .ok_or_else(|| invalid("recovery group result index is inconsistent"))?
                    .completed,
            ),
            None => None,
            _ => return Err(invalid("recovery result index is inconsistent")),
        };
        let failure = match entry.failed.map(|offset| self.record(offset)).transpose()? {
            Some(Record::Failed {
                index: Some(recorded),
                code,
                message,
            }) if recorded == index => Some((code, message)),
            None => None,
            _ => return Err(invalid("recovery failure index is inconsistent")),
        };
        let fallback = match entry
            .fallback
            .map(|offset| self.record(offset))
            .transpose()?
        {
            Some(Record::Fallback {
                index: recorded,
                execution,
            }) if recorded == index => Some(execution),
            None => None,
            _ => return Err(invalid("recovery fallback index is inconsistent")),
        };
        let review = match entry.review.map(|offset| self.record(offset)).transpose()? {
            Some(Record::Review {
                index: recorded,
                decision,
            }) if recorded == index => Some(decision),
            None => None,
            _ => return Err(invalid("recovery review index is inconsistent")),
        };
        Ok(Saved {
            unit,
            plan,
            completed,
            fallback,
            review,
            failure,
            handled: entry.handled,
        })
    }
    pub fn verify(&mut self, unit: &Unit) -> Result<()> {
        let admission = self.entry(unit.index)?.admit;
        let original = match self.record(admission)? {
            Record::Admit { unit } => unit,
            _ => return Err(invalid("recovery admission offset is invalid")),
        };
        if *original != *unit {
            return Err(Error::new(
                ErrorKind::Input,
                format!("recovery input changed at unit {}", unit.index),
            ));
        }
        Ok(())
    }

    fn entry(&self, index: u64) -> Result<Entry> {
        let read = self.index.begin_read().map_err(db_error)?;
        let table = read.open_table(UNITS).map_err(db_error)?;
        let entry = table
            .get(index)
            .map_err(db_error)?
            .ok_or_else(|| invalid("recovery references an unadmitted unit"))?;
        serde_json::from_slice(entry.value()).map_err(json_error)
    }
    fn update(&self, index: u64, entry: &Entry) -> Result<()> {
        let bytes = serde_json::to_vec(entry).map_err(json_error)?;
        let mut write = self.index.begin_write().map_err(db_error)?;
        write.set_durability(Durability::None).map_err(db_error)?;
        {
            write
                .open_table(UNITS)
                .map_err(db_error)?
                .insert(index, bytes.as_slice())
                .map_err(db_error)?;
        }
        write.commit().map_err(db_error)
    }

    fn check(&self, record: &Record) -> Result<()> {
        if self.finished.is_some() {
            return Err(invalid("recovery run is already finished"));
        }
        if self.identity.is_none() && !matches!(record, Record::Begin { .. }) {
            return Err(invalid("recovery journal has no begin"));
        }
        match record {
            Record::ReviewRun { seed } => {
                if self.review_seed.is_some()
                    || seed.len() != 64
                    || !seed.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(invalid("invalid/duplicate review sampling seed"));
                }
            }
            Record::Review { index, decision } => {
                let entry = self.entry(*index)?;
                if entry.ready.is_none()
                    || entry.review.is_some()
                    || entry.handled.is_some()
                    || !decision.is_object()
                {
                    return Err(invalid(
                        "recovery review requires a completed unhandled result and is recorded once",
                    ));
                }
            }
            Record::Begin { identity } => {
                if self.sequence != 0 || !identity.is_object() {
                    return Err(invalid("invalid or duplicate recovery begin"));
                }
            }
            Record::Admit { unit } => {
                if self.input_end.is_some() || unit.index <= self.last_index {
                    return Err(invalid(
                        "recovery admissions are not increasing or input already ended",
                    ));
                }
            }
            Record::Plan { indices, .. } => {
                if indices.is_empty() || indices.windows(2).any(|pair| pair[0] >= pair[1]) {
                    return Err(invalid(
                        "recovery plan indices must be nonempty and increasing",
                    ));
                }
                for index in indices {
                    let entry = self.entry(*index)?;
                    if entry.handled.is_some() || entry.ready.is_some() {
                        return Err(invalid(
                            "recovery cannot replan a completed or handled unit",
                        ));
                    }
                }
            }
            Record::Ready { index, .. } => {
                let entry = self.entry(*index)?;
                if entry.ready.is_some() || entry.handled.is_some() {
                    return Err(invalid("recovery result is duplicate or already handled"));
                }
            }
            Record::ReadyMany { completions } => {
                if completions.is_empty()
                    || completions
                        .windows(2)
                        .any(|pair| pair[0].index >= pair[1].index)
                {
                    return Err(invalid(
                        "recovery group results must be nonempty and increasing",
                    ));
                }
                for completion in completions {
                    self.check(&Record::Ready {
                        index: completion.index,
                        completed: Value::Null,
                    })?;
                }
            }
            Record::Fallback { index, execution } => {
                let entry = self.entry(*index)?;
                if entry.ready.is_none()
                    || entry.fallback.is_some()
                    || entry.handled.is_some()
                    || !execution.is_object()
                {
                    return Err(invalid(
                        "recovery fallback requires a completed unhandled result and is recorded once",
                    ));
                }
            }
            Record::Failed {
                index,
                code,
                message,
            } => {
                if *code == 0 || message.is_empty() {
                    return Err(invalid("invalid recovery failure"));
                }
                if let Some(index) = index
                    && self.entry(*index)?.handled.is_some()
                {
                    return Err(invalid("recovery cannot fail a handled unit"));
                }
            }
            Record::Handled { index, .. } => {
                let entry = self.entry(*index)?;
                if entry.handled.is_some() || entry.ready.is_none() && entry.failed.is_none() {
                    return Err(invalid("recovery handling has no outcome or is duplicate"));
                }
            }
            Record::InputEnd { .. } => {
                if self.input_end.is_some() {
                    return Err(invalid("duplicate recovery input end"));
                }
            }
            Record::OutputCheckpoint {
                namespace,
                index,
                emitted,
            } => {
                if self.input_end.is_none()
                    || namespace.is_empty()
                    || namespace.len() > 64
                    || !namespace
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                    || *index == 0
                    || *emitted == 0
                    || self
                        .output_checkpoint(namespace)
                        .is_some_and(|(previous, count)| *index <= previous || *emitted <= count)
                {
                    return Err(invalid("invalid or non-increasing output checkpoint"));
                }
            }
            Record::Finish { exit_code, .. } => {
                if self.input_end.is_none() || !matches!(*exit_code, 0..=2) {
                    return Err(invalid(
                        "recovery finish requires input EOF and a normal exit",
                    ));
                }
                let read = self.index.begin_read().map_err(db_error)?;
                let table = read.open_table(UNITS).map_err(db_error)?;
                for row in table.iter().map_err(db_error)? {
                    let (_, value) = row.map_err(db_error)?;
                    let entry: Entry = serde_json::from_slice(value.value()).map_err(json_error)?;
                    if entry.ready.is_none() && entry.handled.is_none() && entry.failed.is_none() {
                        return Err(invalid("recovery finish would omit pending work"));
                    }
                }
            }
        }
        Ok(())
    }
    fn apply(&mut self, offset: u64, record: &Record) -> Result<()> {
        match record {
            Record::ReviewRun { seed } => self.review_seed = Some(seed.clone()),
            Record::Review { index, .. } => {
                let mut entry = self.entry(*index)?;
                entry.review = Some(offset);
                self.update(*index, &entry)?;
            }
            Record::Begin { identity } => self.identity = Some(identity.clone()),
            Record::Admit { unit } => {
                self.update(
                    unit.index,
                    &Entry {
                        admit: offset,
                        plan: None,
                        ready: None,
                        fallback: None,
                        review: None,
                        failed: None,
                        handled: None,
                    },
                )?;
                self.last_index = unit.index;
            }
            Record::Plan { indices, .. } => {
                for index in indices {
                    let mut entry = self.entry(*index)?;
                    entry.plan = Some(offset);
                    self.update(*index, &entry)?;
                }
            }
            Record::Ready { index, .. } => {
                let mut entry = self.entry(*index)?;
                entry.ready = Some(offset);
                self.update(*index, &entry)?;
            }
            Record::ReadyMany { completions } => {
                for completion in completions {
                    let mut entry = self.entry(completion.index)?;
                    entry.ready = Some(offset);
                    self.update(completion.index, &entry)?;
                }
            }
            Record::Fallback { index, .. } => {
                let mut entry = self.entry(*index)?;
                entry.fallback = Some(offset);
                self.update(*index, &entry)?;
            }
            Record::Failed {
                index: Some(index), ..
            } => {
                let mut entry = self.entry(*index)?;
                entry.failed = Some(offset);
                self.update(*index, &entry)?;
            }
            Record::Failed { index: None, .. } => {}
            Record::Handled { index, disposition } => {
                let mut entry = self.entry(*index)?;
                entry.handled = Some(*disposition);
                self.update(*index, &entry)?;
            }
            Record::InputEnd { fingerprint } => self.input_end = Some(fingerprint.clone()),
            Record::OutputCheckpoint {
                namespace,
                index,
                emitted,
            } => {
                self.output_checkpoints
                    .insert(namespace.clone(), (*index, *emitted));
            }
            Record::Finish { emitted, exit_code } => self.finished = Some((*emitted, *exit_code)),
        }
        Ok(())
    }

    fn append(&mut self, record: Record) -> Result<()> {
        if self.poisoned {
            return Err(invalid(
                "recovery write failed; reopen the artifact before continuing",
            ));
        }
        self.check(&record)?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("recovery sequence overflow"))?;
        let checksum = Frame::digest(sequence, &self.checksum, &record)?;
        let frame = Frame {
            version: VERSION,
            sequence,
            previous: self.checksum.clone(),
            record,
            checksum,
        };
        let mut bytes = serde_json::to_vec(&frame).map_err(json_error)?;
        bytes.push(b'\n');
        // Poison before writing: a partial append or failed fsync cannot be followed by new work.
        self.poisoned = true;
        let offset = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|error| Error::io("seeking recovery journal", error))?;
        self.checkpoint("before_write");
        #[cfg(test)]
        if self.crash_at == Some("partial") {
            self.file
                .write_all(&bytes[..bytes.len() / 2])
                .map_err(|error| Error::io("writing recovery journal", error))?;
            self.checkpoint("partial");
        }
        self.file
            .write_all(&bytes)
            .map_err(|error| Error::io("writing recovery journal", error))?;
        self.checkpoint("written");
        self.file
            .sync_all()
            .map_err(|error| Error::io("syncing recovery journal", error))?;
        self.checkpoint("persisted");
        self.apply(offset, &frame.record)?;
        self.checkpoint("indexed");
        self.sequence = sequence;
        self.checksum = frame.checksum;
        self.poisoned = false;
        self.checkpoint("complete");
        Ok(())
    }

    #[cfg(not(test))]
    fn checkpoint(&self, _stage: &str) {}
    #[cfg(test)]
    fn checkpoint(&self, stage: &str) {
        if self.crash_at == Some(stage) {
            let _ = rustix::process::kill_process(
                rustix::process::getpid(),
                rustix::process::Signal::KILL,
            );
        }
    }

    fn scan(&mut self) -> Result<Option<u64>> {
        let mut reader = BufReader::new(
            self.file
                .try_clone()
                .map_err(|error| Error::io("reading recovery journal", error))?,
        );
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| Error::io("seeking recovery journal", error))?;
        let mut offset = 0;
        loop {
            let mut bytes = Vec::new();
            let length = reader
                .read_until(b'\n', &mut bytes)
                .map_err(|error| Error::io("reading recovery journal", error))?;
            if length == 0 {
                return Ok(None);
            }
            if bytes.last() != Some(&b'\n') {
                return Ok(Some(offset));
            }
            let frame: Frame = serde_json::from_slice(&bytes).map_err(json_error)?;
            if frame.version != VERSION
                || frame.sequence
                    != self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| invalid("recovery sequence overflow"))?
                || frame.previous != self.checksum
                || frame.checksum != Frame::digest(frame.sequence, &frame.previous, &frame.record)?
            {
                return Err(invalid("recovery version/sequence/checksum mismatch"));
            }
            self.check(&frame.record)?;
            self.apply(offset, &frame.record)?;
            self.sequence = frame.sequence;
            self.checksum = frame.checksum;
            offset = offset
                .checked_add(
                    u64::try_from(length).map_err(|_| invalid("recovery offset overflow"))?,
                )
                .ok_or_else(|| invalid("recovery offset overflow"))?;
        }
    }
    fn record(&mut self, offset: u64) -> Result<Record> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|error| Error::io("seeking recovery record", error))?;
        let mut reader = BufReader::new(&mut self.file);
        let mut bytes = Vec::new();
        reader
            .read_until(b'\n', &mut bytes)
            .map_err(|error| Error::io("reading recovery record", error))?;
        let frame: Frame = serde_json::from_slice(&bytes).map_err(json_error)?;
        if frame.version != VERSION
            || frame.checksum != Frame::digest(frame.sequence, &frame.previous, &frame.record)?
        {
            return Err(invalid("recovery record checksum changed"));
        }
        Ok(frame.record)
    }
}

fn open(path: &Path, create: bool, protected: &[(u64, u64)]) -> Result<(File, File)> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "recovery path must name a file"))?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory: File = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| Error::io("opening recovery journal directory", error.into()))?
    .into();
    let mut flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
    if create {
        flags |= OFlags::CREATE | OFlags::EXCL;
    }
    let file: File = rustix::fs::openat(&directory, name, flags, Mode::RUSR | Mode::WUSR)
        .map_err(|error| Error::io("opening recovery journal", error.into()))?
        .into();
    let metadata = file
        .metadata()
        .map_err(|error| Error::io("checking recovery journal", error))?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(invalid(
            "recovery journal must be a regular file with one link",
        ));
    }
    if protected.contains(&(metadata.dev(), metadata.ino())) {
        return Err(Error::new(
            ErrorKind::Usage,
            "recovery journal aliases an input/output file",
        ));
    }
    file.try_lock()
        .map_err(|error| Error::new(ErrorKind::Io, format!("locking recovery journal: {error}")))?;
    Ok((file, directory))
}

fn anonymous_index() -> Result<File> {
    let directory = File::open(std::env::temp_dir())
        .map_err(|error| Error::io("opening recovery scratch directory", error))?;
    #[cfg(target_os = "linux")]
    {
        match rustix::fs::openat(
            &directory,
            ".",
            OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => return Ok(file.into()),
            Err(
                rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::INVAL | rustix::io::Errno::ISDIR,
            ) => {}
            Err(error) => return Err(Error::io("opening anonymous recovery index", error.into())),
        }
    }
    // The named fallback is exclusive and immediately unlinked through the held directory.
    for attempt in 0..128 {
        let name = format!(".loupe-recovery-index-{}-{attempt}", std::process::id());
        match rustix::fs::openat(
            &directory,
            name.as_str(),
            OFlags::CREATE | OFlags::EXCL | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => {
                rustix::fs::unlinkat(&directory, name.as_str(), rustix::fs::AtFlags::empty())
                    .map_err(|error| Error::io("unlinking recovery index", error.into()))?;
                return Ok(file.into());
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(Error::io("creating recovery index", error.into())),
        }
    }
    Err(invalid("cannot allocate recovery scratch index"))
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
fn db_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, format!("recovery index: {error}"))
}
fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::Io, format!("recovery JSON: {error}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::fs::{File, OpenOptions};
    use std::io::{BufReader, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use redb::ReadableDatabase;
    use serde_json::json;

    use crate::error::ErrorKind;
    use crate::input::Unit;
    use crate::records::{Format, Options, Source};
    use crate::recovery::{Disposition, Frame, INDEX_CACHE_BYTES, Record, Store, VERSION};

    fn identity() -> serde_json::Value {
        json!({"version":1,"command":"keep","questions":[["present",{"type":"noul","instructions":"Is checkout down?"}]],"encoder":"clef-v1","model_identity":null})
    }
    fn units(root: &Path) -> Vec<Unit> {
        let path = root.join("input");
        std::fs::write(&path, "First 東京🙂\r\n\nThird.\nFourth.\n").unwrap();
        let mut source = Source::new(
            BufReader::new(File::open(&path).unwrap()),
            Options::new(Format::Lines),
            Some(path),
        )
        .unwrap();
        let mut units = Vec::new();
        while let Some(unit) = source.next_unit().unwrap() {
            units.push(unit);
        }
        assert_eq!(
            units.iter().map(|unit| unit.index).collect::<Vec<_>>(),
            [1, 3, 4]
        );
        units
    }
    fn create(path: &Path) -> Store {
        Store::create(path, identity(), &[]).unwrap()
    }
    fn resume(path: &Path) -> Store {
        Store::resume(path, &identity(), &[]).unwrap()
    }
    fn completed() -> serde_json::Value {
        json!({"source":"protocol fixture","operation":"completed-but-unemitted","byte_ranges":[[0,14]],"usage_provenance":"retained verbatim by the caller"})
    }
    #[test]
    fn group_results_are_one_frame_and_each_member_replays_after_every_torn_cut() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        store.admit(&units[0]).unwrap();
        store.admit(&units[1]).unwrap();
        let prefix = std::fs::read(&path).unwrap();
        store
            .ready_many(vec![
                crate::recovery::Completion {
                    index: 1,
                    completed: completed(),
                },
                crate::recovery::Completion {
                    index: 3,
                    completed: json!({"result":"second stored result"}),
                },
            ])
            .unwrap();
        drop(store);
        let complete = std::fs::read(&path).unwrap();
        assert_eq!(
            complete[prefix.len()..]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            1
        );
        for cut in prefix.len()..complete.len() {
            std::fs::write(&path, &complete[..cut]).unwrap();
            let mut store = resume(&path);
            assert!(store.saved(1).unwrap().completed.is_none());
            assert!(store.saved(3).unwrap().completed.is_none());
            drop(store);
            assert_eq!(std::fs::read(&path).unwrap(), prefix);
        }
        std::fs::write(&path, &complete).unwrap();
        let mut store = resume(&path);
        assert_eq!(store.saved(1).unwrap().completed, Some(completed()));
        assert_eq!(
            store.saved(3).unwrap().completed,
            Some(json!({"result":"second stored result"}))
        );
        let before = std::fs::read(&path).unwrap();
        assert!(
            store
                .ready_many(vec![crate::recovery::Completion {
                    index: 1,
                    completed: serde_json::Value::Null
                }])
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn round_trip_retains_pending_failed_completed_unemitted_and_sparse_handling() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        for unit in &units {
            store.admit(unit).unwrap();
        }
        store
            .plan(
                vec![1, 3],
                json!({"request_indices":[1,3],"encoder":"clef-v1","counts":{"input_tokens":400}}),
            )
            .unwrap();
        store.ready(3, completed()).unwrap();
        store
            .failed(Some(1), 69, "real request failed".to_owned())
            .unwrap();
        store.handled(1, Disposition::Omitted).unwrap();
        drop(store);
        let original = std::fs::read(&path).unwrap();
        let mut store = resume(&path);
        assert_eq!(store.last_index(), 4);
        assert_eq!(store.next_index(0).unwrap(), Some(1));
        assert_eq!(store.next_index(1).unwrap(), Some(3));
        assert_eq!(store.next_index(4).unwrap(), None);
        let first = store.saved(1).unwrap();
        assert_eq!(first.unit, units[0]);
        assert_eq!(first.failure, Some((69, "real request failed".to_owned())));
        assert_eq!(first.handled, Some(Disposition::Omitted));
        assert!(first.completed.is_none());
        let third = store.saved(3).unwrap();
        assert_eq!(third.unit, units[1]);
        assert_eq!(third.completed, Some(completed()));
        assert!(third.handled.is_none());
        assert!(third.plan.is_some());
        let fourth = store.saved(4).unwrap();
        assert!(fourth.completed.is_none());
        assert!(fourth.failure.is_none());
        assert!(fourth.plan.is_none());
        assert!(fourth.handled.is_none());
        for unit in &units {
            store.verify(unit).unwrap();
        }
        let mut changed = units[1].clone();
        changed.raw.push(b' ');
        assert_eq!(store.verify(&changed).unwrap_err().kind, ErrorKind::Input);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        store
            .end_input(json!({"consumed":4,"fingerprint":"complete original source hash"}))
            .unwrap();
        assert!(store.finish(2, 0).is_err());
        store.ready(4, completed()).unwrap();
        store.finish(2, 0).unwrap();
        drop(store);
        let mut store = resume(&path);
        assert_eq!(store.finished(), Some((2, 0)));
        assert!(store.admit(&units[0]).is_err());
        assert!(store.input_end().is_some());
    }

    #[test]
    fn invalid_state_transitions_fail_before_writing_and_acks_are_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        store.admit(&units[0]).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(store.admit(&units[0]).is_err());
        assert!(store.ready(3, completed()).is_err());
        assert!(store.handled(1, Disposition::Emitted).is_err());
        assert!(store.plan(vec![], json!({})).is_err());
        assert!(store.plan(vec![1, 1], json!({})).is_err());
        assert!(store.finish(1, 0).is_err());
        assert!(store.fallback(1, json!({"status":"failed"})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        store.ready(1, completed()).unwrap();
        let external =
            json!({"status":"resolved","selected":{"boolean":false},"source":"external-process"});
        store.fallback(1, external.clone()).unwrap();
        assert_eq!(store.saved(1).unwrap().fallback, Some(external.clone()));
        let before = std::fs::read(&path).unwrap();
        assert!(store.fallback(1, external).is_err());
        assert!(store.ready(1, completed()).is_err());
        assert!(store.plan(vec![1], json!({})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        store.handled(1, Disposition::Emitted).unwrap();
        let before = std::fs::read(&path).unwrap();
        store.handled(1, Disposition::Emitted).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(store.handled(1, Disposition::Omitted).is_err());
        assert!(store.fallback(1, json!({})).is_err());
        assert!(store.failed(Some(1), 69, "later".to_owned()).is_err());
        store.end_input(json!({"consumed":1})).unwrap();
        assert!(store.admit(&units[1]).is_err());
        assert!(store.end_input(json!({"consumed":3})).is_err());
        store.finish(1, 0).unwrap();
        let before = std::fs::read(&path).unwrap();
        store.finish(1, 0).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn all_torn_ready_suffixes_repair_only_after_identity_validation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        store.admit(&units[0]).unwrap();
        let prefix = std::fs::read(&path).unwrap();
        store.ready(1, completed()).unwrap();
        drop(store);
        let complete = std::fs::read(&path).unwrap();
        for cut in prefix.len()..complete.len() {
            std::fs::write(&path, &complete[..cut]).unwrap();
            assert_eq!(
                Store::resume(&path, &json!({"different":"run"}), &[])
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::Input
            );
            assert_eq!(std::fs::read(&path).unwrap(), &complete[..cut]);
            let mut store = resume(&path);
            assert!(store.saved(1).unwrap().completed.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), prefix);
            store.ready(1, completed()).unwrap();
            drop(store);
            assert_eq!(std::fs::read(&path).unwrap(), complete);
        }
    }

    #[test]
    fn torn_fallback_frames_leave_the_raw_result_intact_and_complete_frames_replay() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("fallback.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        store.admit(&units[0]).unwrap();
        let raw = completed();
        store.ready(1, raw.clone()).unwrap();
        let prefix = std::fs::read(&path).unwrap();
        let external = json!({"status":"failed","kind":"timeout","message":"real subprocess deadline","outcome":null});
        store.fallback(1, external.clone()).unwrap();
        drop(store);
        let complete = std::fs::read(&path).unwrap();
        for cut in prefix.len()..complete.len() {
            std::fs::write(&path, &complete[..cut]).unwrap();
            let mut store = resume(&path);
            let saved = store.saved(1).unwrap();
            assert_eq!(saved.completed, Some(raw.clone()));
            assert!(saved.fallback.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), prefix);
            store.fallback(1, external.clone()).unwrap();
            drop(store);
            assert_eq!(std::fs::read(&path).unwrap(), complete);
        }
        let mut store = resume(&path);
        assert_eq!(store.saved(1).unwrap().fallback, Some(external));
        assert_eq!(store.saved(1).unwrap().completed, Some(raw));
    }

    #[test]
    fn durable_review_seed_and_decision_replay_without_changing_raw_results() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("review-run.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        let seed = store.review_seed().unwrap();
        assert_eq!(store.review_seed().unwrap(), seed);
        store.admit(&units[0]).unwrap();
        let raw = completed();
        store.ready(1, raw.clone()).unwrap();
        let decision = json!({"task":"captured-id","sampling":"uniform-audit","queued":false,"label":{"boolean":false},"skipped":false});
        store.review(1, decision.clone()).unwrap();
        let complete = std::fs::read(&path).unwrap();
        assert!(store.review(1, decision.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), complete);
        drop(store);
        let mut store = resume(&path);
        assert_eq!(store.review_seed().unwrap(), seed);
        assert_eq!(store.saved(1).unwrap().review, Some(decision));
        assert_eq!(store.saved(1).unwrap().completed, Some(raw));
    }

    #[test]
    fn complete_corruption_reordering_and_validly_checksummed_invalid_records_are_retained() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut store = create(&path);
        store.admit(&units[0]).unwrap();
        store.ready(1, completed()).unwrap();
        drop(store);
        let original = std::fs::read_to_string(&path).unwrap();
        let frames: Vec<_> = original
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect();
        let mut corrupted = frames.clone();
        corrupted[2]["record"]["completed"]["operation"] = json!("changed");
        let mut reordered = frames.clone();
        reordered.swap(1, 2);
        let mut future_version = frames.clone();
        future_version[0]["version"] = json!(2);
        for frames in [
            corrupted,
            reordered,
            future_version,
            vec![frames[0].clone(), frames[2].clone()],
        ] {
            let bytes = frames
                .iter()
                .map(|frame| serde_json::to_string(frame).unwrap() + "\n")
                .collect::<String>()
                + "torn";
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(
                Store::resume(&path, &identity(), &[]).err().unwrap().kind,
                ErrorKind::Io
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        }
        std::fs::write(&path, &original).unwrap();
        let previous = frames[2]["checksum"].as_str().unwrap();
        let record = Record::Admit {
            unit: Box::new(units[0].clone()),
        };
        let frame = Frame {
            version: VERSION,
            sequence: 4,
            previous: previous.to_owned(),
            checksum: Frame::digest(4, previous, &record).unwrap(),
            record,
        };
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", serde_json::to_string(&frame).unwrap()).unwrap();
        drop(file);
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            Store::resume(&path, &identity(), &[]).err().unwrap().kind,
            ErrorKind::Io
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn actual_file_aliases_symlinks_hardlinks_fifo_and_parallel_owner_are_denied() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let store = create(&path);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(Store::create(&path, identity(), &[]).is_err());
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::busy_child", "--nocapture"])
            .env("LOUPE_RECOVERY_BUSY_PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        drop(store);
        let original = std::fs::read(&path).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(
            Store::resume(&path, &identity(), &[(metadata.dev(), metadata.ino())])
                .err()
                .unwrap()
                .kind,
            ErrorKind::Usage
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let symlink = root.path().join("link");
        std::os::unix::fs::symlink(&path, &symlink).unwrap();
        assert!(Store::resume(&symlink, &identity(), &[]).is_err());
        let hardlink = root.path().join("hardlink");
        std::fs::hard_link(&path, &hardlink).unwrap();
        assert!(Store::resume(&hardlink, &identity(), &[]).is_err());
        std::fs::remove_file(&hardlink).unwrap();
        let fifo = root.path().join("fifo");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        assert!(Store::resume(&fifo, &identity(), &[]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    #[test]
    fn busy_child() {
        let Some(path) = std::env::var_os("LOUPE_RECOVERY_BUSY_PATH") else {
            return;
        };
        assert_eq!(
            Store::resume(Path::new(&path), &identity(), &[])
                .err()
                .unwrap()
                .kind,
            ErrorKind::Io
        );
    }

    #[test]
    fn crash_child() {
        let Some(path) = std::env::var_os("LOUPE_RECOVERY_CRASH_PATH") else {
            return;
        };
        let stage = std::env::var("LOUPE_RECOVERY_CRASH_STAGE").unwrap();
        let mut store = resume(Path::new(&path));
        store.crash_at = match stage.as_str() {
            "before_write" => Some("before_write"),
            "partial" => Some("partial"),
            "written" => Some("written"),
            "persisted" => Some("persisted"),
            "indexed" => Some("indexed"),
            "complete" => Some("complete"),
            _ => panic!("unknown recovery crash stage"),
        };
        store.ready(1, completed()).unwrap();
        panic!("expected SIGKILL");
    }
    #[test]
    fn actual_sigkill_rebuilds_index_at_every_result_write_boundary() {
        for stage in [
            "before_write",
            "partial",
            "written",
            "persisted",
            "indexed",
            "complete",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("failed.jsonl");
            let units = units(root.path());
            let mut store = create(&path);
            store.admit(&units[0]).unwrap();
            drop(store);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "recovery::tests::crash_child", "--nocapture"])
                .env("LOUPE_RECOVERY_CRASH_PATH", &path)
                .env("LOUPE_RECOVERY_CRASH_STAGE", stage)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .unwrap();
            assert_eq!(
                output.status.signal(),
                Some(9),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut store = resume(&path);
            let saved = store.saved(1).unwrap();
            assert_eq!(saved.unit, units[0]);
            assert!(saved.handled.is_none());
            if matches!(stage, "before_write" | "partial") {
                assert!(saved.completed.is_none());
                store.ready(1, completed()).unwrap();
            } else {
                assert_eq!(saved.completed, Some(completed()));
            }
            store.handled(1, Disposition::Emitted).unwrap();
            store.end_input(json!({"consumed":1})).unwrap();
            store.finish(1, 0).unwrap();
            drop(store);
            let before = std::fs::read(&path).unwrap();
            let mut store = resume(&path);
            assert_eq!(store.finished(), Some((1, 0)));
            assert_eq!(store.saved(1).unwrap().handled, Some(Disposition::Emitted));
            drop(store);
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        }
    }

    #[test]
    fn disk_index_supports_long_sparse_streams_without_retaining_unit_payloads_in_ram() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.jsonl");
        let units = units(root.path());
        let mut unit = units[0].clone();
        let mut store = create(&path);
        assert_eq!(INDEX_CACHE_BYTES, 8 * 1024 * 1024);
        for index in 1..=1000 {
            unit.index = index * 3;
            store.admit(&unit).unwrap();
            store
                .ready(unit.index, json!({"original_index":unit.index}))
                .unwrap();
        }
        store.end_input(json!({"consumed":3000})).unwrap();
        drop(store);
        let mut store = resume(&path);
        assert_eq!(store.next_index(0).unwrap(), Some(3));
        assert_eq!(store.next_index(3).unwrap(), Some(6));
        assert_eq!(store.next_index(2999).unwrap(), Some(3000));
        assert_eq!(store.next_index(3000).unwrap(), None);
        assert_eq!(
            store.saved(3000).unwrap().completed,
            Some(json!({"original_index":3000}))
        );
        store.finish(1000, 0).unwrap();
        let read = store.index.begin_read().unwrap();
        let table = read.open_table(crate::recovery::UNITS).unwrap();
        assert_eq!(redb::ReadableTableMetadata::len(&table).unwrap(), 1000);
    }
    #[test]
    fn eof_output_checkpoints_recover_counts_and_refuse_backwards_progress() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("checkpoint.jsonl");
        let mut store = create(&path);
        assert!(
            store
                .checkpoint_output("join_unmatched_right", 1, 1)
                .is_err()
        );
        store.end_input(json!({"consumed":0})).unwrap();
        store
            .checkpoint_output("join_unmatched_right", 2, 4)
            .unwrap();
        store
            .checkpoint_output("join_unmatched_right", 2, 4)
            .unwrap();
        assert!(
            store
                .checkpoint_output("join_unmatched_right", 1, 5)
                .is_err()
        );
        assert!(
            store
                .checkpoint_output("join_unmatched_right", 3, 4)
                .is_err()
        );
        assert!(store.checkpoint_output("invalid-namespace", 3, 5).is_err());
        drop(store);
        let mut store = resume(&path);
        assert_eq!(
            store.output_checkpoint("join_unmatched_right"),
            Some((2, 4))
        );
        store
            .checkpoint_output("join_unmatched_right", 3, 5)
            .unwrap();
        store.finish(5, 0).unwrap();
        drop(store);
        assert_eq!(
            resume(&path).output_checkpoint("join_unmatched_right"),
            Some((3, 5))
        );
    }
}
