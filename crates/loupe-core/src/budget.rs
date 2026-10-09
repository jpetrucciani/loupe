use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub spent_input: u64,
    pub reserved_input: u64,
    pub attempts: u64,
    pub http_attempts: u64,
    pub unknown_attempts: u64,
    pub unknown_input: u64,
    pub reported_input: u64,
    pub reported_output: u64,
    pub cache_hits: u64,
}

#[derive(Clone)]
pub struct Budget {
    limit: Option<u64>,
    usage: Arc<Mutex<Usage>>,
    pub metrics: crate::metrics::Recorder,
}

pub struct Reservation {
    budget: Budget,
    tokens: u64,
    attempts: u64,
    reserved: bool,
}

pub struct Submitted {
    budget: Budget,
    attempts: u64,
    tokens: u64,
}

impl Budget {
    pub fn new(limit: Option<u64>) -> Self {
        Self {
            limit,
            usage: Arc::new(Mutex::new(Usage::default())),
            metrics: crate::metrics::Recorder::default(),
        }
    }

    fn locked(&self) -> Result<MutexGuard<'_, Usage>> {
        self.usage
            .lock()
            .map_err(|_| Error::new(ErrorKind::Unavailable, "budget ledger lock is poisoned"))
    }

    pub fn usage(&self) -> Result<Usage> {
        Ok(*self.locked()?)
    }

    pub fn reserve(&self, tokens: u64) -> Result<Reservation> {
        self.reserve_batch(&[tokens])
    }
    pub fn record_cache_hits(&self, hits: u64) -> Result<()> {
        let mut usage = self.locked()?;
        usage.cache_hits = usage
            .cache_hits
            .checked_add(hits)
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "cache-hit counter overflow"))?;
        Ok(())
    }

    pub fn reserve_batch(&self, tokens: &[u64]) -> Result<Reservation> {
        let attempts = u64::try_from(tokens.len())
            .ok()
            .filter(|length| *length > 0)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Budget,
                    "a token reservation requires at least one record",
                )
            })?;
        let tokens = tokens.iter().try_fold(0u64, |total, value| {
            total
                .checked_add(*value)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "batch token reservation overflow"))
        })?;
        let mut usage = self.locked()?;
        let reserved = usage
            .reserved_input
            .checked_add(tokens)
            .ok_or_else(|| Error::new(ErrorKind::Budget, "token reservation overflow"))?;
        let total = usage
            .spent_input
            .checked_add(reserved)
            .ok_or_else(|| Error::new(ErrorKind::Budget, "token budget overflow"))?;
        if let Some(limit) = self.limit
            && total > limit
        {
            return Err(Error::new(
                ErrorKind::Budget,
                format!(
                    "input-token budget exceeded: {total} reserved/submitted tokens, limit {}",
                    limit
                ),
            ));
        }
        usage.reserved_input = reserved;
        Ok(Reservation {
            budget: self.clone(),
            tokens,
            attempts,
            reserved: true,
        })
    }
}

impl Reservation {
    pub fn submit(mut self) -> Result<Submitted> {
        {
            let mut usage = self.budget.locked()?;
            let spent = usage
                .spent_input
                .checked_add(self.tokens)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "spent-token counter overflow"))?;
            let attempts = usage
                .attempts
                .checked_add(self.attempts)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "attempt counter overflow"))?;
            let http_attempts = usage
                .http_attempts
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "HTTP attempt counter overflow"))?;
            let unknown = usage
                .unknown_attempts
                .checked_add(self.attempts)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "unknown-attempt counter overflow"))?;
            let unknown_input = usage
                .unknown_input
                .checked_add(self.tokens)
                .ok_or_else(|| Error::new(ErrorKind::Budget, "unknown-input counter overflow"))?;
            let remaining = usage
                .reserved_input
                .checked_sub(self.tokens)
                .ok_or_else(|| {
                    Error::new(ErrorKind::Unavailable, "inconsistent budget reservation")
                })?;
            usage.spent_input = spent;
            usage.reserved_input = remaining;
            usage.attempts = attempts;
            usage.http_attempts = http_attempts;
            usage.unknown_attempts = unknown;
            usage.unknown_input = unknown_input;
        }
        self.reserved = false;
        Ok(Submitted {
            budget: self.budget.clone(),
            attempts: self.attempts,
            tokens: self.tokens,
        })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.reserved
            && let Ok(mut usage) = self.budget.usage.lock()
        {
            usage.reserved_input = usage.reserved_input.saturating_sub(self.tokens);
        }
    }
}

impl Submitted {
    pub fn input_tokens(&self) -> u64 {
        self.tokens
    }
    pub fn finish(self, input_tokens: u64, output_tokens: u64) -> Result<()> {
        let mut usage = self.budget.locked()?;
        let reported_input = usage
            .reported_input
            .checked_add(input_tokens)
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "reported token counter overflow"))?;
        let reported_output = usage
            .reported_output
            .checked_add(output_tokens)
            .ok_or_else(|| {
                Error::new(ErrorKind::Unavailable, "reported output counter overflow")
            })?;
        let unknown = usage
            .unknown_attempts
            .checked_sub(self.attempts)
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "inconsistent completed attempt"))?;
        let unknown_input = usage
            .unknown_input
            .checked_sub(self.tokens)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unavailable,
                    "inconsistent completed input tokens",
                )
            })?;
        usage.reported_input = reported_input;
        usage.reported_output = reported_output;
        usage.unknown_attempts = unknown;
        usage.unknown_input = unknown_input;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::budget::Budget;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn cache_hits_never_spend_or_reserve_input_and_counter_overflow_is_atomic() {
        let budget = Budget::new(Some(0));
        budget.record_cache_hits(5).unwrap();
        let usage = budget.usage().unwrap();
        assert_eq!(usage.cache_hits, 5);
        assert_eq!(usage.spent_input, 0);
        assert_eq!(usage.reserved_input, 0);
        assert_eq!(usage.attempts, 0);
        assert_eq!(usage.reported_input, 0);
        assert_eq!(usage.http_attempts, 0);
        assert!(budget.record_cache_hits(u64::MAX).is_err());
        assert_eq!(budget.usage().unwrap(), usage);
    }

    #[test]
    fn batch_reservations_are_atomic_and_count_records_separately_from_http_attempts() {
        let budget = Budget::new(Some(100));
        let reservation = budget.reserve_batch(&[30, 40]).unwrap();
        assert!(budget.reserve_batch(&[20, 20]).is_err());
        assert_eq!(budget.usage().unwrap().reserved_input, 70);
        drop(reservation);
        assert_eq!(budget.usage().unwrap().reserved_input, 0);
        let submitted = budget.reserve_batch(&[30, 40]).unwrap().submit().unwrap();
        let usage = budget.usage().unwrap();
        assert_eq!(usage.spent_input, 70);
        assert_eq!(usage.attempts, 2);
        assert_eq!(usage.http_attempts, 1);
        assert_eq!(usage.unknown_attempts, 2);
        submitted.finish(65, 0).unwrap();
        assert_eq!(budget.usage().unwrap().spent_input, 70);
        assert_eq!(budget.usage().unwrap().reported_input, 65);
        assert_eq!(budget.usage().unwrap().unknown_attempts, 0);
        assert!(budget.reserve_batch(&[20, 11]).is_err());
        assert!(budget.reserve_batch(&[]).is_err());
        assert!(budget.reserve_batch(&[u64::MAX, 1]).is_err());
    }

    #[test]
    fn unknown_batch_retries_keep_every_record_attempt_spent() {
        let budget = Budget::new(Some(40));
        for _ in 0..2 {
            drop(budget.reserve_batch(&[10, 10]).unwrap().submit().unwrap());
        }
        let usage = budget.usage().unwrap();
        assert_eq!(usage.spent_input, 40);
        assert_eq!(usage.attempts, 4);
        assert_eq!(usage.http_attempts, 2);
        assert_eq!(usage.unknown_attempts, 4);
        assert!(budget.reserve_batch(&[1, 1]).is_err());
    }

    #[test]
    fn unsent_cancelled_reservations_release_and_unknown_submissions_stay_spent() {
        let budget = Budget::new(Some(100));
        drop(budget.reserve(90).unwrap());
        assert_eq!(budget.usage().unwrap().reserved_input, 0);
        drop(budget.reserve(90).unwrap().submit().unwrap());
        assert_eq!(budget.usage().unwrap().spent_input, 90);
        assert_eq!(budget.usage().unwrap().unknown_attempts, 1);
        assert!(budget.reserve(11).is_err());
    }

    #[test]
    fn reconciliation_does_not_refund_submitted_tokens_and_retries_spend_separately() {
        let budget = Budget::new(Some(100));
        budget
            .reserve(40)
            .unwrap()
            .submit()
            .unwrap()
            .finish(35, 2)
            .unwrap();
        budget
            .reserve(40)
            .unwrap()
            .submit()
            .unwrap()
            .finish(40, 0)
            .unwrap();
        let usage = budget.usage().unwrap();
        assert_eq!(usage.spent_input, 80);
        assert_eq!(usage.reported_input, 75);
        assert_eq!(usage.reported_output, 2);
        assert_eq!(usage.attempts, 2);
        assert_eq!(usage.unknown_attempts, 0);
        assert!(budget.reserve(40).is_err());
    }

    #[test]
    fn concurrent_admission_cannot_overshoot() {
        let budget = Budget::new(Some(250));
        let barrier = Arc::new(Barrier::new(16));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let budget = budget.clone();
            let barrier = barrier.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                if let Ok(reservation) = budget.reserve(100) {
                    drop(reservation.submit().unwrap());
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(budget.usage().unwrap().spent_input, 200);
        assert_eq!(budget.usage().unwrap().reserved_input, 0);
        assert_eq!(budget.usage().unwrap().attempts, 2);
    }

    #[test]
    fn counter_overflow_is_rejected_before_mutation() {
        let budget = Budget::new(None);
        let reservation = budget.reserve(u64::MAX).unwrap();
        assert!(budget.reserve(1).is_err());
        drop(reservation);
        assert_eq!(budget.usage().unwrap().reserved_input, 0);
    }
}
