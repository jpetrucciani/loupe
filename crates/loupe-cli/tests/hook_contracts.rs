#![allow(clippy::unwrap_used)]

use loupe_core::encoder::ClefEncoder;
use loupe_core::wire::Request;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn setup(root: &Path, extra: &str) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!("[profile.local]\nbase_url='http://127.0.0.1:1'\n{extra}"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("hooks")).unwrap();
    std::fs::write(root.join("diff.patch"), "diff --git a/readme b/readme\n--- a/readme\n+++ b/readme\n@@ -0,0 +1 @@\n+Document the ALPHA feature.\n").unwrap();
    std::fs::write(
        root.join("message"),
        "Document ALPHA\n\nDetailed rationale.\n",
    )
    .unwrap();
    config
}

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .current_dir(root)
        .args(["--config", root.join("config.toml").to_str().unwrap()])
        .env("XDG_CONFIG_HOME", root.join("xdg"));
    for key in [
        "LOUPE_API_KEY",
        "LOUPE_URL",
        "LOUPE_PATH",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
    ] {
        command.env_remove(key);
    }
    command
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    command(root).args(arguments).output().unwrap()
}

fn code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_hooks_run_literal_configuration_and_exact_plans_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project's $(false)");
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("credentials-ran");
    setup(
        &root,
        &format!("api_key_cmd=\"touch {}\"\n", marker.display()),
    );
    let installed = run(&root, &["hook", "install", "--directory", "hooks"]);
    code(&installed, 0);
    let installed: Value = serde_json::from_slice(&installed.stdout).unwrap();
    assert_eq!(installed["installed"].as_array().unwrap().len(), 2);
    let repeated = run(&root, &["hook", "install", "--directory", "hooks"]);
    code(&repeated, 0);
    assert!(
        serde_json::from_slice::<Value>(&repeated.stdout).unwrap()["installed"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!marker.exists());
    for (name, args) in [
        ("pre-commit", vec!["--diff", "diff.patch", "--dry-run"]),
        (
            "commit-msg",
            vec!["message", "--diff", "diff.patch", "--dry-run"],
        ),
    ] {
        let output = Command::new(root.join("hooks").join(name))
            .current_dir(&root)
            .env("XDG_CONFIG_HOME", root.join("xdg"))
            .args(args)
            .output()
            .unwrap();
        code(&output, 0);
        let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["hook"], name);
        assert_eq!(plan["counts"]["exact"], true);
        let request: Request = serde_json::from_value(plan["request"].clone()).unwrap();
        let tokenizer = std::env::var_os("LOUPE_TEST_TOKENIZER")
            .or_else(|| std::env::var_os("LOUPE_TOKENIZER"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache")
                    })
                    .join("loupe/tokenizers/Cloudflare--clef-flash")
                    .join(loupe_core::config::CLEF_REVISION)
                    .join("tokenizer.json")
            });
        let encoder = ClefEncoder::load(&tokenizer, loupe_core::config::CLEF_REVISION).unwrap();
        let (_, counts) = encoder.encode(&request).unwrap();
        assert_eq!(plan["counts"]["input_tokens"], counts.input_tokens);
        if name == "commit-msg" {
            assert_eq!(request.questions.len(), 2);
            assert_eq!(request.state["subject"], "Document ALPHA");
            assert!(
                request.state["diff"]
                    .as_str()
                    .unwrap()
                    .contains("+Document the ALPHA")
            );
        }
    }
    assert!(!marker.exists());
}

#[test]
fn installation_preserves_conflicts_and_project_presets_require_current_trust() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(root, "");
    std::fs::write(root.join("hooks/commit-msg"), b"existing user hook").unwrap();
    code(&run(root, &["hook", "install", "--directory", "hooks"]), 64);
    assert!(!root.join("hooks/pre-commit").exists());
    assert_eq!(
        std::fs::read(root.join("hooks/commit-msg")).unwrap(),
        b"existing user hook"
    );
    std::fs::create_dir_all(root.join(".loupe/presets")).unwrap();
    let preset = root.join(".loupe/presets/check.toml");
    std::fs::write(
        &preset,
        "name='check'\ntype='noul'\ninstructions='Does this mention ALPHA?'\n",
    )
    .unwrap();
    setup(root, "[hooks]\nsecret='check'\n");
    code(
        &run(
            root,
            &[
                "hook",
                "run",
                "pre-commit",
                "--diff",
                "diff.patch",
                "--dry-run",
            ],
        ),
        64,
    );
    code(&run(root, &["presets", "trust", "check"]), 0);
    code(
        &run(
            root,
            &[
                "hook",
                "run",
                "pre-commit",
                "--diff",
                "diff.patch",
                "--dry-run",
            ],
        ),
        0,
    );
    std::fs::write(
        &preset,
        "name='check'\ntype='noul'\ninstructions='Changed instructions'\n",
    )
    .unwrap();
    code(
        &run(
            root,
            &[
                "hook",
                "run",
                "pre-commit",
                "--diff",
                "diff.patch",
                "--dry-run",
            ],
        ),
        64,
    );
}

#[test]
fn budgets_input_and_configuration_errors_fail_closed_before_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(
        root,
        "api_key_cmd='touch credentials-ran'\n[hooks]\nbudget=1\n",
    );
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        4,
    );
    assert!(!root.join("credentials-ran").exists());
    setup(
        root,
        "api_key_cmd='touch credentials-ran'\n[hooks]\nsecret='severity'\n",
    );
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        64,
    );
    assert!(!root.join("credentials-ran").exists());
    setup(root, "api_key_cmd='touch credentials-ran'\n");
    code(
        &run(root, &["hook", "run", "commit-msg", "--diff", "diff.patch"]),
        64,
    );
    std::fs::write(root.join("diff.patch"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        65,
    );
    assert!(!root.join("credentials-ran").exists());
    std::fs::write(root.join("diff.patch"), b"").unwrap();
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        0,
    );
    assert!(!root.join("credentials-ran").exists());
}

#[test]
fn actual_credential_failure_and_deadline_obey_hook_fail_open_policy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(root, "api_key_cmd='printf credential-secret >&2; exit 7'\n");
    let output = run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]);
    code(&output, 0);
    assert!(String::from_utf8_lossy(&output.stderr).contains("fail_open=true"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("credential-secret"));
    setup(
        root,
        "api_key_cmd='printf credential-secret >&2; exit 7'\n[hooks]\nfail_open=false\n",
    );
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        77,
    );
    setup(
        root,
        "api_key_cmd='(sleep 1.2; touch orphan-ran) & sleep 30'\n[hooks]\ndeadline_seconds=1\n",
    );
    let started = Instant::now();
    code(
        &run(root, &["hook", "run", "pre-commit", "--diff", "diff.patch"]),
        0,
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!root.join("orphan-ran").exists());
}

#[test]
#[ignore = "requires a real LOUPE_TEST_URL System One endpoint"]
fn actual_server_hook_blocks_and_allows_with_exact_budgeted_requests() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(root, "[hooks]\nsecret='explicit-alpha'\nfail_open=false\n");
    std::fs::create_dir_all(root.join("xdg/loupe/presets")).unwrap();
    std::fs::write(
        root.join("xdg/loupe/presets/explicit-alpha.toml"),
        "type='noul'\ninstructions='Does the added text explicitly mention ALPHA?'\n",
    )
    .unwrap();
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let output = command(root)
        .args([
            "--url",
            &url,
            "hook",
            "run",
            "pre-commit",
            "--diff",
            "diff.patch",
        ])
        .output()
        .unwrap();
    code(&output, 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("likely secret"));
    std::fs::write(root.join("diff.patch"), "diff --git a/readme b/readme\n--- a/readme\n+++ b/readme\n@@ -0,0 +1 @@\n+Document the BETA feature.\n").unwrap();
    let output = command(root)
        .args([
            "--url",
            &url,
            "hook",
            "run",
            "pre-commit",
            "--diff",
            "diff.patch",
        ])
        .output()
        .unwrap();
    code(&output, 0);
}
