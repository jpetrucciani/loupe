use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{Error, ErrorKind, Result};

const BINS: usize = 7424;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Interval {
    pub lower_ns: u64,
    pub upper_ns: u64,
}

#[derive(Debug)]
struct Histogram {
    counts: Box<[u64]>,
    samples: u64,
    minimum: Option<u64>,
    maximum: Option<u64>,
}
impl Default for Histogram {
    fn default() -> Self {
        Self {
            counts: vec![0; BINS].into_boxed_slice(),
            samples: 0,
            minimum: None,
            maximum: None,
        }
    }
}
impl Histogram {
    fn observe(&mut self, nanoseconds: u64, weight: u64) -> Result<()> {
        if weight == 0 {
            return Ok(());
        }
        let cell = self.counts.get_mut(bin(nanoseconds)).ok_or_else(overflow)?;
        let count = cell.checked_add(weight).ok_or_else(overflow)?;
        let total = self.samples.checked_add(weight).ok_or_else(overflow)?;
        *cell = count;
        self.samples = total;
        self.minimum = Some(
            self.minimum
                .map_or(nanoseconds, |minimum| minimum.min(nanoseconds)),
        );
        self.maximum = Some(
            self.maximum
                .map_or(nanoseconds, |maximum| maximum.max(nanoseconds)),
        );
        Ok(())
    }
    fn percentile(&self, percentile: u8) -> Result<Option<Interval>> {
        if self.samples == 0 {
            return Ok(None);
        }
        let rank = (u128::from(self.samples) * u128::from(percentile)).div_ceil(100);
        let rank = u64::try_from(rank).map_err(|_| overflow())?;
        let mut cumulative = 0u64;
        for (index, count) in self.counts.iter().enumerate() {
            cumulative = cumulative.checked_add(*count).ok_or_else(overflow)?;
            if cumulative >= rank {
                return Ok(Some(bounds(index)?));
            }
        }
        Err(overflow())
    }
}
fn bin(value: u64) -> usize {
    if value < 128 {
        return value as usize;
    }
    let power = 63 - value.leading_zeros();
    ((power - 7) * 128) as usize + (value >> (power - 7)) as usize
}
fn bounds(index: usize) -> Result<Interval> {
    if index >= BINS {
        return Err(overflow());
    }
    if index < 128 {
        return Ok(Interval {
            lower_ns: index as u64,
            upper_ns: index as u64,
        });
    }
    let shift = index / 128 - 1;
    let mantissa = 128 + (index % 128) as u64;
    let lower_ns = mantissa.checked_shl(shift as u32).ok_or_else(overflow)?;
    let upper_ns = if mantissa == 255 && shift == 56 {
        u64::MAX
    } else {
        ((mantissa + 1) << shift) - 1
    };
    Ok(Interval { lower_ns, upper_ns })
}
#[derive(Default)]
struct State {
    logical_records: u64,
    succeeded_records: u64,
    failed_records: u64,
    cancelled_records: u64,
    latency: Histogram,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub logical_records: u64,
    pub succeeded_records: u64,
    pub failed_records: u64,
    pub cancelled_records: u64,
    pub latency_samples: u64,
    pub latency_min_ns: Option<u64>,
    pub latency_max_ns: Option<u64>,
    pub p50: Option<Interval>,
    pub p95: Option<Interval>,
}

#[derive(Clone, Default)]
pub struct Recorder {
    state: Arc<Mutex<State>>,
}
pub struct Evaluation {
    recorder: Recorder,
    records: u64,
    cached: u64,
    submitted: bool,
    started: Instant,
    finished: bool,
}
impl Recorder {
    pub fn begin(&self, records: usize) -> Result<Evaluation> {
        let records = u64::try_from(records).map_err(|_| overflow())?;
        if records == 0 {
            return Err(Error::new(
                ErrorKind::Usage,
                "logical evaluation requires at least one record",
            ));
        }
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        state.logical_records = state
            .logical_records
            .checked_add(records)
            .ok_or_else(overflow)?;
        Ok(Evaluation {
            recorder: self.clone(),
            records,
            cached: 0,
            submitted: false,
            started: Instant::now(),
            finished: false,
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        Ok(Snapshot {
            logical_records: state.logical_records,
            succeeded_records: state.succeeded_records,
            failed_records: state.failed_records,
            cancelled_records: state.cancelled_records,
            latency_samples: state.latency.samples,
            latency_min_ns: state.latency.minimum,
            latency_max_ns: state.latency.maximum,
            p50: state.latency.percentile(50)?,
            p95: state.latency.percentile(95)?,
        })
    }
}
impl Evaluation {
    pub fn submitted(&mut self) {
        self.submitted = true;
    }
    pub fn cached(&mut self, records: usize) -> Result<()> {
        let cached = u64::try_from(records).map_err(|_| overflow())?;
        if cached > self.records {
            return Err(overflow());
        }
        self.cached = cached;
        Ok(())
    }
    pub fn finish(mut self, succeeded: usize) -> Result<()> {
        self.complete(succeeded, self.started.elapsed())?;
        self.finished = true;
        Ok(())
    }
    fn complete(&self, succeeded: usize, elapsed: Duration) -> Result<()> {
        let succeeded = u64::try_from(succeeded).map_err(|_| overflow())?;
        let failed = self.records.checked_sub(succeeded).ok_or_else(overflow)?;
        let samples = if self.submitted {
            self.records.checked_sub(self.cached).ok_or_else(overflow)?
        } else {
            0
        };
        let nanoseconds = u64::try_from(elapsed.as_nanos()).map_err(|_| overflow())?;
        let mut state = self.recorder.state.lock().map_err(|_| poisoned())?;
        let successes = state
            .succeeded_records
            .checked_add(succeeded)
            .ok_or_else(overflow)?;
        let failures = state
            .failed_records
            .checked_add(failed)
            .ok_or_else(overflow)?;
        state.latency.observe(nanoseconds, samples)?;
        state.succeeded_records = successes;
        state.failed_records = failures;
        Ok(())
    }
}
impl Drop for Evaluation {
    fn drop(&mut self) {
        if !self.finished
            && let Ok(mut state) = self.recorder.state.lock()
            && let Some(count) = state.cancelled_records.checked_add(self.records)
        {
            state.cancelled_records = count;
        }
    }
}
fn overflow() -> Error {
    Error::new(
        ErrorKind::Unavailable,
        "logical evaluation/latency counter overflow or inconsistent records",
    )
}
fn poisoned() -> Error {
    Error::new(ErrorKind::Unavailable, "run metrics lock is poisoned")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::metrics::{BINS, Histogram, Recorder, bin, bounds};
    use std::time::Duration;
    #[test]
    fn histogram_bounds_cover_every_bit_boundary_with_less_than_one_percent_width() {
        for power in 0..64 {
            let value = 1u64 << power;
            for sample in [
                value.saturating_sub(1),
                value,
                value.saturating_add(1),
                u64::MAX,
            ] {
                let interval = bounds(bin(sample)).unwrap();
                assert!(interval.lower_ns <= sample && sample <= interval.upper_ns);
                assert!(
                    interval.lower_ns == 0
                        || u128::from(interval.upper_ns - interval.lower_ns) * 128
                            <= u128::from(interval.lower_ns)
                );
            }
        }
        assert_eq!(bin(u64::MAX), BINS - 1);
        assert!(bounds(BINS).is_err());
    }
    #[test]
    fn weighted_percentiles_use_nearest_rank_and_overflow_never_mutates_samples() {
        let mut histogram = Histogram::default();
        assert!(histogram.percentile(50).unwrap().is_none());
        for value in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
            histogram.observe(value, 1).unwrap();
        }
        assert_eq!(histogram.percentile(50).unwrap().unwrap().upper_ns, 5);
        assert_eq!(histogram.percentile(95).unwrap().unwrap().upper_ns, 10);
        let before = histogram.counts.clone();
        assert!(histogram.observe(10, u64::MAX).is_err());
        assert_eq!(histogram.counts, before);
        assert_eq!(histogram.samples, 10);
        histogram.observe(1024, 90).unwrap();
        assert_eq!(histogram.percentile(95).unwrap().unwrap().lower_ns, 1024);
    }
    #[test]
    fn logical_counts_are_per_record_and_exclude_cache_hits_from_latency() {
        let recorder = Recorder::default();
        let mut evaluation = recorder.begin(4).unwrap();
        evaluation.cached(2).unwrap();
        evaluation.submitted();
        evaluation.complete(3, Duration::from_millis(12)).unwrap();
        evaluation.finished = true;
        drop(evaluation);
        let metrics = recorder.snapshot().unwrap();
        assert_eq!(metrics.logical_records, 4);
        assert_eq!(metrics.succeeded_records, 3);
        assert_eq!(metrics.failed_records, 1);
        assert_eq!(metrics.latency_samples, 2);
        assert_eq!(metrics.cancelled_records, 0);
        let interval = metrics.p50.unwrap();
        assert!(interval.lower_ns <= 12_000_000 && interval.upper_ns >= 12_000_000);
        let mut cached = recorder.begin(10).unwrap();
        cached.cached(10).unwrap();
        cached.finish(10).unwrap();
        assert_eq!(recorder.snapshot().unwrap().latency_samples, 2);
    }
    #[test]
    fn dropping_inflight_work_counts_cancellation_without_inventing_a_completed_latency() {
        let recorder = Recorder::default();
        drop(recorder.begin(3).unwrap());
        let snapshot = recorder.snapshot().unwrap();
        assert_eq!(snapshot.logical_records, 3);
        assert_eq!(snapshot.cancelled_records, 3);
        assert_eq!(snapshot.latency_samples, 0);
        assert_eq!(snapshot.succeeded_records, 0);
        assert!(snapshot.p50.is_none());
        assert!(recorder.begin(0).is_err());
        let evaluation = recorder.begin(1).unwrap();
        assert!(evaluation.complete(2, Duration::ZERO).is_err());
        drop(evaluation);
        assert_eq!(recorder.snapshot().unwrap().cancelled_records, 4);
    }
    #[test]
    fn histogram_memory_is_fixed_across_a_long_stream() {
        let mut histogram = Histogram::default();
        for value in 0..100_000 {
            histogram.observe(value, 1).unwrap();
        }
        assert_eq!(histogram.counts.len(), BINS);
        assert_eq!(histogram.samples, 100_000);
        let median = histogram.percentile(50).unwrap().unwrap();
        assert!(median.lower_ns <= 49_999 && median.upper_ns >= 49_999);
    }
}
