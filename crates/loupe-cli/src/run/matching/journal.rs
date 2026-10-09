use crate::args::Join;
use crate::run::json_error;
use indexmap::IndexMap;
use loupe_core::backend::Evaluation;
use loupe_core::error::{Error, Result};
use loupe_core::input::Unit;
use loupe_core::recovery::{Disposition, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pair {
    pub(crate) right_index: u64,
    pub(crate) evaluation: Evaluation,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub(crate) fallbacks: IndexMap<String, loupe_core::fallback::Execution>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub(crate) pairs: Vec<Pair>,
    pub(crate) complete: bool,
    pub(crate) selected_rights: Vec<u64>,
    pub(crate) error: Option<(u8, String)>,
}
pub(crate) struct Journal {
    pub(crate) store: Store,
    pub(crate) original_last: u64,
    pub(crate) original_end: Option<Value>,
    pub(crate) finished: Option<(u64, u8)>,
}
impl Journal {
    pub(crate) fn open(options: &Join, identity: Value) -> Result<Option<Self>> {
        let Some(path) = options.failed.as_deref().or(options.resume.as_deref()) else {
            return Ok(None);
        };
        let mut protected = Vec::new();
        for input in [
            options.left.as_str(),
            options.right.as_str(),
            "/dev/fd/0",
            "/dev/fd/1",
        ] {
            if input == "-" {
                continue;
            }
            match std::fs::metadata(input) {
                Ok(metadata) => protected.push((metadata.dev(), metadata.ino())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::io(
                        "checking join journal input/output aliases",
                        error,
                    ));
                }
            }
        }
        let store = if options.resume.is_some() {
            Store::resume(path, &identity, &protected)?
        } else {
            Store::create(path, identity, &protected)?
        };
        Ok(Some(Self {
            original_last: store.last_index(),
            original_end: store.input_end().cloned(),
            finished: store.finished(),
            store,
        }))
    }
    pub(crate) fn snapshot(&mut self, index: u64) -> Result<(Snapshot, Option<Disposition>)> {
        if index > self.original_last {
            return Ok((Snapshot::default(), None));
        }
        let saved = self.store.saved(index)?;
        let value = saved
            .completed
            .or_else(|| saved.plan.and_then(|plan| plan.get("progress").cloned()));
        let snapshot = value
            .map(|value| serde_json::from_value(value).map_err(json_error))
            .transpose()?
            .unwrap_or_default();
        Ok((snapshot, saved.handled))
    }
    pub(crate) fn progress(&mut self, index: u64, plans: Value, snapshot: &Snapshot) -> Result<()> {
        self.store.plan(
            vec![index],
            serde_json::json!({"operation":"join-pairs","plans":plans,"progress":snapshot}),
        )
    }
    pub(crate) fn ready(&mut self, index: u64, snapshot: &Snapshot) -> Result<()> {
        self.store
            .ready(index, serde_json::to_value(snapshot).map_err(json_error)?)
    }
    pub(crate) fn failed(&mut self, index: Option<u64>, error: &Error) -> Result<()> {
        self.store
            .failed(index, error.exit_code(), error.to_string())
    }
    pub(crate) fn verify(&mut self, unit: &Unit) -> Result<()> {
        self.store.verify(unit)
    }
}
