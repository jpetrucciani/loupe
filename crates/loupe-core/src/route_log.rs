use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::route_labels::Name;

pub const VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: Name,
    pub device: u64,
    pub inode: u64,
    pub start: u64,
    pub before_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Append {
    pub run: u64,
    pub sequence: u64,
    pub index: u64,
    pub unit_hash: String,
    pub prefix_hash: String,
    pub label: String,
    pub target: Option<Target>,
    pub payload: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    Begin {
        run: u64,
        identity: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        labels: Option<Vec<String>>,
    },
    Prepare {
        append: Append,
    },
    Commit {
        run: u64,
        sequence: u64,
        prepare_hash: String,
    },
    Finish {
        run: u64,
        sequence: u64,
        prefix_hash: String,
    },
}
impl Record {
    pub fn hash(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self).map_err(json_error)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
    fn validate(&self) -> Result<()> {
        let hashes = match self {
            Self::Begin {
                run,
                identity,
                labels,
            } => {
                if *run == 0 {
                    return Err(invalid("zero route run"));
                }
                if let Some(labels) = labels {
                    crate::route_labels::Labels::new(labels.iter().map(String::as_str))
                        .map_err(|error| invalid(&format!("route label snapshot: {error}")))?;
                }
                vec![identity.as_str()]
            }
            Self::Prepare { append } => {
                if append.run == 0 || append.sequence == 0 || append.index == 0 {
                    return Err(invalid("zero route run/sequence/index"));
                }
                if append.target.is_none() && !append.payload.is_empty() {
                    return Err(invalid("omitted route decision has a payload"));
                }
                if append.label.is_empty() {
                    return Err(invalid("empty route label"));
                }
                let mut hashes = vec![append.unit_hash.as_str(), append.prefix_hash.as_str()];
                if let Some(target) = &append.target {
                    target
                        .start
                        .checked_add(append.payload.len() as u64)
                        .ok_or_else(|| invalid("route byte range overflow"))?;
                    hashes.push(target.before_hash.as_str());
                }
                hashes
            }
            Self::Commit {
                run,
                sequence,
                prepare_hash,
            } => {
                if *run == 0 || *sequence == 0 {
                    return Err(invalid("zero route commit run/sequence"));
                }
                vec![prepare_hash.as_str()]
            }
            Self::Finish {
                run, prefix_hash, ..
            } => {
                if *run == 0 {
                    return Err(invalid("zero route finish run"));
                }
                vec![prefix_hash.as_str()]
            }
        };
        if hashes.iter().any(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        }) {
            return Err(invalid("invalid route checksum"));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u64,
    record: Record,
    checksum: String,
}

pub struct Log {
    file: File,
}
impl Log {
    /// The caller supplies a confined regular file held under the route writer lock.
    pub fn new(file: File) -> Self {
        Self { file }
    }
    pub fn append(&mut self, record: &Record) -> Result<String> {
        record.validate()?;
        let checksum = record.hash()?;
        let mut bytes = serde_json::to_vec(&Envelope {
            version: VERSION,
            record: record.clone(),
            checksum: checksum.clone(),
        })
        .map_err(json_error)?;
        bytes.push(b'\n');
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|error| Error::io("seeking route journal", error))?;
        self.file
            .write_all(&bytes)
            .map_err(|error| Error::io("writing route journal", error))?;
        self.file
            .sync_all()
            .map_err(|error| Error::io("syncing route journal", error))?;
        Ok(checksum)
    }
    /// Read one frame at a time, using memory proportional to the largest unit.
    pub fn scan(&mut self, mut visit: impl FnMut(u64, &Record) -> Result<()>) -> Result<()> {
        self.scan_frames(true, &mut visit)
    }
    /// Inspect ownership before mutation; an incomplete trailing frame remains untouched.
    pub fn inspect(&mut self, mut visit: impl FnMut(u64, &Record) -> Result<()>) -> Result<()> {
        self.scan_frames(false, &mut visit)
    }
    fn scan_frames(
        &mut self,
        repair: bool,
        mut visit: impl FnMut(u64, &Record) -> Result<()>,
    ) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|error| Error::io("seeking route journal", error))?;
        let mut reader = BufReader::new(&mut self.file);
        let mut offset = 0u64;
        loop {
            let mut bytes = Vec::new();
            let length = reader
                .read_until(b'\n', &mut bytes)
                .map_err(|error| Error::io("reading route journal", error))?;
            if length == 0 {
                break;
            }
            if bytes.last() != Some(&b'\n') {
                if !repair {
                    break;
                }
                // No destination can change until the complete prepare frame is durable.
                reader
                    .get_mut()
                    .set_len(offset)
                    .map_err(|error| Error::io("repairing torn route journal frame", error))?;
                reader
                    .get_mut()
                    .sync_all()
                    .map_err(|error| Error::io("syncing repaired route journal", error))?;
                break;
            }
            let envelope: Envelope = serde_json::from_slice(&bytes).map_err(json_error)?;
            if envelope.version != VERSION {
                return Err(invalid("unsupported route journal version"));
            }
            envelope.record.validate()?;
            if envelope.record.hash()? != envelope.checksum {
                return Err(invalid("route journal checksum mismatch"));
            }
            visit(offset, &envelope.record)?;
            offset = offset
                .checked_add(length as u64)
                .ok_or_else(|| invalid("route journal offset overflow"))?;
        }
        Ok(())
    }
    pub fn read_frame(&mut self, offset: u64) -> Result<Record> {
        let mut cursor = offset;
        self.next_frame(&mut cursor)?
            .ok_or_else(|| invalid("route frame is missing"))
    }
    /// Sequential cursors do not share the file's seek position.
    pub fn next_frame(&mut self, offset: &mut u64) -> Result<Option<Record>> {
        self.file
            .seek(SeekFrom::Start(*offset))
            .map_err(|error| Error::io("seeking route journal frame", error))?;
        let mut reader = BufReader::new(&mut self.file);
        let mut bytes = Vec::new();
        let length = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|error| Error::io("reading route journal frame", error))?;
        if length == 0 {
            return Ok(None);
        }
        if bytes.last() != Some(&b'\n') {
            return Err(invalid("route frame is incomplete"));
        }
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(json_error)?;
        if envelope.version != VERSION || envelope.record.hash()? != envelope.checksum {
            return Err(invalid("route frame version/checksum mismatch"));
        }
        envelope.record.validate()?;
        *offset = offset
            .checked_add(length as u64)
            .ok_or_else(|| invalid("route journal offset overflow"))?;
        Ok(Some(envelope.record))
    }
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::Io, format!("route journal JSON: {error}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::route_labels::Name;
    use crate::route_log::{Append, Log, Record, Target};
    use std::io::{Read, Seek, SeekFrom, Write};
    fn records() -> Vec<Record> {
        let hash = "a".repeat(64);
        let prepare = Record::Prepare {
            append: Append {
                run: 1,
                sequence: 1,
                index: 2,
                unit_hash: hash.clone(),
                prefix_hash: hash.clone(),
                label: "PAGE".to_owned(),
                target: Some(Target {
                    name: Name::parse("page").unwrap(),
                    device: 1,
                    inode: 2,
                    start: 10,
                    before_hash: hash.clone(),
                }),
                payload: "東京🙂\r\n\0".to_owned(),
            },
        };
        vec![
            Record::Begin {
                run: 1,
                identity: hash.clone(),
                labels: None,
            },
            prepare.clone(),
            Record::Commit {
                run: 1,
                sequence: 1,
                prepare_hash: prepare.hash().unwrap(),
            },
            Record::Finish {
                run: 1,
                sequence: 1,
                prefix_hash: hash,
            },
        ]
    }
    #[test]
    fn real_file_frames_preserve_raw_payloads_and_support_bounded_sequential_replay() {
        let mut file = tempfile::tempfile().unwrap();
        let expected = records();
        let mut log = Log::new(file.try_clone().unwrap());
        for record in &expected {
            log.append(record).unwrap();
        }
        let mut observed = Vec::new();
        let mut offsets = Vec::new();
        log.scan(|offset, record| {
            offsets.push(offset);
            observed.push(record.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(observed, expected);
        for (offset, record) in offsets.into_iter().zip(&expected) {
            assert_eq!(&log.read_frame(offset).unwrap(), record);
        }
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 4);
        assert!(!String::from_utf8(bytes).unwrap().contains("\r"));
    }
    #[test]
    fn every_torn_final_frame_is_discarded_without_losing_complete_prepares() {
        let records = records();
        let file = tempfile::tempfile().unwrap();
        let mut log = Log::new(file.try_clone().unwrap());
        log.append(&records[0]).unwrap();
        let prefix = file.metadata().unwrap().len();
        log.append(&records[1]).unwrap();
        let mut bytes = Vec::new();
        let mut original = file;
        original.seek(SeekFrom::Start(0)).unwrap();
        original.read_to_end(&mut bytes).unwrap();
        for length in prefix as usize..bytes.len() {
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(&bytes[..length]).unwrap();
            let mut log = Log::new(file.try_clone().unwrap());
            log.inspect(|_, _| Ok(())).unwrap();
            assert_eq!(file.metadata().unwrap().len(), length as u64);
            let mut count = 0;
            log.scan(|_, _| {
                count += 1;
                Ok(())
            })
            .unwrap();
            assert_eq!(count, 1);
            assert_eq!(file.metadata().unwrap().len(), prefix);
            log.append(&records[1]).unwrap();
            let mut count = 0;
            log.scan(|_, _| {
                count += 1;
                Ok(())
            })
            .unwrap();
            assert_eq!(count, 2);
        }
    }
    #[test]
    fn complete_corrupt_or_incompatible_frames_fail_without_discarding_bytes() {
        let file = tempfile::tempfile().unwrap();
        let mut log = Log::new(file.try_clone().unwrap());
        log.append(&records()[0]).unwrap();
        let mut file = file;
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        for (from, to) in [
            ("\"version\":1", "\"version\":2"),
            ("\"run\":1", "\"run\":2"),
        ] {
            let corrupt = String::from_utf8(bytes.clone()).unwrap().replace(from, to);
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(corrupt.as_bytes()).unwrap();
            let mut log = Log::new(file.try_clone().unwrap());
            assert_eq!(log.scan(|_, _| Ok(())).unwrap_err().exit_code(), 74);
            assert_eq!(file.metadata().unwrap().len(), corrupt.len() as u64);
        }
        let invalid = Record::Begin {
            run: 0,
            identity: "bad".to_owned(),
            labels: None,
        };
        assert!(log.append(&invalid).is_err());
    }
}
