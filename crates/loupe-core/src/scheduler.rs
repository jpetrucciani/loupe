use std::time::Duration;
use tokio::sync::mpsc;

use crate::error::{Error, ErrorKind, Result};

pub struct Batcher<T> {
    receiver: mpsc::Receiver<Result<T>>,
    max_records: usize,
    idle: Duration,
    stopped: bool,
}

impl<T> Batcher<T> {
    pub fn new(
        receiver: mpsc::Receiver<Result<T>>,
        max_records: usize,
        idle: Duration,
    ) -> Result<Self> {
        if max_records == 0 || idle.is_zero() {
            return Err(Error::new(
                ErrorKind::Usage,
                "batch size and idle flush interval must be positive",
            ));
        }
        Ok(Self {
            receiver,
            max_records,
            idle,
            stopped: false,
        })
    }

    pub async fn next(&mut self) -> Option<Vec<Result<T>>> {
        if self.stopped {
            return None;
        }
        let first = self.receiver.recv().await?;
        let mut records = vec![first];
        while records.len() < self.max_records && records.last().is_some_and(Result::is_ok) {
            match tokio::time::timeout(self.idle, self.receiver.recv()).await {
                Ok(Some(record)) => records.push(record),
                Ok(None) => break,
                Err(_) => break,
            }
        }
        if records.last().is_some_and(Result::is_err) {
            self.stopped = true;
            self.receiver.close();
        }
        Some(records)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::error::{Error, ErrorKind};
    use crate::scheduler::Batcher;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test(start_paused = true)]
    async fn idle_flush_does_not_wait_for_eof_and_retains_source_order() {
        let (sender, receiver) = mpsc::channel(4);
        let mut batcher = Batcher::new(receiver, 4, Duration::from_millis(10)).unwrap();
        sender.send(Ok(1)).await.unwrap();
        let start = tokio::time::Instant::now();
        assert_eq!(
            batcher
                .next()
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(10));
        for n in 2..=5 {
            sender.send(Ok(n)).await.unwrap();
        }
        let start = tokio::time::Instant::now();
        assert_eq!(
            batcher
                .next()
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        assert_eq!(start.elapsed(), Duration::ZERO);
        drop(sender);
        assert!(batcher.next().await.is_none());
    }

    #[tokio::test]
    async fn input_failure_is_retained_after_the_valid_prefix_and_stops_read_ahead() {
        let (sender, receiver) = mpsc::channel(4);
        sender.send(Ok(1)).await.unwrap();
        sender
            .send(Err(Error::new(ErrorKind::Input, "malformed unit")))
            .await
            .unwrap();
        sender.send(Ok(3)).await.unwrap();
        let mut batcher = Batcher::new(receiver, 4, Duration::from_millis(10)).unwrap();
        let records = batcher.next().await.unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].is_ok());
        assert_eq!(records[1].as_ref().unwrap_err().exit_code(), 65);
        assert!(batcher.next().await.is_none());
        assert!(sender.send(Ok(4)).await.is_err());
    }
}
