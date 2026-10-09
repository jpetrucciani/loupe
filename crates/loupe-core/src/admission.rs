use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::error::{Error, ErrorKind, Result};

struct State {
    active: usize,
    base: usize,
    ceiling: usize,
    recover_at: Option<Instant>,
}

pub struct Admission {
    state: Mutex<State>,
    changed: Notify,
    queue: tokio::sync::Mutex<()>,
}
pub struct Permit {
    admission: Arc<Admission>,
    records: usize,
}

impl Admission {
    pub fn new(concurrency: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                active: 0,
                base: concurrency.max(1),
                ceiling: concurrency.max(1),
                recover_at: None,
            }),
            changed: Notify::new(),
            queue: tokio::sync::Mutex::new(()),
        })
    }

    pub async fn acquire(self: &Arc<Self>) -> Result<Permit> {
        self.acquire_many(1).await
    }

    pub async fn acquire_many(self: &Arc<Self>, records: usize) -> Result<Permit> {
        // FIFO queue ownership prevents a new chunk from stealing a freed slot
        // before an already notified unit gets polled. Cancellation releases ownership.
        let _queue = self.queue.lock().await;
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let recover_at = {
                let mut state = self.state.lock().map_err(|_| {
                    Error::new(ErrorKind::Unavailable, "admission lock is poisoned")
                })?;
                if state
                    .recover_at
                    .is_some_and(|deadline| deadline <= Instant::now())
                {
                    state.ceiling = state.base;
                    state.recover_at = None;
                }
                if records == 0 || records > state.base {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "batch admission must fit the configured record concurrency",
                    ));
                }
                if records <= state.ceiling.saturating_sub(state.active) {
                    state.active += records;
                    return Ok(Permit {
                        admission: self.clone(),
                        records,
                    });
                }
                state.recover_at
            };
            tokio::select! {
                _ = &mut changed => {},
                _ = async { if let Some(deadline) = recover_at { tokio::time::sleep_until(deadline).await; } else { std::future::pending::<()>().await; } } => {},
            }
        }
    }

    pub fn congestion(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Unavailable, "admission lock is poisoned"))?;
        state.ceiling = (state.ceiling / 2).max(1);
        state.recover_at = Some(Instant::now() + Duration::from_secs(10));
        self.changed.notify_waiters();
        Ok(())
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.admission.state.lock() {
            state.active = state.active.saturating_sub(self.records);
        }
        self.admission.changed.notify_waiters();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::admission::Admission;

    #[tokio::test(start_paused = true)]
    async fn queued_units_get_freed_slots_before_new_chunks_and_cancelled_heads_release_the_queue()
    {
        let admission = Admission::new(1);
        let first = admission.acquire().await.unwrap();
        let older = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire().await.unwrap() })
        };
        tokio::task::yield_now().await;
        drop(first);
        assert!(
            tokio::time::timeout(std::time::Duration::ZERO, admission.acquire())
                .await
                .is_err()
        );
        let older = older.await.unwrap();
        let cancelled = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire().await.unwrap() })
        };
        tokio::task::yield_now().await;
        let tail = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire().await.unwrap() })
        };
        tokio::task::yield_now().await;
        cancelled.abort();
        assert!(cancelled.await.err().unwrap().is_cancelled());
        drop(older);
        let tail = tail.await.unwrap();
        assert_eq!(admission.state.lock().unwrap().active, 1);
        drop(tail);
        assert_eq!(admission.state.lock().unwrap().active, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn weighted_batch_admission_holds_records_and_recovers_without_overshooting() {
        let admission = Admission::new(4);
        let first = admission.acquire_many(3).await.unwrap();
        let waiter = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire_many(2).await.unwrap() })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        assert_eq!(admission.state.lock().unwrap().active, 3);
        drop(first);
        let two = waiter.await.unwrap();
        assert_eq!(admission.state.lock().unwrap().active, 2);
        admission.congestion().unwrap();
        let waiter = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire_many(4).await.unwrap() })
        };
        drop(two);
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        let four = waiter.await.unwrap();
        assert_eq!(admission.state.lock().unwrap().active, 4);
        drop(four);
        assert_eq!(admission.state.lock().unwrap().active, 0);
        assert!(admission.acquire_many(5).await.is_err());
        assert!(admission.acquire_many(0).await.is_err());
    }
    #[tokio::test(start_paused = true)]
    async fn congestion_halves_admission_until_recovery_without_losing_waiters() {
        let admission = Admission::new(4);
        let mut permits = Vec::new();
        for _ in 0..4 {
            permits.push(admission.acquire().await.unwrap());
        }
        admission.congestion().unwrap();
        assert_eq!(admission.state.lock().unwrap().ceiling, 2);
        let waiter = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.acquire().await.unwrap() })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        permits.pop();
        permits.pop();
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        permits.pop();
        tokio::task::yield_now().await;
        let permit = waiter.await.unwrap();
        assert_eq!(admission.state.lock().unwrap().active, 2);
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        let next = admission.acquire().await.unwrap();
        assert_eq!(admission.state.lock().unwrap().ceiling, 4);
        drop(next);
        drop(permit);
        drop(permits);
        assert_eq!(admission.state.lock().unwrap().active, 0);
    }
}
