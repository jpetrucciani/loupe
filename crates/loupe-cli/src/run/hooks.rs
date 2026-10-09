use crate::args::{Cli, Hook, HookKind};
use indexmap::IndexMap;
use loupe_core::backend::{Backend, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::config::Profile;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::hooks::{Check, Settings, quote};
use loupe_core::policy::{Policy, Probability};
use loupe_core::preset_store::{Preset, Store};
use loupe_core::wire::{Question, Request};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::Instant;

const INPUT_LIMIT: usize = 1024 * 1024;

pub async fn execute(
    cli: &Cli,
    command: &Hook,
    store: &Store,
    mut profile: Profile,
    settings: &Settings,
    started: Instant,
) -> Result<u8> {
    settings.validate()?;
    let expires = started
        .checked_add(Duration::from_secs(settings.deadline_seconds))
        .ok_or_else(|| Error::new(ErrorKind::Config, "hook deadline outside clock range"))?;
    match command {
        Hook::Install { directory, hooks } => {
            let selected = if hooks.is_empty() {
                vec![HookKind::PreCommit, HookKind::CommitMsg]
            } else {
                hooks.clone()
            };
            let mut scripts = Vec::new();
            for kind in selected {
                prepare(kind, store, &profile, settings)?;
                if scripts.iter().any(|(name, _)| *name == kind.name()) {
                    continue;
                }
                scripts.push((kind.name(), script(cli, kind)?));
            }
            let directory = match directory {
                Some(path) => path.clone(),
                None => PathBuf::from(
                    git(
                        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
                        expires,
                        65536,
                    )
                    .await?
                    .trim_end_matches('\n'),
                ),
            };
            let installed = loupe_core::hooks::install(&directory, &scripts)?;
            crate::run::write_json(
                &mut io::stdout().lock(),
                &json!({"directory":directory,"installed":installed,"unchanged":scripts.iter().filter(|(name, _)| !installed.iter().any(|created| created == name)).map(|(name, _)| name).collect::<Vec<_>>()}),
            )?;
            Ok(0)
        }
        Hook::Run {
            kind,
            message,
            diff,
            dry_run,
        } => {
            if matches!(kind, HookKind::PreCommit) && message.is_some()
                || matches!(kind, HookKind::CommitMsg) && message.is_none()
            {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "pre-commit takes no message file; commit-msg requires one",
                ));
            }
            // Trust/type checks precede file reads, credential commands and HTTP.
            let prepared = prepare(*kind, store, &profile, settings)?;
            let diff = match diff {
                Some(path) => loupe_core::hooks::read_input(path, INPUT_LIMIT)?,
                None => {
                    git(
                        &[
                            "--no-pager",
                            "diff",
                            "--cached",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-color",
                            "--binary",
                            "--",
                        ],
                        expires,
                        INPUT_LIMIT,
                    )
                    .await?
                }
            };
            let message = message
                .as_deref()
                .map(|path| loupe_core::hooks::read_input(path, INPUT_LIMIT))
                .transpose()?;
            let subject = message
                .as_deref()
                .and_then(|message| message.lines().next())
                .unwrap_or("");
            if diff.is_empty() && matches!(kind, HookKind::PreCommit) {
                return Ok(0);
            }
            let state = match kind {
                HookKind::PreCommit => Value::String(diff),
                HookKind::CommitMsg => json!({"diff":diff,"subject":subject,"message":message}),
            };
            let request = Request {
                model: profile.model.clone(),
                state,
                questions: prepared
                    .iter()
                    .map(|field| (field.check.id().to_owned(), field.question.clone()))
                    .collect(),
            };
            if profile.encoder != "clef-v1" {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "hook --budget guarantees require a verified clef-v1 encoder",
                ));
            }
            let encoder = loupe_core::encoder::load_profile(&profile)?;
            profile.limits.validate_request(&request)?;
            let counts = loupe_core::pack::count(encoder.as_ref(), &request)?;
            counts.preflight(&profile.limits)?;
            if counts.input_tokens as u64 > settings.budget {
                return Err(Error::new(
                    ErrorKind::Budget,
                    format!(
                        "hook needs {} exact input tokens, budget is {}",
                        counts.input_tokens, settings.budget
                    ),
                ));
            }
            if *dry_run {
                crate::run::write_json(
                    &mut io::stdout().lock(),
                    &json!({"hook":kind.name(),"request":request,"counts":counts,"budget":settings.budget,"deadline_seconds":settings.deadline_seconds,"fail_open":settings.fail_open,"presets":prepared.iter().map(|field| json!({"id":field.check.id(),"name":field.preset.name,"content_hash":field.preset.content_hash})).collect::<Vec<_>>()}),
                )?;
                return Ok(0);
            }
            let result = with_deadline(Some(expires), async {
                let backend = Backend::new(&profile).await?;
                let discovery = backend.discover(&profile.limits).await?;
                profile.limits = discovery.limits.clone();
                profile.limits.validate_request(&request)?;
                counts.preflight(&profile.limits)?;
                for field in &prepared {
                    field
                        .preset
                        .check_policy_identity(discovery.full_model_identity.as_deref())?;
                }
                // Hook execution is uncached and performs no local response writes.
                let evaluation = backend
                    .evaluate(
                        &request,
                        &counts,
                        &Budget::new(Some(settings.budget)),
                        true,
                        Some(expires),
                    )
                    .await?;
                let answers = evaluation.response.validate(&request)?;
                let mut blocked = false;
                for field in &prepared {
                    let assessment = answers
                        .get(field.check.id())
                        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "hook answer missing"))?
                        .assessment(&field.policy)?;
                    let decision = field.check.decide(&assessment, subject, settings)?;
                    blocked |= decision.blocked;
                    if let Some(warning) = decision.warning {
                        writeln!(io::stderr().lock(), "loupe hook: {warning}")
                            .map_err(crate::run::output_error)?;
                    }
                }
                Ok(u8::from(blocked))
            })
            .await;
            match result {
                Err(error) if settings.allows_failure(&error) => {
                    writeln!(
                        io::stderr().lock(),
                        "loupe hook: {error}; continuing because hooks.fail_open=true"
                    )
                    .map_err(crate::run::output_error)?;
                    Ok(0)
                }
                result => result,
            }
        }
    }
}

struct Field {
    check: Check,
    preset: Preset,
    question: Question,
    policy: Policy,
}

fn prepare(
    kind: HookKind,
    store: &Store,
    profile: &Profile,
    settings: &Settings,
) -> Result<Vec<Field>> {
    let fields = match kind {
        HookKind::PreCommit => vec![(Check::Secret, &settings.secret)],
        HookKind::CommitMsg => vec![
            (Check::BreakingChange, &settings.breaking_change),
            (Check::ConventionalCommit, &settings.conventional_commit),
        ],
    };
    fields
        .into_iter()
        .map(|(check, name)| {
            let id = check.id();
            let expected = check.kind();
            let preset = store.lookup(name)?;
            preset.require_trusted()?;
            preset.validate(&profile.limits, &profile.model)?;
            if loupe_core::calibration_pin::validate(&preset.document)?.is_some() {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "hooks use a distinct request view; scalar calibration pins cannot be applied",
                ));
            }
            let document = loupe_core::templates::expand(&preset, store, &HashMap::new())?;
            let mut rendered = preset.clone();
            rendered.document = document.clone();
            rendered.validate_rendered(&profile.limits, &profile.model)?;
            let question = loupe_core::presets::question(&document)?;
            if question.kind() != expected {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!("hook {id} needs a {expected:?} preset"),
                ));
            }
            let mut questions = IndexMap::from([(id.to_owned(), question)]);
            if let Some(context) = document.get("context") {
                loupe_core::presets::add_context(&mut questions, context.clone());
            }
            let question = questions
                .shift_remove(id)
                .ok_or_else(|| Error::new(ErrorKind::Usage, "hook question missing"))?;
            let mut policy = Policy::from_document(&document, expected)?;
            if id == "secret" {
                policy.threshold = Probability::new(settings.secret_threshold)?;
            }
            Ok(Field {
                check,
                preset,
                question,
                policy,
            })
        })
        .collect()
}

async fn git(arguments: &[&str], expires: Instant, limit: usize) -> Result<String> {
    let mut command = Command::new("git");
    command.args(arguments).env("GIT_OPTIONAL_LOCKS", "0");
    let outcome = loupe_core::process::execute(command, b"", expires, limit).await?;
    if outcome.timed_out || outcome.code != Some(0) {
        return Err(Error::new(
            ErrorKind::Io,
            "read-only Git hook inspection failed or timed out",
        ));
    }
    if outcome.stdout.truncated {
        return Err(Error::new(
            ErrorKind::Input,
            format!("Git hook input exceeds {limit} bytes"),
        ));
    }
    String::from_utf8(outcome.stdout.bytes)
        .map_err(|_| Error::new(ErrorKind::Input, "Git hook input is not UTF-8"))
}

fn script(cli: &Cli, kind: HookKind) -> Result<String> {
    let executable =
        std::env::current_exe().map_err(|error| Error::io("resolving hook executable", error))?;
    let text = |path: &Path| {
        path.to_str().map(str::to_owned).ok_or_else(|| {
            Error::new(
                ErrorKind::Config,
                "hook executable/config paths must be UTF-8",
            )
        })
    };
    let mut arguments = vec![text(&executable)?];
    if let Some(config) = &cli.config {
        let config = config
            .canonicalize()
            .map_err(|error| Error::io("resolving hook config", error))?;
        arguments.extend(["--config".into(), text(&config)?]);
    }
    for (flag, value) in [
        ("--profile", &cli.profile),
        ("--url", &cli.url),
        ("--path", &cli.path),
        ("--model", &cli.model),
    ] {
        if let Some(value) = value {
            arguments.extend([flag.into(), value.clone()]);
        }
    }
    if let Some(concurrency) = cli.concurrency {
        arguments.extend(["--concurrency".into(), concurrency.to_string()]);
    }
    arguments.extend(["hook".into(), "run".into(), kind.name().into()]);
    let command = arguments
        .iter()
        .map(|argument| quote(argument))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(format!(
        "#!/bin/sh\n# Generated by loupe hook install.\nexec {command} \"$@\"\n"
    ))
}
