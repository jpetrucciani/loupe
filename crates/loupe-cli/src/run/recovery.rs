use std::io::{self, BufRead, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use loupe_core::backend::Evaluation;
use loupe_core::encoder::Counts;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::recovery::{Completion, Disposition, Store};
use loupe_core::wire::Request;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::run::failure::{WorkError, WorkResult};
use crate::run::{Completed, Execution, Job};

#[derive(Clone)]
pub(crate) struct Registry(Arc<State>);
struct State {
    store: Mutex<Store>,
    stopped: AtomicBool,
    original_last: u64,
    original_end: Option<Value>,
    finished: Option<(u64, u8)>,
    emitted: u64,
    resume: bool,
    validated: AtomicBool,
}
pub(crate) struct Session(pub(crate) Registry);
impl Drop for Session {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    request: Request,
    question_order: Vec<String>,
    answer_order: Vec<String>,
    counts: Counts,
    evaluation: Evaluation,
    answer_id: String,
    packed_indices: Option<Vec<u64>>,
    split_plan: Option<Value>,
    view: Option<Value>,
    focus_plan: Option<Value>,
}

pub(crate) enum Piece {
    Fresh(Vec<Result<Unit>>),
    Saved(Box<Completed>),
}

impl Registry {
    pub(crate) fn open(
        path: &Path,
        resume: bool,
        identity: Value,
        files: &[String],
        uncertain: Option<&Path>,
    ) -> Result<Session> {
        let mut protected = Vec::new();
        for path in files
            .iter()
            .filter(|file| file.as_str() != "-")
            .map(Path::new)
            .chain([Path::new("/dev/fd/0"), Path::new("/dev/fd/1")])
            .chain(uncertain)
        {
            match std::fs::metadata(path) {
                Ok(metadata) => protected.push((metadata.dev(), metadata.ino())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::io("checking recovery input/output aliases", error));
                }
            }
        }
        let store = if resume {
            Store::resume(path, &identity, &protected)?
        } else {
            Store::create(path, identity, &protected)?
        };
        let original_last = store.last_index();
        let original_end = store.input_end().cloned();
        let finished = store.finished();
        let mut emitted = 0u64;
        let mut cursor = 0;
        while let Some(index) = store.next_index(cursor)? {
            if store.disposition(index)? == Some(Disposition::Emitted) {
                emitted = emitted
                    .checked_add(1)
                    .ok_or_else(|| invalid("recovery emitted count overflow"))?;
            }
            cursor = index;
        }
        Ok(Session(Self(Arc::new(State {
            store: Mutex::new(store),
            stopped: AtomicBool::new(false),
            original_last,
            original_end,
            finished,
            emitted,
            resume,
            validated: AtomicBool::new(!resume),
        }))))
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Store>> {
        self.0
            .store
            .lock()
            .map_err(|_| invalid("recovery registry lock was poisoned"))
    }
    pub(crate) fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
    }
    pub(crate) fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    pub(crate) fn original_emitted(&self) -> u64 {
        self.0.emitted
    }
    pub(crate) fn finished(&self) -> Option<(u64, u8)> {
        self.0.finished
    }
    pub(crate) fn reject_output_alias(&self, path: &Path) -> Result<()> {
        if let Ok(metadata) = std::fs::metadata(path)
            && self.lock()?.file_identity()? == (metadata.dev(), metadata.ino())
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "output aliases the recovery journal",
            ));
        }
        Ok(())
    }
    pub(crate) fn plan(&self, indices: Vec<u64>, provenance: Value) -> Result<()> {
        self.lock()?.plan(indices, provenance)
    }
    pub(crate) fn saved(&self, index: u64) -> Result<loupe_core::recovery::Saved> {
        self.lock()?.saved(index)
    }
    pub(crate) fn ready(&self, index: u64, completed: Value) -> Result<()> {
        self.lock()?.ready(index, completed)
    }
    pub(crate) fn fallback(&self, index: u64, execution: Value) -> Result<()> {
        self.lock()?.fallback(index, execution)
    }
    pub(crate) fn review_seed(&self) -> Result<String> {
        self.lock()?.review_seed()
    }
    pub(crate) fn review(&self, index: u64, decision: Value) -> Result<()> {
        self.lock()?.review(index, decision)
    }
    pub(crate) fn failure(&self, index: Option<u64>, error: &Error) -> Result<()> {
        if self.0.finished.is_some() || !self.0.validated.load(Ordering::Acquire) {
            return Ok(());
        }
        self.lock()?
            .failed(index, error.exit_code(), error.to_string())
    }
    pub(crate) fn capture(&self, results: &[WorkResult<Completed>]) -> Result<()> {
        let mut completions = Vec::new();
        let mut failures = Vec::new();
        for result in results {
            match result {
                Ok(completed) => {
                    let evaluation = completed
                        .evaluation
                        .as_deref()
                        .ok_or_else(|| invalid("recovery cannot capture a planning result"))?;
                    let mut store = self.lock()?;
                    if store.saved(completed.unit.index)?.completed.is_some() {
                        continue;
                    }
                    let snapshot = Snapshot {
                        request: completed.request.as_ref().clone(),
                        question_order: completed.request.questions.keys().cloned().collect(),
                        answer_order: evaluation.response.answers.keys().cloned().collect(),
                        counts: completed.counts.as_ref().clone(),
                        evaluation: evaluation.clone(),
                        answer_id: completed.answer_id.clone(),
                        packed_indices: completed.packed_indices.clone(),
                        split_plan: completed.split_plan.clone(),
                        view: completed.view.clone(),
                        focus_plan: completed.focus_plan.clone(),
                    };
                    completions.push(Completion {
                        index: completed.unit.index,
                        completed: serde_json::to_value(snapshot).map_err(json_error)?,
                    });
                }
                Err(failure) => {
                    failures.push((failure.unit.as_ref().map(|unit| unit.index), &failure.error));
                }
            }
        }
        if completions.len() == 1 {
            if let Some(completion) = completions.pop() {
                self.lock()?.ready(completion.index, completion.completed)?;
            }
        } else if !completions.is_empty() {
            self.lock()?.ready_many(completions)?;
        }
        for (index, error) in failures {
            self.failure(index, error)?;
        }
        Ok(())
    }
    pub(crate) fn handled(&self, index: u64, emitted: bool) -> Result<()> {
        self.lock()?.handled(
            index,
            if emitted {
                Disposition::Emitted
            } else {
                Disposition::Omitted
            },
        )
    }
    pub(crate) fn finish(&self, emitted: u64, code: u8) -> Result<()> {
        self.lock()?.finish(emitted, code)
    }
    pub(crate) fn pieces(
        &self,
        records: Vec<Result<Unit>>,
        execution: &Execution,
    ) -> Result<Vec<Piece>> {
        let mut pieces = Vec::new();
        let mut fresh = Vec::new();
        for record in records {
            let unit = match record {
                Ok(unit) => unit,
                Err(error) => {
                    fresh.push(Err(error));
                    continue;
                }
            };
            let saved = self.lock()?.saved(unit.index)?;
            if saved.handled.is_some() || self.0.finished.is_some() {
                if !fresh.is_empty() {
                    pieces.push(Piece::Fresh(std::mem::take(&mut fresh)));
                }
                continue;
            }
            if let Some(value) = saved.completed {
                if !fresh.is_empty() {
                    pieces.push(Piece::Fresh(std::mem::take(&mut fresh)));
                }
                let mut snapshot: Snapshot = serde_json::from_value(value).map_err(json_error)?;
                snapshot.request.questions = reorder(
                    std::mem::take(&mut snapshot.request.questions),
                    snapshot.question_order,
                )?;
                snapshot.evaluation.response.answers = reorder(
                    std::mem::take(&mut snapshot.evaluation.response.answers),
                    snapshot.answer_order,
                )?;
                snapshot.evaluation.response.validate(&snapshot.request)?;
                if !snapshot.request.questions.contains_key(&snapshot.answer_id) {
                    return Err(invalid("saved recovery answer binding is missing"));
                }
                let base_id = execution
                    .questions
                    .first()
                    .map(|(id, _)| id.as_str())
                    .ok_or_else(|| invalid("recovery has no questions"))?;
                let expected = if let Some(indices) = &snapshot.packed_indices {
                    if !indices.contains(&unit.index)
                        || indices.windows(2).any(|pair| pair[0] >= pair[1])
                    {
                        return Err(invalid("saved recovery packing indices are invalid"));
                    }
                    let source = execution.source.id(unit.index);
                    if execution.questions.len() == 1 {
                        source
                    } else {
                        format!("{source}/{base_id}")
                    }
                } else {
                    base_id.to_owned()
                };
                if snapshot.answer_id != expected {
                    return Err(invalid(
                        "saved recovery answer belongs to a different source unit",
                    ));
                }
                let counts =
                    loupe_core::pack::count(execution.encoder.as_deref(), &snapshot.request)?;
                if serde_json::to_value(&counts).map_err(json_error)?
                    != serde_json::to_value(&snapshot.counts).map_err(json_error)?
                {
                    return Err(invalid("saved recovery request counts changed"));
                }
                pieces.push(Piece::Saved(Box::new(Completed {
                    unit,
                    request: Arc::new(snapshot.request),
                    counts: Arc::new(snapshot.counts),
                    evaluation: Some(Arc::new(snapshot.evaluation)),
                    answer_id: snapshot.answer_id,
                    packed_indices: snapshot.packed_indices,
                    split_plan: snapshot.split_plan,
                    view: snapshot.view,
                    focus_plan: snapshot.focus_plan,
                    fallback: saved
                        .fallback
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(json_error)?
                        .map(Arc::new),
                    ensemble: None,
                    review: saved.review,
                })));
            } else {
                fresh.push(Ok(unit));
            }
        }
        if !fresh.is_empty() {
            pieces.push(Piece::Fresh(fresh));
        }
        Ok(pieces)
    }
    pub(crate) fn reader(&self) -> Result<ReplayReader> {
        Ok(ReplayReader {
            registry: self.clone(),
            expected: if self.0.resume {
                self.lock()?.next_index(0)?
            } else {
                None
            },
            released: !self.0.resume,
        })
    }
}

pub(crate) struct ReplayReader {
    registry: Registry,
    expected: Option<u64>,
    released: bool,
}
impl ReplayReader {
    fn release(&mut self, sender: &tokio::sync::mpsc::Sender<Result<Unit>>) -> Result<bool> {
        if self.released {
            return Ok(true);
        }
        self.released = true;
        self.registry.0.validated.store(true, Ordering::Release);
        let mut cursor = 0;
        loop {
            if self.registry.stopped() {
                return Ok(false);
            }
            let next = self.registry.lock()?.next_index(cursor)?;
            let Some(index) = next.filter(|index| *index <= self.registry.0.original_last) else {
                break;
            };
            let unit = self.registry.lock()?.saved(index)?.unit;
            if sender.blocking_send(Ok(unit)).is_err() {
                return Ok(false);
            }
            cursor = index;
        }
        Ok(true)
    }
    pub(crate) fn unit(
        &mut self,
        unit: Unit,
        sender: &tokio::sync::mpsc::Sender<Result<Unit>>,
    ) -> Result<bool> {
        if self.registry.stopped() {
            return Ok(false);
        }
        if let Some(expected) = self.expected {
            if unit.index != expected {
                return Err(Error::new(
                    ErrorKind::Input,
                    format!(
                        "recovery recorded input prefix changed: expected unit {expected}, got {}",
                        unit.index
                    ),
                ));
            }
            self.registry.lock()?.verify(&unit)?;
            self.expected = self.registry.lock()?.next_index(expected)?;
            if self.expected.is_none() && self.registry.0.original_end.is_none() {
                return self.release(sender);
            }
            return Ok(true);
        }
        if self.registry.0.original_end.is_some() {
            return Err(Error::new(
                ErrorKind::Input,
                "recovery input extends the recorded EOF",
            ));
        }
        if !self.release(sender)? {
            return Ok(false);
        }
        {
            let mut store = self.registry.lock()?;
            if self.registry.stopped() {
                return Ok(false);
            }
            store.admit(&unit)?;
        }
        Ok(sender.blocking_send(Ok(unit)).is_ok())
    }
    pub(crate) fn end(
        &mut self,
        fingerprint: Value,
        sender: &tokio::sync::mpsc::Sender<Result<Unit>>,
    ) -> Result<bool> {
        if self.registry.stopped() {
            return Ok(false);
        }
        if self.expected.is_some() {
            return Err(Error::new(
                ErrorKind::Input,
                "recovery input ended before the recorded prefix",
            ));
        }
        if let Some(original) = &self.registry.0.original_end
            && original != &fingerprint
        {
            return Err(Error::new(
                ErrorKind::Input,
                "recovery source fingerprint changed",
            ));
        }
        // Persist EOF before releasing an already complete original source to output.
        self.registry.lock()?.end_input(fingerprint)?;
        self.release(sender)
    }
}

#[derive(Clone, Default)]
pub(crate) struct Fingerprint(Arc<Mutex<HashState>>);
#[derive(Default)]
struct HashState {
    hasher: blake3::Hasher,
    invalid: bool,
}
impl Fingerprint {
    pub(crate) fn value(&self, source: &str, consumed: u64) -> Result<Value> {
        let state = self
            .0
            .lock()
            .map_err(|_| invalid("input hash lock was poisoned"))?;
        if state.invalid {
            return Err(invalid("input reader consumed an invalid buffer range"));
        }
        Ok(
            json!({"source":source,"consumed_units":consumed,"blake3":state.hasher.finalize().to_hex().to_string()}),
        )
    }
}
pub(crate) struct HashReader {
    inner: Box<dyn BufRead>,
    fingerprint: Fingerprint,
}
impl HashReader {
    pub(crate) fn new(inner: Box<dyn BufRead>, fingerprint: Fingerprint) -> Self {
        Self { inner, fingerprint }
    }
}
impl Read for HashReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let bytes = self.fill_buf()?;
        let length = output.len().min(bytes.len());
        output[..length].copy_from_slice(&bytes[..length]);
        self.consume(length);
        Ok(length)
    }
}
impl BufRead for HashReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, length: usize) {
        if let Ok(mut state) = self.fingerprint.0.lock() {
            match self.inner.fill_buf() {
                Ok(bytes) => match bytes.get(..length) {
                    Some(bytes) => {
                        state.hasher.update(bytes);
                    }
                    None => state.invalid = true,
                },
                Err(_) => state.invalid = true,
            }
        }
        self.inner.consume(length);
    }
}
pub(crate) async fn jobs(
    records: Vec<Result<Unit>>,
    execution: &Execution,
) -> Vec<WorkResult<Job>> {
    let pieces = match &execution.recovery {
        Some(registry) => match registry.pieces(records, execution) {
            Ok(pieces) => pieces,
            Err(error) => return vec![Err(WorkError::from(error))],
        },
        None => vec![Piece::Fresh(records)],
    };
    let mut output = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Saved(completed) => output.push(Ok(Job::Saved(completed))),
            Piece::Fresh(records) => {
                output.extend(if execution.focus.is_some() {
                    crate::run::focus_jobs::plan_focus_jobs(records, execution).await
                } else {
                    crate::run::plan_jobs(records, execution)
                });
            }
        }
    }
    output
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Io, message)
}
fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::Io, format!("recovery snapshot: {error}"))
}
fn reorder<T>(
    mut values: indexmap::IndexMap<String, T>,
    order: Vec<String>,
) -> Result<indexmap::IndexMap<String, T>> {
    let mut result = indexmap::IndexMap::new();
    for key in order {
        let value = values.shift_remove(&key).ok_or_else(|| {
            invalid("recovery question/answer order has duplicate or unknown keys")
        })?;
        result.insert(key, value);
    }
    if !values.is_empty() {
        return Err(invalid("recovery question/answer order omits keys"));
    }
    Ok(result)
}
