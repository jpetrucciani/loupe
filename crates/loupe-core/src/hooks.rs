//! Trusted hook policy and exclusive, atomic publication of executable hooks.

use crate::error::{Error, ErrorKind, Result};
use crate::policy::{Assessment, Probability, QuestionKind, Selected};
use rustix::fs::{AtFlags, Mode, OFlags};
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub budget: u64,
    pub deadline_seconds: u64,
    pub fail_open: bool,
    pub secret: String,
    pub secret_threshold: f64,
    pub breaking_change: String,
    pub conventional_commit: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            budget: 8192,
            deadline_seconds: 60,
            fail_open: true,
            secret: "secret".into(),
            secret_threshold: 0.9,
            breaking_change: "breaking-change".into(),
            conventional_commit: "conventional-commit".into(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.budget == 0 || self.deadline_seconds == 0 {
            return Err(Error::new(
                ErrorKind::Config,
                "hook budget and deadline_seconds must be positive",
            ));
        }
        Probability::new(self.secret_threshold).map_err(|_| {
            Error::new(ErrorKind::Config, "hook secret_threshold must be in [0, 1]")
        })?;
        for preset in [
            &self.secret,
            &self.breaking_change,
            &self.conventional_commit,
        ] {
            crate::preset_store::valid_name(preset)?;
        }
        Ok(())
    }

    /// Bad input, trust and configuration are never treated as API outages.
    pub fn allows_failure(&self, error: &Error) -> bool {
        self.fail_open && matches!(error.kind, ErrorKind::Unavailable | ErrorKind::Auth)
    }
}

/// Quote one literal shell argument, including quotes and newlines.
pub fn quote(value: &str) -> Result<String> {
    if value.contains('\0') {
        return Err(Error::new(ErrorKind::Usage, "hook argument contains NUL"));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

pub fn read_input(path: &Path, limit: usize) -> Result<String> {
    let file: File = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| Error::io("opening hook input", error.into()))?
    .into();
    let metadata = file
        .metadata()
        .map_err(|error| Error::io("reading hook input metadata", error))?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(Error::new(
            ErrorKind::Input,
            format!("hook input must be a regular file of at most {limit} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io("reading hook input", error))?;
    if bytes.len() > limit {
        return Err(Error::new(
            ErrorKind::Input,
            "hook input grew beyond its byte limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| Error::new(ErrorKind::Input, "hook input must be UTF-8"))
}

pub fn conventional_subject(subject: &str) -> Result<bool> {
    let pattern =
        regex::Regex::new(r"^[a-z][a-z0-9_-]*(?:\([^()\r\n]+\))?!?:\s+\S").map_err(|error| {
            Error::new(
                ErrorKind::Config,
                format!("commit subject pattern: {error}"),
            )
        })?;
    Ok(pattern.is_match(subject))
}

pub fn breaking_marker(subject: &str) -> bool {
    subject
        .split_once(':')
        .is_some_and(|(header, _)| header.ends_with('!'))
}

#[derive(Clone, Copy)]
pub enum Check {
    Secret,
    BreakingChange,
    ConventionalCommit,
}
pub struct Decision {
    pub blocked: bool,
    pub warning: Option<String>,
}
impl Check {
    pub fn id(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::BreakingChange => "breaking-change",
            Self::ConventionalCommit => "conventional-commit",
        }
    }
    pub fn kind(self) -> QuestionKind {
        match self {
            Self::Secret | Self::BreakingChange => QuestionKind::Noul,
            Self::ConventionalCommit => QuestionKind::Choice,
        }
    }
    pub fn decide(
        self,
        assessment: &Assessment,
        subject: &str,
        settings: &Settings,
    ) -> Result<Decision> {
        if assessment.uncertain {
            return Err(Error::new(
                ErrorKind::Uncertain,
                format!("hook {} decision is uncertain", self.id()),
            ));
        }
        let mut decision = Decision {
            blocked: false,
            warning: None,
        };
        match (self, &assessment.selected) {
            (Self::Secret, Selected::Boolean(true)) => {
                decision.blocked = true;
                decision.warning = Some(format!(
                    "staged diff contains a likely secret (p >= {})",
                    settings.secret_threshold
                ));
            }
            (Self::BreakingChange, Selected::Boolean(true)) if !breaking_marker(subject) => {
                decision.warning = Some(
                    "possible breaking change; add ! before the subject's colon if intentional"
                        .into(),
                );
            }
            (Self::ConventionalCommit, Selected::Choice(label))
                if !conventional_subject(subject)? =>
            {
                decision.warning = Some(format!(
                    "suggested conventional type {}",
                    serde_json::to_string(label).map_err(|error| Error::new(
                        ErrorKind::Input,
                        format!("serializing hook label: {error}")
                    ))?
                ));
            }
            (Self::Secret | Self::BreakingChange, Selected::Boolean(_))
            | (Self::ConventionalCommit, Selected::Choice(_)) => {}
            _ => {
                return Err(Error::new(
                    ErrorKind::Input,
                    "hook assessment has the wrong type",
                ));
            }
        }
        Ok(decision)
    }
}

/// Existing hooks are accepted only when byte-identical and already executable.
/// Every conflict is checked before creating any hook. Concurrent creation is
/// still protected by a no-replacement link of the complete, synced script.
pub fn install(directory: &Path, scripts: &[(&str, String)]) -> Result<Vec<String>> {
    let normalized: std::path::PathBuf = directory.components().collect();
    let root: File = rustix::fs::open(
        &normalized,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| Error::io("opening hooks directory", error.into()))?
    .into();
    let mut missing = Vec::new();
    for (name, script) in scripts {
        if !matches!(*name, "pre-commit" | "commit-msg") {
            return Err(Error::new(ErrorKind::Usage, "unsupported hook name"));
        }
        match rustix::fs::openat(
            &root,
            *name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(fd) => {
                let mut file: File = fd.into();
                let metadata = file
                    .metadata()
                    .map_err(|error| Error::io("reading existing hook metadata", error))?;
                let mut bytes = Vec::new();
                if metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= 65536 {
                    file.read_to_end(&mut bytes)
                        .map_err(|error| Error::io("reading existing hook", error))?;
                }
                if bytes != script.as_bytes() || metadata.mode() & 0o111 == 0 {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        format!(
                            "hook conflict: {} already exists; preserve it and integrate loupe hook run {name} manually",
                            normalized.join(name).display()
                        ),
                    ));
                }
            }
            Err(rustix::io::Errno::NOENT) => missing.push((*name, script)),
            Err(error) => {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!("hook conflict: {name} cannot be safely opened: {error}"),
                ));
            }
        }
    }
    let mut installed = Vec::new();
    for (name, script) in missing {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::new(ErrorKind::Io, "hook publication clock is before Unix epoch"))?
            .as_nanos();
        let temporary = format!(".loupe-{name}-{}-{stamp}", std::process::id());
        let mut file: File = rustix::fs::openat(
            &root,
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR | Mode::XUSR,
        )
        .map_err(|error| Error::io("creating staged hook", error.into()))?
        .into();
        let result = (|| {
            file.write_all(script.as_bytes())
                .map_err(|error| Error::io("writing hook", error))?;
            file.sync_all()
                .map_err(|error| Error::io("syncing hook", error))?;
            drop(file);
            rustix::fs::linkat(&root, &temporary, &root, name, AtFlags::empty())
                .map_err(|error| Error::io("publishing hook without replacement", error.into()))?;
            root.sync_all()
                .map_err(|error| Error::io("syncing hooks directory", error))
        })();
        let cleanup = rustix::fs::unlinkat(&root, &temporary, AtFlags::empty());
        result?;
        cleanup.map_err(|error| Error::io("removing staged hook", error.into()))?;
        root.sync_all()
            .map_err(|error| Error::io("syncing hooks directory", error))?;
        installed.push(name.to_owned());
    }
    Ok(installed)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::hooks::{Settings, install, quote};
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn policy_decisions_block_only_secrets_and_preserve_uncertainty() {
        let settings = Settings::default();
        let policy = crate::policy::Policy {
            threshold: crate::policy::Probability::new(0.9).unwrap(),
            ..crate::policy::Policy::default()
        };
        for (p, blocked) in [(0.89, false), (0.9, true), (0.99, true)] {
            let assessment = policy
                .noul(crate::policy::Probability::new(p).unwrap())
                .unwrap();
            assert_eq!(
                crate::hooks::Check::Secret
                    .decide(&assessment, "fix: bug", &settings)
                    .unwrap()
                    .blocked,
                blocked
            );
        }
        let yes = crate::policy::Policy::default()
            .noul(crate::policy::Probability::new(0.99).unwrap())
            .unwrap();
        assert!(
            crate::hooks::Check::BreakingChange
                .decide(&yes, "feat: change", &settings)
                .unwrap()
                .warning
                .is_some()
        );
        assert!(
            crate::hooks::Check::BreakingChange
                .decide(&yes, "feat(api)!: change", &settings)
                .unwrap()
                .warning
                .is_none()
        );
        let mut uncertain = yes.clone();
        uncertain.uncertain = true;
        assert_eq!(
            crate::hooks::Check::Secret
                .decide(&uncertain, "", &settings)
                .err()
                .unwrap()
                .exit_code(),
            3
        );
        let probabilities = std::collections::BTreeMap::from([
            ("fix".into(), crate::policy::Probability::new(0.8).unwrap()),
            ("feat".into(), crate::policy::Probability::new(0.2).unwrap()),
        ]);
        let choice = crate::policy::Policy::default()
            .choice(&probabilities)
            .unwrap();
        assert!(
            crate::hooks::Check::ConventionalCommit
                .decide(&choice, "Plain subject", &settings)
                .unwrap()
                .warning
                .unwrap()
                .contains("fix")
        );
        for subject in ["fix: bug", "feat(api)!: change", "docs: documentation"] {
            assert!(
                crate::hooks::Check::ConventionalCommit
                    .decide(&choice, subject, &settings)
                    .unwrap()
                    .warning
                    .is_none()
            );
        }
        assert!(
            crate::hooks::Check::Secret
                .decide(&choice, "", &settings)
                .is_err()
        );
    }

    #[test]
    fn exclusive_publication_preserves_hooks_and_checks_all_conflicts_first() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = [
            ("pre-commit", "#!/bin/sh\nexit 0\n".to_owned()),
            ("commit-msg", "#!/bin/sh\nexit 1\n".to_owned()),
        ];
        std::fs::write(dir.path().join("commit-msg"), b"user hook").unwrap();
        assert!(install(dir.path(), &scripts).is_err());
        assert!(!dir.path().join("pre-commit").exists());
        assert_eq!(
            std::fs::read(dir.path().join("commit-msg")).unwrap(),
            b"user hook"
        );
        std::fs::remove_file(dir.path().join("commit-msg")).unwrap();
        assert_eq!(install(dir.path(), &scripts).unwrap().len(), 2);
        assert!(install(dir.path(), &scripts).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pre-commit")).unwrap(),
            scripts[0].1
        );
        assert_ne!(
            std::fs::metadata(dir.path().join("pre-commit"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        std::fs::set_permissions(
            dir.path().join("commit-msg"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(install(dir.path(), &scripts).is_err());
    }

    #[test]
    fn symlinks_are_conflicts_and_literal_shell_arguments_stay_literal() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"keep").unwrap();
        symlink(&target, dir.path().join("pre-commit")).unwrap();
        assert!(install(dir.path(), &[("pre-commit", "changed".into())]).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        let literal = "a'\n$(touch should-not-exist) `false`";
        let output = std::process::Command::new("sh")
            .current_dir(dir.path())
            .args(["-c", &format!("printf %s {}", quote(literal).unwrap())])
            .output()
            .unwrap();
        assert_eq!(output.stdout, literal.as_bytes());
        assert!(!dir.path().join("should-not-exist").exists());
        assert!(quote("\0").is_err());
        let policy = Settings::default();
        assert!(policy.allows_failure(&crate::error::Error::new(
            crate::error::ErrorKind::Auth,
            "test"
        )));
        assert!(!policy.allows_failure(&crate::error::Error::new(
            crate::error::ErrorKind::Input,
            "test"
        )));
    }
}
