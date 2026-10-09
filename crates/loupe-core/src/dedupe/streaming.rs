//! Removable LSH history. Only emitted representatives enter this index.

use crate::blocking::{Band, bands};
use crate::encoder;
use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Horizon {
    Units(NonZeroUsize),
    Time {
        span: Duration,
        capacity: NonZeroUsize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedEntry {
    unit: Unit,
    admitted: Duration,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    horizon: Horizon,
    max_bytes: NonZeroUsize,
    current: u64,
    now: Duration,
    expired: u64,
    peak_count: usize,
    peak_bytes: usize,
    entries: Vec<SavedEntry>,
}
impl Snapshot {
    pub fn clock(&self) -> Duration {
        self.now
    }
}
struct Entry {
    unit: Unit,
    admitted: Duration,
    bytes: usize,
    bands: Vec<Band>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Stats {
    pub representatives: usize,
    pub serialized_bytes: usize,
    pub distinct_bands: usize,
    pub band_postings: usize,
    pub capacity: usize,
    pub byte_capacity: usize,
    pub expired: u64,
    pub peak_representatives: usize,
    pub peak_serialized_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct Candidates {
    pub indices: Vec<u64>,
    pub eligible: usize,
}
pub struct Index {
    horizon: Horizon,
    capacity: usize,
    max_bytes: NonZeroUsize,
    entries: BTreeMap<u64, Entry>,
    postings: HashMap<Band, BTreeSet<u64>>,
    bytes: usize,
    current: u64,
    now: Duration,
    expired: u64,
    peak_count: usize,
    peak_bytes: usize,
}
impl Index {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            horizon: self.horizon,
            max_bytes: self.max_bytes,
            current: self.current,
            now: self.now,
            expired: self.expired,
            peak_count: self.peak_count,
            peak_bytes: self.peak_bytes,
            entries: self
                .entries
                .values()
                .map(|entry| SavedEntry {
                    unit: entry.unit.clone(),
                    admitted: entry.admitted,
                })
                .collect(),
        }
    }
    pub fn restore<F>(&mut self, snapshot: Snapshot, select: F) -> Result<()>
    where
        F: for<'a> Fn(&'a Unit) -> &'a Value,
    {
        if snapshot.horizon != self.horizon || snapshot.max_bytes != self.max_bytes {
            return Err(input("streaming snapshot limits changed"));
        }
        let mut restored = Self::new(self.horizon, self.max_bytes)?;
        if snapshot.entries.len() > restored.capacity
            || snapshot.peak_count > restored.capacity
            || snapshot.peak_bytes > restored.max_bytes.get()
        {
            return Err(input("streaming snapshot exceeds configured bounds"));
        }
        for entry in snapshot.entries {
            if entry.unit.index > snapshot.current || entry.admitted > snapshot.now {
                return Err(input("streaming snapshot has future entries"));
            }
            restored.advance(entry.unit.index, entry.admitted)?;
            let comparison = select(&entry.unit).clone();
            restored.retain(entry.unit, &comparison)?;
        }
        if snapshot.current < restored.current
            || snapshot.now < restored.now
            || snapshot.peak_count < restored.entries.len()
            || snapshot.peak_bytes < restored.bytes
        {
            return Err(input("invalid streaming snapshot counters"));
        }
        for (index, entry) in &restored.entries {
            let stale = match restored.horizon {
                Horizon::Units(count) => {
                    snapshot.current.saturating_sub(*index) > count.get() as u64
                }
                Horizon::Time { span, .. } => snapshot.now.saturating_sub(entry.admitted) >= span,
            };
            if stale {
                return Err(input("streaming snapshot retained expired history"));
            }
        }
        restored.current = snapshot.current;
        restored.now = snapshot.now;
        restored.expired = snapshot.expired;
        restored.peak_count = snapshot.peak_count;
        restored.peak_bytes = snapshot.peak_bytes;
        *self = restored;
        Ok(())
    }
    pub fn new(horizon: Horizon, max_bytes: NonZeroUsize) -> Result<Self> {
        // N prior physical units and the current unit can coexist until the next lookup.
        let capacity = match horizon {
            Horizon::Units(count) => count.get().checked_add(1).ok_or_else(|| {
                Error::new(
                    ErrorKind::Usage,
                    "dedupe window plus current unit exceeds index range",
                )
            })?,
            Horizon::Time { capacity, .. } => capacity.get(),
        };
        if matches!(horizon, Horizon::Time { span, .. } if span.is_zero()) {
            return Err(Error::new(
                ErrorKind::Usage,
                "--window-time must be positive",
            ));
        }
        Ok(Self {
            horizon,
            capacity,
            max_bytes,
            entries: BTreeMap::new(),
            postings: HashMap::new(),
            bytes: 0,
            current: 0,
            now: Duration::ZERO,
            expired: 0,
            peak_count: 0,
            peak_bytes: 0,
        })
    }
    /// Expiry is evaluated at decision admission, before candidate lookup.
    pub fn advance(&mut self, index: u64, now: Duration) -> Result<Vec<u64>> {
        if index <= self.current || now < self.now {
            return Err(input(
                "streaming dedupe requires increasing indices and monotonic time",
            ));
        }
        self.current = index;
        self.now = now;
        let mut expired = Vec::new();
        while let Some((index, entry)) = self.entries.first_key_value() {
            let stale = match self.horizon {
                Horizon::Units(count) => self.current.saturating_sub(*index) > count.get() as u64,
                Horizon::Time { span, .. } => self.now.saturating_sub(entry.admitted) >= span,
            };
            if !stale {
                break;
            }
            let (index, entry) = self
                .entries
                .pop_first()
                .ok_or_else(|| input("expired dedupe entry missing"))?;
            for band in entry.bands {
                let empty = if let Some(posting) = self.postings.get_mut(&band) {
                    posting.remove(&index);
                    posting.is_empty()
                } else {
                    return Err(input("dedupe band posting missing"));
                };
                if empty {
                    self.postings.remove(&band);
                }
            }
            self.bytes = self
                .bytes
                .checked_sub(entry.bytes)
                .ok_or_else(|| input("dedupe retained byte underflow"))?;
            self.expired = self
                .expired
                .checked_add(1)
                .ok_or_else(|| input("dedupe expiry counter overflow"))?;
            expired.push(index);
        }
        Ok(expired)
    }
    pub fn candidates(&self, state: &Value, limit: NonZeroUsize) -> Result<Candidates> {
        let mut indices = BTreeSet::new();
        for band in fingerprint(state)? {
            if let Some(posting) = self.postings.get(&band) {
                indices.extend(posting);
            }
        }
        let eligible = indices.len();
        Ok(Candidates {
            indices: indices.into_iter().take(limit.get()).collect(),
            eligible,
        })
    }
    pub fn unit(&self, index: u64) -> Result<&Unit> {
        self.entries
            .get(&index)
            .map(|entry| &entry.unit)
            .ok_or_else(|| input("streaming dedupe candidate expired or missing"))
    }
    pub fn retain(&mut self, unit: Unit, comparison: &Value) -> Result<()> {
        if unit.index != self.current || self.entries.contains_key(&unit.index) {
            return Err(input(
                "streaming dedupe retention requires the current unretained unit",
            ));
        }
        if self.entries.len() >= self.capacity {
            return Err(input(
                "streaming dedupe index capacity exhausted within --window-time",
            ));
        }
        let bytes = serde_json::to_vec(&unit)
            .map_err(|error| input(&error.to_string()))?
            .len();
        let retained = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| input("dedupe retained byte overflow"))?;
        if retained > self.max_bytes.get() {
            return Err(input(
                "streaming dedupe --max-index-bytes exhausted; live history cannot be evicted",
            ));
        }
        let bands = fingerprint(comparison)?;
        for band in &bands {
            self.postings
                .entry(band.clone())
                .or_default()
                .insert(unit.index);
        }
        self.entries.insert(
            unit.index,
            Entry {
                unit,
                admitted: self.now,
                bytes,
                bands,
            },
        );
        self.bytes = retained;
        self.peak_count = self.peak_count.max(self.entries.len());
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn stats(&self) -> Stats {
        Stats {
            representatives: self.entries.len(),
            serialized_bytes: self.bytes,
            distinct_bands: self.postings.len(),
            band_postings: self.postings.values().map(BTreeSet::len).sum(),
            capacity: self.capacity,
            byte_capacity: self.max_bytes.get(),
            expired: self.expired,
            peak_representatives: self.peak_count,
            peak_serialized_bytes: self.peak_bytes,
        }
    }
}
fn fingerprint(state: &Value) -> Result<Vec<Band>> {
    let normalized = encoder::render(state)?
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    let tokens = normalized
        .split_whitespace()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    Ok(bands(&tokens))
}
fn input(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::dedupe::streaming::{Horizon, Index};
    use crate::input::Unit;
    use crate::records::{Format, Options, Source};
    use serde_json::json;
    use std::{io::Cursor, num::NonZeroUsize, time::Duration};
    fn n(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }
    fn unit(index: u64, text: &str) -> Unit {
        let mut unit = Source::new(
            Cursor::new(format!("{text}\n")),
            Options::new(Format::Lines),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap();
        unit.index = index;
        unit
    }
    #[test]
    fn unit_horizon_counts_dropped_and_blank_indices_and_removes_all_postings() {
        let mut index = Index::new(Horizon::Units(n(2)), n(10000)).unwrap();
        index.advance(1, Duration::ZERO).unwrap();
        let first = unit(1, "ALPHA BRAVO");
        index.retain(first.clone(), &first.state).unwrap();
        index.advance(3, Duration::from_secs(1)).unwrap();
        assert_eq!(
            index
                .candidates(&json!("bravo alpha"), n(20))
                .unwrap()
                .indices,
            vec![1]
        );
        assert_eq!(index.advance(4, Duration::from_secs(2)).unwrap(), vec![1]);
        assert!(
            index
                .candidates(&first.state, n(20))
                .unwrap()
                .indices
                .is_empty()
        );
        let stats = index.stats();
        assert_eq!(stats.representatives, 0);
        assert_eq!(stats.band_postings, 0);
        assert_eq!(stats.distinct_bands, 0);
        assert_eq!(stats.serialized_bytes, 0);
    }
    #[test]
    fn time_expiry_capacity_byte_limits_and_candidate_caps_are_explicit() {
        let mut index = Index::new(
            Horizon::Time {
                span: Duration::from_secs(5),
                capacity: n(2),
            },
            n(10000),
        )
        .unwrap();
        for id in 1..=2 {
            index.advance(id, Duration::from_secs(id)).unwrap();
            let unit = unit(id, "same words");
            index.retain(unit.clone(), &unit.state).unwrap();
        }
        let candidates = index.candidates(&json!("same words"), n(1)).unwrap();
        assert_eq!(candidates.indices, vec![1]);
        assert_eq!(candidates.eligible, 2);
        index.advance(3, Duration::from_secs(3)).unwrap();
        let third = unit(3, "different words");
        assert!(index.retain(third.clone(), &third.state).is_err());
        assert_eq!(index.stats().representatives, 2);
        assert_eq!(index.advance(4, Duration::from_secs(6)).unwrap(), vec![1]);
        let fourth = unit(4, "new words");
        index.retain(fourth.clone(), &fourth.state).unwrap();
        assert!(index.advance(4, Duration::from_secs(7)).is_err());
        assert!(index.advance(5, Duration::from_secs(5)).is_err());
        let mut tiny = Index::new(Horizon::Units(n(2)), n(1)).unwrap();
        tiny.advance(1, Duration::ZERO).unwrap();
        let oversized = unit(1, "oversized");
        assert!(tiny.retain(oversized.clone(), &oversized.state).is_err());
        assert_eq!(tiny.stats().band_postings, 0);
        assert!(
            Index::new(
                Horizon::Time {
                    span: Duration::ZERO,
                    capacity: n(2)
                },
                n(100)
            )
            .is_err()
        );
    }
    #[test]
    fn ten_thousand_distinct_arrivals_do_not_accumulate_entries_or_postings() {
        let mut index = Index::new(Horizon::Units(n(8)), n(20000)).unwrap();
        for id in 1..=10000 {
            index.advance(id, Duration::from_millis(id)).unwrap();
            let unit = unit(id, &format!("record number {id}"));
            index.retain(unit.clone(), &unit.state).unwrap();
            let stats = index.stats();
            assert!(stats.representatives <= 9);
            assert!(stats.band_postings <= 72);
            assert!(stats.distinct_bands <= 72);
            assert!(stats.serialized_bytes <= 20000);
        }
        assert_eq!(index.stats().expired, 9991);
        assert_eq!(index.stats().peak_representatives, 9);
    }
    #[test]
    fn snapshots_round_trip_without_retaining_expired_source_history() {
        let horizon = Horizon::Units(n(2));
        let mut original = Index::new(horizon, n(10000)).unwrap();
        for id in 1..=10 {
            original.advance(id, Duration::from_secs(id)).unwrap();
            let unit = unit(id, "ALPHA");
            original.retain(unit.clone(), &unit.state).unwrap();
        }
        let snapshot =
            serde_json::from_value(serde_json::to_value(original.snapshot()).unwrap()).unwrap();
        let mut restored = Index::new(horizon, n(10000)).unwrap();
        restored.restore(snapshot, |unit| &unit.state).unwrap();
        assert_eq!(
            serde_json::to_value(original.stats()).unwrap(),
            serde_json::to_value(restored.stats()).unwrap()
        );
        assert_eq!(
            restored.candidates(&json!("ALPHA"), n(20)).unwrap().indices,
            vec![8, 9, 10]
        );
        assert_eq!(
            restored.advance(12, Duration::from_secs(20)).unwrap(),
            vec![8, 9]
        );
        assert_eq!(restored.stats().expired, 9);
    }
    #[test]
    fn snapshot_restore_refuses_changed_limits_future_entries_and_invalid_counters() {
        let mut index = Index::new(Horizon::Units(n(2)), n(10000)).unwrap();
        index.advance(1, Duration::from_secs(1)).unwrap();
        let unit = unit(1, "ALPHA");
        index.retain(unit.clone(), &unit.state).unwrap();
        let snapshot = index.snapshot();
        let mut changed = Index::new(Horizon::Units(n(3)), n(10000)).unwrap();
        assert!(
            changed
                .restore(snapshot.clone(), |unit| &unit.state)
                .is_err()
        );
        let mut bad = snapshot.clone();
        bad.current = 0;
        assert!(index.restore(bad, |unit| &unit.state).is_err());
        let mut bad = snapshot.clone();
        bad.peak_bytes = 0;
        assert!(index.restore(bad, |unit| &unit.state).is_err());
        let mut bad = snapshot;
        bad.entries[0].admitted = Duration::from_secs(100);
        assert!(index.restore(bad, |unit| &unit.state).is_err());
        assert_eq!(index.stats().representatives, 1);
    }
}
