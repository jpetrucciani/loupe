//! Bounded one-shot processes with owned process groups and cancellation cleanup.

use crate::error::{Error, ErrorKind, Result};
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use serde::{Deserialize, Serialize};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub bytes: Vec<u8>,
    pub total_bytes: u64,
    pub truncated: bool,
}
struct Process {
    child: Child,
    pid: Pid,
    reaped: bool,
}
impl Process {
    fn stop(&self) {
        let _ = rustix::process::kill_process_group(self.pid, Signal::KILL);
    }
    async fn finish(&mut self, expires: Instant) -> Result<(Option<i32>, Option<i32>, bool)> {
        let observed = async {
            loop {
                match rustix::process::waitid(
                    WaitId::Pid(self.pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(status)) => {
                        return Ok((status.exit_status(), status.terminating_signal(), false));
                    }
                    Ok(None) => {}
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(Error::io("observing child process", error.into())),
                }
                if Instant::now() >= expires {
                    return Ok((None, None, true));
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        .await;
        // WNOWAIT keeps the direct child waitable, so its process-group ID is still owned.
        self.stop();
        let reaped = tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await;
        let mut observed = observed;
        if let Ok(status) = reaped {
            let status = status.map_err(|error| Error::io("reaping child process", error))?;
            self.reaped = true;
            if observed.as_ref().is_ok_and(|(_, _, timed_out)| *timed_out) {
                observed = Ok((status.code(), status.signal(), true));
            }
        }
        observed
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !self.reaped {
            self.stop();
        }
    }
}
async fn capture(mut input: impl AsyncRead + Unpin, limit: usize) -> Result<Capture> {
    let mut bytes = Vec::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let count = input
            .read(&mut buffer)
            .await
            .map_err(|error| Error::io("reading child output", error))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| invalid("child output count overflow"))?;
        let retained = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
    }
    Ok(Capture {
        truncated: total > bytes.len() as u64,
        bytes,
        total_bytes: total,
    })
}
#[derive(Debug)]
pub struct Outcome {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub input_error: Option<String>,
    pub stdout: Capture,
    pub stderr: Capture,
}

/// The output limit applies separately to each stream; excess bytes are drained.
pub async fn execute(
    mut builder: Command,
    input_bytes: &[u8],
    expires: Instant,
    limit: usize,
) -> Result<Outcome> {
    if Instant::now() >= expires {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "child deadline expired before launch",
        ));
    }
    builder
        .process_group(0)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = builder
        .spawn()
        .map_err(|error| Error::io("launching child process", error))?;
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .filter(|pid| pid.as_raw_nonzero().get() > 1)
        .ok_or_else(|| invalid("child has no valid owned process ID"))?;
    let mut process = Process {
        child,
        pid,
        reaped: false,
    };
    let mut input = process
        .child
        .stdin
        .take()
        .ok_or_else(|| invalid("child stdin missing"))?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| invalid("child stdout missing"))?;
    let stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| invalid("child stderr missing"))?;
    let feed = async {
        let result = input.write_all(input_bytes).await;
        drop(input);
        result.err().map(|error| error.to_string())
    };
    let io_expires = expires
        .checked_add(Duration::from_secs(1))
        .ok_or_else(|| invalid("child cleanup timeout outside clock range"))?;
    let (input_error, stdout, stderr, status) = tokio::time::timeout_at(io_expires, async {
        tokio::join!(
            feed,
            capture(stdout, limit),
            capture(stderr, limit),
            process.finish(expires)
        )
    })
    .await
    .map_err(|_| {
        Error::new(
            ErrorKind::Unavailable,
            "child I/O remained open after its deadline",
        )
    })?;
    let (code, signal, timed_out) = status?;
    Ok(Outcome {
        code,
        signal,
        timed_out,
        input_error,
        stdout: stdout?,
        stderr: stderr?,
    })
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Unavailable, message)
}
