use std::collections::BTreeMap;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::{Value, json};
use tokio::time::Instant;

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::policy::{Assessment, Destination, Selected, UncertainAction};
use loupe_core::records::{Format, Options, Source};
use loupe_core::route_dir::{Artifact, Identity};
use loupe_core::route_labels::{Labels, Name};
use loupe_core::route_process::Sinks;
use loupe_core::route_store::Store;
use loupe_core::wire::Question;

use crate::args::{Output, Route};

pub struct Plan {
    labels: Labels,
    commands: BTreeMap<Name, String>,
    out: Option<PathBuf>,
    resume: bool,
    reset: bool,
    capture_uncertain: bool,
    timeout: Duration,
}
impl Plan {
    pub fn new(
        args: &Route,
        questions: &IndexMap<String, Question>,
        uncertain: UncertainAction,
    ) -> Result<Self> {
        let question = questions
            .values()
            .next()
            .ok_or_else(|| usage("route question is missing"))?;
        let Question::Choice { criteria, .. } = question else {
            return Err(usage("route requires one Choice question"));
        };
        let labels = Labels::new(criteria.keys().map(String::as_str))?;
        if args.out.is_none() && args.to.is_empty() {
            return Err(usage(
                "route requires --out DIR or declared --to LABEL=COMMAND destinations",
            ));
        }
        if (args.truncate || args.common.resume.is_some() || args.common.uncertain.is_some())
            && args.out.is_none()
        {
            return Err(usage(
                "route truncate/resume/uncertain-file handling requires --out DIR",
            ));
        }
        if args.common.resume.is_some() && !args.to.is_empty() {
            return Err(usage(
                "route --resume applies to file destinations; process outcomes cannot be replayed safely",
            ));
        }
        if args.to.is_empty() && args.to_timeout.is_some() {
            return Err(usage("--to-timeout requires process destinations"));
        }
        let mut commands = BTreeMap::new();
        for declaration in &args.to {
            let (label, command) = declaration
                .split_once('=')
                .ok_or_else(|| usage("--to requires LABEL=COMMAND"))?;
            let name = labels
                .resolve(label)
                .ok()
                .or_else(|| {
                    labels
                        .iter()
                        .find_map(|(_, name)| (name.as_str() == label).then_some(name))
                })
                .ok_or_else(|| {
                    usage("--to label must be a declared criterion or none/uncertain")
                })?;
            if command.trim().is_empty() || command.contains('\0') {
                return Err(usage("--to command must be nonempty and contain no NUL"));
            }
            if commands.insert(name.clone(), command.to_owned()).is_some() {
                return Err(usage("duplicate --to destination"));
            }
        }
        if args.out.is_none() {
            for label in criteria
                .keys()
                .map(String::as_str)
                .chain((uncertain == UncertainAction::Keep).then_some("uncertain"))
            {
                if !commands.contains_key(labels.resolve(label)?) {
                    return Err(usage(
                        "without --out, every emitting label requires --to LABEL=COMMAND",
                    ));
                }
            }
        }
        if let Some(out) = &args.out {
            if let Some(path) = &args.common.resume
                && Self::manifest_resume(path)
            {
                validate_artifact_path(out, path, Artifact::Manifest.as_str())?;
            }
            if let Some(path) = &args.common.uncertain {
                validate_artifact_path(out, path, "uncertain")?;
            }
        }
        if args.common.uncertain.is_some() && commands.contains_key(labels.resolve("uncertain")?) {
            return Err(usage(
                "--uncertain FILE conflicts with a process destination for uncertain",
            ));
        }
        let timeout = args
            .to_timeout
            .as_deref()
            .map(crate::run::duration)
            .transpose()?
            .unwrap_or(Duration::from_secs(30));
        Ok(Self {
            labels,
            commands,
            out: args.out.clone(),
            resume: args.common.resume.is_some(),
            reset: args.truncate,
            capture_uncertain: args.common.uncertain.is_some(),
            timeout,
        })
    }
    pub fn manifest_resume(path: &Path) -> bool {
        path.file_name() == Some(std::ffi::OsStr::new(Artifact::Manifest.as_str()))
    }
    pub fn description(&self) -> Value {
        json!({"out":self.out,"labels":self.labels,"process_labels":self.commands.keys().collect::<Vec<_>>(),"resume":self.resume,"reset":self.reset})
    }
    pub fn start(
        self,
        identity: &str,
        files: &[String],
        deadline: Option<Instant>,
        quiet: bool,
    ) -> Result<Router> {
        let state = self
            .out
            .as_ref()
            .map(|out| {
                let mut store = Store::open_options(
                    out,
                    self.labels.clone(),
                    self.reset,
                    &protected_inputs(files)?,
                )?;
                store.begin(identity, self.resume)?;
                Ok(Arc::new(Mutex::new(FileState {
                    store,
                    skipped: 0,
                    skipped_delivered: 0,
                })))
            })
            .transpose()?;
        let processes = Sinks::new(self.commands, self.timeout, deadline)?;
        Ok(Router {
            state,
            processes,
            labels: self.labels,
            headers: BTreeMap::new(),
            capture_uncertain: self.capture_uncertain,
            quiet,
        })
    }
}
struct FileState {
    store: Store,
    skipped: u64,
    skipped_delivered: u64,
}
#[derive(Clone)]
pub struct Replay {
    state: Option<Arc<Mutex<FileState>>>,
}
impl Replay {
    pub fn filter(&self, batch: Vec<Result<Unit>>) -> Vec<Result<Unit>> {
        let Some(state) = &self.state else {
            return batch;
        };
        let filtered = (|| -> Result<Vec<Result<Unit>>> {
            let mut state = lock(state)?;
            let mut units = Vec::new();
            for unit in batch {
                match unit {
                    Ok(unit) => {
                        if let Some(delivered) =
                            state.store.replay_record(unit.index, &unit_hash(&unit)?)?
                        {
                            state.skipped = state
                                .skipped
                                .checked_add(1)
                                .ok_or_else(|| io_error("route skipped count overflow"))?;
                            state.skipped_delivered = state
                                .skipped_delivered
                                .checked_add(u64::from(delivered))
                                .ok_or_else(|| io_error("route delivered count overflow"))?;
                        } else if state.store.current_run().is_some_and(|run| run.finished) {
                            units.push(Err(Error::new(
                                ErrorKind::Input,
                                "route resume input extends the finished run",
                            )));
                            break;
                        } else {
                            units.push(Ok(unit));
                        }
                    }
                    Err(error) => {
                        units.push(Err(error));
                        break;
                    }
                }
            }
            Ok(units)
        })();
        filtered.unwrap_or_else(|error| vec![Err(error)])
    }
}
pub struct Router {
    state: Option<Arc<Mutex<FileState>>>,
    labels: Labels,
    processes: Sinks,
    headers: BTreeMap<Name, Vec<u8>>,
    capture_uncertain: bool,
    quiet: bool,
}
impl Router {
    pub fn replay(&self) -> Replay {
        Replay {
            state: self.state.clone(),
        }
    }
    pub async fn deliver(
        &mut self,
        unit: &Unit,
        assessment: &Assessment,
        action: UncertainAction,
        payload: Vec<u8>,
        format: Format,
        output: Output,
    ) -> Result<bool> {
        let destination = assessment.destination(action)?;
        let label = if assessment.uncertain {
            "uncertain"
        } else {
            match &assessment.selected {
                Selected::Choice(label) => label,
                _ => return Err(io_error("route result has no Choice label")),
            }
        };
        let emit =
            destination == Destination::Stdout || assessment.uncertain && self.capture_uncertain;
        if destination == Destination::Stderr {
            std::io::stderr()
                .lock()
                .write_all(&unit.raw)
                .map_err(|error| Error::io("writing uncertain route unit to stderr", error))?;
        }
        self.append(
            unit,
            label,
            emit.then_some(payload),
            format,
            output,
            assessment.uncertain && self.capture_uncertain,
        )
        .await
    }
    /// Failed units use the reserved uncertain destination, but have no assessment.
    /// They never enter the uncertainty capture or uncertainty counters.
    pub async fn deliver_error(
        &mut self,
        unit: &Unit,
        payload: Option<Vec<u8>>,
        format: Format,
        output: Output,
    ) -> Result<bool> {
        self.append(unit, "uncertain", payload, format, output, false)
            .await
    }
    async fn append(
        &mut self,
        unit: &Unit,
        label: &str,
        payload: Option<Vec<u8>>,
        format: Format,
        output: Output,
        capture: bool,
    ) -> Result<bool> {
        let name = self.labels.resolve(label)?.clone();
        let emit = payload.is_some();
        if let Some(mut payload) = payload {
            if capture {
                payload.clone_from(&unit.raw);
            }
            if (output == Output::Plain || capture) && matches!(format, Format::Csv | Format::Tsv) {
                payload = self.header_payload(&name, label, unit, payload, format)?;
            }
            if self.processes.contains(&name) {
                self.processes.write(&name, &payload).await?;
                if let Some(state) = &self.state {
                    lock(state)?
                        .store
                        .append(unit.index, &unit_hash(unit)?, label, None)?;
                }
            } else {
                let state = self
                    .state
                    .as_ref()
                    .ok_or_else(|| usage("no destination for emitted route label"))?;
                let payload = std::str::from_utf8(&payload)
                    .map_err(|error| io_error(&format!("route output must be UTF-8: {error}")))?;
                lock(state)?
                    .store
                    .append(unit.index, &unit_hash(unit)?, label, Some(payload))?;
            }
        } else if let Some(state) = &self.state {
            lock(state)?
                .store
                .append(unit.index, &unit_hash(unit)?, label, None)?;
        }
        Ok(emit)
    }
    fn header_payload(
        &mut self,
        name: &Name,
        label: &str,
        unit: &Unit,
        mut payload: Vec<u8>,
        format: Format,
    ) -> Result<Vec<u8>> {
        let Some(header) = &unit.header else {
            return Ok(payload);
        };
        if let Some(previous) = self.headers.get(name) {
            if previous != header {
                return Err(Error::new(
                    ErrorKind::Input,
                    "route destination requires identical CSV/TSV headers",
                ));
            }
            return Ok(payload);
        }
        let mut needs_header = true;
        if !self.processes.contains(name)
            && let Some(state) = &self.state
        {
            let state = lock(state)?;
            if let Some(mut output) = state.store.destination(label)?
                && output.identity()?.bytes != 0
            {
                let source = Source::new(
                    BufReader::new(
                        output
                            .file()
                            .try_clone()
                            .map_err(|error| Error::io("reading route header", error))?,
                    ),
                    Options::new(format),
                    None,
                )?;
                if source.header() != Some(header.as_slice()) {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "existing route destination has a different CSV/TSV header",
                    ));
                }
                output
                    .file_mut()
                    .seek(SeekFrom::End(-1))
                    .map_err(|error| Error::io("checking route delimiter", error))?;
                let mut last = [0];
                output
                    .file_mut()
                    .read_exact(&mut last)
                    .map_err(|error| Error::io("reading route delimiter", error))?;
                if !matches!(last[0], b'\n' | b'\r') {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "existing CSV/TSV route output lacks a final delimiter",
                    ));
                }
                needs_header = false;
            }
        }
        if needs_header {
            payload = [header.as_slice(), &payload].concat();
        }
        self.headers.insert(name.clone(), header.clone());
        Ok(payload)
    }
    pub async fn finish(&mut self) -> Result<u64> {
        for exit in self.processes.finish().await? {
            if !self.quiet {
                eprintln!(
                    "loupe: route {:?}: process exited {}",
                    exit.label.as_str(),
                    exit.status
                );
            }
        }
        if let Some(state) = &self.state {
            let mut state = lock(state)?;
            state.store.finish()?;
            Ok(state.skipped_delivered)
        } else {
            Ok(0)
        }
    }
}
fn lock(state: &Mutex<FileState>) -> Result<std::sync::MutexGuard<'_, FileState>> {
    state
        .lock()
        .map_err(|_| io_error("route state lock was poisoned"))
}
fn protected_inputs(files: &[String]) -> Result<Vec<Identity>> {
    use std::os::unix::fs::MetadataExt;
    let names: Vec<&str> = if files.is_empty() {
        vec!["-"]
    } else {
        files.iter().map(String::as_str).collect()
    };
    let mut identities = Vec::new();
    for name in names {
        let path = if name == "-" {
            Path::new("/dev/fd/0")
        } else {
            Path::new(name)
        };
        match std::fs::metadata(path) {
            Ok(metadata) => identities.push(Identity {
                device: metadata.dev(),
                inode: metadata.ino(),
                bytes: metadata.len(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io("checking route input identity", error)),
        }
    }
    Ok(identities)
}
pub fn unit_hash(unit: &Unit) -> Result<String> {
    let semantic = serde_json::to_vec(&json!({"index":unit.index,"state":unit.state,"subject":unit.subject,"location":unit.location}))
        .map_err(|error| io_error(&format!("hashing route input: {error}")))?;
    let mut digest = blake3::Hasher::new();
    digest.update(b"loupe-route-unit-v1");
    for bytes in [
        &unit.raw[..],
        &semantic,
        unit.header.as_deref().unwrap_or_default(),
    ] {
        digest.update(&(bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(digest.finalize().to_hex().to_string())
}
fn validate_artifact_path(root: &Path, path: &Path, filename: &str) -> Result<()> {
    let expected = absolute(root)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.file_name() != Some(std::ffi::OsStr::new(filename)) || absolute(parent)? != expected {
        return Err(usage(&format!("route path must be --out/{filename}")));
    }
    Ok(())
}
fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| Error::io("resolving route path", error))?
            .join(path)
    };
    match std::fs::canonicalize(&path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(path.components().collect())
        }
        Err(error) => Err(Error::io("resolving route directory", error)),
    }
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}
fn io_error(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
