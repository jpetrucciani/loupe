//! Durable claims precede effects. A committed claim may lose an action, never retry it.

use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use redb::{
    Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
};
use rustix::fs::{Mode, OFlags};
use serde_json::Value;

use crate::encoder;
use crate::error::{Error, ErrorKind, Result};

const META: TableDefinition<&str, u64> = TableDefinition::new("on_metadata");
const IDENTITY: TableDefinition<u8, &[u8]> = TableDefinition::new("on_identity");
const CLAIMS: TableDefinition<&[u8], u64> = TableDefinition::new("on_claims");
const EXPIRY: TableDefinition<&[u8], ()> = TableDefinition::new("on_expiry");
pub const CACHE_BYTES: usize = 8 * 1024 * 1024;

pub struct Store {
    database: Database,
    inode: (u64, u64),
    window_ns: Option<u64>,
    capacity: Option<u64>,
}

impl Store {
    pub fn open(
        path: &Path,
        identity: &Value,
        window_ns: Option<u64>,
        capacity: Option<u64>,
        now: u64,
        protected: &[(u64, u64)],
    ) -> Result<Self> {
        if window_ns == Some(0) || capacity == Some(0) || window_ns.is_none() && capacity.is_some()
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "durable claim window/capacity must be positive; permanent claims have no live window cap",
            ));
        }
        let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        let file = match rustix::fs::open(
            path,
            flags | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::EXIST) => File::from(
                rustix::fs::open(path, flags, Mode::empty())
                    .map_err(|error| Error::io("opening on state", error.into()))?,
            ),
            Err(error) => return Err(Error::io("creating on state", error.into())),
        };
        let metadata = file
            .metadata()
            .map_err(|error| Error::io("checking on state", error))?;
        let inode = (metadata.dev(), metadata.ino());
        if !metadata.is_file() || metadata.nlink() != 1 || protected.contains(&inode) {
            return Err(Error::new(
                ErrorKind::Io,
                "on state must be a regular single-link file distinct from inputs and outputs",
            ));
        }
        let mut builder = Database::builder();
        builder.set_cache_size(CACHE_BYTES);
        let database = builder.create_file(file).map_err(db_error)?;
        let rendered = encoder::render(identity)?;
        let initialize = database
            .begin_read()
            .map_err(db_error)?
            .list_tables()
            .map_err(db_error)?
            .next()
            .is_none();
        if initialize {
            let mut transaction = database.begin_write().map_err(db_error)?;
            transaction
                .set_durability(Durability::Immediate)
                .map_err(db_error)?;
            {
                let mut meta = transaction.open_table(META).map_err(db_error)?;
                meta.insert("version", 1).map_err(db_error)?;
                meta.insert("window_ns", window_ns.unwrap_or(0))
                    .map_err(db_error)?;
                meta.insert("capacity", capacity.unwrap_or(0))
                    .map_err(db_error)?;
                meta.insert("clock_ns", now).map_err(db_error)?;
                transaction
                    .open_table(IDENTITY)
                    .map_err(db_error)?
                    .insert(0, rendered.as_bytes())
                    .map_err(db_error)?;
                transaction.open_table(CLAIMS).map_err(db_error)?;
                transaction.open_table(EXPIRY).map_err(db_error)?;
            }
            transaction.commit().map_err(db_error)?;
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| Error::io("syncing on state directory", error))?;
        {
            let transaction = database.begin_read().map_err(db_error)?;
            let meta = transaction.open_table(META).map_err(db_error)?;
            for (key, value) in [
                ("version", 1),
                ("window_ns", window_ns.unwrap_or(0)),
                ("capacity", capacity.unwrap_or(0)),
            ] {
                if meta.get(key).map_err(db_error)?.map(|value| value.value()) != Some(value) {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "on state version/window/capacity changed; use a separate state file",
                    ));
                }
            }
            if meta
                .get("clock_ns")
                .map_err(db_error)?
                .is_none_or(|clock| clock.value() > now)
            {
                return Err(Error::new(
                    ErrorKind::Input,
                    "on state clock moved backwards; claims remain unchanged",
                ));
            }
            let identity = transaction.open_table(IDENTITY).map_err(db_error)?;
            if identity
                .get(0)
                .map_err(db_error)?
                .is_none_or(|value| value.value() != rendered.as_bytes())
            {
                return Err(Error::new(
                    ErrorKind::Input,
                    "on state action/question/input policy changed; use a separate state file",
                ));
            }
            transaction.open_table(CLAIMS).map_err(db_error)?;
            transaction.open_table(EXPIRY).map_err(db_error)?;
        }
        Ok(Self {
            database,
            inode,
            window_ns,
            capacity,
        })
    }

    pub fn inode(&self) -> (u64, u64) {
        self.inode
    }

    pub fn claim(&self, content: &[u8; 32], action: usize, now: u64) -> Result<bool> {
        self.claim_with(content, action, now, |_| {})
    }

    fn claim_with(
        &self,
        content: &[u8; 32],
        action: usize,
        now: u64,
        mut boundary: impl FnMut(bool),
    ) -> Result<bool> {
        self.expire(now)?;
        let mut key = [0u8; 40];
        key[..32].copy_from_slice(content);
        let action = u64::try_from(action)
            .map_err(|_| Error::new(ErrorKind::Input, "on action ID outside state range"))?;
        key[32..].copy_from_slice(&action.to_be_bytes());
        let until = match self.window_ns {
            Some(window) => now
                .checked_add(window)
                .filter(|until| *until < u64::MAX)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Input,
                        "on claim expiry outside state clock range",
                    )
                })?,
            None => u64::MAX,
        };
        let mut transaction = self.database.begin_write().map_err(db_error)?;
        transaction
            .set_durability(Durability::Immediate)
            .map_err(db_error)?;
        {
            let mut claims = transaction.open_table(CLAIMS).map_err(db_error)?;
            if claims.get(key.as_slice()).map_err(db_error)?.is_some() {
                return Ok(false);
            }
            if let Some(capacity) = self.capacity
                && claims.len().map_err(db_error)? >= capacity
            {
                return Err(Error::new(
                    ErrorKind::Input,
                    "on durable live claim capacity exhausted",
                ));
            }
            claims.insert(key.as_slice(), until).map_err(db_error)?;
            if self.window_ns.is_some() {
                let mut expiry_key = [0u8; 48];
                expiry_key[..8].copy_from_slice(&until.to_be_bytes());
                expiry_key[8..].copy_from_slice(&key);
                transaction
                    .open_table(EXPIRY)
                    .map_err(db_error)?
                    .insert(expiry_key.as_slice(), ())
                    .map_err(db_error)?;
            }
        }
        boundary(false);
        transaction.commit().map_err(db_error)?;
        boundary(true);
        Ok(true)
    }

    fn expire(&self, now: u64) -> Result<()> {
        loop {
            let mut transaction = self.database.begin_write().map_err(db_error)?;
            transaction
                .set_durability(Durability::Immediate)
                .map_err(db_error)?;
            let removed = {
                let mut meta = transaction.open_table(META).map_err(db_error)?;
                if meta
                    .get("clock_ns")
                    .map_err(db_error)?
                    .is_none_or(|clock| clock.value() > now)
                {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "on state clock moved backwards",
                    ));
                }
                meta.insert("clock_ns", now).map_err(db_error)?;
                if self.window_ns.is_none() {
                    0
                } else {
                    let mut upper = [u8::MAX; 48];
                    upper[..8].copy_from_slice(&now.to_be_bytes());
                    let mut expiry = transaction.open_table(EXPIRY).map_err(db_error)?;
                    let keys = expiry
                        .range(..=upper.as_slice())
                        .map_err(db_error)?
                        .take(1024)
                        .map(|entry| entry.map(|(key, _)| key.value().to_vec()).map_err(db_error))
                        .collect::<Result<Vec<_>>>()?;
                    let mut claims = transaction.open_table(CLAIMS).map_err(db_error)?;
                    for key in &keys {
                        let content = key
                            .get(8..)
                            .filter(|key| key.len() == 40)
                            .ok_or_else(|| Error::new(ErrorKind::Io, "invalid on expiry key"))?;
                        claims.remove(content).map_err(db_error)?;
                        expiry.remove(key.as_slice()).map_err(db_error)?;
                    }
                    keys.len()
                }
            };
            transaction.commit().map_err(db_error)?;
            if removed < 1024 {
                return Ok(());
            }
        }
    }
}

fn db_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, format!("on state: {error}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::trigger_state::Store;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::process::{Command, Stdio};

    #[test]
    fn permanent_claims_reopen_independently_per_content_action_and_policy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let identity = json!({"policy":"one"});
        let store = Store::open(&path, &identity, None, None, 1, &[]).unwrap();
        assert!(store.claim(&[1; 32], 1, 2).unwrap());
        assert!(!store.claim(&[1; 32], 1, 3).unwrap());
        assert!(store.claim(&[1; 32], 2, 3).unwrap());
        assert!(store.claim(&[2; 32], 1, 3).unwrap());
        assert!(Store::open(&path, &identity, None, None, 3, &[]).is_err());
        drop(store);
        let store = Store::open(&path, &identity, None, None, 100000, &[]).unwrap();
        assert!(!store.claim(&[1; 32], 1, 100000).unwrap());
        drop(store);
        assert!(Store::open(&path, &json!({"policy":"changed"}), None, None, 100000, &[]).is_err());
        assert!(Store::open(&path, &identity, Some(10), Some(3), 100000, &[]).is_err());
        assert!(Store::open(&path, &identity, None, None, 1, &[]).is_err());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn expiry_equality_downtime_capacity_and_batch_pruning_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let identity = json!({"policy":"window"});
        let store = Store::open(&path, &identity, Some(10), Some(1030), 0, &[]).unwrap();
        for index in 0..1030usize {
            assert!(store.claim(&[1; 32], index, 0).unwrap());
        }
        assert!(store.claim(&[2; 32], 1, 9).is_err());
        assert!(!store.claim(&[1; 32], 1, 9).unwrap());
        drop(store);
        let store = Store::open(&path, &identity, Some(10), Some(1030), 10, &[]).unwrap();
        assert!(store.claim(&[2; 32], 1, 10).unwrap());
        assert!(store.claim(&[1; 32], 1, 10).unwrap());
        assert!(!store.claim(&[1; 32], 1, 19).unwrap());
        assert!(store.claim(&[1; 32], 1, 20).unwrap());
        assert!(store.claim(&[1; 32], 2, 19).is_err());
    }

    #[test]
    fn symlinks_hardlinks_input_aliases_and_non_state_bytes_remain_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, b"original").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let identity = json!({});
        assert!(Store::open(&path, &identity, None, None, 0, &[(meta.dev(), meta.ino())]).is_err());
        assert!(Store::open(&path, &identity, None, None, 0, &[]).is_err());
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Store::open(&link, &identity, None, None, 0, &[]).is_err());
        let hard = dir.path().join("hard");
        std::fs::hard_link(&path, &hard).unwrap();
        assert!(Store::open(&hard, &identity, None, None, 0, &[]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    }

    #[tokio::test]
    async fn saved_real_server_decision_runs_exec_only_after_a_fresh_durable_claim() {
        use crate::policy::{Policy, Selected};
        use crate::records::{Format, Options, Source};
        use crate::wire::{Request, Response};
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-trigger.json")).unwrap();
        assert_eq!(fixture["endpoint"], "http://127.0.0.1:8015/v1/systemone");
        let row = &fixture;
        let request: Request = serde_json::from_value(row["request"].clone()).unwrap();
        let response: Response = serde_json::from_value(row["response"].clone()).unwrap();
        let answer = response
            .validate(&request)
            .unwrap()
            .shift_remove("decision")
            .unwrap();
        let assessment = answer.assessment(&Policy::default()).unwrap();
        assert!(matches!(assessment.selected, Selected::Boolean(true)));
        assert_eq!(
            row["counts"]["input_tokens"],
            row["response"]["usage"]["input_tokens"]
        );
        let mut source = Source::new(
            std::io::Cursor::new(fixture["input"].as_str().unwrap().as_bytes()),
            Options::new(Format::Lines),
            None,
        )
        .unwrap();
        let unit = source.next_unit().unwrap().unwrap();
        assert_eq!(unit.state, request.state);
        let content =
            *blake3::hash(crate::encoder::render(&unit.state).unwrap().as_bytes()).as_bytes();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let effect = dir.path().join("effect");
        let failed_effect = dir.path().join("failed-effect");
        let command = format!("printf once >> '{}'", effect.display());
        let failed_command = format!("printf once >> '{}'; exit 7", failed_effect.display());
        let identity = json!({"request":request,"actions":[command,failed_command]});
        for clock in [1, 2] {
            let store = Store::open(&path, &identity, None, None, clock, &[]).unwrap();
            for (position, command) in [&command, &failed_command].into_iter().enumerate() {
                if !store.claim(&content, position + 1, clock).unwrap() {
                    continue;
                }
                let result = crate::trigger::execute(
                    command,
                    &unit,
                    "yes",
                    &assessment,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                )
                .await
                .unwrap();
                assert_eq!(result.failed(), position == 1);
            }
        }
        assert_eq!(std::fs::read(effect).unwrap(), b"once");
        assert_eq!(std::fs::read(failed_effect).unwrap(), b"once");
    }

    #[test]
    #[ignore = "subprocess helper for actual SIGKILL claim boundaries"]
    fn claim_process() {
        let root = std::path::PathBuf::from(std::env::var("LOUPE_STATE_TEST_DIR").unwrap());
        let phase = std::env::var("LOUPE_STATE_TEST_PHASE").unwrap();
        let store = Store::open(
            &root.join("state"),
            &json!({"test":"crash"}),
            None,
            None,
            1,
            &[],
        )
        .unwrap();
        let stop = || {
            println!("ready");
            std::io::stdout().flush().unwrap();
            rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::STOP)
                .unwrap();
        };
        if store
            .claim_with(&[7; 32], 1, 1, |committed| {
                if (committed && phase == "committed") || (!committed && phase == "pending") {
                    stop();
                }
            })
            .unwrap()
        {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("effect"))
                .unwrap();
            file.write_all(b"once\n").unwrap();
            file.sync_all().unwrap();
        }
        if phase == "effect" {
            stop();
        }
    }

    #[test]
    fn killed_writers_rollback_pending_claims_and_preserve_committed_claims_before_effects() {
        for phase in ["pending", "committed", "effect"] {
            let dir = tempfile::tempdir().unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "trigger_state::tests::claim_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("LOUPE_STATE_TEST_DIR", dir.path())
                .env("LOUPE_STATE_TEST_PHASE", phase)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut reader = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line.trim() == "ready" {
                    break;
                }
            }
            child.kill().unwrap();
            child.wait().unwrap();
            let store = Store::open(
                &dir.path().join("state"),
                &json!({"test":"crash"}),
                None,
                None,
                2,
                &[],
            )
            .unwrap();
            assert_eq!(store.claim(&[7; 32], 1, 2).unwrap(), phase == "pending");
            let effect = std::fs::read(dir.path().join("effect")).unwrap_or_default();
            assert_eq!(
                effect,
                if phase == "effect" {
                    b"once\n".to_vec()
                } else {
                    vec![]
                }
            );
        }
    }
}
