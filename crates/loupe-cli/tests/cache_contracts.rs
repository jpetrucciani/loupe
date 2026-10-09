#![allow(clippy::unwrap_used)]

use loupe_core::cache::{Entry, Store};
use loupe_core::identity::{Discovery, Model};
use loupe_core::wire::{Request, Response};
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
        ])
        .args(args)
        .env("LOUPE_CACHE_DIR", root)
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn seed(directory: &Path) -> String {
    let identity = Discovery {
        model: Model::parse(&format!(
            "clef-flash@{};compute=bf16;quantization=nf4;server={}",
            "a".repeat(40),
            "b".repeat(64)
        ))
        .unwrap(),
        encoder_identity: "fixture-encoder".to_owned(),
    };
    let captured: Value = serde_json::from_str(include_str!(
        "../../loupe-core/testdata/captured-response.json"
    ))
    .unwrap();
    let request: Request = serde_json::from_value(captured["request"].clone()).unwrap();
    let mut response: Response = serde_json::from_value(captured["response"].clone()).unwrap();
    response.model = identity.model.as_str().to_owned();
    let mut old = Entry::new(&identity, response.clone()).unwrap();
    old.created_at = 1;
    let recent = Entry::new(&identity, response).unwrap();
    let mut other = request.clone();
    other.state = json!("a different state");
    let store = Store::open(directory, false).unwrap().unwrap();
    store
        .put_many(&[
            (
                loupe_core::cache::key(&identity, None, &request).unwrap(),
                old,
            ),
            (
                loupe_core::cache::key(&identity, None, &other).unwrap(),
                recent,
            ),
        ])
        .unwrap();
    identity.model.as_str().to_owned()
}
#[test]
fn absent_cache_administration_stays_offline_and_does_not_create_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("missing");
    for args in [
        vec!["cache", "stats", "--json"],
        vec!["cache", "prune", "--older", "30d"],
        vec!["cache", "clear"],
    ] {
        let output = run(&absent, &args, b"");
        success(&output);
        assert!(output.stderr.is_empty());
        assert!(!absent.exists());
    }
    let stats: Value =
        serde_json::from_slice(&run(&absent, &["cache", "stats", "--json"], b"").stdout).unwrap();
    assert_eq!(stats["present"], false);
    assert_eq!(stats["entries"], 0);
    for age in ["bad", "-1d", "99999999999999999999d", "1w"] {
        assert_eq!(
            run(&absent, &["cache", "prune", "--older", age], b"")
                .status
                .code(),
            Some(64)
        );
        assert!(!absent.exists());
    }
    assert_eq!(
        run(
            &absent,
            &["keep", "question", "--refresh", "--no-cache", "--dry-run"],
            b"x"
        )
        .status
        .code(),
        Some(64)
    );
}
#[test]
fn cache_administration_never_executes_credential_commands() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let marker = dir.path().join("credentials-executed");
    std::fs::write(
        &config,
        format!(
            "[profile.offline]\nbase_url='http://127.0.0.1:1'\napi_key_cmd='touch {}'\n",
            marker.display()
        ),
    )
    .unwrap();
    for args in [
        vec!["cache", "stats", "--json"],
        vec!["cache", "prune"],
        vec!["cache", "clear"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_loupe"))
            .args(["--config", config.to_str().unwrap(), "--profile", "offline"])
            .args(args)
            .env("LOUPE_CACHE_DIR", dir.path().join("cache"))
            .env_remove("LOUPE_URL")
            .env_remove("LOUPE_API_KEY")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        success(&output);
        assert!(!marker.exists());
        assert!(!dir.path().join("cache").exists());
    }
}
#[test]
fn real_disk_stats_prune_and_clear_preserve_runtime_assets_and_read_only_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let model = seed(dir.path());
    let database = dir.path().join(loupe_core::cache::FILE_NAME);
    std::fs::write(dir.path().join("tokenizer.json"), b"runtime asset").unwrap();
    let before = std::fs::read(&database).unwrap();
    let output = run(dir.path(), &["cache", "stats", "--json"], b"");
    success(&output);
    let stats: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stats["entries"], 2);
    assert_eq!(stats["by_model"][model], 2);
    assert_eq!(stats["oldest"], 1);
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let output = run(dir.path(), &["cache", "prune", "--older", "30d"], b"");
    success(&output);
    assert_eq!(output.stdout, b"pruned=1\n");
    assert_eq!(
        Store::open(dir.path(), true)
            .unwrap()
            .unwrap()
            .stats()
            .unwrap()
            .entries,
        1
    );
    let output = run(dir.path(), &["cache", "clear"], b"");
    success(&output);
    assert_eq!(output.stdout, b"cleared\n");
    assert_eq!(
        Store::open(dir.path(), true)
            .unwrap()
            .unwrap()
            .stats()
            .unwrap()
            .entries,
        0
    );
    assert_eq!(
        std::fs::read(dir.path().join("tokenizer.json")).unwrap(),
        b"runtime asset"
    );
}
#[test]
fn conflicting_process_commands_report_io_error_instead_of_mutating_the_store() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let writer = Store::open(dir.path(), false).unwrap().unwrap();
    for args in [
        vec!["cache", "stats", "--json"],
        vec!["cache", "prune", "--older", "0s"],
        vec!["cache", "clear"],
    ] {
        let output = run(dir.path(), &args, b"");
        assert_eq!(output.status.code(), Some(74));
        assert!(output.stdout.is_empty());
    }
    assert_eq!(writer.stats().unwrap().entries, 2);
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL without authoritative metadata"]
fn server_unavailable_identity_prevents_all_cache_access_even_for_refresh() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(loupe_core::cache::FILE_NAME);
    let original = b"a corrupt cache that must never be opened";
    std::fs::write(&path, original).unwrap();
    for flag in [None, Some("--refresh"), Some("--no-cache")] {
        let mut args = vec![
            "--url",
            &url,
            "keep",
            "Is checkout down?",
            "--json",
            "--strict",
            "--summary",
            "--pack",
            "unit",
        ];
        if let Some(flag) = flag {
            args.push(flag);
        }
        let output = run(
            dir.path(),
            &args,
            b"Checkout is down and customers cannot pay.\n",
        );
        success(&output);
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(row["cached"], false);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.contains("fresh authoritative model/encoder identity is unavailable"),
            flag != Some("--no-cache")
        );
        assert!(stderr.contains("cache_hits=0"));
        assert!(!stderr.contains("cache database"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let output = run(dir.path(), &["--url", &url, "doctor", "--json"], b"");
    success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["cache_eligible"], false);
    assert!(report["discovery"]["identity"].is_null());
}
