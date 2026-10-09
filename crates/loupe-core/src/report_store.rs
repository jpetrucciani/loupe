use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags, linkat, mkdirat, open, openat, unlinkat};

use crate::error::{Error, ErrorKind, Result};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    generated_at: String,
    files: BTreeMap<String, FilePin>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePin {
    bytes: u64,
    blake3: String,
}

/// Read an already published bundle without creating state or following file links.
pub fn read(root: &Path) -> Result<Vec<u8>> {
    let descriptor = open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| Error::io("holding saved report bundle", error.into()))?;
    let directory = File::from(descriptor);
    read_held(&directory)
}

/// Walk a relative bundle beneath an already held baseline directory.
pub fn read_relative(parent: &File, path: &Path) -> Result<Vec<u8>> {
    if path.as_os_str().is_empty() {
        return Err(invalid(
            "saved report bundle path must be nonempty and relative",
        ));
    }
    let mut directory = parent
        .try_clone()
        .map_err(|error| Error::io("holding baseline directory", error))?;
    for part in path.components() {
        let Component::Normal(name) = part else {
            return Err(invalid(
                "saved report bundle must stay beneath the baseline directory",
            ));
        };
        directory = File::from(
            openat(
                &directory,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| Error::io("holding pinned baseline report bundle", error.into()))?,
        );
    }
    read_held(&directory)
}

fn read_held(directory: &File) -> Result<Vec<u8>> {
    let bytes = read_file(directory, "manifest.json", None, 64 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("invalid report manifest: {error}")))?;
    if manifest.schema_version != 1
        || manifest.generated_at.is_empty()
        || manifest.files.len() != 2
        || !manifest.files.contains_key("report.json")
        || !manifest.files.contains_key("report.md")
    {
        return Err(invalid("unsupported or incomplete report bundle manifest"));
    }
    let mut result = None;
    for (name, pin) in &manifest.files {
        if pin.blake3.len() != 64
            || !pin
                .blake3
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("invalid saved report content digest"));
        }
        let bytes = read_file(directory, name, Some(pin.bytes), pin.bytes)?;
        if blake3::hash(&bytes).to_hex().as_str() != pin.blake3 {
            return Err(invalid(format!("saved report content changed: {name}")));
        }
        if name == "report.json" {
            result = Some(bytes);
        }
    }
    result.ok_or_else(|| invalid("published report JSON is missing"))
}

fn read_file(directory: &File, name: &str, expected: Option<u64>, cap: u64) -> Result<Vec<u8>> {
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| Error::io("opening saved report file", error.into()))?;
    let file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|error| Error::io("checking saved report file", error))?;
    if !metadata.is_file()
        || metadata.len() > cap
        || expected.is_some_and(|length| length != metadata.len())
    {
        return Err(invalid(
            "saved report is not a regular file of the pinned size",
        ));
    }
    let mut bytes = Vec::new();
    file.take(
        cap.checked_add(1)
            .ok_or_else(|| invalid("saved report size is outside the supported range"))?,
    )
    .read_to_end(&mut bytes)
    .map_err(|error| Error::io("reading saved report file", error))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(invalid("saved report size changed during reading"));
    }
    Ok(bytes)
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}

pub struct Stamp {
    pub generated_at: String,
    id: String,
}
impl Stamp {
    pub fn now() -> Self {
        let now = chrono::Utc::now();
        Self {
            generated_at: now.to_rfc3339(),
            id: format!(
                "eval-{}-{}",
                now.format("%Y%m%dT%H%M%S%fZ"),
                std::process::id()
            ),
        }
    }
}
pub struct Store {
    directory: File,
    root: PathBuf,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).map_err(|error| Error::io("creating report root", error))?;
        let descriptor = open(
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding report root", error.into()))?;
        Ok(Self {
            directory: File::from(descriptor),
            root: root.to_owned(),
        })
    }
    pub fn publish(&self, json: &[u8], markdown: &[u8]) -> Result<PathBuf> {
        let stamp = Stamp::now();
        mkdirat(
            &self.directory,
            &stamp.id,
            Mode::RUSR | Mode::WUSR | Mode::XUSR,
        )
        .map_err(|error| Error::io("creating private report bundle", error.into()))?;
        let descriptor = openat(
            &self.directory,
            &stamp.id,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::io("holding report bundle", error.into()))?;
        let bundle = File::from(descriptor);
        write(&bundle, "report.json", json)?;
        write(&bundle, "report.md", markdown)?;
        bundle
            .sync_all()
            .map_err(|error| Error::io("syncing report files", error))?;
        let manifest=serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"generated_at":stamp.generated_at,"files":{"report.json":{"bytes":json.len(),"blake3":blake3::hash(json).to_hex().to_string()},"report.md":{"bytes":markdown.len(),"blake3":blake3::hash(markdown).to_hex().to_string()}}})).map_err(|error|crate::error::Error::new(crate::error::ErrorKind::Io,error.to_string()))?;
        write(&bundle, ".manifest.pending", &manifest)?;
        linkat(
            &bundle,
            ".manifest.pending",
            &bundle,
            "manifest.json",
            AtFlags::empty(),
        )
        .map_err(|error| Error::io("publishing report manifest", error.into()))?;
        unlinkat(&bundle, ".manifest.pending", AtFlags::empty())
            .map_err(|error| Error::io("removing pending report manifest", error.into()))?;
        bundle
            .sync_all()
            .map_err(|error| Error::io("publishing report manifest", error))?;
        self.directory
            .sync_all()
            .map_err(|error| Error::io("syncing report root", error))?;
        Ok(self.root.join(stamp.id))
    }
}

fn write(directory: &File, name: &str, bytes: &[u8]) -> Result<()> {
    let descriptor = openat(
        directory,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| Error::io("creating report file", error.into()))?;
    let mut file = File::from(descriptor);
    file.write_all(bytes)
        .map_err(|error| Error::io("writing report file", error))?;
    file.sync_all()
        .map_err(|error| Error::io("syncing report file", error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use crate::report_store::Store;

    #[test]
    fn publishes_private_non_overwriting_bundles_with_verified_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let first = store
            .publish(b"{\"actual\":true}\n", b"Measured result\n")
            .unwrap();
        let second = store.publish(b"{}\n", b"Another result\n").unwrap();
        assert_ne!(first, second);
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(first.join("manifest.json")).unwrap()).unwrap();
        for name in ["report.json", "report.md"] {
            let path = first.join(name);
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(manifest["files"][name]["bytes"], bytes.len());
            assert_eq!(
                manifest["files"][name]["blake3"],
                blake3::hash(&bytes).to_hex().as_str()
            );
        }
        assert_eq!(
            std::fs::metadata(first.join("manifest.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(!first.join(".manifest.pending").exists());
        assert_eq!(
            std::fs::read(first.join("report.json")).unwrap(),
            b"{\"actual\":true}\n"
        );
        assert_eq!(
            crate::report_store::read(&first).unwrap(),
            b"{\"actual\":true}\n"
        );
    }

    #[test]
    fn held_root_survives_replacement_and_final_symlinks_are_denied() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("reports");
        let moved = directory.path().join("original");
        let outside = directory.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let store = Store::open(&root).unwrap();
        std::fs::rename(&root, &moved).unwrap();
        symlink(&outside, &root).unwrap();
        let published = store.publish(b"{}", b"result").unwrap();
        let name = published.file_name().unwrap();
        assert!(moved.join(name).join("manifest.json").is_file());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert!(Store::open(&root).is_err());
        assert!(crate::report_store::read(&root).is_err());
    }

    #[test]
    fn relative_bundle_reads_hold_the_root_and_reject_traversal_and_linked_ancestors() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let moved = directory.path().join("held");
        let outside = directory.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let store = Store::open(&root.join("reports")).unwrap();
        let bundle = store
            .publish(b"{\"codec\":true}\n", b"Filesystem codec fixture\n")
            .unwrap();
        let relative = bundle.strip_prefix(&root).unwrap().to_owned();
        let held = std::fs::File::open(&root).unwrap();
        assert_eq!(
            crate::report_store::read_relative(&held, &relative).unwrap(),
            b"{\"codec\":true}\n"
        );
        symlink("reports", root.join("linked")).unwrap();
        assert!(
            crate::report_store::read_relative(
                &held,
                &std::path::Path::new("linked").join(bundle.file_name().unwrap())
            )
            .is_err()
        );
        for path in [
            std::path::Path::new(""),
            std::path::Path::new("."),
            std::path::Path::new("../outside"),
            outside.as_path(),
        ] {
            assert!(crate::report_store::read_relative(&held, path).is_err());
        }
        std::fs::rename(&root, &moved).unwrap();
        symlink(&outside, &root).unwrap();
        assert_eq!(
            crate::report_store::read_relative(&held, &relative).unwrap(),
            b"{\"codec\":true}\n"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn saved_bundles_refuse_unpublished_tampered_and_redirected_content() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let bundle = store.publish(b"{}\n", b"Measured result\n").unwrap();
        std::fs::write(bundle.join("report.json"), b"[]\n").unwrap();
        assert!(crate::report_store::read(&bundle).is_err());
        std::fs::write(bundle.join("report.json"), b"{}\n").unwrap();
        std::fs::write(bundle.join("report.md"), b"Changed result\n").unwrap();
        assert!(crate::report_store::read(&bundle).is_err());
        std::fs::write(bundle.join("report.md"), b"Measured result\n").unwrap();
        let outside = directory.path().join("outside.json");
        std::fs::write(&outside, b"{}\n").unwrap();
        std::fs::remove_file(bundle.join("report.json")).unwrap();
        symlink(&outside, bundle.join("report.json")).unwrap();
        assert!(crate::report_store::read(&bundle).is_err());
        std::fs::remove_file(bundle.join("report.json")).unwrap();
        rustix::fs::mknodat(
            rustix::fs::CWD,
            bundle.join("report.json"),
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        assert!(crate::report_store::read(&bundle).is_err());
        std::fs::remove_file(bundle.join("manifest.json")).unwrap();
        assert!(crate::report_store::read(&bundle).is_err());
    }
}
