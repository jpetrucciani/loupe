use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{
    Database, ReadOnlyDatabase, ReadTransaction, ReadableDatabase, ReadableTable,
    ReadableTableMetadata, TableDefinition,
};
use serde::{Deserialize, Serialize};

use crate::encoder::{self, ClefEncoder};
use crate::error::{Error, ErrorKind, Result};
use crate::identity::{Discovery, Model};
use crate::wire::{Request, Response};

pub const VERSION: u64 = 1;
pub const FILE_NAME: &str = "responses.redb";
const META: TableDefinition<&str, u64> = TableDefinition::new("loupe_metadata");
const ENTRIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("loupe_responses");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key([u8; 32]);

pub fn key(identity: &Discovery, encoder: Option<&ClefEncoder>, request: &Request) -> Result<Key> {
    request.validate()?;
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"loupe-request-cache-v1")?;
    frame(&mut hasher, identity.model.as_str().as_bytes())?;
    frame(&mut hasher, identity.encoder_identity.as_bytes())?;
    if let Some(encoder) = encoder {
        if encoder.identity != identity.encoder_identity {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "cache encoder/tokenizer identity does not match fresh discovery",
            ));
        }
        frame(&mut hasher, encoder.identity.as_bytes())?;
        let encoded = encoder.encode(request)?.0;
        frame(
            &mut hasher,
            &serde_json::to_vec(&encoded).map_err(json_error)?,
        )?;
        // Preserve wire types such as numeric versus string Score legend values.
        let typed = serde_json::to_value(request).map_err(json_error)?;
        frame(&mut hasher, encoder::render(&typed)?.as_bytes())?;
    } else {
        frame(&mut hasher, b"unverified-encoder-full-wire-v1")?;
        // An unknown backend may preserve object order in its prompt encoding.
        frame(
            &mut hasher,
            &serde_json::to_vec(request).map_err(json_error)?,
        )?;
    }
    Ok(Key(*hasher.finalize().as_bytes()))
}
fn frame(hasher: &mut blake3::Hasher, value: &[u8]) -> Result<()> {
    let length = u64::try_from(value.len())
        .map_err(|_| Error::new(ErrorKind::Io, "cache-key field length overflow"))?;
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u64,
    pub created_at: u64,
    pub model: Model,
    pub encoder_identity: String,
    pub response: Response,
}
impl Entry {
    pub fn new(identity: &Discovery, response: Response) -> Result<Self> {
        if Model::parse(&response.model)? != identity.model {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "cache response identity differs from fresh discovery",
            ));
        }
        Ok(Self {
            version: VERSION,
            created_at: now()?,
            model: identity.model.clone(),
            encoder_identity: identity.encoder_identity.clone(),
            response,
        })
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        let entry: Self = serde_json::from_slice(bytes).map_err(json_error)?;
        if entry.version != VERSION {
            return Err(Error::new(
                ErrorKind::Io,
                "unsupported cache-entry version; clear the cache explicitly",
            ));
        }
        Ok(entry)
    }
}

enum Access {
    ReadOnly(ReadOnlyDatabase),
    Write(Database),
}
pub struct Store {
    access: Access,
    path: PathBuf,
}
#[derive(Debug, Serialize)]
pub struct Stats {
    pub version: u64,
    pub present: bool,
    pub entries: u64,
    pub payload_bytes: u64,
    pub database_bytes: u64,
    pub oldest: Option<u64>,
    pub newest: Option<u64>,
    pub by_model: BTreeMap<String, u64>,
}
impl Default for Stats {
    fn default() -> Self {
        Self {
            version: VERSION,
            present: false,
            entries: 0,
            payload_bytes: 0,
            database_bytes: 0,
            oldest: None,
            newest: None,
            by_model: BTreeMap::new(),
        }
    }
}

impl Store {
    pub fn open(directory: &Path, read_only: bool) -> Result<Option<Self>> {
        let path = directory.join(FILE_NAME);
        if read_only
            && !path
                .try_exists()
                .map_err(|error| Error::io("checking cache", error))?
        {
            return Ok(None);
        }
        let access = if read_only {
            Access::ReadOnly(ReadOnlyDatabase::open(&path).map_err(db_error)?)
        } else {
            std::fs::create_dir_all(directory)
                .map_err(|error| Error::io("creating cache directory", error))?;
            let (file, new) = open_file(&path)?;
            let database = Database::builder().create_file(file).map_err(db_error)?;
            if new {
                initialize(&database)?;
            }
            Access::Write(database)
        };
        let store = Self { access, path };
        store.validate()?;
        Ok(Some(store))
    }
    fn read(&self) -> Result<ReadTransaction> {
        match &self.access {
            Access::ReadOnly(db) => db.begin_read().map_err(db_error),
            Access::Write(db) => db.begin_read().map_err(db_error),
        }
    }
    fn writable(&self) -> Result<&Database> {
        match &self.access {
            Access::Write(db) => Ok(db),
            Access::ReadOnly(_) => Err(Error::new(ErrorKind::Io, "cache opened read-only")),
        }
    }
    fn validate(&self) -> Result<()> {
        let transaction = self.read()?;
        let table = transaction.open_table(META).map_err(db_error)?;
        if table
            .get("format")
            .map_err(db_error)?
            .map(|value| value.value())
            != Some(VERSION)
        {
            return Err(Error::new(
                ErrorKind::Io,
                "unsupported cache format; clear the cache explicitly",
            ));
        }
        transaction.open_table(ENTRIES).map_err(db_error)?;
        Ok(())
    }
    pub fn get_many(&self, keys: &[Key]) -> Result<Vec<Option<Entry>>> {
        let transaction = self.read()?;
        let table = transaction.open_table(ENTRIES).map_err(db_error)?;
        keys.iter()
            .map(|key| {
                table
                    .get(key.0.as_slice())
                    .map_err(db_error)?
                    .map(|value| Entry::decode(value.value()))
                    .transpose()
            })
            .collect()
    }
    pub fn put_many(&self, entries: &[(Key, Entry)]) -> Result<()> {
        let transaction = self.writable()?.begin_write().map_err(db_error)?;
        {
            let mut table = transaction.open_table(ENTRIES).map_err(db_error)?;
            for (key, entry) in entries {
                if entry.version != VERSION {
                    return Err(Error::new(
                        ErrorKind::Io,
                        "cannot write an unsupported cache-entry version",
                    ));
                }
                let bytes = serde_json::to_vec(entry).map_err(json_error)?;
                table
                    .insert(key.0.as_slice(), bytes.as_slice())
                    .map_err(db_error)?;
            }
        }
        transaction.commit().map_err(db_error)
    }
    pub fn stats(&self) -> Result<Stats> {
        let transaction = self.read()?;
        let table = transaction.open_table(ENTRIES).map_err(db_error)?;
        let mut stats = Stats {
            present: true,
            entries: table.len().map_err(db_error)?,
            database_bytes: std::fs::metadata(&self.path)
                .map_err(|error| Error::io("reading cache metadata", error))?
                .len(),
            ..Stats::default()
        };
        for row in table.iter().map_err(db_error)? {
            let (_, bytes) = row.map_err(db_error)?;
            let entry = Entry::decode(bytes.value())?;
            stats.payload_bytes = stats
                .payload_bytes
                .checked_add(bytes.value().len() as u64)
                .ok_or_else(|| Error::new(ErrorKind::Io, "cache size overflow"))?;
            stats.oldest = Some(
                stats
                    .oldest
                    .map_or(entry.created_at, |oldest| oldest.min(entry.created_at)),
            );
            stats.newest = Some(
                stats
                    .newest
                    .map_or(entry.created_at, |newest| newest.max(entry.created_at)),
            );
            let count = stats
                .by_model
                .entry(entry.model.as_str().to_owned())
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorKind::Io, "cache entry count overflow"))?;
        }
        Ok(stats)
    }
    pub fn prune(&self, before: u64) -> Result<u64> {
        let transaction = self.writable()?.begin_write().map_err(db_error)?;
        let mut removed = 0u64;
        {
            let mut table = transaction.open_table(ENTRIES).map_err(db_error)?;
            let mut failure = None;
            table
                .retain(|_, bytes| match Entry::decode(bytes) {
                    Ok(entry) if entry.created_at < before => {
                        match removed.checked_add(1) {
                            Some(count) => removed = count,
                            None => {
                                failure =
                                    Some(Error::new(ErrorKind::Io, "cache prune count overflow"))
                            }
                        }
                        false
                    }
                    Ok(_) => true,
                    Err(error) => {
                        failure = Some(error);
                        true
                    }
                })
                .map_err(db_error)?;
            if let Some(error) = failure {
                return Err(error);
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(removed)
    }
    pub fn clear(&self) -> Result<u64> {
        let transaction = self.writable()?.begin_write().map_err(db_error)?;
        let count = transaction
            .open_table(ENTRIES)
            .map_err(db_error)?
            .len()
            .map_err(db_error)?;
        transaction.delete_table(ENTRIES).map_err(db_error)?;
        transaction.open_table(ENTRIES).map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(count)
    }
    pub fn clear_directory(directory: &Path) -> Result<()> {
        let path = directory.join(FILE_NAME);
        if !path
            .try_exists()
            .map_err(|error| Error::io("checking cache", error))?
        {
            return Ok(());
        }
        let (file, _) = open_file(&path)?;
        let database = Database::builder().create_file(file).map_err(db_error)?;
        let transaction = database.begin_write().map_err(db_error)?;
        transaction.delete_table(ENTRIES).map_err(db_error)?;
        transaction.delete_table(META).map_err(db_error)?;
        transaction
            .open_table(META)
            .map_err(db_error)?
            .insert("format", VERSION)
            .map_err(db_error)?;
        transaction.open_table(ENTRIES).map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }
}
fn initialize(database: &Database) -> Result<()> {
    let transaction = database.begin_write().map_err(db_error)?;
    transaction
        .open_table(META)
        .map_err(db_error)?
        .insert("format", VERSION)
        .map_err(db_error)?;
    transaction.open_table(ENTRIES).map_err(db_error)?;
    transaction.commit().map_err(db_error)
}
fn open_file(path: &Path) -> Result<(File, bool)> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => Ok((file, true)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(path)
                .map_err(|error| Error::io("reading cache path", error))?
                .file_type()
                .is_symlink()
            {
                return Err(Error::new(
                    ErrorKind::Io,
                    "cache path must not be a symlink",
                ));
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|error| Error::io("opening cache file", error))?;
            let empty = file
                .metadata()
                .map_err(|error| Error::io("reading cache file", error))?
                .len()
                == 0;
            Ok((file, empty))
        }
        Err(error) => Err(Error::io("creating cache file", error)),
    }
}
pub fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| Error::new(ErrorKind::Io, "cache clock precedes Unix epoch"))
}
fn db_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, format!("cache database: {error}"))
}
fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::Io, format!("cache JSON: {error}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::cache::{ENTRIES, Entry, FILE_NAME, Key, META, Store, VERSION, key};
    use crate::encoder::ClefEncoder;
    use crate::identity::{Discovery, Model};
    use crate::wire::{Request, Response};
    use redb::Database;
    use serde_json::json;

    fn encoder() -> ClefEncoder {
        let env =
            std::collections::HashMap::from([("HOME".to_owned(), std::env::var("HOME").unwrap())]);
        let profile = crate::config::Config::default()
            .resolve(
                None,
                &crate::config::Settings {
                    tokenizer: std::env::var("LOUPE_TEST_TOKENIZER").ok(),
                    ..crate::config::Settings::default()
                },
                &env,
            )
            .unwrap();
        crate::encoder::load_profile(&profile).unwrap().unwrap()
    }
    fn identity(encoder: &ClefEncoder) -> Discovery {
        Discovery {
            model: Model::parse(&format!(
                "clef-flash@{};compute=bf16;quantization=nf4;server={}",
                "a".repeat(40),
                "b".repeat(64)
            ))
            .unwrap(),
            encoder_identity: encoder.identity.clone(),
        }
    }
    fn request() -> Request {
        serde_json::from_value(json!({"model":"clef-flash","state":{"message":"Checkout down","reference":"healthy"},"questions":{
            "L1":{"type":"noul","instructions":"Is checkout down?"},
            "L2":{"type":"choice","criteria":{"a":"down","b":"healthy"}}
        }})).unwrap()
    }
    fn entry() -> Entry {
        // Mathematical complete identity with a captured real distribution; no HTTP backend.
        let captured: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let mut response: Response = serde_json::from_value(captured["response"].clone()).unwrap();
        let identity = Discovery {
            model: Model::parse(&format!(
                "clef-flash@{};compute=bf16;quantization=nf4;server={}",
                "a".repeat(40),
                "b".repeat(64)
            ))
            .unwrap(),
            encoder_identity: "fixture-encoder".to_owned(),
        };
        response.model = identity.model.as_str().to_owned();
        Entry::new(&identity, response).unwrap()
    }
    #[test]
    fn full_request_identity_preserves_prompt_head_context_order_and_model_components() {
        let encoder = encoder();
        let identity = identity(&encoder);
        let original = request();
        let expected = key(&identity, Some(&encoder), &original).unwrap();
        let mut reordered = original.clone();
        reordered.questions.reverse();
        let mut renamed = original.clone();
        let question = renamed.questions.shift_remove("L1").unwrap();
        renamed.questions.insert("L10".to_owned(), question);
        let mut context = original.clone();
        context.state["reference"] = json!("degraded");
        let mut companion = original.clone();
        companion.questions.shift_remove("L2");
        for changed in [reordered, renamed, context, companion] {
            assert_ne!(expected, key(&identity, Some(&encoder), &changed).unwrap());
        }
        for (from, to) in [
            ("bf16", "fp16"),
            ("nf4", "none"),
            (&"a".repeat(40), &"c".repeat(40)),
            (&"b".repeat(64), &"d".repeat(64)),
        ] {
            let mut changed = identity.clone();
            changed.model = Model::parse(&identity.model.as_str().replace(from, to)).unwrap();
            assert_ne!(expected, key(&changed, Some(&encoder), &original).unwrap());
        }
        let mut mismatch = identity.clone();
        mismatch.encoder_identity.push_str("-different");
        assert!(key(&mismatch, Some(&encoder), &original).is_err());
        assert_ne!(
            key(&identity, None, &original).unwrap(),
            key(&mismatch, None, &original).unwrap()
        );
        let mut sorted = original.clone();
        sorted.state =
            serde_json::from_str(r#"{"reference":"healthy","message":"Checkout down"}"#).unwrap();
        assert_eq!(expected, key(&identity, Some(&encoder), &sorted).unwrap());
        assert_ne!(
            key(&identity, None, &original).unwrap(),
            key(&identity, None, &sorted).unwrap()
        );
    }

    #[test]
    fn declared_signals_change_exact_prompt_head_and_cache_identity_with_identical_state() {
        let encoder = encoder();
        let identity = identity(&encoder);
        let original = request();
        let (before, counts_before) = encoder.encode(&original).unwrap();
        let baseline = key(&identity, Some(&encoder), &original).unwrap();
        let mut extended = original.clone();
        crate::signals::Set::from_document(&json!({"signals":[{"id":"impact","instructions":"Are customers affected?"},{"id":"retried"}]})).unwrap().append(&mut extended.questions).unwrap();
        let (after, counts_after) = encoder.encode(&extended).unwrap();
        assert_eq!(extended.state, original.state);
        assert_ne!(before.input_ids, after.input_ids);
        assert_eq!(after.questions.len(), before.questions.len() + 2);
        assert_eq!(
            after.questions.last().unwrap().question_id,
            "signal:retried"
        );
        assert_eq!(
            after.questions.last().unwrap().option_ids,
            vec!["true", "false"]
        );
        assert!(counts_after.input_tokens > counts_before.input_tokens);
        assert_ne!(baseline, key(&identity, Some(&encoder), &extended).unwrap());
        let ordered = key(&identity, Some(&encoder), &extended).unwrap();
        extended.questions.reverse();
        assert_ne!(ordered, key(&identity, Some(&encoder), &extended).unwrap());
    }
    #[test]
    fn typed_request_keys_disambiguate_equal_state_tokens_and_preserve_score_legends() {
        let encoder = encoder();
        let identity = identity(&encoder);
        let numeric: Request = serde_json::from_value(json!({"model":"clef-flash","state":"x","questions":{"score":{"type":"score","criteria":[0,1]}}})).unwrap();
        let text: Request = serde_json::from_value(json!({"model":"clef-flash","state":"x","questions":{"score":{"type":"score","criteria":["0","1"]}}})).unwrap();
        assert_ne!(
            encoder.encode(&numeric).unwrap().0.input_ids,
            encoder.encode(&text).unwrap().0.input_ids
        );
        assert_ne!(
            key(&identity, Some(&encoder), &numeric).unwrap(),
            key(&identity, Some(&encoder), &text).unwrap()
        );
        let mut numeric_state = numeric.clone();
        numeric_state.state = json!(1);
        let mut string_state = numeric_state.clone();
        string_state.state = json!("1");
        assert_eq!(
            encoder.encode(&numeric_state).unwrap().0.input_ids,
            encoder.encode(&string_state).unwrap().0.input_ids
        );
        assert_ne!(
            key(&identity, Some(&encoder), &numeric_state).unwrap(),
            key(&identity, Some(&encoder), &string_state).unwrap()
        );
    }
    #[test]
    fn raw_response_transactions_reopen_read_only_and_rollback_invalid_batches() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path(), false).unwrap().unwrap();
        let expected = entry();
        store.put_many(&[(Key([1; 32]), expected.clone())]).unwrap();
        let mut invalid = expected.clone();
        invalid.version = VERSION + 1;
        assert!(
            store
                .put_many(&[(Key([2; 32]), expected.clone()), (Key([3; 32]), invalid)])
                .is_err()
        );
        assert_eq!(store.stats().unwrap().entries, 1);
        drop(store);
        let reader = Store::open(dir.path(), true).unwrap().unwrap();
        let other_reader = Store::open(dir.path(), true).unwrap().unwrap();
        let hits = reader.get_many(&[Key([2; 32]), Key([1; 32])]).unwrap();
        assert!(hits[0].is_none());
        assert_eq!(
            serde_json::to_value(&hits[1].as_ref().unwrap().response).unwrap(),
            serde_json::to_value(&expected.response).unwrap()
        );
        assert!(reader.put_many(&[(Key([4; 32]), expected)]).is_err());
        assert!(reader.clear().is_err());
        assert!(reader.prune(u64::MAX).is_err());
        assert!(Store::open(dir.path(), false).is_err());
        assert_eq!(other_reader.stats().unwrap().entries, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join(FILE_NAME))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn writer_lock_prune_and_explicit_clear_preserve_other_assets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), b"keep this asset").unwrap();
        let store = Store::open(dir.path(), false).unwrap().unwrap();
        assert!(Store::open(dir.path(), false).is_err());
        assert!(Store::open(dir.path(), true).is_err());
        assert!(Store::clear_directory(dir.path()).is_err());
        let mut old = entry();
        old.created_at = 1;
        let mut boundary = entry();
        boundary.created_at = 20;
        store
            .put_many(&[(Key([1; 32]), old), (Key([2; 32]), boundary)])
            .unwrap();
        assert_eq!(store.prune(20).unwrap(), 1);
        let stats = store.stats().unwrap();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.oldest, Some(20));
        assert!(stats.payload_bytes > 0);
        drop(store);
        Store::clear_directory(dir.path()).unwrap();
        assert_eq!(
            Store::open(dir.path(), true)
                .unwrap()
                .unwrap()
                .stats()
                .unwrap()
                .entries,
            0
        );
        assert_eq!(
            std::fs::read(dir.path().join("tokenizer.json")).unwrap(),
            b"keep this asset"
        );
    }
    #[test]
    fn incompatible_header_or_entry_requires_explicit_clear_and_prune_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        drop(Store::open(dir.path(), false).unwrap());
        let database = Database::open(dir.path().join(FILE_NAME)).unwrap();
        let transaction = database.begin_write().unwrap();
        transaction
            .open_table(META)
            .unwrap()
            .insert("format", VERSION + 1)
            .unwrap();
        transaction.commit().unwrap();
        drop(database);
        assert!(Store::open(dir.path(), true).is_err());
        assert!(Store::open(dir.path(), false).is_err());
        Store::clear_directory(dir.path()).unwrap();
        let database = Database::open(dir.path().join(FILE_NAME)).unwrap();
        let transaction = database.begin_write().unwrap();
        {
            let mut table = transaction.open_table(ENTRIES).unwrap();
            let mut old = entry();
            old.created_at = 1;
            let bytes = serde_json::to_vec(&old).unwrap();
            table
                .insert(Key([1; 32]).0.as_slice(), bytes.as_slice())
                .unwrap();
            table
                .insert(Key([2; 32]).0.as_slice(), b"invalid JSON".as_slice())
                .unwrap();
        }
        transaction.commit().unwrap();
        drop(database);
        let store = Store::open(dir.path(), false).unwrap().unwrap();
        assert!(store.prune(u64::MAX).is_err());
        assert!(store.stats().is_err());
        let read = store.read().unwrap();
        assert!(
            read.open_table(ENTRIES)
                .unwrap()
                .get(Key([1; 32]).0.as_slice())
                .unwrap()
                .is_some()
        );
        drop(read);
        drop(store);
        Store::clear_directory(dir.path()).unwrap();
        assert_eq!(
            Store::open(dir.path(), true)
                .unwrap()
                .unwrap()
                .stats()
                .unwrap()
                .entries,
            0
        );
    }
    #[test]
    fn read_only_missing_cache_and_clear_do_not_create_directories() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent");
        assert!(Store::open(&missing, true).unwrap().is_none());
        Store::clear_directory(&missing).unwrap();
        assert!(!missing.exists());
    }
    #[test]
    fn cache_crash_child() {
        let Some(path) = std::env::var_os("LOUPE_CACHE_CRASH_DIRECTORY") else {
            return;
        };
        let database = Database::open(std::path::Path::new(&path).join(FILE_NAME)).unwrap();
        let transaction = database.begin_write().unwrap();
        let bytes = serde_json::to_vec(&entry()).unwrap();
        transaction
            .open_table(ENTRIES)
            .unwrap()
            .insert(Key([2; 32]).0.as_slice(), bytes.as_slice())
            .unwrap();
        if std::env::var_os("LOUPE_CACHE_CRASH_COMMIT").is_some() {
            transaction.commit().unwrap();
        }
        use std::io::{Read, Write};
        println!("CACHE_CRASH_READY");
        std::io::stdout().flush().unwrap();
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
    }
    #[test]
    fn killed_writer_recovers_committed_data_and_discards_uncommitted_transaction() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        for commit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path(), false).unwrap().unwrap();
            store.put_many(&[(Key([1; 32]), entry())]).unwrap();
            drop(store);
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "cache::tests::cache_crash_child", "--nocapture"])
                .env("LOUPE_CACHE_CRASH_DIRECTORY", dir.path())
                .env_remove("LOUPE_CACHE_CRASH_COMMIT")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if commit {
                command.env("LOUPE_CACHE_CRASH_COMMIT", "1");
            }
            let mut child = command.spawn().unwrap();
            let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
            let mut ready = false;
            for _ in 0..20 {
                let mut line = String::new();
                if output.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if line.trim() == "CACHE_CRASH_READY" {
                    ready = true;
                    break;
                }
            }
            assert!(
                ready,
                "child did not reach the requested transaction boundary"
            );
            assert!(Store::open(dir.path(), true).is_err());
            assert!(Store::open(dir.path(), false).is_err());
            child.kill().unwrap();
            child.wait().unwrap();
            // Read-only consumers do not repair an unclean database; a writer can recover it.
            let store = Store::open(dir.path(), false).unwrap().unwrap();
            let recovered = store.get_many(&[Key([1; 32]), Key([2; 32])]).unwrap();
            assert!(recovered[0].is_some());
            assert_eq!(recovered[1].is_some(), commit);
        }
    }
}
