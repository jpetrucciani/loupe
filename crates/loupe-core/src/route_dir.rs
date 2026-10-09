use std::fs::{DirBuilder, File};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;

use crate::error::{Error, ErrorKind, Result};
use crate::route_labels::Name;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Artifact {
    Journal,
    Manifest,
    ManifestTemp,
    Reset,
    ResetTemp,
}
impl Artifact {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Journal => ".loupe-route.journal",
            Self::Manifest => ".loupe-route.jsonl",
            Self::ManifestTemp => ".loupe-route.manifest.tmp",
            Self::Reset => ".loupe-route.reset.json",
            Self::ResetTemp => ".loupe-route.reset.tmp",
        }
    }
}
struct Root {
    directory: File,
    path: PathBuf,
}
impl Drop for Root {
    fn drop(&mut self) {
        // A concurrently spawned child may briefly inherit descriptors before exec.
        // Release ownership explicitly when the last route handle closes.
        let _ = self.directory.unlock();
    }
}
#[derive(Clone)]
pub struct Directory {
    root: Arc<Root>,
}
pub struct Output {
    file: File,
    root: Arc<Root>,
    created: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
    pub bytes: u64,
}
impl Output {
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
    pub fn created(&self) -> bool {
        self.created
    }
    pub fn identity(&self) -> Result<Identity> {
        let metadata = self
            .file
            .metadata()
            .map_err(|error| Error::io("reading route file identity", error))?;
        regular(&metadata)?;
        Ok(Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
        })
    }
    pub fn sync_directory(&self) -> Result<()> {
        self.root
            .directory
            .sync_all()
            .map_err(|error| Error::io("syncing route output directory", error))
    }
}
impl Directory {
    pub fn open(path: &Path, create: bool) -> Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "route output directory must be nonempty",
            ));
        }
        // A trailing slash or `/.` must not turn a final symlink into an ancestor.
        let normalized: PathBuf = path.components().collect();
        let path = normalized.as_path();
        let mut missing = Vec::new();
        if create {
            let absolute = if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir()
                    .map_err(|error| Error::io("resolving route directory", error))?
                    .join(path)
            };
            let mut cursor = absolute.as_path();
            while !cursor
                .try_exists()
                .map_err(|error| Error::io("checking route directory", error))?
            {
                missing.push(cursor.to_owned());
                cursor = cursor.parent().ok_or_else(|| {
                    Error::new(ErrorKind::Io, "route directory has no existing ancestor")
                })?;
            }
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .map_err(|error| Error::io("creating route output directory", error))?;
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        let directory: File = rustix::fs::open(path, flags, Mode::empty())
            .map_err(io_error)?
            .into();
        directory.try_lock().map_err(|error| {
            Error::new(
                ErrorKind::Io,
                format!("locking route output directory: {error}"),
            )
        })?;
        let resolved = std::fs::canonicalize(path)
            .map_err(|error| Error::io("resolving route output directory", error))?;
        // Durability includes newly created ancestors, rather than only leaf files.
        for created in &missing {
            if let Some(parent) = created.parent() {
                File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|error| Error::io("syncing created route directory parent", error))?;
            }
        }
        directory
            .sync_all()
            .map_err(|error| Error::io("syncing route output directory", error))?;
        Ok(Self {
            root: Arc::new(Root {
                directory,
                path: resolved,
            }),
        })
    }
    pub fn path(&self) -> &Path {
        &self.root.path
    }
    pub fn label(&self, name: &Name, create: bool) -> Result<Option<Output>> {
        self.entry(name.as_str(), create)
    }
    pub fn artifact(&self, artifact: Artifact, create: bool) -> Result<Option<Output>> {
        self.entry(artifact.as_str(), create)
    }
    fn entry(&self, name: &str, create: bool) -> Result<Option<Output>> {
        let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        let (descriptor, created) = if create {
            match rustix::fs::openat(
                &self.root.directory,
                name,
                flags | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(file) => (file, true),
                Err(Errno::EXIST) => (
                    rustix::fs::openat(&self.root.directory, name, flags, Mode::empty())
                        .map_err(io_error)?,
                    false,
                ),
                Err(error) => return Err(io_error(error)),
            }
        } else {
            match rustix::fs::openat(&self.root.directory, name, flags, Mode::empty()) {
                Ok(file) => (file, false),
                Err(Errno::NOENT) => return Ok(None),
                Err(error) => return Err(io_error(error)),
            }
        };
        let file: File = descriptor.into();
        regular(
            &file
                .metadata()
                .map_err(|error| Error::io("reading route file metadata", error))?,
        )?;
        let output = Output {
            file,
            root: self.root.clone(),
            created,
        };
        if created {
            output
                .file
                .sync_all()
                .map_err(|error| Error::io("syncing created route file", error))?;
            output.sync_directory()?;
        }
        Ok(Some(output))
    }
    pub fn replace_manifest(&self) -> Result<()> {
        self.replace_manifest_at(|| {})
    }
    pub(crate) fn replace_manifest_at(&self, after_rename: impl FnOnce()) -> Result<()> {
        self.replace_artifact(Artifact::ManifestTemp, Artifact::Manifest, after_rename)
    }
    pub(crate) fn replace_artifact(
        &self,
        from: Artifact,
        to: Artifact,
        after_rename: impl FnOnce(),
    ) -> Result<()> {
        // Validate both entries before replacement; renameat never follows the destination.
        let temporary = self
            .artifact(from, false)?
            .ok_or_else(|| Error::new(ErrorKind::Io, "route manifest temporary file is missing"))?;
        let _existing = self.artifact(to, false)?;
        temporary
            .file
            .sync_all()
            .map_err(|error| Error::io("syncing route manifest temporary file", error))?;
        rustix::fs::renameat(
            &self.root.directory,
            from.as_str(),
            &self.root.directory,
            to.as_str(),
        )
        .map_err(io_error)?;
        after_rename();
        temporary.sync_directory()
    }
    pub(crate) fn remove_artifact(&self, artifact: Artifact) -> Result<()> {
        if let Some(output) = self.artifact(artifact, false)? {
            rustix::fs::unlinkat(
                &self.root.directory,
                artifact.as_str(),
                rustix::fs::AtFlags::empty(),
            )
            .map_err(io_error)?;
            output.sync_directory()?;
        }
        Ok(())
    }
}
fn regular(metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(Error::new(
            ErrorKind::Io,
            "route entries must be regular files with exactly one hard link",
        ));
    }
    Ok(())
}
fn io_error(error: Errno) -> Error {
    Error::io("opening confined route entry", std::io::Error::from(error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::route_dir::{Artifact, Directory};
    use crate::route_labels::Name;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn new_files_are_private_and_directory_lock_lives_as_long_as_open_outputs() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new/nested");
        let directory = Directory::open(&path, true).unwrap();
        let name = Name::parse("page").unwrap();
        let mut output = directory.label(&name, true).unwrap().unwrap();
        assert!(output.created());
        assert_eq!(output.identity().unwrap().bytes, 0);
        output.file_mut().write_all(b"original\r\n").unwrap();
        output.file().sync_all().unwrap();
        assert_eq!(
            std::fs::metadata(path.join("page"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(Directory::open(&path, false).is_err());
        drop(directory);
        assert!(Directory::open(&path, false).is_err());
        drop(output);
        let directory = Directory::open(&path, false).unwrap();
        let mut reopened = directory.label(&name, false).unwrap().unwrap();
        assert!(!reopened.created());
        let mut raw = Vec::new();
        reopened.file_mut().read_to_end(&mut raw).unwrap();
        assert_eq!(raw, b"original\r\n");
        assert!(
            directory
                .label(&Name::parse("missing").unwrap(), false)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn external_symlinks_hardlinks_directories_and_fifos_fail_without_writing_outside() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"keep these bytes").unwrap();
        let path = root.path().join("out");
        let directory = Directory::open(&path, true).unwrap();
        symlink(&outside, path.join("page")).unwrap();
        assert!(
            directory
                .label(&Name::parse("page").unwrap(), true)
                .is_err()
        );
        std::fs::hard_link(&outside, path.join("ticket")).unwrap();
        assert!(
            directory
                .label(&Name::parse("ticket").unwrap(), true)
                .is_err()
        );
        std::fs::create_dir(path.join("noise")).unwrap();
        assert!(
            directory
                .label(&Name::parse("noise").unwrap(), true)
                .is_err()
        );
        let status = std::process::Command::new("mkfifo")
            .arg(path.join("fifo"))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            directory
                .label(&Name::parse("fifo").unwrap(), true)
                .is_err()
        );
        for artifact in [
            Artifact::Journal,
            Artifact::Manifest,
            Artifact::ManifestTemp,
        ] {
            symlink(&outside, path.join(artifact.as_str())).unwrap();
            assert!(directory.artifact(artifact, true).is_err());
        }
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep these bytes");
        symlink(&path, root.path().join("linked-root")).unwrap();
        assert!(Directory::open(&root.path().join("linked-root"), false).is_err());
        let trailing = format!("{}/", root.path().join("linked-root").display());
        assert!(Directory::open(std::path::Path::new(&trailing), true).is_err());
        assert!(Directory::open(&root.path().join("linked-root/./"), true).is_err());
    }
    #[test]
    fn held_directory_descriptor_does_not_follow_a_replaced_output_root_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        let directory = Directory::open(&path, true).unwrap();
        let moved = root.path().join("original-root");
        std::fs::rename(&path, &moved).unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, &path).unwrap();
        let mut output = directory
            .label(&Name::parse("page").unwrap(), true)
            .unwrap()
            .unwrap();
        output
            .file_mut()
            .write_all(b"under original descriptor")
            .unwrap();
        output.file().sync_all().unwrap();
        assert!(!outside.join("page").exists());
        assert_eq!(
            std::fs::read(moved.join("page")).unwrap(),
            b"under original descriptor"
        );
    }
    #[test]
    fn recoverable_manifest_replacement_is_atomic_and_rejects_external_aliases() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(root.path(), false).unwrap();
        let mut manifest = directory
            .artifact(Artifact::Manifest, true)
            .unwrap()
            .unwrap();
        manifest.file_mut().write_all(b"old manifest\n").unwrap();
        let mut temporary = directory
            .artifact(Artifact::ManifestTemp, true)
            .unwrap()
            .unwrap();
        temporary.file_mut().write_all(b"new manifest\n").unwrap();
        directory.replace_manifest().unwrap();
        assert_eq!(
            std::fs::read(root.path().join(Artifact::Manifest.as_str())).unwrap(),
            b"new manifest\n"
        );
        manifest.file_mut().seek(SeekFrom::Start(0)).unwrap();
        let mut old = String::new();
        manifest.file_mut().read_to_string(&mut old).unwrap();
        assert_eq!(old, "old manifest\n");
        assert!(
            directory
                .artifact(Artifact::ManifestTemp, false)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn route_directory_lock_child() {
        let Some(path) = std::env::var_os("LOUPE_ROUTE_LOCK_TEST_DIRECTORY") else {
            return;
        };
        assert!(Directory::open(std::path::Path::new(&path), false).is_err());
    }
    #[test]
    fn independent_process_cannot_open_an_active_directory_writer() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(root.path(), false).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "route_dir::tests::route_directory_lock_child"])
            .env("LOUPE_ROUTE_LOCK_TEST_DIRECTORY", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(directory);
    }
    #[test]
    fn final_handle_drop_releases_lock_even_when_an_inherited_descriptor_remains_open() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(root.path(), false).unwrap();
        let inherited = directory.root.directory.try_clone().unwrap();
        assert!(Directory::open(root.path(), false).is_err());
        drop(directory);
        let reopened = Directory::open(root.path(), false).unwrap();
        drop(inherited);
        assert!(Directory::open(root.path(), false).is_err());
        drop(reopened);
        assert!(Directory::open(root.path(), false).is_ok());
    }
}
