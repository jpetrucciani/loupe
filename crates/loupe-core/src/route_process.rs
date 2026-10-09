use std::collections::BTreeMap;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::{Pid, Signal};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::time::Instant;

use crate::error::{Error, ErrorKind, Result};
use crate::route_labels::Name;

struct Process {
    child: Child,
    input: Option<ChildStdin>,
    pid: Pid,
    reaped: bool,
}
impl Drop for Process {
    fn drop(&mut self) {
        if !self.reaped {
            // The unreaped direct child retains ownership of this process-group ID.
            let _ = rustix::process::kill_process_group(self.pid, Signal::KILL);
        }
    }
}
pub struct Sinks {
    commands: BTreeMap<Name, String>,
    processes: BTreeMap<Name, Process>,
    timeout: Duration,
    deadline: Option<Instant>,
}
#[derive(Debug)]
pub struct Exit {
    pub label: Name,
    pub status: ExitStatus,
}
impl Sinks {
    pub fn new(
        commands: BTreeMap<Name, String>,
        timeout: Duration,
        deadline: Option<Instant>,
    ) -> Result<Self> {
        if commands
            .values()
            .any(|command| command.trim().is_empty() || command.contains('\0'))
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "route commands must be nonempty shell strings without NUL",
            ));
        }
        Ok(Self {
            commands,
            processes: BTreeMap::new(),
            timeout,
            deadline,
        })
    }
    pub fn contains(&self, name: &Name) -> bool {
        self.commands.contains_key(name)
    }
    fn expires(&self) -> Result<Instant> {
        let timeout = Instant::now().checked_add(self.timeout).ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "route process timeout is outside the clock range",
            )
        })?;
        Ok(self
            .deadline
            .map_or(timeout, |deadline| deadline.min(timeout)))
    }
    pub async fn write(&mut self, name: &Name, payload: &[u8]) -> Result<()> {
        let expires = self.expires()?;
        if expires <= Instant::now() {
            return Err(failure(name, "process write deadline expired"));
        }
        if !self.processes.contains_key(name) {
            let command = self
                .commands
                .get(name)
                .ok_or_else(|| failure(name, "process command is undeclared"))?;
            let mut child = Command::new("sh")
                .args(["-c", command])
                .process_group(0)
                .kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|error| failure(name, &format!("spawning process: {error}")))?;
            let pid = child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .filter(|id| *id > 1)
                .and_then(Pid::from_raw)
                .ok_or_else(|| failure(name, "spawned process has no valid child ID"))?;
            let input = child
                .stdin
                .take()
                .ok_or_else(|| failure(name, "spawned process has no stdin"))?;
            self.processes.insert(
                name.clone(),
                Process {
                    child,
                    input: Some(input),
                    pid,
                    reaped: false,
                },
            );
        }
        let process = self
            .processes
            .get_mut(name)
            .ok_or_else(|| failure(name, "process state is missing"))?;
        if let Some(status) = process
            .child
            .try_wait()
            .map_err(|error| failure(name, &format!("checking process: {error}")))?
        {
            process.reaped = true;
            return Err(failure(
                name,
                &format!("process exited before input completed: {status}"),
            ));
        }
        let input = process
            .input
            .as_mut()
            .ok_or_else(|| failure(name, "process stdin is closed"))?;
        tokio::time::timeout_at(expires, input.write_all(payload))
            .await
            .map_err(|_| failure(name, "process stdin write timed out"))?
            .map_err(|error| failure(name, &format!("writing process stdin: {error}")))
    }
    pub async fn finish(&mut self) -> Result<Vec<Exit>> {
        // Close all pipes first so independent children can drain concurrently.
        for process in self.processes.values_mut() {
            process.input.take();
        }
        let mut exits = Vec::new();
        let expires = self.expires()?;
        for (name, process) in &mut self.processes {
            let status = tokio::time::timeout_at(expires, process.child.wait())
                .await
                .map_err(|_| failure(name, "process EOF shutdown timed out"))?
                .map_err(|error| failure(name, &format!("waiting for process: {error}")))?;
            process.reaped = true;
            if !status.success() {
                return Err(failure(name, &format!("process exited: {status}")));
            }
            exits.push(Exit {
                label: name.clone(),
                status,
            });
        }
        Ok(exits)
    }
}
fn failure(name: &Name, message: &str) -> Error {
    Error::new(
        ErrorKind::Unavailable,
        format!(
            "route {:?}: {message}; process delivery has no replay guarantee",
            name.as_str()
        ),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::route_labels::Name;
    use crate::route_process::Sinks;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Duration;
    use tokio::time::Instant;
    fn quote(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }
    fn sinks(command: String, timeout: Duration, deadline: Option<Instant>) -> Sinks {
        Sinks::new(
            BTreeMap::from([(Name::parse("page").unwrap(), command)]),
            timeout,
            deadline,
        )
        .unwrap()
    }
    #[tokio::test]
    async fn persistent_real_process_preserves_raw_bytes_and_starts_only_once() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        let starts = root.path().join("starts");
        let mut sinks = sinks(
            format!("printf x >> {}; cat > {}", quote(&starts), quote(&output)),
            Duration::from_secs(2),
            None,
        );
        let name = Name::parse("page").unwrap();
        sinks.write(&name, b"first\r\n").await.unwrap();
        sinks.write(&name, "東京🙂\0last".as_bytes()).await.unwrap();
        let exits = sinks.finish().await.unwrap();
        assert_eq!(exits.len(), 1);
        assert!(exits[0].status.success());
        assert_eq!(std::fs::read(starts).unwrap(), b"x");
        assert_eq!(
            std::fs::read(output).unwrap(),
            "first\r\n東京🙂\0last".as_bytes()
        );
    }
    #[tokio::test]
    async fn failed_child_reports_external_delivery_uncertainty_without_retrying() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        let mut sinks = sinks(
            format!("cat > {}; exit 7", quote(&output)),
            Duration::from_secs(2),
            None,
        );
        sinks
            .write(&Name::parse("page").unwrap(), b"sent\n")
            .await
            .unwrap();
        let error = sinks.finish().await.unwrap_err();
        assert_eq!(error.exit_code(), 69);
        assert!(error.to_string().contains('7'));
        assert!(error.to_string().contains("no replay guarantee"));
        assert_eq!(std::fs::read(output).unwrap(), b"sent\n");
    }
    #[tokio::test]
    async fn actual_blocked_stdin_timeout_kills_owned_process_group_and_descendant_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("leaked");
        let nested = format!("sleep 0.3; printf leaked > {}", quote(&marker));
        let command = format!("sh -c '{}' & sleep 30", nested.replace('\'', "'\\''"));
        let mut sinks = sinks(command, Duration::from_millis(50), None);
        let error = sinks
            .write(&Name::parse("page").unwrap(), &vec![b'x'; 2_000_000])
            .await
            .unwrap_err();
        assert_eq!(error.exit_code(), 69);
        assert!(error.to_string().contains("timed out"));
        drop(sinks);
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(!marker.exists());
    }
    #[tokio::test]
    async fn eof_shutdown_is_bounded_by_both_process_timeout_and_overall_deadline() {
        for deadline in [false, true] {
            let expires = deadline.then(|| Instant::now() + Duration::from_millis(50));
            let mut sinks = sinks(
                "cat >/dev/null; sleep 30".to_owned(),
                if deadline {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(50)
                },
                expires,
            );
            sinks
                .write(&Name::parse("page").unwrap(), b"one\n")
                .await
                .unwrap();
            assert_eq!(sinks.finish().await.unwrap_err().exit_code(), 69);
            drop(sinks);
        }
    }
}
