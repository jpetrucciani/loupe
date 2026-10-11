use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Serialize;

use crate::error::{Error, ErrorKind, Result};
use crate::route_dir::{Artifact, Directory, Output};
use crate::route_labels::{Labels, Name};
use crate::route_log::{Append, Log, Record, Target};

pub fn empty_prefix() -> String {
    blake3::hash(b"loupe-route-input-v1").to_hex().to_string()
}
pub fn next_prefix(previous: &str, index: u64, unit_hash: &str) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(b"loupe-route-prefix-v1");
    hash.update(previous.as_bytes());
    hash.update(&index.to_le_bytes());
    hash.update(unit_hash.as_bytes());
    hash.finalize().to_hex().to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    pub id: u64,
    pub identity: String,
    pub sequence: u64,
    pub last_index: u64,
    pub prefix_hash: String,
    pub finished: bool,
}
struct Destination {
    output: Output,
    end: u64,
    hash: blake3::Hasher,
}
impl Destination {
    fn load(directory: &Directory, name: &Name, target: Option<&Target>) -> Result<Self> {
        let output = directory
            .label(name, target.is_none())?
            .ok_or_else(|| invalid("recorded route destination is missing"))?;
        let identity = output.identity()?;
        let end = target.map_or(identity.bytes, |target| target.start);
        if identity.bytes < end {
            return Err(invalid(
                "route destination is shorter than its recorded prefix",
            ));
        }
        if let Some(target) = target
            && (identity.device != target.device || identity.inode != target.inode)
        {
            return Err(invalid("route destination inode changed"));
        }
        let mut destination = Self {
            output,
            end,
            hash: blake3::Hasher::new(),
        };
        destination.hash = hash_prefix(destination.output.file_mut(), end)?;
        Ok(destination)
    }
    fn check_target(&self, target: &Target) -> Result<()> {
        let identity = self.output.identity()?;
        if identity.device != target.device
            || identity.inode != target.inode
            || self.end != target.start
            || self.hash.finalize().to_hex().as_str() != target.before_hash
        {
            return Err(invalid(
                "route destination identity/range/prefix checksum changed",
            ));
        }
        Ok(())
    }
    fn commit_existing(&mut self, append: &Append) -> Result<()> {
        let target = append
            .target
            .as_ref()
            .ok_or_else(|| invalid("route append has no destination"))?;
        self.check_target(target)?;
        verify_bytes(
            self.output.file_mut(),
            target.start,
            append.payload.as_bytes(),
        )?;
        self.hash.update(append.payload.as_bytes());
        self.end = range_end(append)?;
        Ok(())
    }
}

#[derive(Default)]
struct Recovery {
    run: Option<Run>,
    run_labels: Option<Labels>,
    pending: Option<Append>,
    destinations: BTreeMap<Name, Destination>,
}
impl Recovery {
    fn visit(&mut self, directory: &Directory, labels: &Labels, record: &Record) -> Result<()> {
        match record {
            Record::Begin {
                run,
                identity,
                labels: snapshot,
            } => {
                let expected = self.run.as_ref().map_or(Ok(1), |previous| {
                    previous
                        .id
                        .checked_add(1)
                        .ok_or_else(|| invalid("route run overflow"))
                })?;
                if self.pending.is_some() || *run != expected {
                    return Err(invalid("route begin has an invalid run or pending append"));
                }
                self.run = Some(Run {
                    id: *run,
                    identity: identity.clone(),
                    sequence: 0,
                    last_index: 0,
                    prefix_hash: empty_prefix(),
                    finished: false,
                });
                self.run_labels = Some(match snapshot {
                    Some(snapshot) => Labels::new(snapshot.iter().map(String::as_str))?,
                    None => labels.clone(),
                });
            }
            Record::Prepare { append } => {
                let run = self
                    .run
                    .as_ref()
                    .ok_or_else(|| invalid("route prepare precedes begin"))?;
                validate_append(run, append)?;
                if self.pending.is_some() {
                    return Err(invalid("route journal has multiple pending appends"));
                }
                let name = self
                    .run_labels
                    .as_ref()
                    .ok_or_else(|| invalid("route run labels are missing"))?
                    .resolve(&append.label)?;
                if let Some(target) = &append.target {
                    if name != &target.name {
                        return Err(invalid("route target does not match its declared label"));
                    }
                    if !self.destinations.contains_key(name) {
                        let destination = Destination::load(directory, name, Some(target))?;
                        self.destinations.insert(name.clone(), destination);
                    }
                    destination_mut(&mut self.destinations, name)?.check_target(target)?;
                }
                self.pending = Some(append.clone());
            }
            Record::Commit {
                run,
                sequence,
                prepare_hash,
            } => {
                let append = self
                    .pending
                    .as_ref()
                    .ok_or_else(|| invalid("route commit has no prepare"))?;
                if *run != append.run
                    || *sequence != append.sequence
                    || (Record::Prepare {
                        append: append.clone(),
                    })
                    .hash()?
                        != *prepare_hash
                {
                    return Err(invalid("route commit does not match its prepare"));
                }
                if let Some(target) = &append.target {
                    destination_mut(&mut self.destinations, &target.name)?
                        .commit_existing(append)?;
                }
                commit_run(self.run.as_mut(), append)?;
                self.pending = None;
            }
            Record::Finish {
                run,
                sequence,
                prefix_hash,
            } => {
                let active = self
                    .run
                    .as_mut()
                    .ok_or_else(|| invalid("route finish precedes begin"))?;
                if self.pending.is_some()
                    || active.finished
                    || active.id != *run
                    || active.sequence != *sequence
                    || active.prefix_hash != *prefix_hash
                {
                    return Err(invalid("route finish does not match the committed prefix"));
                }
                active.finished = true;
            }
        }
        Ok(())
    }
    fn check_lengths(&self) -> Result<()> {
        for (name, destination) in &self.destinations {
            let actual = destination.output.identity()?.bytes;
            let pending = self.pending.as_ref().filter(|append| {
                append
                    .target
                    .as_ref()
                    .is_some_and(|target| &target.name == name)
            });
            if let Some(append) = pending {
                if actual < destination.end || actual > range_end(append)? {
                    return Err(invalid(
                        "interrupted route append has conflicting destination bytes",
                    ));
                }
            } else if actual != destination.end {
                return Err(invalid(
                    "route destination has unrecorded appended or removed bytes",
                ));
            }
        }
        Ok(())
    }
}

struct Replay {
    through: u64,
    consumed: u64,
    offset: u64,
}
#[derive(Default)]
pub(crate) struct Checkpoints {
    #[cfg(test)]
    crash_at: Option<&'static str>,
}
impl Checkpoints {
    pub(crate) fn reached(&self, _stage: &'static str) {
        #[cfg(test)]
        boundary(self.crash_at, _stage);
    }
}

/// One writer per output directory. Memory is bounded by labels plus one journal payload.
pub struct Store {
    directory: Directory,
    labels: Labels,
    log: Log,
    state: Recovery,
    manifest: Output,
    replay: Option<Replay>,
    poisoned: bool,
    checkpoints: Checkpoints,
}
impl Store {
    pub fn open(path: &Path, labels: Labels) -> Result<Self> {
        Self::open_options(path, labels, false, &[])
    }
    pub fn open_options(
        path: &Path,
        labels: Labels,
        reset: bool,
        protected: &[crate::route_dir::Identity],
    ) -> Result<Self> {
        Self::open_with(path, labels, reset, protected, Checkpoints::default())
    }
    #[cfg(test)]
    fn open_at(path: &Path, labels: Labels, checkpoints: Checkpoints) -> Result<Self> {
        Self::open_with(path, labels, false, &[], checkpoints)
    }
    fn open_with(
        path: &Path,
        labels: Labels,
        reset: bool,
        protected: &[crate::route_dir::Identity],
        checkpoints: Checkpoints,
    ) -> Result<Self> {
        let directory = Directory::open(path, true)?;
        crate::route_reset::validate_inputs(&directory, &labels, protected)?;
        crate::route_reset::recover(&directory, protected, &checkpoints)?;
        if reset {
            crate::route_reset::start(&directory, &labels, protected, &checkpoints)?;
        }
        let journal = directory
            .artifact(Artifact::Journal, true)?
            .ok_or_else(|| invalid("route journal was not created"))?;
        let mut log = Log::new(
            journal
                .file()
                .try_clone()
                .map_err(|error| Error::io("opening route journal cursor", error))?,
        );
        let mut state = Recovery::default();
        log.scan(|_, record| state.visit(&directory, &labels, record))?;
        state.check_lengths()?;
        // Validate all committed files before completing any interrupted destination write.
        if let Some(append) = &state.pending {
            recover_payload(&mut state.destinations, append, &checkpoints)?;
            let prepare = Record::Prepare {
                append: append.clone(),
            };
            log.append(&Record::Commit {
                run: append.run,
                sequence: append.sequence,
                prepare_hash: prepare.hash()?,
            })?;
            checkpoints.reached("recovery_commit");
            commit_run(state.run.as_mut(), append)?;
            state.pending = None;
        }
        let manifest = rebuild_manifest(&directory, &mut log, &checkpoints)?;
        Ok(Self {
            directory,
            labels,
            log,
            state,
            manifest,
            replay: None,
            poisoned: false,
            checkpoints,
        })
    }
    pub fn current_run(&self) -> Option<&Run> {
        self.state.run.as_ref()
    }
    /// A confined handle for inspecting an existing destination before formatting output.
    pub fn destination(&self, label: &str) -> Result<Option<Output>> {
        self.directory.label(self.labels.resolve(label)?, false)
    }
    pub fn begin(&mut self, identity: &str, resume: bool) -> Result<Run> {
        self.check_ready()?;
        if self.replay.is_some() {
            return Err(invalid("route session already began"));
        }
        if resume {
            let run =
                self.state.run.as_ref().ok_or_else(|| {
                    Error::new(ErrorKind::Input, "route output has no run to resume")
                })?;
            if run.identity != identity {
                return Err(Error::new(
                    ErrorKind::Input,
                    "route resume run identity changed",
                ));
            }
            self.replay = Some(Replay {
                through: run.sequence,
                consumed: 0,
                offset: 0,
            });
            return Ok(run.clone());
        }
        let id = self.state.run.as_ref().map_or(Ok(1), |run| {
            run.id
                .checked_add(1)
                .ok_or_else(|| invalid("route run overflow"))
        })?;
        let record = Record::Begin {
            run: id,
            identity: identity.to_owned(),
            labels: Some(
                self.labels
                    .iter()
                    .map(|(label, _)| label.to_owned())
                    .collect(),
            ),
        };
        self.log.append(&record)?;
        self.state.visit(&self.directory, &self.labels, &record)?;
        self.replay = Some(Replay {
            through: 0,
            consumed: 0,
            offset: 0,
        });
        self.state
            .run
            .clone()
            .ok_or_else(|| invalid("route run is missing"))
    }
    /// Consume the original input prefix before permitting any new destination append.
    /// Return true only for a recorded decision with matching index and unit identity.
    pub fn skip_recorded(&mut self, index: u64, unit_hash: &str) -> Result<bool> {
        Ok(self.replay_record(index, unit_hash)?.is_some())
    }
    /// Some identifies a matching prior decision; its bool identifies a file delivery.
    pub fn replay_record(&mut self, index: u64, unit_hash: &str) -> Result<Option<bool>> {
        self.check_ready()?;
        let replay = self
            .replay
            .as_mut()
            .ok_or_else(|| invalid("route session has not begun"))?;
        if replay.consumed == replay.through {
            return Ok(None);
        }
        let run_id = self
            .state
            .run
            .as_ref()
            .ok_or_else(|| invalid("route run is missing"))?
            .id;
        let mut cursor = replay.offset;
        while let Some(record) = self.log.next_frame(&mut cursor)? {
            if let Record::Prepare { append } = record
                && append.run == run_id
            {
                if append.index != index || append.unit_hash != unit_hash {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "route resume input prefix changed",
                    ));
                }
                replay.consumed = replay
                    .consumed
                    .checked_add(1)
                    .ok_or_else(|| invalid("route replay count overflow"))?;
                replay.offset = cursor;
                return Ok(Some(append.target.is_some()));
            }
        }
        Err(invalid("route replay is missing a committed decision"))
    }
    /// A None payload records an omitted unit, preserving resume progress without invented output.
    pub fn append(
        &mut self,
        index: u64,
        unit_hash: &str,
        label: &str,
        payload: Option<&str>,
    ) -> Result<()> {
        self.check_ready()?;
        self.check_replayed()?;
        let run = self
            .state
            .run
            .as_ref()
            .ok_or_else(|| invalid("route run is missing"))?;
        if run.finished {
            return Err(Error::new(
                ErrorKind::Input,
                "route resume input extends a finished run",
            ));
        }
        let name = self.labels.resolve(label)?.clone();
        let target = if payload.is_some() {
            if !self.state.destinations.contains_key(&name) {
                self.state.destinations.insert(
                    name.clone(),
                    Destination::load(&self.directory, &name, None)?,
                );
                self.boundary("created");
            }
            let destination = destination_mut(&mut self.state.destinations, &name)?;
            let actual = destination.output.identity()?;
            if actual.bytes != destination.end {
                return Err(invalid("route destination size changed before append"));
            }
            let bound = self
                .directory
                .label(&name, false)?
                .ok_or_else(|| invalid("route destination pathname disappeared"))?
                .identity()?;
            if bound.device != actual.device || bound.inode != actual.inode {
                return Err(invalid("route destination pathname changed before append"));
            }
            Some(Target {
                name,
                device: actual.device,
                inode: actual.inode,
                start: destination.end,
                before_hash: destination.hash.finalize().to_hex().to_string(),
            })
        } else {
            None
        };
        let append = Append {
            run: run.id,
            sequence: run
                .sequence
                .checked_add(1)
                .ok_or_else(|| invalid("route sequence overflow"))?,
            index,
            unit_hash: unit_hash.to_owned(),
            prefix_hash: next_prefix(&run.prefix_hash, index, unit_hash),
            label: label.to_owned(),
            target,
            payload: payload.unwrap_or_default().to_owned(),
        };
        validate_append(run, &append)?;
        self.poisoned = true;
        let prepare_hash = self.log.append(&Record::Prepare {
            append: append.clone(),
        })?;
        self.boundary("prepare");
        if let Some(target) = &append.target {
            let destination = destination_mut(&mut self.state.destinations, &target.name)?;
            destination
                .output
                .file_mut()
                .seek(SeekFrom::Start(target.start))
                .map_err(|error| Error::io("seeking route append", error))?;
            #[cfg(test)]
            {
                let middle = append.payload.len() / 2;
                destination
                    .output
                    .file_mut()
                    .write_all(&append.payload.as_bytes()[..middle])
                    .map_err(|error| Error::io("writing route payload prefix", error))?;
                self.checkpoints.reached("partial");
                destination
                    .output
                    .file_mut()
                    .write_all(&append.payload.as_bytes()[middle..])
                    .map_err(|error| Error::io("writing route payload suffix", error))?;
            }
            #[cfg(not(test))]
            destination
                .output
                .file_mut()
                .write_all(append.payload.as_bytes())
                .map_err(|error| Error::io("writing route payload", error))?;
            self.boundary("payload");
            let destination = destination_mut(&mut self.state.destinations, &target.name)?;
            destination
                .output
                .file()
                .sync_all()
                .map_err(|error| Error::io("syncing route payload", error))?;
            self.boundary("output");
            destination_mut(&mut self.state.destinations, &target.name)?
                .commit_existing(&append)?;
        }
        self.log.append(&Record::Commit {
            run: append.run,
            sequence: append.sequence,
            prepare_hash,
        })?;
        commit_run(self.state.run.as_mut(), &append)?;
        self.boundary("commit");
        write_manifest(&mut self.manifest, &append)?;
        self.boundary("manifest");
        self.manifest
            .file()
            .sync_all()
            .map_err(|error| Error::io("syncing route manifest", error))?;
        self.boundary("manifest_sync");
        self.poisoned = false;
        Ok(())
    }
    pub fn finish(&mut self) -> Result<()> {
        self.check_ready()?;
        self.check_replayed()?;
        let run = self
            .state
            .run
            .as_mut()
            .ok_or_else(|| invalid("route run is missing"))?;
        if !run.finished {
            self.log.append(&Record::Finish {
                run: run.id,
                sequence: run.sequence,
                prefix_hash: run.prefix_hash.clone(),
            })?;
            run.finished = true;
        }
        Ok(())
    }
    fn check_ready(&self) -> Result<()> {
        if self.poisoned {
            Err(invalid(
                "route write failed; reopen the directory to recover before continuing",
            ))
        } else {
            Ok(())
        }
    }
    fn check_replayed(&self) -> Result<()> {
        let replay = self
            .replay
            .as_ref()
            .ok_or_else(|| invalid("route session has not begun"))?;
        if replay.consumed != replay.through {
            Err(Error::new(
                ErrorKind::Input,
                "route resume has not validated the complete recorded input prefix",
            ))
        } else {
            Ok(())
        }
    }
    fn boundary(&self, _stage: &'static str) {
        self.checkpoints.reached(_stage);
    }
}

fn validate_append(run: &Run, append: &Append) -> Result<()> {
    if run.finished
        || append.run != run.id
        || Some(append.sequence) != run.sequence.checked_add(1)
        || append.index <= run.last_index
        || append.prefix_hash != next_prefix(&run.prefix_hash, append.index, &append.unit_hash)
    {
        return Err(invalid(
            "route append does not extend the current input prefix",
        ));
    }
    Ok(())
}
fn commit_run(run: Option<&mut Run>, append: &Append) -> Result<()> {
    let run = run.ok_or_else(|| invalid("route run is missing"))?;
    run.sequence = append.sequence;
    run.last_index = append.index;
    run.prefix_hash.clone_from(&append.prefix_hash);
    Ok(())
}
fn range_end(append: &Append) -> Result<u64> {
    append
        .target
        .as_ref()
        .ok_or_else(|| invalid("route append has no target"))?
        .start
        .checked_add(append.payload.len() as u64)
        .ok_or_else(|| invalid("route range overflow"))
}
fn destination_mut<'a>(
    destinations: &'a mut BTreeMap<Name, Destination>,
    name: &Name,
) -> Result<&'a mut Destination> {
    destinations
        .get_mut(name)
        .ok_or_else(|| invalid("route destination state is missing"))
}
fn hash_prefix(file: &mut File, length: u64) -> Result<blake3::Hasher> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| Error::io("seeking route prefix", error))?;
    let mut hash = blake3::Hasher::new();
    let mut remaining = length;
    let mut buffer = [0; 32768];
    while remaining > 0 {
        let amount = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| invalid("route prefix size overflow"))?;
        file.read_exact(&mut buffer[..amount])
            .map_err(|error| Error::io("reading route prefix", error))?;
        hash.update(&buffer[..amount]);
        remaining -= amount as u64;
    }
    Ok(hash)
}
fn verify_bytes(file: &mut File, start: u64, expected: &[u8]) -> Result<()> {
    file.seek(SeekFrom::Start(start))
        .map_err(|error| Error::io("seeking recorded route bytes", error))?;
    let mut buffer = [0; 32768];
    for chunk in expected.chunks(buffer.len()) {
        file.read_exact(&mut buffer[..chunk.len()])
            .map_err(|error| Error::io("reading recorded route bytes", error))?;
        if &buffer[..chunk.len()] != chunk {
            return Err(invalid("route destination payload checksum mismatch"));
        }
    }
    Ok(())
}
fn recover_payload(
    destinations: &mut BTreeMap<Name, Destination>,
    append: &Append,
    checkpoints: &Checkpoints,
) -> Result<()> {
    let Some(target) = &append.target else {
        return Ok(());
    };
    let destination = destination_mut(destinations, &target.name)?;
    let length = destination.output.identity()?.bytes;
    let written = usize::try_from(
        length
            .checked_sub(target.start)
            .ok_or_else(|| invalid("route recovery range underflow"))?,
    )
    .map_err(|_| invalid("route recovery range overflow"))?;
    let prefix = append
        .payload
        .as_bytes()
        .get(..written)
        .ok_or_else(|| invalid("route interrupted append exceeds its expected payload"))?;
    verify_bytes(destination.output.file_mut(), target.start, prefix)?;
    destination
        .output
        .file_mut()
        .seek(SeekFrom::Start(length))
        .map_err(|error| Error::io("seeking route recovery append", error))?;
    let remaining = append
        .payload
        .as_bytes()
        .get(written..)
        .ok_or_else(|| invalid("route recovery suffix is invalid"))?;
    destination
        .output
        .file_mut()
        .write_all(remaining)
        .map_err(|error| Error::io("completing route append", error))?;
    checkpoints.reached("recovery_payload");
    destination
        .output
        .file()
        .sync_all()
        .map_err(|error| Error::io("syncing recovered route append", error))?;
    checkpoints.reached("recovery_output");
    destination.commit_existing(append)
}

#[derive(Serialize)]
struct Manifest<'a> {
    run: u64,
    sequence: u64,
    index: u64,
    label: &'a str,
    unit_hash: &'a str,
    prefix_hash: &'a str,
    target: Option<&'a Target>,
    payload_bytes: usize,
    payload_hash: String,
}
fn write_manifest(output: &mut Output, append: &Append) -> Result<()> {
    let mut bytes = serde_json::to_vec(&Manifest {
        run: append.run,
        sequence: append.sequence,
        index: append.index,
        label: &append.label,
        unit_hash: &append.unit_hash,
        prefix_hash: &append.prefix_hash,
        target: append.target.as_ref(),
        payload_bytes: append.payload.len(),
        payload_hash: blake3::hash(append.payload.as_bytes()).to_hex().to_string(),
    })
    .map_err(|error| invalid(&format!("serializing route manifest: {error}")))?;
    bytes.push(b'\n');
    output
        .file_mut()
        .seek(SeekFrom::End(0))
        .map_err(|error| Error::io("seeking route manifest", error))?;
    output
        .file_mut()
        .write_all(&bytes)
        .map_err(|error| Error::io("writing route manifest", error))
}
fn rebuild_manifest(
    directory: &Directory,
    log: &mut Log,
    checkpoints: &Checkpoints,
) -> Result<Output> {
    let mut output = directory
        .artifact(Artifact::ManifestTemp, true)?
        .ok_or_else(|| invalid("route manifest temporary file was not created"))?;
    output
        .file()
        .set_len(0)
        .map_err(|error| Error::io("resetting recoverable route manifest", error))?;
    checkpoints.reached("rebuild_reset");
    let mut pending = None;
    log.scan(|_, record| {
        match record {
            Record::Prepare { append } => pending = Some(append.clone()),
            Record::Commit { .. } => {
                let append = pending
                    .take()
                    .ok_or_else(|| invalid("manifest commit has no prepare"))?;
                write_manifest(&mut output, &append)?;
                checkpoints.reached("rebuild_row");
            }
            _ => {}
        }
        Ok(())
    })?;
    output
        .file()
        .sync_all()
        .map_err(|error| Error::io("syncing rebuilt route manifest", error))?;
    checkpoints.reached("rebuild_ready");
    directory.replace_manifest_at(|| checkpoints.reached("rebuild_rename"))?;
    checkpoints.reached("rebuild_publish");
    Ok(output)
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}

#[cfg(test)]
fn boundary(crash_at: Option<&str>, stage: &str) {
    if crash_at == Some(stage) {
        println!("\nroute boundary {stage}");
        let _ = std::io::stdout().flush();
        loop {
            std::thread::park();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::route_labels::Labels;
    use crate::route_store::{Checkpoints, Store};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    const FIRST: &str = "page one\r\n";
    const SECOND: &str = "東京🙂\r\n\0next\n";
    fn hash(value: &str) -> String {
        blake3::hash(value.as_bytes()).to_hex().to_string()
    }
    fn labels() -> Labels {
        Labels::new(["PAGE", "ticket"]).unwrap()
    }
    fn open(path: &Path) -> Store {
        Store::open(path, labels()).unwrap()
    }
    fn seed(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("page"), b"preexisting\n").unwrap();
        let mut store = open(path);
        store.begin(&hash("run"), false).unwrap();
        store.append(1, &hash(FIRST), "PAGE", Some(FIRST)).unwrap();
    }
    fn crash(path: &Path, stage: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "route_store::tests::route_crash_child",
                "--nocapture",
            ])
            .env("LOUPE_ROUTE_CRASH_DIRECTORY", path)
            .env("LOUPE_ROUTE_CRASH_STAGE", stage)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut captured = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                let output = child.wait_with_output().unwrap();
                panic!(
                    "boundary {stage} was not reached: {captured} {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            captured.push_str(&line);
            if line.trim() == format!("route boundary {stage}") {
                break;
            }
        }
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
    }
    fn manifest(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path.join(".loupe-route.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn route_crash_child() {
        let Some(path) = std::env::var_os("LOUPE_ROUTE_CRASH_DIRECTORY") else {
            return;
        };
        let stage = std::env::var("LOUPE_ROUTE_CRASH_STAGE").unwrap();
        let reset = match stage.as_str() {
            "reset_ready" => Some("reset_ready"),
            "reset_rename" => Some("reset_rename"),
            "reset_intent" => Some("reset_intent"),
            "reset_journal" => Some("reset_journal"),
            "reset_file" => Some("reset_file"),
            "reset_file_sync" => Some("reset_file_sync"),
            "reset_cleared" => Some("reset_cleared"),
            "reset_removed" => Some("reset_removed"),
            _ => None,
        };
        if reset.is_some() {
            let _store = Store::open_with(
                Path::new(&path),
                labels(),
                true,
                &[],
                Checkpoints { crash_at: reset },
            )
            .unwrap();
            panic!("expected a reset crash");
        }
        let recovery = match stage.as_str() {
            "recovery_payload" => Some("recovery_payload"),
            "recovery_output" => Some("recovery_output"),
            "recovery_commit" => Some("recovery_commit"),
            "rebuild_reset" => Some("rebuild_reset"),
            "rebuild_row" => Some("rebuild_row"),
            "rebuild_ready" => Some("rebuild_ready"),
            "rebuild_rename" => Some("rebuild_rename"),
            "rebuild_publish" => Some("rebuild_publish"),
            _ => None,
        };
        if recovery.is_some() {
            let _store = Store::open_at(
                Path::new(&path),
                labels(),
                Checkpoints { crash_at: recovery },
            )
            .unwrap();
            panic!("expected a recovery crash");
        }
        let mut store = open(Path::new(&path));
        store.begin(&hash("run"), true).unwrap();
        assert!(store.skip_recorded(1, &hash(FIRST)).unwrap());
        store.checkpoints.crash_at = match stage.as_str() {
            "created" => Some("created"),
            "prepare" => Some("prepare"),
            "partial" => Some("partial"),
            "payload" => Some("payload"),
            "output" => Some("output"),
            "commit" => Some("commit"),
            "manifest" => Some("manifest"),
            "manifest_sync" => Some("manifest_sync"),
            _ => panic!("unknown crash boundary"),
        };
        store
            .append(3, &hash(SECOND), "ticket", Some(SECOND))
            .unwrap();
        panic!("expected a forced crash");
    }
    #[test]
    fn actual_sigkill_at_each_append_boundary_recovers_without_duplicate_bytes_or_manifest_rows() {
        for stage in [
            "created",
            "prepare",
            "partial",
            "payload",
            "output",
            "commit",
            "manifest",
            "manifest_sync",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("out");
            seed(&path);
            crash(&path, stage);
            let mut store = open(&path);
            let run = store.begin(&hash("run"), true).unwrap();
            assert_eq!(run.sequence, if stage == "created" { 1 } else { 2 });
            assert!(store.skip_recorded(1, &hash(FIRST)).unwrap());
            if !store.skip_recorded(3, &hash(SECOND)).unwrap() {
                store
                    .append(3, &hash(SECOND), "ticket", Some(SECOND))
                    .unwrap();
            }
            store.finish().unwrap();
            drop(store);
            assert_eq!(
                std::fs::read(path.join("page")).unwrap(),
                [b"preexisting\n".as_slice(), FIRST.as_bytes()].concat()
            );
            assert_eq!(
                std::fs::read(path.join("ticket")).unwrap(),
                SECOND.as_bytes()
            );
            let rows = manifest(&path);
            assert_eq!(rows.len(), 2, "{stage}");
            assert_eq!(rows[0]["index"], 1);
            assert_eq!(rows[1]["index"], 3);
            assert_eq!(rows[1]["payload_bytes"], SECOND.len());
            let mut store = open(&path);
            assert!(store.begin(&hash("run"), true).unwrap().finished);
            assert!(store.skip_recorded(1, &hash(FIRST)).unwrap());
            assert!(store.skip_recorded(3, &hash(SECOND)).unwrap());
            store.finish().unwrap();
            assert_eq!(manifest(&path), rows);
        }
    }
    #[test]
    fn repeated_actual_sigkill_during_recovery_and_manifest_publication_is_idempotent() {
        for stage in [
            "recovery_payload",
            "recovery_output",
            "recovery_commit",
            "rebuild_reset",
            "rebuild_row",
            "rebuild_ready",
            "rebuild_rename",
            "rebuild_publish",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("out");
            seed(&path);
            crash(&path, "partial");
            crash(&path, stage);
            let mut store = open(&path);
            assert_eq!(store.begin(&hash("run"), true).unwrap().sequence, 2);
            assert!(store.skip_recorded(1, &hash(FIRST)).unwrap());
            assert!(store.skip_recorded(3, &hash(SECOND)).unwrap());
            store.finish().unwrap();
            drop(store);
            assert_eq!(
                std::fs::read(path.join("ticket")).unwrap(),
                SECOND.as_bytes(),
                "{stage}"
            );
            assert_eq!(manifest(&path).len(), 2, "{stage}");
            drop(open(&path));
            assert_eq!(manifest(&path).len(), 2, "{stage}");
        }
    }
    #[test]
    fn external_destination_changes_fail_before_completing_any_pending_append() {
        for change in [
            "prefix", "partial", "append", "truncate", "inode", "missing",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("out");
            seed(&path);
            crash(&path, "partial");
            let file = if change == "prefix" {
                path.join("page")
            } else {
                path.join("ticket")
            };
            match change {
                "prefix" | "partial" => {
                    let mut bytes = std::fs::read(&file).unwrap();
                    bytes[0] ^= 1;
                    std::fs::write(&file, bytes).unwrap();
                }
                "append" => {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(&file)
                        .unwrap()
                        .write_all(&[b'x'; 256])
                        .unwrap();
                }
                "truncate" => {
                    std::fs::write(&file, b"conflict").unwrap();
                }
                "inode" => {
                    std::fs::rename(&file, path.join("old-ticket")).unwrap();
                    std::fs::write(&file, b"replacement").unwrap();
                }
                "missing" => {
                    std::fs::remove_file(&file).unwrap();
                }
                _ => panic!("unknown change"),
            }
            let before = std::fs::read(path.join("ticket")).ok();
            let journal = std::fs::read(path.join(".loupe-route.journal")).unwrap();
            let error = match Store::open(&path, labels()) {
                Ok(_) => panic!("accepted conflicting {change}"),
                Err(error) => error,
            };
            assert_eq!(error.exit_code(), 74, "{change}: {error}");
            assert_eq!(std::fs::read(path.join("ticket")).ok(), before, "{change}");
            assert_eq!(
                std::fs::read(path.join(".loupe-route.journal")).unwrap(),
                journal
            );
        }
    }
    #[test]
    fn resume_validates_run_and_complete_input_prefix_before_new_writes_and_detects_early_eof() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        seed(&path);
        let mut store = open(&path);
        assert_eq!(
            store
                .begin(&hash("different"), true)
                .unwrap_err()
                .exit_code(),
            65
        );
        store.begin(&hash("run"), true).unwrap();
        assert_eq!(
            store
                .append(3, &hash(SECOND), "ticket", Some(SECOND))
                .unwrap_err()
                .exit_code(),
            65
        );
        assert_eq!(store.finish().unwrap_err().exit_code(), 65);
        assert_eq!(
            store
                .skip_recorded(2, &hash(FIRST))
                .unwrap_err()
                .exit_code(),
            65
        );
        // Failed replay does not consume a decision; it can still be validated correctly.
        assert!(store.skip_recorded(1, &hash(FIRST)).unwrap());
        store
            .append(3, &hash(SECOND), "ticket", Some(SECOND))
            .unwrap();
        store
            .append(4, &hash("uncertain"), "uncertain", None)
            .unwrap();
        store.finish().unwrap();
        drop(store);
        let mut store = open(&path);
        store.begin(&hash("run"), true).unwrap();
        for (index, hash) in [(1, hash(FIRST)), (3, hash(SECOND)), (4, hash("uncertain"))] {
            assert!(store.skip_recorded(index, &hash).unwrap());
        }
        assert_eq!(
            store
                .append(5, &hash("extra"), "PAGE", Some("extra"))
                .unwrap_err()
                .exit_code(),
            65
        );
        store.finish().unwrap();
        assert!(!path.join("uncertain").exists());
        assert_eq!(manifest(&path).len(), 3);
    }
    #[test]
    fn lost_or_torn_manifest_is_rebuilt_and_multiple_runs_preserve_preexisting_data() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        seed(&path);
        let mut store = open(&path);
        assert_eq!(store.begin(&hash("new-run"), false).unwrap().id, 2);
        store
            .append(1, &hash(SECOND), "PAGE", Some(SECOND))
            .unwrap();
        store.finish().unwrap();
        drop(store);
        let expected = manifest(&path);
        std::fs::write(path.join(".loupe-route.jsonl"), b"torn or corrupt manifest").unwrap();
        drop(open(&path));
        assert_eq!(manifest(&path), expected);
        std::fs::remove_file(path.join(".loupe-route.jsonl")).unwrap();
        drop(open(&path));
        assert_eq!(manifest(&path), expected);
        assert_eq!(
            std::fs::read(path.join("page")).unwrap(),
            [
                b"preexisting\n".as_slice(),
                FIRST.as_bytes(),
                SECOND.as_bytes()
            ]
            .concat()
        );
    }
    #[test]
    fn recovery_memory_tracks_labels_and_one_payload_instead_of_total_units() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        let mut store = open(&path);
        store.begin(&hash("run"), false).unwrap();
        let payload = "東京🙂\r\n".repeat(8192);
        for index in 1..=24 {
            store
                .append(
                    index,
                    &hash(&format!("{index}")),
                    if index % 2 == 0 { "ticket" } else { "PAGE" },
                    Some(&payload),
                )
                .unwrap();
            assert_eq!(
                store.state.destinations.len(),
                if index == 1 { 1 } else { 2 }
            );
            assert!(store.state.pending.is_none());
        }
        store.finish().unwrap();
        drop(store);
        let store = open(&path);
        assert_eq!(store.state.destinations.len(), 2);
        assert!(store.state.pending.is_none());
        assert_eq!(store.current_run().unwrap().sequence, 24);
        assert_eq!(
            std::fs::metadata(path.join("page")).unwrap().len(),
            payload.len() as u64 * 12
        );
    }
    #[test]
    fn reset_intent_survives_actual_sigkill_at_every_clear_boundary_without_restoring_old_output() {
        for stage in [
            "reset_ready",
            "reset_rename",
            "reset_intent",
            "reset_journal",
            "reset_file",
            "reset_file_sync",
            "reset_cleared",
            "reset_removed",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("out");
            seed(&path);
            std::fs::write(path.join("unrelated"), b"leave this alone").unwrap();
            crash(&path, stage);
            let mut store =
                Store::open_options(&path, Labels::new(["new-label"]).unwrap(), true, &[]).unwrap();
            assert_eq!(std::fs::read(path.join("page")).unwrap(), b"", "{stage}");
            assert!(manifest(&path).is_empty());
            assert_eq!(
                std::fs::read(path.join("unrelated")).unwrap(),
                b"leave this alone"
            );
            assert!(!path.join(".loupe-route.reset.json").exists());
            assert_eq!(store.begin(&hash("new"), false).unwrap().id, 1);
            store
                .append(1, &hash(SECOND), "new-label", Some(SECOND))
                .unwrap();
            store.finish().unwrap();
            drop(store);
            let mut store = Store::open(&path, Labels::new(["new-label"]).unwrap()).unwrap();
            store.begin(&hash("new"), true).unwrap();
            assert!(store.skip_recorded(1, &hash(SECOND)).unwrap());
            store.finish().unwrap();
            assert_eq!(
                std::fs::read(path.join("new-label")).unwrap(),
                SECOND.as_bytes()
            );
            assert_eq!(manifest(&path).len(), 1);
        }
    }
    #[test]
    fn reset_recovery_rejects_conflicting_bytes_before_clearing_any_remaining_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        seed(&path);
        crash(&path, "reset_journal");
        let mut changed = std::fs::read(path.join("page")).unwrap();
        changed[0] ^= 1;
        std::fs::write(path.join("page"), &changed).unwrap();
        let manifest = std::fs::read(path.join(".loupe-route.jsonl")).unwrap();
        let intent = std::fs::read(path.join(".loupe-route.reset.json")).unwrap();
        let error = match Store::open(&path, labels()) {
            Ok(_) => panic!("accepted conflicting reset bytes"),
            Err(error) => error,
        };
        assert_eq!(error.exit_code(), 74);
        assert_eq!(std::fs::read(path.join("page")).unwrap(), changed);
        assert_eq!(
            std::fs::read(path.join(".loupe-route.jsonl")).unwrap(),
            manifest
        );
        assert_eq!(
            std::fs::read(path.join(".loupe-route.reset.json")).unwrap(),
            intent
        );
    }
    #[test]
    fn source_inode_protection_precedes_reset_or_journal_repair_including_historical_labels() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        seed(&path);
        for name in ["page", ".loupe-route.journal", ".loupe-route.jsonl"] {
            let file = path.join(name);
            let before = std::fs::read(&file).unwrap();
            let meta = std::fs::metadata(&file).unwrap();
            let protected = crate::route_dir::Identity {
                device: meta.dev(),
                inode: meta.ino(),
                bytes: meta.len(),
            };
            let error = match Store::open_options(
                &path,
                Labels::new(["different"]).unwrap(),
                true,
                &[protected],
            ) {
                Ok(_) => panic!("accepted an input/destination alias"),
                Err(error) => error,
            };
            assert_eq!(error.exit_code(), 64);
            assert_eq!(std::fs::read(file).unwrap(), before);
        }
    }
    #[test]
    fn historical_label_snapshots_allow_new_append_runs_without_changing_old_decisions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        seed(&path);
        let mut store = Store::open(&path, Labels::new(["new-label"]).unwrap()).unwrap();
        assert_eq!(store.begin(&hash("new"), false).unwrap().id, 2);
        store
            .append(1, &hash(SECOND), "new-label", Some(SECOND))
            .unwrap();
        store.finish().unwrap();
        drop(store);
        drop(Store::open(&path, Labels::new(["new-label"]).unwrap()).unwrap());
        assert_eq!(
            std::fs::read(path.join("page")).unwrap(),
            [b"preexisting\n".as_slice(), FIRST.as_bytes()].concat()
        );
        assert_eq!(
            std::fs::read(path.join("new-label")).unwrap(),
            SECOND.as_bytes()
        );
        assert_eq!(manifest(&path).len(), 2);
    }
}
