//! Ordered trigger declarations and one-shot child process actions.

use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::policy::{Assessment, Selected, UncertainAction};
use crate::wire::Question;
use rustix::fs::{Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::Instant;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Target {
    Exec(String),
    Append(PathBuf),
}
#[derive(Clone, Debug, Serialize)]
pub struct Declaration {
    pub when: String,
    pub target: Target,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Matcher {
    Yes,
    No,
    Uncertain,
    Label(String),
}
#[derive(Clone, Debug, Serialize)]
pub struct Action {
    pub id: usize,
    pub matcher: Matcher,
    pub target: Target,
}
impl Action {
    pub fn matches(&self, assessment: &Assessment, uncertain: UncertainAction) -> bool {
        match &self.matcher {
            Matcher::Uncertain => assessment.uncertain,
            _ if assessment.uncertain && uncertain != UncertainAction::Keep => false,
            Matcher::Yes => matches!(assessment.selected, Selected::Boolean(true)),
            Matcher::No => matches!(assessment.selected, Selected::Boolean(false)),
            Matcher::Label(label) => {
                matches!(&assessment.selected,Selected::Choice(selected) if selected==label)
            }
        }
    }
}
pub fn plan(declarations: &[Declaration], question: &Question) -> Result<Vec<Action>> {
    if matches!(question, Question::Score { .. }) {
        return Err(usage("on accepts Noul or Choice questions"));
    }
    if let Question::Choice { criteria, .. } = question
        && criteria.contains_key("uncertain")
    {
        return Err(usage(
            "on reserves uncertain for the uncertainty selector; choose another Choice key",
        ));
    }
    if declarations.is_empty() {
        return Err(usage("on needs declared actions"));
    }
    declarations
        .iter()
        .enumerate()
        .map(|(position, declaration)| {
            if declaration.when.trim().is_empty() {
                return Err(usage("--when needs a selector"));
            }
            let matcher = match (question, declaration.when.as_str()) {
                (_, "uncertain") => Matcher::Uncertain,
                (Question::Noul { .. }, "yes") => Matcher::Yes,
                (Question::Noul { .. }, "no") => Matcher::No,
                (Question::Choice { criteria, .. }, label) if criteria.contains_key(label) => {
                    Matcher::Label(label.to_owned())
                }
                _ => {
                    return Err(usage(
                        "--when must be yes/no/uncertain for Noul or a declared Choice label",
                    ));
                }
            };
            match &declaration.target {
                Target::Exec(command) if command.trim().is_empty() || command.contains('\0') => {
                    return Err(usage("--exec needs a nonempty shell command without NUL"));
                }
                Target::Append(path)
                    if path.as_os_str().is_empty()
                        || path.as_os_str().as_encoded_bytes().contains(&0) =>
                {
                    return Err(usage("--append needs a valid file path"));
                }
                _ => {}
            }
            Ok(Action {
                id: position
                    .checked_add(1)
                    .ok_or_else(|| usage("action ID overflow"))?,
                matcher,
                target: declaration.target.clone(),
            })
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub input_error: Option<String>,
    pub stdout: crate::process::Capture,
    pub stderr: crate::process::Capture,
    pub unit_in_stdin_only: bool,
}
impl Outcome {
    pub fn failed(&self) -> bool {
        self.code != Some(0) || self.timed_out
    }
}
pub async fn execute(
    command: &str,
    unit: &Unit,
    label: &str,
    assessment: &Assessment,
    expires: Instant,
) -> Result<Outcome> {
    execute_with_env(
        command,
        unit,
        label,
        assessment,
        expires,
        &std::collections::BTreeMap::new(),
    )
    .await
}

/// Explicit metadata is applied after Loupe's model metadata. Callers must trust it.
pub async fn execute_with_env(
    command: &str,
    unit: &Unit,
    label: &str,
    assessment: &Assessment,
    expires: Instant,
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<Outcome> {
    if Instant::now() >= expires {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "action deadline expired before launch",
        ));
    }
    let stdin_only = unit.text.contains('\0') || unit.text.len() > 8192;
    let mut builder = Command::new("sh");
    builder
        .args(["-c", command])
        .env("LOUPE_INDEX", unit.index.to_string())
        .env("LOUPE_CONFIDENCE", assessment.confidence.get().to_string())
        .env("LOUPE_UNCERTAIN", assessment.uncertain.to_string())
        .env("LOUPE_DECISION_SOURCE", "model")
        .env("LOUPE_UNIT_IN_STDIN_ONLY", stdin_only.to_string());
    builder.env_remove("LOUPE_UNIT").env_remove("LOUPE_LABEL");
    for key in [
        "LOUPE_QUESTION",
        "LOUPE_QUESTION_ID",
        "LOUPE_ANSWER_JSON",
        "LOUPE_ENSEMBLE_JSON",
    ] {
        builder.env_remove(key);
    }
    builder.envs(environment);
    if !stdin_only {
        builder.env("LOUPE_UNIT", &unit.text);
    }
    if label.len() <= 8192 && !label.contains('\0') {
        builder.env("LOUPE_LABEL", label);
    }
    let outcome = crate::process::execute(builder, &unit.raw, expires, 65536).await?;
    Ok(Outcome {
        code: outcome.code,
        signal: outcome.signal,
        timed_out: outcome.timed_out,
        input_error: outcome.input_error,
        stdout: outcome.stdout,
        stderr: outcome.stderr,
        unit_in_stdin_only: stdin_only,
    })
}

pub struct Appends {
    files: BTreeMap<PathBuf, File>,
}
impl Appends {
    pub fn open(actions: &[Action], protected: &[(u64, u64)]) -> Result<Self> {
        let mut files = BTreeMap::new();
        for action in actions {
            let Target::Append(path) = &action.target else {
                continue;
            };
            if files.contains_key(path) {
                continue;
            }
            let file: File = rustix::fs::open(
                path,
                OFlags::WRONLY
                    | OFlags::CREATE
                    | OFlags::APPEND
                    | OFlags::CLOEXEC
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|error| Error::io("opening trigger append", error.into()))?
            .into();
            let metadata = file
                .metadata()
                .map_err(|error| Error::io("checking trigger append", error))?;
            if !metadata.is_file()
                || metadata.nlink() != 1
                || protected.contains(&(metadata.dev(), metadata.ino()))
            {
                return Err(usage(
                    "trigger append must be a regular single-link file distinct from input/stdout",
                ));
            }
            file.try_lock().map_err(|error| {
                Error::new(ErrorKind::Io, format!("locking trigger append: {error}"))
            })?;
            files.insert(path.clone(), file);
        }
        Ok(Self { files })
    }
    pub fn write(&mut self, path: &PathBuf, unit: &Unit) -> Result<()> {
        let file = self
            .files
            .get_mut(path)
            .ok_or_else(|| usage("undeclared trigger append"))?;
        if file
            .metadata()
            .map_err(|error| Error::io("checking append length", error))?
            .len()
            == 0
            && let Some(header) = &unit.header
        {
            file.write_all(header)
                .map_err(|error| Error::io("appending trigger header", error))?;
        }
        file.write_all(&unit.raw)
            .and_then(|_| file.sync_all())
            .map_err(|error| Error::io("appending trigger unit", error))
    }
}
impl Drop for Appends {
    fn drop(&mut self) {
        for file in self.files.values() {
            let _ = file.unlock();
        }
    }
}
fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::input::Unit;
    use crate::policy::{Assessment, Probability, Selected, UncertainAction};
    use crate::records::{Format, Options, Source};
    use crate::trigger::{Action, Appends, Declaration, Matcher, Target, execute, plan};
    use crate::wire::Question;
    use std::{io::Cursor, time::Duration};
    use tokio::time::Instant;
    fn unit(text: &str) -> Unit {
        Source::new(
            Cursor::new(text.as_bytes()),
            Options::new(Format::Lines),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap()
    }
    fn yes() -> Assessment {
        Assessment {
            selected: Selected::Boolean(true),
            confidence: Probability::new(0.9).unwrap(),
            uncertain: false,
            gap: None,
        }
    }
    #[test]
    fn selectors_follow_primary_types_uncertainty_and_declaration_order() {
        let question: Question = serde_json::from_str("{\"type\":\"noul\"}").unwrap();
        let declarations = vec![
            Declaration {
                when: "yes".into(),
                target: Target::Exec("true".into()),
            },
            Declaration {
                when: "no".into(),
                target: Target::Exec("false".into()),
            },
        ];
        let actions = plan(&declarations, &question).unwrap();
        assert_eq!(actions[0].id, 1);
        assert_eq!(actions[1].id, 2);
        assert!(actions[0].matches(&yes(), UncertainAction::Drop));
        assert!(!actions[1].matches(&yes(), UncertainAction::Drop));
        let mut assessment = yes();
        assessment.uncertain = true;
        assert!(!actions[0].matches(&assessment, UncertainAction::Drop));
        assert!(actions[0].matches(&assessment, UncertainAction::Keep));
        let uncertain = Action {
            id: 3,
            matcher: Matcher::Uncertain,
            target: Target::Exec("true".into()),
        };
        assert!(uncertain.matches(&assessment, UncertainAction::Drop));
        let score: Question =
            serde_json::from_str("{\"type\":\"score\",\"criteria\":[\"bad\",\"good\"]}").unwrap();
        assert!(plan(&declarations, &score).is_err());
        let reserved: Question = serde_json::from_value(
            serde_json::json!({"type":"choice","criteria":{"uncertain":"Label"}}),
        )
        .unwrap();
        assert!(plan(&declarations, &reserved).is_err());
    }
    #[tokio::test]
    async fn real_exec_receives_original_bytes_environment_and_exit_status_once() {
        let unit = unit("ALPHA\r\n");
        let result=execute("printf '%s:%s:' \"$LOUPE_INDEX\" \"$LOUPE_LABEL\"; cat; printf 'diagnostic' >&2; exit 7",&unit,"yes",&yes(),Instant::now()+Duration::from_secs(2)).await.unwrap();
        assert_eq!(result.code, Some(7));
        assert_eq!(result.stdout.bytes, b"1:yes:ALPHA\r\n");
        assert_eq!(result.stderr.bytes, b"diagnostic");
        assert!(result.failed());
        assert!(!result.timed_out);
    }
    #[tokio::test]
    async fn timeout_and_successful_shell_exit_stop_background_descendants() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("leaked");
        let source = unit("ALPHA\n");
        let command = format!(
            "(sleep 0.3; printf leaked > '{}') & sleep 30",
            marker.display()
        );
        let start = Instant::now();
        let result = execute(
            &command,
            &source,
            "yes",
            &yes(),
            start + Duration::from_millis(50),
        )
        .await
        .unwrap();
        assert!(result.timed_out);
        assert_eq!(result.signal, Some(9));
        assert!(start.elapsed() < Duration::from_secs(2));
        let command = format!(
            "(sleep 0.3; printf leaked > '{}') & exit 0",
            marker.display()
        );
        let result = execute(
            &command,
            &source,
            "yes",
            &yes(),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(result.code, Some(0));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!marker.exists());
    }
    #[tokio::test]
    async fn cancellation_kills_owned_group_and_large_output_is_drained_with_bounded_capture() {
        let root = tempfile::tempdir().unwrap();
        let started = root.path().join("started");
        let leaked = root.path().join("leaked");
        let source = unit("ALPHA\n");
        let command = format!(
            "printf started > '{}'; (sleep 0.3; printf leaked > '{}') & sleep 30",
            started.display(),
            leaked.display()
        );
        let task = tokio::spawn(async move {
            execute(
                &command,
                &source,
                "yes",
                &yes(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !started.exists() {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!leaked.exists());
        let result = execute(
            "head -c 200000 /dev/zero",
            &unit("ALPHA\n"),
            "yes",
            &yes(),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout.total_bytes, 200000);
        assert_eq!(result.stdout.bytes.len(), 65536);
        assert!(result.stdout.truncated);
    }
    #[test]
    fn actual_appends_preserve_crlf_and_refuse_input_aliases() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("events");
        let actions = vec![Action {
            id: 1,
            matcher: Matcher::Yes,
            target: Target::Append(file.clone()),
        }];
        let mut appends = Appends::open(&actions, &[]).unwrap();
        appends.write(&file, &unit("ALPHA\r\n")).unwrap();
        drop(appends);
        assert_eq!(std::fs::read(&file).unwrap(), b"ALPHA\r\n");
        let metadata = std::fs::metadata(&file).unwrap();
        assert!(Appends::open(&actions, &[(metadata.dev(), metadata.ino())]).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"ALPHA\r\n");
    }
    #[test]
    #[ignore = "real detached helper process, launched only by the deadline test"]
    fn detached_output_child() {
        let path = std::env::var_os("LOUPE_ACTION_TEST_PID").unwrap();
        let pid = rustix::process::setsid().unwrap();
        std::fs::write(path, pid.as_raw_nonzero().get().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[tokio::test]
    async fn inherited_output_from_a_detached_child_cannot_block_the_action_deadline() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pid");
        let binary = std::env::current_exe().unwrap();
        let command = format!(
            "LOUPE_ACTION_TEST_PID='{}' '{}' --exact trigger::tests::detached_output_child --ignored --nocapture & while test ! -s '{}'; do sleep 0.01; done; exit 0",
            path.display(),
            binary.display(),
            path.display()
        );
        let started = Instant::now();
        let result = execute(
            &command,
            &unit("ALPHA\n"),
            "yes",
            &yes(),
            started + Duration::from_millis(100),
        )
        .await;
        let pid = std::fs::read_to_string(path)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let pid = rustix::process::Pid::from_raw(pid).unwrap();
        rustix::process::kill_process_group(pid, rustix::process::Signal::KILL).unwrap();
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
