use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{Limits, xdg_path};
use crate::error::{Error, ErrorKind, Result};
use crate::policy::Policy;
use crate::presets::{self, BUILTINS};
use crate::wire::Request;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "lowercase")]
pub enum Origin {
    Builtin,
    Project(PathBuf),
    Xdg(PathBuf),
    External(PathBuf),
    Explicit(PathBuf),
}

impl Origin {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Builtin => None,
            Self::Project(path) | Self::Xdg(path) | Self::External(path) | Self::Explicit(path) => {
                Some(path)
            }
        }
    }
    pub fn project(&self) -> bool {
        matches!(self, Self::Project(_))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Preset {
    pub name: String,
    pub document: Value,
    pub origin: Origin,
    pub trusted: bool,
    pub content_hash: String,
}

impl Preset {
    pub fn policy_identity(&self) -> Option<&str> {
        self.document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .and_then(Value::as_str)
    }

    pub fn check_policy_identity(&self, current: Option<&str>) -> Result<()> {
        if let Some(expected) = self.policy_identity() {
            let expected = crate::identity::Model::parse(expected)
                .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
            match current {
                Some(current) => {
                    if crate::identity::Model::parse(current)? != expected {
                        return Err(Error::new(
                            ErrorKind::Usage,
                            "preset policy belongs to a different model identity; recalibrate for this model",
                        ));
                    }
                }
                None => {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        "preset policy identity cannot be verified: fresh authoritative full model identity is unavailable",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn require_trusted(&self) -> Result<()> {
        if self.trusted {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!(
                    "preset {} is untrusted or has changed; inspect it with loupe presets show, then run loupe presets trust",
                    self.name
                ),
            ))
        }
    }

    pub fn validate(&self, limits: &Limits, model: &str) -> Result<()> {
        crate::calibration_pin::validate(&self.document)?;
        self.validate_rendered(limits, model)
    }

    /// Validate expanded questions after the original preset has been checked.
    pub fn validate_rendered(&self, limits: &Limits, model: &str) -> Result<()> {
        let object = self
            .document
            .as_object()
            .ok_or_else(|| Error::new(ErrorKind::Usage, "preset must be an object"))?;
        if self.origin.project()
            && (object.contains_key("exec")
                || object.contains_key("fallback")
                || object.get("policy").is_some_and(|policy| {
                    policy.get("exec").is_some() || policy.get("fallback").is_some()
                }))
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "project presets cannot declare exec or fallback commands, even when trusted",
            ));
        }
        if let Some(name) = object.get("name")
            && name.as_str() != Some(self.name.as_str())
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "preset name must match its filename or built-in name",
            ));
        }
        if let Some(version) = object.get("version")
            && version.as_u64().is_none_or(|version| version == 0)
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "preset version must be a positive integer",
            ));
        }
        let mut questions = if let Some(questions) = object.get("questions") {
            serde_json::from_value(questions.clone()).map_err(|error| {
                Error::new(ErrorKind::Usage, format!("preset questions: {error}"))
            })?
        } else {
            indexmap::IndexMap::from([(self.name.clone(), presets::question(&self.document)?)])
        };
        limits.validate_schema(&Request {
            model: model.to_owned(),
            state: Value::Null,
            questions: questions.clone(),
        })?;
        for (id, question) in &questions {
            Policy::from_field(&self.document, id, question.kind())?;
        }
        if let Some(fields) = object.get("policy").and_then(|policy| policy.get("fields")) {
            let fields = fields
                .as_object()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "policy.fields must be a table"))?;
            if fields.keys().any(|id| !questions.contains_key(id)) {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "field policy names must be declared question IDs",
                ));
            }
        }
        crate::factors::Set::from_document(&self.document)?.append(&mut questions)?;
        crate::ensemble::Set::from_document(&self.document, &questions)?;
        crate::ensemble::minimum(&self.document, None)?;
        let signals = crate::signals::Set::from_document(&self.document)?;
        signals.append(&mut questions)?;
        limits.validate_schema(&Request {
            model: model.to_owned(),
            state: Value::Null,
            questions: questions.clone(),
        })?;
        crate::records::Options::from_document(&self.document, crate::records::Format::Lines)?
            .validate()?;
        if let Some(policy) = object.get("policy") {
            let policy = policy
                .as_object()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "preset policy must be a table"))?;
            if let Some(pack) = policy.get("pack") {
                pack.as_str()
                    .ok_or_else(|| {
                        Error::new(ErrorKind::Usage, "preset packing mode must be a string")
                    })?
                    .parse::<crate::pack::Mode>()?;
            }
            if let Some(identity) = policy.get("model_identity") {
                let identity = identity.as_str().ok_or_else(|| {
                    Error::new(ErrorKind::Usage, "model_identity must be a string")
                })?;
                crate::identity::Model::parse(identity)
                    .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
            }
            if let Some(combine) = policy.get("combine") {
                crate::combine::resolve(&questions, Some(combine), None)?;
            }
            if policy
                .get("splittable")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(Error::new(ErrorKind::Usage, "splittable must be boolean"));
            }
            if policy.get("splittable") == Some(&Value::Bool(true))
                && policy.get("combine").is_none()
            {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "splittable presets require a combine mode",
                ));
            }
            if let Some(action) = policy.get("on_uncertain")
                && !matches!(
                    action.as_str(),
                    Some("keep" | "drop" | "fail" | "stderr" | "fallback")
                )
            {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "invalid preset uncertainty action",
                ));
            }
        }
        if let Some(permissions) = object.get("permissions") {
            let permissions = permissions.as_object().ok_or_else(|| {
                Error::new(ErrorKind::Usage, "preset permissions must be a table")
            })?;
            if permissions.keys().any(|key| key != "env") {
                return Err(Error::new(ErrorKind::Usage, "unknown preset permission"));
            }
            if let Some(env) = permissions.get("env") {
                let values = env.as_array().ok_or_else(|| {
                    Error::new(ErrorKind::Usage, "permissions.env must be a list")
                })?;
                if values
                    .iter()
                    .any(|value| value.as_str().is_none_or(str::is_empty))
                {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "env permissions must be nonempty variable names",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Grants {
    version: u32,
    grants: BTreeMap<PathBuf, String>,
}

pub struct Store {
    pub project_root: PathBuf,
    pub config_dir: PathBuf,
    cwd: PathBuf,
    directories: Vec<(PathBuf, bool, bool)>,
    env: HashMap<String, String>,
}

impl Store {
    pub fn new(cwd: &Path, env: &HashMap<String, String>) -> Result<Self> {
        let cwd = cwd
            .canonicalize()
            .map_err(|error| Error::io("resolving working directory", error))?;
        let project_root = cwd
            .ancestors()
            .find(|path| path.join(".git").exists())
            .unwrap_or(&cwd)
            .to_owned();
        let config_dir = xdg_path(env, "XDG_CONFIG_HOME", ".config")?;
        let mut directories = Vec::new();
        for path in cwd.ancestors() {
            directories.push((path.join(".loupe/presets"), true, false));
            if path == project_root {
                break;
            }
        }
        directories.push((config_dir.join("presets"), false, true));
        if let Some(paths) = env.get("LOUPE_PRESET_PATH") {
            for path in std::env::split_paths(paths) {
                directories.push((path, false, false));
            }
        }
        Ok(Self {
            project_root,
            config_dir,
            cwd,
            directories,
            env: env.clone(),
        })
    }

    pub fn environment(&self) -> &HashMap<String, String> {
        &self.env
    }

    pub fn names(&self) -> Result<Vec<String>> {
        let mut names: BTreeSet<_> = BUILTINS.iter().map(|name| (*name).to_owned()).collect();
        for (directory, _, _) in &self.directories {
            let entries = match std::fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::io("listing presets", error)),
            };
            for entry in entries {
                let path = entry
                    .map_err(|error| Error::io("listing preset entry", error))?
                    .path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "toml" || extension == "json")
                {
                    let name =
                        path.file_stem()
                            .and_then(|name| name.to_str())
                            .ok_or_else(|| {
                                Error::new(ErrorKind::Usage, "preset filename is not UTF-8")
                            })?;
                    valid_name(name)?;
                    names.insert(name.to_owned());
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    pub fn resolve_origin(&self, name: &str) -> Result<Origin> {
        valid_name(name)?;
        for (directory, project, trusted) in &self.directories {
            let matches: Vec<_> = ["toml", "json"]
                .into_iter()
                .map(|extension| directory.join(format!("{name}.{extension}")))
                .filter(|path| path.exists())
                .collect();
            if matches.len() > 1 {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!(
                        "ambiguous preset {name}: TOML and JSON coexist in {}",
                        directory.display()
                    ),
                ));
            }
            if let Some(path) = matches.first() {
                let path = path
                    .canonicalize()
                    .map_err(|error| Error::io("resolving preset", error))?;
                if *project && !path.starts_with(&self.project_root) {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "project preset resolves outside the project root",
                    ));
                }
                return Ok(if *project {
                    Origin::Project(path)
                } else if *trusted {
                    Origin::Xdg(path)
                } else {
                    Origin::External(path)
                });
            }
        }
        if BUILTINS.contains(&name) {
            Ok(Origin::Builtin)
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!("unknown preset {name}"),
            ))
        }
    }

    pub fn lookup(&self, name: &str) -> Result<Preset> {
        self.load(name, self.resolve_origin(name)?)
    }

    pub fn explicit(&self, path: &Path) -> Result<Preset> {
        let logical = if path.is_absolute() {
            path.to_owned()
        } else {
            self.cwd.join(path)
        };
        let mut normalized = PathBuf::new();
        for component in logical.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                component => normalized.push(component.as_os_str()),
            }
        }
        let canonical = logical
            .canonicalize()
            .map_err(|error| Error::io("resolving question file", error))?;
        let is_project = |path: &Path| {
            path.strip_prefix(&self.project_root)
                .is_ok_and(|path| path.components().any(|part| part.as_os_str() == ".loupe"))
        };
        let project = is_project(&normalized) || is_project(&canonical);
        if project && !canonical.starts_with(&self.project_root) {
            return Err(Error::new(
                ErrorKind::Usage,
                "project question file resolves outside the project root",
            ));
        }
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::new(ErrorKind::Usage, "question filename is not UTF-8"))?;
        self.load(
            name,
            if project {
                Origin::Project(canonical)
            } else {
                Origin::Explicit(canonical)
            },
        )
    }

    fn load(&self, name: &str, origin: Origin) -> Result<Preset> {
        let (document, content_hash) = if let Some(path) = origin.path() {
            let bytes = std::fs::read(path).map_err(|error| Error::io("reading preset", error))?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            (presets::parse_document(path, &bytes)?, hash)
        } else {
            let mut document = presets::builtin(name)?;
            document["name"] = json!(name);
            document["version"] = json!(1);
            let bytes = serde_json::to_vec(&document)
                .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
            (document, blake3::hash(&bytes).to_hex().to_string())
        };
        let trusted = if matches!(
            origin,
            Origin::Builtin | Origin::Xdg(_) | Origin::Explicit(_)
        ) {
            true
        } else if let Some(path) = origin.path() {
            self.granted(path, &content_hash)?
        } else {
            false
        };
        Ok(Preset {
            name: name.to_owned(),
            document,
            origin,
            trusted,
            content_hash,
        })
    }

    fn grants(&self) -> Result<Grants> {
        let path = self.config_dir.join("trust.json");
        match std::fs::read(path) {
            Ok(bytes) => {
                let grants: Grants = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::new(ErrorKind::Config, "invalid preset trust registry"))?;
                if grants.version != 1 {
                    return Err(Error::new(
                        ErrorKind::Config,
                        "unsupported trust registry version",
                    ));
                }
                Ok(grants)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Grants {
                version: 1,
                ..Grants::default()
            }),
            Err(error) => Err(Error::io("reading preset trust registry", error)),
        }
    }
    fn granted(&self, path: &Path, hash: &str) -> Result<bool> {
        Ok(self
            .grants()?
            .grants
            .get(path)
            .is_some_and(|approved| approved == hash))
    }

    pub fn trust(&self, preset: &Preset) -> Result<()> {
        let path = preset
            .origin
            .path()
            .ok_or_else(|| Error::new(ErrorKind::Usage, "built-in presets already trusted"))?;
        let current =
            std::fs::read(path).map_err(|error| Error::io("reading preset before trust", error))?;
        if blake3::hash(&current).to_hex().as_str() != preset.content_hash {
            return Err(Error::new(
                ErrorKind::Usage,
                "preset changed while granting trust; inspect its new content",
            ));
        }
        std::fs::create_dir_all(&self.config_dir)
            .map_err(|error| Error::io("creating trust directory", error))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.config_dir.join("trust.lock"))
            .map_err(|error| Error::io("opening trust lock", error))?;
        lock.lock()
            .map_err(|error| Error::io("locking trust registry", error))?;
        let mut grants = self.grants()?;
        grants
            .grants
            .insert(path.to_owned(), preset.content_hash.clone());
        let bytes = serde_json::to_vec_pretty(&grants)
            .map_err(|error| Error::new(ErrorKind::Config, error.to_string()))?;
        atomic_write(&self.config_dir.join("trust.json"), &bytes)
    }

    pub fn create(&self, name: &str, question_type: &str, project: bool) -> Result<PathBuf> {
        valid_name(name)?;
        let directory = if project {
            self.project_root.join(".loupe/presets")
        } else {
            self.config_dir.join("presets")
        };
        if project {
            confined_parent(&self.project_root, &directory)?;
        }
        std::fs::create_dir_all(&directory)
            .map_err(|error| Error::io("creating preset directory", error))?;
        if project
            && !directory
                .canonicalize()
                .map_err(|error| Error::io("resolving preset directory", error))?
                .starts_with(&self.project_root)
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "preset directory escapes project root",
            ));
        }
        let mut text = format!(
            "name = \"{name}\"\nversion = 1\ntype = \"{question_type}\"\ninstructions = \"Write a specific question here\"\n"
        );
        match question_type {
            "noul" => {}
            "choice" => {
                text.push_str("\n[criteria]\nyes = \"First option\"\nno = \"Second option\"\n")
            }
            "score" => text.push_str("criteria = [\"Low\", \"High\"]\n"),
            _ => return Err(Error::new(ErrorKind::Usage, "unknown preset question type")),
        }
        let path = directory.join(format!("{name}.toml"));
        if directory.join(format!("{name}.json")).exists() {
            return Err(Error::new(
                ErrorKind::Usage,
                "a JSON preset with this name already exists",
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::new(ErrorKind::Usage, "preset already exists")
                } else {
                    Error::io("creating preset", error)
                }
            })?;
        file.write_all(text.as_bytes())
            .map_err(|error| Error::io("writing preset", error))?;
        file.sync_all()
            .map_err(|error| Error::io("flushing preset", error))?;
        Ok(path)
    }
}

pub fn valid_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Err(Error::new(
            ErrorKind::Usage,
            "preset names must use letters, digits, dots, underscores or hyphens, without a leading dot",
        ))
    } else {
        Ok(())
    }
}

fn confined_parent(root: &Path, path: &Path) -> Result<()> {
    let existing = path
        .ancestors()
        .find(|path| path.exists())
        .ok_or_else(|| Error::new(ErrorKind::Usage, "preset path has no existing parent"))?;
    if !existing
        .canonicalize()
        .map_err(|error| Error::io("resolving preset parent", error))?
        .starts_with(root)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "preset parent escapes project root",
        ));
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::Io, "output path has no parent"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::new(ErrorKind::Io, "system clock is before epoch"))?
        .as_nanos();
    let temporary = parent.join(format!(".loupe-write-{}-{nonce}", std::process::id()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| Error::io("creating atomic output", error))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| Error::io("writing atomic output", error))?;
        file.sync_all()
            .map_err(|error| Error::io("flushing atomic output", error))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| Error::io("committing atomic output", error))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| Error::io("flushing output directory", error))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::config::Limits;
    use crate::preset_store::{Origin, Store};
    use serde_json::json;
    use std::collections::HashMap;

    fn setup() -> (tempfile::TempDir, Store, HashMap<String, String>) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join(".git")).unwrap();
        let env = HashMap::from([(
            "XDG_CONFIG_HOME".to_owned(),
            directory.path().join("xdg").to_str().unwrap().to_owned(),
        )]);
        let store = Store::new(directory.path(), &env).unwrap();
        (directory, store, env)
    }

    #[test]
    fn lookup_precedence_trust_snapshots_and_changed_hashes_use_real_files() {
        let (directory, store, env) = setup();
        assert!(matches!(
            store.lookup("secret").unwrap().origin,
            Origin::Builtin
        ));
        let xdg = store.create("secret", "noul", false).unwrap();
        assert!(matches!(
            store.lookup("secret").unwrap().origin,
            Origin::Xdg(_)
        ));
        let project = store.create("secret", "noul", true).unwrap();
        let untrusted = store.lookup("secret").unwrap();
        assert!(!untrusted.trusted);
        assert_eq!(untrusted.require_trusted().unwrap_err().exit_code(), 64);
        store.trust(&untrusted).unwrap();
        let fresh = Store::new(directory.path(), &env).unwrap();
        assert!(fresh.lookup("secret").unwrap().trusted);
        std::fs::write(
            &project,
            "name='secret'\nversion=2\ntype='noul'\ninstructions='New question'\n",
        )
        .unwrap();
        assert!(!fresh.lookup("secret").unwrap().trusted);
        assert!(fresh.trust(&untrusted).is_err());
        let changed = fresh.lookup("secret").unwrap();
        fresh.trust(&changed).unwrap();
        assert!(fresh.explicit(&project).unwrap().trusted);
        std::fs::remove_file(project).unwrap();
        assert!(fresh.lookup("secret").unwrap().trusted);
        assert_eq!(fresh.lookup("secret").unwrap().origin.path().unwrap(), xdg);
    }

    #[test]
    fn explicit_paths_resolve_from_cwd_and_keep_project_grants() {
        let (directory, store, env) = setup();
        store.create("rubric", "noul", true).unwrap();
        let nested = directory.path().join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();
        let nested_store = Store::new(&nested, &env).unwrap();
        let preset = nested_store
            .explicit(std::path::Path::new("../../.loupe/presets/rubric.toml"))
            .unwrap();
        assert!(preset.origin.project());
        assert!(!preset.trusted);
        assert_eq!(
            nested_store.lookup("rubric").unwrap().content_hash,
            preset.content_hash
        );
    }

    #[test]
    fn ambiguity_bad_grants_and_project_exec_fail_explicitly() {
        let (_directory, store, _env) = setup();
        let path = store.create("rubric", "noul", true).unwrap();
        std::fs::write(path.with_extension("json"), "{\"type\":\"noul\"}").unwrap();
        assert_eq!(store.lookup("rubric").unwrap_err().exit_code(), 64);
        std::fs::remove_file(path.with_extension("json")).unwrap();
        std::fs::create_dir_all(&store.config_dir).unwrap();
        std::fs::write(store.config_dir.join("trust.json"), "broken").unwrap();
        assert_eq!(store.lookup("rubric").unwrap_err().exit_code(), 78);
        std::fs::remove_file(store.config_dir.join("trust.json")).unwrap();
        let mut preset = store.lookup("rubric").unwrap();
        preset.trusted = true;
        preset.document["exec"] = json!("touch forbidden");
        assert_eq!(
            preset
                .validate(&Limits::default(), "clef-flash")
                .unwrap_err()
                .exit_code(),
            64
        );
    }

    #[test]
    fn policies_input_shape_and_question_limits_are_checked() {
        let (_directory, store, _env) = setup();
        let mut preset = store.lookup("secret").unwrap();
        let full_identity = format!(
            "clef@{};quantization=nf4;compute=bf16;server={}",
            "a".repeat(40),
            "b".repeat(64)
        );
        preset.document["policy"] = json!({"model_identity":full_identity});
        preset.validate(&Limits::default(), "clef").unwrap();
        assert_eq!(
            preset.check_policy_identity(None).unwrap_err().exit_code(),
            69
        );
        assert_eq!(
            preset
                .check_policy_identity(Some(&full_identity.replace("nf4", "none")))
                .unwrap_err()
                .exit_code(),
            64
        );
        preset
            .check_policy_identity(preset.policy_identity())
            .unwrap();
        let normalized = crate::identity::Model::parse(&full_identity).unwrap();
        preset
            .check_policy_identity(Some(normalized.as_str()))
            .unwrap();
        for policy in [
            json!({"min_gap":0.1}),
            json!({"margin":"0.2"}),
            json!({"splittable":true}),
            json!({"combine":"vote"}),
            json!({"model_identity":"clef"}),
            json!({"model_identity":"clef@abc;quantization=nf4;compute=bf16;server=build"}),
        ] {
            preset.document["policy"] = policy;
            assert!(preset.validate(&Limits::default(), "clef").is_err());
        }
        preset.document["policy"] = json!({"threshold":0.8,"margin":0.1});
        preset.document["input"] = json!({"mode":"records","records":"["});
        assert!(preset.validate(&Limits::default(), "clef").is_err());
        preset.document["input"] = json!({"mode":"jsonl","field":"/a~2"});
        assert!(preset.validate(&Limits::default(), "clef").is_err());
        preset.document["input"] = json!({"mode":"lines","typo":true});
        assert!(preset.validate(&Limits::default(), "clef").is_err());
        let preset = store.lookup("severity").unwrap();
        let limits = Limits {
            max_options: 2,
            ..Limits::default()
        };
        assert!(preset.validate(&limits, "clef").is_err());
    }

    #[test]
    fn create_never_overwrites_and_locked_grants_preserve_concurrent_updates() {
        let (directory, store, env) = setup();
        for kind in ["noul", "choice", "score"] {
            let path = store.create(kind, kind, true).unwrap();
            store
                .lookup(kind)
                .unwrap()
                .validate(&Limits::default(), "clef")
                .unwrap();
            assert!(store.create(kind, kind, true).is_err());
            assert!(path.exists());
        }
        let threads: Vec<_> = ["noul", "choice", "score"]
            .into_iter()
            .map(|name| {
                let root = directory.path().to_owned();
                let env = env.clone();
                std::thread::spawn(move || {
                    let store = Store::new(&root, &env).unwrap();
                    let preset = store.lookup(name).unwrap();
                    store.trust(&preset).unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        for name in ["noul", "choice", "score"] {
            assert!(store.lookup(name).unwrap().trusted);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(store.config_dir.join("trust.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn project_symlinks_cannot_escape_for_lookup_explicit_or_creation() {
        let (directory, store, _env) = setup();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("escape.toml");
        std::fs::write(&path, "type='noul'").unwrap();
        std::fs::create_dir_all(directory.path().join(".loupe/presets")).unwrap();
        std::os::unix::fs::symlink(&path, directory.path().join(".loupe/presets/escape.toml"))
            .unwrap();
        assert!(store.lookup("escape").is_err());
        assert!(
            store
                .explicit(std::path::Path::new(".loupe/presets/escape.toml"))
                .is_err()
        );
        std::fs::remove_dir_all(directory.path().join(".loupe")).unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join(".loupe")).unwrap();
        assert!(store.create("new", "noul", true).is_err());
        assert!(!outside.path().join("presets").exists());
    }
}
