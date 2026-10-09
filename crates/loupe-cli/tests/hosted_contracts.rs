#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, profile: &str, args: &[&str], input: &[u8]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .current_dir(root)
        .args([
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--profile",
            profile,
        ])
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "LOUPE_API_KEY",
        "LOUPE_URL",
        "LOUPE_PATH",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_ENCODER",
        "LOUPE_CONCURRENCY",
    ] {
        command.env_remove(key);
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn setup(root: &Path) {
    let config = include_str!("../../../examples/config.toml")
        .replace(
            "api_key_env = \"TYPESAFE_API_KEY\"",
            "api_key_cmd = 'touch auth-ran; printf key'",
        )
        .replace(
            "api_key_env = \"LITELLM_API_KEY\"",
            "api_key_cmd = 'touch auth-ran; printf key'",
        );
    std::fs::write(root.join("config.toml"), config).unwrap();
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
fn direct_and_proxy_plans_use_estimates_and_never_run_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    for profile in ["jev", "litellm"] {
        let output = run(
            root,
            profile,
            &["tokens", "Does this describe an outage?"],
            b"Checkout is down.\n",
        );
        code(&output, 0);
        let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["counts"]["exact"], false);
        assert!(
            plan["request"]["model"]
                .as_str()
                .unwrap()
                .starts_with("jev-")
        );
        code(
            &run(
                root,
                profile,
                &[
                    "keep",
                    "Does this describe an outage?",
                    "--budget",
                    "1000",
                    "--dry-run",
                ],
                b"Checkout is down.\n",
            ),
            64,
        );
        assert!(!root.join("auth-ran").exists());
    }
    let config = loupe_core::config::Config::load(&root.join("config.toml"), true).unwrap();
    let proxy = config
        .resolve(
            Some("litellm"),
            &loupe_core::config::Settings::default(),
            &std::collections::HashMap::from([("HOME".into(), root.to_str().unwrap().into())]),
        )
        .unwrap();
    assert_eq!(proxy.base_url, "https://llm.example.com/typesafe");
    assert_eq!(proxy.path, "/v1/systemone");
}

#[test]
fn unsupported_jev_questions_fail_before_credential_commands() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let questions = root.join("questions.json");
    for question in [
        json!({"type":"noul"}),
        json!({"type":"score","instructions":"Rate this","criteria":["Only level"]}),
        json!({"type":"score","instructions":"Rate this","criteria":(0..11).map(|n|n.to_string()).collect::<Vec<_>>()}),
        json!({"type":"choice","instructions":"Choose","criteria":(0..256).map(|n|(format!("option{n}"),json!("description"))).collect::<serde_json::Map<_,_>>()}),
    ] {
        std::fs::write(
            &questions,
            json!({"questions":{"question":question}}).to_string(),
        )
        .unwrap();
        code(
            &run(
                root,
                "jev",
                &["ask", "-f", questions.to_str().unwrap()],
                b"record\n",
            ),
            65,
        );
        assert!(!root.join("auth-ran").exists());
    }
}

#[test]
#[ignore = "requires explicit LOUPE_HOSTED_CONFIG and LOUPE_HOSTED_PROFILE with real credentials"]
fn actual_hosted_endpoint_returns_valid_noul_choice_and_score() {
    let config = std::env::var("LOUPE_HOSTED_CONFIG").unwrap();
    let profile = std::env::var("LOUPE_HOSTED_PROFILE").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let questions = directory.path().join("questions.json");
    let request = loupe_core::wire::Request {
        model: "unused-for-response-validation".into(),
        state: json!("ALPHA is explicitly present."),
        questions: serde_json::from_value(json!({
            "alpha":{"type":"noul","instructions":"Does the text explicitly mention ALPHA?"},
            "token":{"type":"choice","instructions":"Which of these tokens is explicitly present?","criteria":{"alpha":"ALPHA","beta":"BETA","none":"Neither token"}},
            "clarity":{"type":"score","instructions":"How explicitly does the text mention ALPHA?","criteria":["Absent","Ambiguous","Explicit"]}
        })).unwrap(),
    };
    std::fs::write(
        &questions,
        json!({"questions":request.questions}).to_string(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            &config,
            "--profile",
            &profile,
            "--concurrency",
            "1",
            "ask",
            "-f",
            questions.to_str().unwrap(),
            "--deadline",
            "30s",
            "--no-cache",
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"ALPHA is explicitly present.")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    code(&output, 0);
    let response: loupe_core::wire::Response = serde_json::from_slice(&output.stdout).unwrap();
    let answers = response.validate(&request).unwrap();
    assert_eq!(answers.len(), 3);
    assert!(!response.model.is_empty());
    assert!(response.usage.input_tokens > 0);
}
