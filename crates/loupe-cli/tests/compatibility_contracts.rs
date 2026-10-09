#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compatibility/v0.1");

fn tokenizer() -> PathBuf {
    std::env::var_os("LOUPE_TEST_TOKENIZER")
        .or_else(|| std::env::var_os("LOUPE_TOKENIZER"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache"))
                .join("loupe/tokenizers/Cloudflare--clef-flash")
                .join(loupe_core::config::CLEF_REVISION)
                .join("tokenizer.json")
        })
}

fn run(root: &Path, url: &str, args: &[&str], bytes: &[u8]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    // A caller's profiles, paths, cache and project presets cannot affect fixtures.
    for (key, _) in
        std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("LOUPE_"))
    {
        command.env_remove(key);
    }
    if !args.contains(&"--config") {
        command.args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
        ]);
    }
    let mut child = command
        .current_dir(root)
        .args(["--profile", "server", "--url", url])
        .args(args)
        .env("LOUPE_TOKENIZER", tokenizer())
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env("LOUPE_CACHE_DIR", root.join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Invalid grammar may finish before reading stdin.
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(bytes);
    }
    child.wait_with_output().unwrap()
}

fn exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn compatibility_native_global_conflicts_precede_configuration_and_actions() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["-q", "keep", "question", "-v"],
        vec!["-v", "keep", "question", "-q"],
        vec!["-q", "presets", "list", "-v"],
        vec!["presets", "-v", "list", "-q"],
        vec![
            "-q",
            "on",
            "question",
            "--when",
            "yes",
            "--exec",
            "touch action-must-not-run",
            "-v",
        ],
    ] {
        let mut invalid = vec!["--config", "config-does-not-exist.toml"];
        invalid.extend(args);
        let output = run(root.path(), "http://127.0.0.1:1", &invalid, b"");
        exit(&output, 64);
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("quiet"));
    }
    assert!(!root.path().join("action-must-not-run").exists());
    assert!(!root.path().join("cache").exists());
}

#[test]
fn compatibility_native_token_plan_is_byte_identical_offline() {
    let root = tempfile::tempdir().unwrap();
    let capture: Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(FIXTURES).join("actual-response.json")).unwrap(),
    )
    .unwrap();
    let schema = root.path().join("questions.json");
    std::fs::write(
        &schema,
        serde_json::to_vec(&serde_json::json!({"questions":capture["request"]["questions"]}))
            .unwrap(),
    )
    .unwrap();
    let output = run(
        root.path(),
        "http://127.0.0.1:1",
        &["tokens", "-f", schema.to_str().unwrap()],
        capture["request"]["state"].as_str().unwrap().as_bytes(),
    );
    exit(&output, 0);
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        std::fs::read(Path::new(FIXTURES).join("tokens.jsonl")).unwrap()
    );
    assert!(!root.path().join("cache").exists());
}

#[test]
fn compatibility_native_offline_exit_precedence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("malformed.toml"), "[invalid").unwrap();
    let cases: Value =
        serde_json::from_str(include_str!("compatibility/v0.1/offline-exits.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let args: Vec<_> = case["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let bytes = match case["input"].as_str().unwrap() {
            "invalid-utf8" => vec![0xff],
            input => input.as_bytes().to_vec(),
        };
        let output = run(root.path(), "http://127.0.0.1:1", &args, &bytes);
        exit(
            &output,
            i32::try_from(case["exit"].as_i64().unwrap()).unwrap(),
        );
        if case["stdout_empty"] == true {
            assert!(output.stdout.is_empty(), "{}", case["name"]);
        }
    }
}

#[test]
#[ignore = "requires the real LOUPE_TEST_URL endpoint and prepared pinned tokenizer"]
fn compatibility_native_live_decision_exit_precedence_and_json_envelopes() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let state = b"Checkout is down. Customers cannot place orders.";
    let query = "Is the checkout service experiencing an outage?";
    let cases: Value =
        serde_json::from_str(include_str!("compatibility/v0.1/live-exits.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let args: Vec<_> = case["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| {
                if arg == "$QUERY" {
                    query
                } else {
                    arg.as_str().unwrap()
                }
            })
            .collect();
        let output = run(root.path(), &url, &args, state);
        exit(
            &output,
            i32::try_from(case["exit"].as_i64().unwrap()).unwrap(),
        );
        if let Some(fields) = case["fields"].as_array() {
            let object: Value = serde_json::from_slice(&output.stdout).unwrap();
            let mut actual = object
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            actual.sort_unstable();
            let mut expected = fields
                .iter()
                .map(|field| field.as_str().unwrap())
                .collect::<Vec<_>>();
            expected.sort_unstable();
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }
    let capture: Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(FIXTURES).join("actual-response.json")).unwrap(),
    )
    .unwrap();
    let schema = root.path().join("questions.json");
    std::fs::write(
        &schema,
        serde_json::to_vec(&serde_json::json!({"questions":capture["request"]["questions"]}))
            .unwrap(),
    )
    .unwrap();
    let output = run(
        root.path(),
        &url,
        &[
            "ask",
            "-f",
            schema.to_str().unwrap(),
            "--strict",
            "--no-cache",
        ],
        state,
    );
    exit(&output, 0);
    let response: loupe_core::wire::Response = serde_json::from_slice(&output.stdout).unwrap();
    let request: loupe_core::wire::Request =
        serde_json::from_value(capture["request"].clone()).unwrap();
    response.validate(&request).unwrap();
    let object: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        object.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["model", "answers", "usage"]
    );
    assert_eq!(
        response.usage.input_tokens,
        capture["counts"]["input_tokens"].as_u64().unwrap()
    );
    assert!(!root.path().join("cache").exists());
}
