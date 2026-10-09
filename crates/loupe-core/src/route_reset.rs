use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::route_dir::{Artifact, Directory, Identity, Output};
use crate::route_labels::{Labels, Name};
use crate::route_log::{Log, Record};
use crate::route_store::Checkpoints;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Label { name: Name },
    Artifact { artifact: Artifact },
}
impl Entry {
    fn name(&self) -> &str {
        match self {
            Self::Label { name } => name.as_str(),
            Self::Artifact { artifact } => artifact.as_str(),
        }
    }
    fn open(&self, directory: &Directory) -> Result<Option<Output>> {
        match self {
            Self::Label { name } => directory.label(name, false),
            Self::Artifact {
                artifact: Artifact::Reset | Artifact::ResetTemp,
            } => Err(invalid("reset intent cannot clear itself")),
            Self::Artifact { artifact } => directory.artifact(*artifact, false),
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Clear {
    entry: Entry,
    device: u64,
    inode: u64,
    bytes: u64,
    hash: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u64,
    files: Vec<Clear>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    intent: Intent,
    checksum: String,
}

fn protected_file(output: &Output, protected: &[Identity]) -> Result<()> {
    let identity = output.identity()?;
    if protected
        .iter()
        .any(|source| source.device == identity.device && source.inode == identity.inode)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "route destinations and state must differ from every input file",
        ));
    }
    Ok(())
}
pub(crate) fn validate_inputs(
    directory: &Directory,
    labels: &Labels,
    protected: &[Identity],
) -> Result<()> {
    if protected.is_empty() {
        return Ok(());
    }
    for artifact in [
        Artifact::Journal,
        Artifact::Manifest,
        Artifact::ManifestTemp,
        Artifact::Reset,
        Artifact::ResetTemp,
    ] {
        if let Some(output) = directory.artifact(artifact, false)? {
            protected_file(&output, protected)?;
        }
    }
    for (_, name) in labels.iter() {
        if let Some(output) = directory.label(name, false)? {
            protected_file(&output, protected)?;
        }
    }
    for entry in owned(directory, labels)?.values() {
        if let Some(output) = entry.open(directory)? {
            protected_file(&output, protected)?;
        }
    }
    Ok(())
}
fn owned(directory: &Directory, labels: &Labels) -> Result<BTreeMap<String, Entry>> {
    let mut entries = BTreeMap::new();
    for (_, name) in labels.iter() {
        entries.insert(
            name.as_str().to_owned(),
            Entry::Label { name: name.clone() },
        );
    }
    for artifact in [
        Artifact::Journal,
        Artifact::Manifest,
        Artifact::ManifestTemp,
    ] {
        entries.insert(artifact.as_str().to_owned(), Entry::Artifact { artifact });
    }
    if let Some(output) = directory.artifact(Artifact::Journal, false)? {
        let file = output
            .file()
            .try_clone()
            .map_err(|error| Error::io("reading reset ownership journal", error))?;
        let mut log = Log::new(file);
        log.inspect(|_, record| {
            match record {
                Record::Prepare { append } => {
                    if let Some(target) = &append.target {
                        entries.insert(
                            target.name.as_str().to_owned(),
                            Entry::Label {
                                name: target.name.clone(),
                            },
                        );
                    }
                }
                Record::Begin {
                    labels: Some(snapshot),
                    ..
                } => {
                    let labels = Labels::new(snapshot.iter().map(String::as_str))?;
                    for (_, name) in labels.iter() {
                        entries.insert(
                            name.as_str().to_owned(),
                            Entry::Label { name: name.clone() },
                        );
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
    }
    Ok(entries)
}
fn checksum(intent: &Intent) -> Result<String> {
    let bytes = serde_json::to_vec(intent).map_err(json_error)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
fn hash(output: &mut Output) -> Result<String> {
    output
        .file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(|error| Error::io("seeking reset prefix", error))?;
    let mut digest = blake3::Hasher::new();
    let mut buffer = [0; 32768];
    loop {
        let read = output
            .file_mut()
            .read(&mut buffer)
            .map_err(|error| Error::io("hashing reset prefix", error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().to_hex().to_string())
}
pub(crate) fn start(
    directory: &Directory,
    labels: &Labels,
    protected: &[Identity],
    checkpoints: &Checkpoints,
) -> Result<()> {
    let mut files = Vec::new();
    for entry in owned(directory, labels)?.into_values() {
        if let Some(mut output) = entry.open(directory)? {
            protected_file(&output, protected)?;
            let identity = output.identity()?;
            files.push(Clear {
                entry,
                device: identity.device,
                inode: identity.inode,
                bytes: identity.bytes,
                hash: hash(&mut output)?,
            });
        }
    }
    let intent = Intent { version: 1, files };
    let checksum = checksum(&intent)?;
    let mut bytes = serde_json::to_vec(&Envelope { intent, checksum }).map_err(json_error)?;
    bytes.push(b'\n');
    let mut output = directory
        .artifact(Artifact::ResetTemp, true)?
        .ok_or_else(|| invalid("reset intent temporary file is missing"))?;
    output
        .file()
        .set_len(0)
        .map_err(|error| Error::io("resetting intent temporary file", error))?;
    output
        .file_mut()
        .write_all(&bytes)
        .map_err(|error| Error::io("writing reset intent", error))?;
    output
        .file()
        .sync_all()
        .map_err(|error| Error::io("syncing reset intent", error))?;
    checkpoints.reached("reset_ready");
    directory.replace_artifact(Artifact::ResetTemp, Artifact::Reset, || {
        checkpoints.reached("reset_rename")
    })?;
    checkpoints.reached("reset_intent");
    recover(directory, protected, checkpoints)
}
pub(crate) fn recover(
    directory: &Directory,
    protected: &[Identity],
    checkpoints: &Checkpoints,
) -> Result<()> {
    let Some(mut output) = directory.artifact(Artifact::Reset, false)? else {
        return Ok(());
    };
    protected_file(&output, protected)?;
    let mut bytes = Vec::new();
    output
        .file_mut()
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io("reading reset intent", error))?;
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(json_error)?;
    if envelope.intent.version != 1 || checksum(&envelope.intent)? != envelope.checksum {
        return Err(invalid("route reset intent version/checksum mismatch"));
    }
    let mut opened = BTreeMap::new();
    // Validate every inode and original-or-empty byte sequence before clearing any file.
    for clear in &envelope.intent.files {
        let mut file = clear
            .entry
            .open(directory)?
            .ok_or_else(|| invalid("route reset target is missing"))?;
        protected_file(&file, protected)?;
        let identity = file.identity()?;
        if identity.device != clear.device || identity.inode != clear.inode {
            return Err(invalid("route reset target inode changed"));
        }
        if identity.bytes != 0 && (identity.bytes != clear.bytes || hash(&mut file)? != clear.hash)
        {
            return Err(invalid("route reset target has conflicting external bytes"));
        }
        if opened.insert(clear.entry.name().to_owned(), file).is_some() {
            return Err(invalid("duplicate route reset target"));
        }
    }
    for (name, file) in opened {
        file.file()
            .set_len(0)
            .map_err(|error| Error::io("clearing route reset target", error))?;
        checkpoints.reached(if name == Artifact::Journal.as_str() {
            "reset_journal"
        } else {
            "reset_file"
        });
        file.file()
            .sync_all()
            .map_err(|error| Error::io("syncing route reset target", error))?;
        checkpoints.reached("reset_file_sync");
    }
    checkpoints.reached("reset_cleared");
    directory.remove_artifact(Artifact::Reset)?;
    checkpoints.reached("reset_removed");
    Ok(())
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
fn json_error(error: serde_json::Error) -> Error {
    invalid(&format!("route reset JSON: {error}"))
}
