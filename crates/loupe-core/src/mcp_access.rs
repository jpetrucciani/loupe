//! Read capabilities for MCP. Held descriptors, rather than validated path strings,
//! determine which filesystem objects may be read.

use std::collections::BTreeSet;
use std::fs::File;
use std::num::NonZeroUsize;
use std::os::unix::fs::FileExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{Mode, OFlags};
use serde::Deserialize;

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub presets: Vec<String>,
    pub allow_paths: Vec<PathBuf>,
    pub max_input_bytes: Option<NonZeroUsize>,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.byte_limit().checked_add(1).is_none() {
            return Err(denied("MCP input limit outside reader range"));
        }
        let mut seen = BTreeSet::new();
        for name in &self.presets {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(denied(
                    "MCP preset names use ASCII letters, digits, underscore and hyphen",
                ));
            }
            if !seen.insert(name) {
                return Err(denied("duplicate MCP preset allowlist entry"));
            }
        }
        if self.allow_paths.iter().any(|path| {
            path.as_os_str().is_empty() || path.as_os_str().as_encoded_bytes().contains(&0)
        }) {
            return Err(denied("MCP allow_paths must contain valid nonempty paths"));
        }
        Ok(())
    }
    pub fn byte_limit(&self) -> usize {
        self.max_input_bytes
            .map_or(16 * 1024 * 1024, NonZeroUsize::get)
    }
}

struct Anchor {
    path: PathBuf,
    file: File,
    directory: bool,
    identity: (u64, u64),
}

pub struct Scope {
    anchors: Vec<Anchor>,
    cwd: PathBuf,
    limit: usize,
}

impl Scope {
    pub fn new(settings: &Settings, cwd: &Path) -> Result<Self> {
        settings.validate()?;
        let cwd = cwd
            .canonicalize()
            .map_err(|error| Error::io("resolving MCP working directory", error))?;
        let mut anchors = Vec::new();
        let mut seen = BTreeSet::new();
        for path in &settings.allow_paths {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            let path = path
                .canonicalize()
                .map_err(|error| Error::io("resolving MCP allow_paths", error))?;
            if !seen.insert(path.clone()) {
                return Err(denied("duplicate resolved MCP allow_paths entry"));
            }
            let file: File = rustix::fs::open(
                &path,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(|error| Error::io("opening MCP read capability", error.into()))?
            .into();
            let metadata = file
                .metadata()
                .map_err(|error| Error::io("checking MCP read capability", error))?;
            if !metadata.is_dir() && (!metadata.is_file() || metadata.nlink() != 1) {
                return Err(denied(
                    "MCP allow_paths must be directories or regular single-link files",
                ));
            }
            anchors.push(Anchor {
                path,
                file,
                directory: metadata.is_dir(),
                identity: (metadata.dev(), metadata.ino()),
            });
        }
        anchors.sort_by(|left, right| {
            right
                .path
                .components()
                .count()
                .cmp(&left.path.components().count())
        });
        Ok(Self {
            anchors,
            cwd,
            limit: settings.byte_limit(),
        })
    }

    pub fn read_text(&self, input: &Path) -> Result<String> {
        if input.as_os_str().is_empty() || input.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(denied("MCP path input must be nonempty without NUL"));
        }
        if self.anchors.is_empty() {
            return Err(denied("path input is disabled: MCP allow_paths is empty"));
        }
        let input = if input.is_absolute() {
            input.to_owned()
        } else {
            self.cwd.join(input)
        };
        let resolved = input
            .canonicalize()
            .map_err(|_| denied("MCP input path cannot be resolved"))?;
        let anchor = self
            .anchors
            .iter()
            .find(|anchor| {
                if anchor.directory {
                    resolved.starts_with(&anchor.path)
                } else {
                    resolved == anchor.path
                }
            })
            .ok_or_else(|| denied("path resolves outside MCP allow_paths"))?;
        let metadata = std::fs::metadata(&anchor.path)
            .map_err(|_| denied("MCP read capability path changed; restart the server"))?;
        if (metadata.dev(), metadata.ino()) != anchor.identity {
            return Err(denied(
                "MCP read capability path changed; restart the server",
            ));
        }
        let file = if anchor.directory {
            let relative = resolved
                .strip_prefix(&anchor.path)
                .map_err(|_| denied("MCP path binding is invalid"))?;
            let components = relative
                .components()
                .map(|component| {
                    if let Component::Normal(name) = component {
                        Ok(name.to_owned())
                    } else {
                        Err(denied("MCP path binding is invalid"))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let (last, parents) = components
                .split_last()
                .ok_or_else(|| denied("MCP input must be a regular file"))?;
            let mut directory = anchor
                .file
                .try_clone()
                .map_err(|error| Error::io("holding MCP directory", error))?;
            for name in parents {
                directory = File::from(
                    rustix::fs::openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                        Mode::empty(),
                    )
                    .map_err(|_| {
                        denied("MCP directory changed or contains an unresolved symlink")
                    })?,
                );
            }
            File::from(
                rustix::fs::openat(
                    &directory,
                    last,
                    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                    Mode::empty(),
                )
                .map_err(|_| denied("MCP input changed or is not readable"))?,
            )
        } else {
            anchor
                .file
                .try_clone()
                .map_err(|error| Error::io("holding MCP file", error))?
        };
        let metadata = file
            .metadata()
            .map_err(|error| Error::io("checking MCP input", error))?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(denied("MCP input must be a regular single-link file"));
        }
        if metadata.len() > self.limit as u64 {
            return Err(denied("MCP input exceeds max_input_bytes"));
        }
        let maximum = self
            .limit
            .checked_add(1)
            .ok_or_else(|| denied("MCP input limit outside reader range"))?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        while bytes.len() < maximum {
            let length = (maximum - bytes.len()).min(buffer.len());
            let offset = u64::try_from(bytes.len())
                .map_err(|_| denied("MCP input offset outside reader range"))?;
            match file.read_at(&mut buffer[..length], offset) {
                Ok(0) => break,
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(Error::io("reading MCP input", error)),
            }
        }
        if bytes.len() > self.limit {
            return Err(denied("MCP input exceeds max_input_bytes"));
        }
        String::from_utf8(bytes).map_err(|_| denied("MCP path input must be UTF-8"))
    }
}

fn denied(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::mcp_access::{Scope, Settings};
    use std::num::NonZeroUsize;
    use std::path::PathBuf;

    #[test]
    fn empty_and_explicit_file_capabilities_deny_other_paths_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let input = root.join("input");
        std::fs::write(&input, "ALPHA\r\n").unwrap();
        let other = root.join("other");
        std::fs::write(&other, "private").unwrap();
        let empty = Scope::new(&Settings::default(), root).unwrap();
        assert!(empty.read_text(&input).is_err());
        let settings = Settings {
            allow_paths: vec![PathBuf::from("input")],
            ..Settings::default()
        };
        let scope = Scope::new(&settings, root).unwrap();
        assert_eq!(
            scope.read_text(&PathBuf::from("input")).unwrap(),
            "ALPHA\r\n"
        );
        std::thread::scope(|threads| {
            for _ in 0..4 {
                let scope = &scope;
                threads.spawn(move || {
                    assert_eq!(
                        scope.read_text(&PathBuf::from("input")).unwrap(),
                        "ALPHA\r\n"
                    )
                });
            }
        });
        assert_eq!(
            scope.read_text(&PathBuf::from("input")).unwrap(),
            "ALPHA\r\n"
        );
        assert!(scope.read_text(&other).is_err());
        assert_eq!(std::fs::read(&other).unwrap(), b"private");
    }

    #[test]
    fn directories_follow_internal_symlinks_and_deny_resolved_escapes_and_siblings() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let allowed = root.join("allowed");
        std::fs::create_dir(&allowed).unwrap();
        std::fs::write(allowed.join("input"), "inside").unwrap();
        std::fs::write(root.join("outside"), "private").unwrap();
        std::os::unix::fs::symlink("input", allowed.join("internal")).unwrap();
        std::os::unix::fs::symlink("../outside", allowed.join("escape")).unwrap();
        let scope = Scope::new(
            &Settings {
                allow_paths: vec![allowed.clone()],
                ..Settings::default()
            },
            root,
        )
        .unwrap();
        assert_eq!(
            scope.read_text(&allowed.join("internal")).unwrap(),
            "inside"
        );
        assert!(scope.read_text(&allowed.join("escape")).is_err());
        assert!(scope.read_text(&allowed.join("../outside")).is_err());
        let sibling = root.join("allowed-sibling");
        std::fs::create_dir(&sibling).unwrap();
        std::fs::write(sibling.join("input"), "private").unwrap();
        assert!(scope.read_text(&sibling.join("input")).is_err());
        assert!(scope.read_text(&allowed).is_err());
        let fifo = allowed.join("fifo");
        assert!(
            std::process::Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(scope.read_text(&fifo).is_err());
        assert!(
            Scope::new(
                &Settings {
                    allow_paths: vec![fifo],
                    ..Settings::default()
                },
                root
            )
            .is_err()
        );
    }

    #[test]
    fn replaced_roots_hardlinks_invalid_utf8_and_byte_overflow_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let allowed = root.join("allowed");
        std::fs::create_dir(&allowed).unwrap();
        let input = allowed.join("input");
        std::fs::write(&input, "inside").unwrap();
        let settings = Settings {
            allow_paths: vec![allowed.clone()],
            max_input_bytes: NonZeroUsize::new(4),
            ..Settings::default()
        };
        let scope = Scope::new(&settings, root).unwrap();
        assert!(scope.read_text(&input).is_err());
        std::fs::write(&input, [255]).unwrap();
        assert!(scope.read_text(&input).is_err());
        std::fs::write(&input, "ok").unwrap();
        std::fs::hard_link(&input, root.join("hard")).unwrap();
        assert!(scope.read_text(&input).is_err());
        std::fs::remove_file(root.join("hard")).unwrap();
        std::fs::rename(&allowed, root.join("held")).unwrap();
        std::fs::create_dir(&allowed).unwrap();
        std::fs::write(allowed.join("input"), "new").unwrap();
        assert!(scope.read_text(&allowed.join("input")).is_err());
        assert_eq!(std::fs::read(root.join("held/input")).unwrap(), b"ok");
    }

    #[test]
    fn configuration_rejects_ambiguous_and_invalid_allowlists() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let settings = Settings {
            presets: vec!["severity".into(), "severity".into()],
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        let settings = Settings {
            presets: vec!["../private".into()],
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        let settings = Settings {
            allow_paths: vec![root.to_owned(), root.join(".")],
            ..Settings::default()
        };
        assert!(Scope::new(&settings, root).is_err());
        assert!(
            serde_json::from_value::<Settings>(serde_json::json!({"max_input_bytes":0})).is_err()
        );
        assert!(serde_json::from_value::<Settings>(serde_json::json!({"exec":"true"})).is_err());
        let settings = Settings {
            max_input_bytes: NonZeroUsize::new(usize::MAX),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }
}
