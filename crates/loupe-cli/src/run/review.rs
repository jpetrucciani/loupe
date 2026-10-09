use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::policy::{Assessment, Selected};
use loupe_core::review::{Captures, Queue, Task};
use loupe_core::suites::Sampling;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(feature = "review")]
use serde_json::json;

use crate::args::{Common, ReviewMode};
use crate::run::recovery::Registry;
use crate::run::{Completed, Kind, json_error};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    pub task: Option<String>,
    pub sampling: Option<Sampling>,
    pub queued: bool,
    pub label: Option<Selected>,
    pub skipped: bool,
}

pub(crate) fn human(value: Option<&Value>) -> Result<Option<Selected>> {
    value
        .map(|value| serde_json::from_value::<Decision>(value.clone()).map_err(json_error))
        .transpose()
        .map(|value| value.and_then(|value| value.label))
}
pub(crate) fn apply(assessment: &mut Assessment, value: Option<&Value>) -> Result<()> {
    if let Some(label) = human(value)? {
        assessment.selected = label;
        assessment.uncertain = false;
    }
    Ok(())
}
pub(crate) fn rate(options: &Common) -> Result<f64> {
    let text = options.audit_rate.as_deref().unwrap_or("2%");
    let value = if let Some(percent) = text.strip_suffix('%') {
        percent.parse::<f64>().ok().map(|value| value / 100.0)
    } else {
        text.parse().ok()
    };
    value
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                "--audit-rate requires a fraction in [0,1] or a percentage in [0,100%]",
            )
        })
}
pub(crate) fn validate(kind: Kind, options: &Common) -> Result<()> {
    if options.review.is_none() {
        return Ok(());
    }
    if !matches!(
        kind,
        Kind::Is
            | Kind::Keep
            | Kind::Drop
            | Kind::Tag
            | Kind::Rate
            | Kind::Sort
            | Kind::Route
            | Kind::On
    ) || options.dry_run
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "--review applies to is/keep/drop/tag/rate/sort/route/on decisions and requires inference",
        ));
    }
    rate(options)?;
    if matches!(options.review, Some(ReviewMode::Interactive)) {
        check_tty()?;
    }
    Ok(())
}

pub(crate) struct Plan {
    mode: ReviewMode,
    queue: Queue,
    labels_dir: PathBuf,
    labels: BTreeMap<String, Captures>,
    files: Vec<String>,
    preset: String,
    seed: String,
    encoder: String,
    rate: f64,
    #[cfg(feature = "review")]
    tty: Option<Tty>,
    quiet: bool,
    audit: u64,
    uncertain: u64,
    labeled: u64,
    skipped: u64,
}
impl Drop for Plan {
    fn drop(&mut self) {
        #[cfg(feature = "review")]
        drop(self.tty.take());
        if !self.quiet {
            eprintln!(
                "loupe: review uniform-audit={} uncertainty={} labeled={} skipped={} mode={:?}",
                self.audit, self.uncertain, self.labeled, self.skipped, self.mode
            );
        }
    }
}
impl Plan {
    pub(crate) fn new(
        options: &Common,
        root: &Path,
        files: &[String],
        encoder: &str,
        recovery: Option<&Registry>,
        quiet: bool,
    ) -> Result<Option<Self>> {
        let Some(mode) = options.review else {
            return Ok(None);
        };
        let path = options
            .review_queue
            .clone()
            .unwrap_or_else(|| root.join(".loupe/review.jsonl"));
        crate::run::reject_input_output_alias(&path, files)?;
        if let Some(recovery) = recovery {
            recovery.reject_output_alias(&path)?;
        }
        let seed = match recovery {
            Some(recovery) => recovery.review_seed()?,
            None => loupe_core::review::seed()?,
        };
        let preset = options.preset.clone().unwrap_or_else(|| "question".into());
        if !loupe_core::review::name(&preset) {
            return Err(Error::new(
                ErrorKind::Usage,
                "review preset name is not a safe label filename",
            ));
        }
        let queue = Queue::open(&path, true)?;
        #[cfg(feature = "review")]
        let tty = if matches!(mode, ReviewMode::Interactive) {
            Some(Tty::new()?)
        } else {
            None
        };
        Ok(Some(Self {
            mode,
            queue,
            labels_dir: options
                .review_labels
                .clone()
                .unwrap_or_else(|| root.join(".loupe/labels")),
            labels: BTreeMap::new(),
            files: files.to_vec(),
            preset,
            seed,
            encoder: encoder.to_owned(),
            rate: rate(options)?,
            quiet,
            audit: 0,
            uncertain: 0,
            labeled: 0,
            skipped: 0,
            #[cfg(feature = "review")]
            tty,
        }))
    }
    pub(crate) async fn assess(
        &mut self,
        completed: &Completed,
        assessment: &mut Assessment,
    ) -> Result<Value> {
        if let Some(saved) = &completed.review {
            if let Some(label) = human(Some(saved))? {
                let question = completed
                    .request
                    .questions
                    .get(&completed.answer_id)
                    .ok_or_else(|| {
                        Error::new(ErrorKind::Input, "review replay question missing")
                    })?;
                let valid = match (question, &label) {
                    (loupe_core::wire::Question::Noul { .. }, Selected::Boolean(_)) => true,
                    (
                        loupe_core::wire::Question::Choice { criteria, .. },
                        Selected::Choice(value),
                    ) => criteria.contains_key(value),
                    (
                        loupe_core::wire::Question::Score { criteria, .. },
                        Selected::Score(value),
                    ) => {
                        value.is_finite()
                            && *value >= 0.0
                            && value.fract() == 0.0
                            && *value < criteria.len() as f64
                    }
                    _ => false,
                };
                if !valid {
                    return Err(Error::new(
                        ErrorKind::Input,
                        "saved review label is not declared by its question",
                    ));
                }
            }
            apply(assessment, Some(saved))?;
            return Ok(saved.clone());
        }
        let sampling = loupe_core::review::sampling(
            &self.seed,
            completed.unit.index,
            assessment.uncertain,
            self.rate,
        )?;
        let Some(sampling) = sampling else {
            return serde_json::to_value(Decision {
                task: None,
                sampling: None,
                queued: false,
                label: None,
                skipped: false,
            })
            .map_err(json_error);
        };
        match sampling {
            Sampling::UniformAudit => self.audit += 1,
            Sampling::Uncertainty => self.uncertain += 1,
        }
        let response = completed
            .evaluation
            .as_ref()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unavailable,
                    "review requires a completed response",
                )
            })?
            .response
            .clone();
        let task = Task {
            id: String::new(),
            run: self.seed.clone(),
            preset: self.preset.clone(),
            unit: completed.unit.clone(),
            request: completed.request.as_ref().clone(),
            response,
            field: completed.answer_id.clone(),
            encoder_identity: self.encoder.clone(),
            sampling,
        }
        .seal()?;
        self.queue.insert(task.clone())?;
        if matches!(self.mode, ReviewMode::Defer) {
            return serde_json::to_value(Decision {
                task: Some(task.id),
                sampling: Some(sampling),
                queued: true,
                label: None,
                skipped: false,
            })
            .map_err(json_error);
        }
        if !self.labels.contains_key(&task.preset) {
            let path = self.labels_dir.join(format!("{}.jsonl", task.preset));
            crate::run::reject_input_output_alias(&path, &self.files)?;
            self.labels
                .insert(task.preset.clone(), Captures::open(&path)?);
        }
        let captures = self
            .labels
            .get_mut(&task.preset)
            .ok_or_else(|| Error::new(ErrorKind::Io, "review captures unavailable"))?;
        let durable = match self
            .queue
            .get(&task.id)
            .and_then(|pending| pending.decision.clone())
        {
            Some(label) => Some(label),
            None => captures.existing(&task)?.map(Some),
        };
        let label = match durable {
            Some(label) => label,
            None => {
                #[cfg(feature = "review")]
                {
                    self.tty
                        .as_mut()
                        .ok_or_else(|| Error::new(ErrorKind::Usage, "review terminal unavailable"))?
                        .choose(&task)
                        .await?
                }
                #[cfg(not(feature = "review"))]
                {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "interactive review requires --features review",
                    ));
                }
            }
        };
        if let Some(label) = &label {
            captures.insert(&task, label.clone())?;
            self.labeled += 1;
        } else {
            self.skipped += 1;
        }
        self.queue.decide(&task.id, label.clone())?;
        self.queue.handled(&task.id)?;
        if let Some(label) = &label {
            assessment.selected = label.clone();
            assessment.uncertain = false;
        }
        serde_json::to_value(Decision {
            task: Some(task.id),
            sampling: Some(sampling),
            queued: false,
            skipped: label.is_none(),
            label,
        })
        .map_err(json_error)
    }
}

pub(crate) async fn consume(path: &Path, labels_dir: &Path, quiet: bool) -> Result<u8> {
    check_tty()?;
    #[cfg(feature = "review")]
    {
        use std::io::Write;
        let interrupt = crate::run::interrupt_signal();
        let mut tty = Tty::new()?;
        let mut queue = Queue::open(path, false)?;
        let pending = queue.pending().cloned().collect::<Vec<_>>();
        let mut captures = BTreeMap::<String, Captures>::new();
        let mut emitted = 0u64;
        let operation = async {
            for pending in pending {
                let task = pending.task;
                if !captures.contains_key(&task.preset) {
                    let target = labels_dir.join(format!("{}.jsonl", task.preset));
                    if target == path {
                        return Err(Error::new(
                            ErrorKind::Usage,
                            "capture output aliases the review queue",
                        ));
                    }
                    captures.insert(task.preset.clone(), Captures::open(&target)?);
                }
                let labels = captures
                    .get_mut(&task.preset)
                    .ok_or_else(|| Error::new(ErrorKind::Io, "review captures unavailable"))?;
                let durable = match pending.decision {
                    Some(decision) => Some(decision),
                    None => labels.existing(&task)?.map(Some),
                };
                let label = match durable {
                    Some(value) => value,
                    None => tty.choose(&task).await?,
                };
                if let Some(label) = &label {
                    labels.insert(&task, label.clone())?;
                }
                queue.decide(&task.id, label.clone())?;
                if let Some(label) = label {
                    let value = json!({"index":task.unit.index,"unit":task.unit.subject,"answer":task.response.answers.get(&task.field),"model":task.response.model,"cached":false,"decision":{"selected":label,"source":"review","confidence":null,"uncertain":false},"review":{"task":task.id,"run":task.run,"sampling":task.sampling},"location":task.unit.location});
                    let mut stdout = std::io::stdout().lock();
                    crate::run::write_json(&mut stdout, &value)?;
                    stdout
                        .flush()
                        .map_err(|error| Error::io("flushing reviewed output", error))?;
                    emitted += 1;
                }
                queue.handled(&task.id)?;
            }
            if !quiet {
                eprintln!(
                    "loupe: reviewed corrections={emitted}; captured labels remain outside held-out test"
                );
            }
            Ok(0)
        };
        tokio::select! {result=operation=>result,signal=interrupt=>signal}
    }
    #[cfg(not(feature = "review"))]
    {
        let _ = (path, labels_dir, quiet);
        Err(Error::new(
            ErrorKind::Usage,
            "interactive review requires --features review",
        ))
    }
}

fn check_tty() -> Result<()> {
    #[cfg(feature = "review")]
    {
        use std::io::IsTerminal;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|_| {
                Error::new(
                    ErrorKind::Usage,
                    "interactive review requires /dev/tty; use --review=defer",
                )
            })?;
        if !tty.is_terminal() {
            return Err(Error::new(
                ErrorKind::Usage,
                "review input is not a terminal",
            ));
        }
        Ok(())
    }
    #[cfg(not(feature = "review"))]
    {
        Err(Error::new(
            ErrorKind::Usage,
            "interactive review requires a build with --features review",
        ))
    }
}

#[cfg(feature = "review")]
struct Restore {
    output: std::fs::File,
    enabled: bool,
}
#[cfg(feature = "review")]
impl Drop for Restore {
    fn drop(&mut self) {
        if self.enabled {
            let _ = ratatui::crossterm::terminal::disable_raw_mode();
            let _ = ratatui::crossterm::execute!(
                self.output,
                ratatui::crossterm::cursor::Show,
                ratatui::crossterm::terminal::LeaveAlternateScreen
            );
        }
    }
}
#[cfg(feature = "review")]
struct Tty {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::fs::File>>,
    input: loupe_core::stdio::Stream,
    _restore: Restore,
}
#[cfg(feature = "review")]
impl Tty {
    fn new() -> Result<Self> {
        check_tty()?;
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|error| Error::io("opening review display", error))?;
        let mut restore = Restore {
            output: output
                .try_clone()
                .map_err(|error| Error::io("holding review terminal", error))?,
            enabled: false,
        };
        ratatui::crossterm::terminal::enable_raw_mode()
            .map_err(|error| Error::io("setting terminal raw mode", error))?;
        restore.enabled = true;
        ratatui::crossterm::execute!(
            restore.output,
            ratatui::crossterm::terminal::EnterAlternateScreen,
            ratatui::crossterm::cursor::Hide
        )
        .map_err(|error| Error::io("opening review screen", error))?;
        let input = std::fs::File::open("/dev/tty")
            .map_err(|error| Error::io("opening review keys", error))?;
        let input = loupe_core::stdio::Stream::new(input)
            .map_err(|error| Error::io("watching review keys", error))?;
        let terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(output))
            .map_err(|error| Error::io("creating review display", error))?;
        Ok(Self {
            terminal,
            input,
            _restore: restore,
        })
    }
    async fn choose(&mut self, task: &Task) -> Result<Option<Selected>> {
        use ratatui::layout::{Constraint, Layout};
        use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
        use tokio::io::AsyncReadExt;
        let question = task.question()?;
        let options = question.options();
        let mut position = 0usize;
        loop {
            let mut state = ListState::default().with_selected(Some(position));
            self.terminal
                .draw(|frame| {
                    let area = Layout::vertical([
                        Constraint::Length(5),
                        Constraint::Min(5),
                        Constraint::Length(12),
                        Constraint::Length(2),
                    ])
                    .split(frame.area());
                    let instructions = question
                        .instructions()
                        .map_or_else(|| task.field.clone(), Value::to_string);
                    frame.render_widget(
                        Paragraph::new(clean(
                            &format!(
                                "{}: unit {} ({:?})\n{}",
                                task.preset, task.unit.index, task.sampling, instructions
                            ),
                            4096,
                        ))
                        .wrap(Wrap { trim: false }),
                        area[0],
                    );
                    frame.render_widget(
                        Paragraph::new(clean(&task.unit.text, 8192))
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Original unit"),
                            )
                            .wrap(Wrap { trim: false }),
                        area[1],
                    );
                    let answer = &task.response.answers[&task.field];
                    let items = options
                        .iter()
                        .enumerate()
                        .map(|(index, (key, description))| {
                            let probability = if let Some(p) = answer["noul"].as_f64() {
                                Some(if key == "true" { p } else { 1.0 - p })
                            } else {
                                answer["probabilities"][key].as_f64()
                            };
                            ListItem::new(clean(
                                &format!(
                                    "{index}: {key}  p={}  {}",
                                    probability
                                        .map_or_else(|| "?".into(), |value| format!("{value:.4}")),
                                    description
                                ),
                                1024,
                            ))
                        })
                        .collect::<Vec<_>>();
                    frame.render_stateful_widget(
                        List::new(items)
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Human label, raw model probabilities"),
                            )
                            .highlight_symbol("> "),
                        area[2],
                        &mut state,
                    );
                    frame.render_widget(
                        Paragraph::new("Arrows + Enter, 0-9, y/n (Noul), s skip, q/Ctrl-C quit"),
                        area[3],
                    );
                })
                .map_err(|error| Error::io("drawing review", error))?;
            let byte = self
                .input
                .read_u8()
                .await
                .map_err(|error| Error::io("reading review key", error))?;
            match byte {
                b'q' | 3 => {
                    return Err(Error::new(
                        ErrorKind::Interrupted,
                        "review cancelled; pending tasks remain in the queue",
                    ));
                }
                b's' => return Ok(None),
                b'y' if matches!(question, loupe_core::wire::Question::Noul { .. }) => {
                    return Ok(Some(Selected::Boolean(true)));
                }
                b'n' if matches!(question, loupe_core::wire::Question::Noul { .. }) => {
                    return Ok(Some(Selected::Boolean(false)));
                }
                27 => {
                    let mut arrow = [0u8; 2];
                    if matches!(
                        tokio::time::timeout(
                            std::time::Duration::from_millis(50),
                            self.input.read_exact(&mut arrow)
                        )
                        .await,
                        Ok(Ok(_))
                    ) {
                        if arrow == *b"[A" {
                            position = position.saturating_sub(1);
                        } else if arrow == *b"[B" {
                            position = position
                                .saturating_add(1)
                                .min(options.len().saturating_sub(1));
                        }
                    }
                    continue;
                }
                b'0'..=b'9' => {
                    position = usize::from(byte - b'0');
                    if position >= options.len() {
                        continue;
                    }
                }
                b'\r' | b'\n' => {}
                _ => continue,
            }
            let (key, _) = options
                .get(position)
                .ok_or_else(|| Error::new(ErrorKind::Input, "review option is missing"))?;
            return Ok(Some(match question {
                loupe_core::wire::Question::Noul { .. } => Selected::Boolean(key == "true"),
                loupe_core::wire::Question::Choice { .. } => Selected::Choice(key.clone()),
                loupe_core::wire::Question::Score { .. } => {
                    Selected::Score(f64::from(key.parse::<u32>().map_err(|_| {
                        Error::new(ErrorKind::Input, "review score level is invalid")
                    })?))
                }
            }));
        }
    }
}
#[cfg(feature = "review")]
fn clean(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .map(|value| {
            if value.is_control() && !matches!(value, '\n' | '\t') {
                '�'
            } else {
                value
            }
        })
        .collect()
}
