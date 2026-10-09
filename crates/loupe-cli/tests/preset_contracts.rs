#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], input: &[u8], extra_env: &[(&str, &str)]) -> Output {
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .current_dir(root)
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
        ])
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_CONFIG")
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in extra_env {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn new_show_list_lint_trust_and_edit_preserve_actual_content_grants() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    success(&run(
        root,
        &["presets", "new", "rubric", "--project"],
        b"",
        &[],
    ));
    let shown = run(root, &["presets", "show", "rubric"], b"", &[]);
    success(&shown);
    let before: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(before["trusted"], false);
    assert_eq!(before["origin"]["kind"], "project");
    success(&run(
        root,
        &["presets", "lint", "rubric", "--json"],
        b"",
        &[],
    ));
    let denied = run(root, &["tokens", "-p", "rubric"], b"original", &[]);
    assert_eq!(denied.status.code(), Some(64));
    success(&run(root, &["presets", "trust", "rubric"], b"", &[]));
    success(&run(root, &["tokens", "-p", "rubric"], b"original", &[]));
    let edited = run(
        root,
        &["presets", "edit", "rubric"],
        b"",
        &[("EDITOR", "sed -i 's/version = 1/version = 2/'")],
    );
    success(&edited);
    let after: Value = serde_json::from_slice(&edited.stdout).unwrap();
    assert_eq!(after["trusted"], false);
    assert_ne!(after["content_hash"], before["content_hash"]);
    assert_eq!(
        run(root, &["tokens", "-p", "rubric"], b"", &[])
            .status
            .code(),
        Some(64)
    );
    success(&run(root, &["presets", "trust"], b"", &[]));
    let listed = run(root, &["presets", "list", "--json"], b"", &[]);
    success(&listed);
    let listed: Vec<Value> = serde_json::from_slice(&listed.stdout).unwrap();
    assert!(
        listed
            .iter()
            .any(|preset| preset["name"] == "rubric" && preset["trusted"] == true)
    );
    assert_eq!(
        run(root, &["presets", "new", "rubric", "--project"], b"", &[])
            .status
            .code(),
        Some(64)
    );
}

#[test]
fn builtin_edit_requires_editor_before_writing_and_creates_an_xdg_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(
        run(root, &["presets", "edit", "secret"], b"", &[])
            .status
            .code(),
        Some(78)
    );
    assert!(!root.join("xdg/loupe/presets/secret.toml").exists());
    success(&run(
        root,
        &["presets", "edit", "secret"],
        b"",
        &[("EDITOR", "true")],
    ));
    let shown = run(root, &["presets", "show", "secret"], b"", &[]);
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["origin"]["kind"], "xdg");
    assert_eq!(shown["trusted"], true);
    assert_eq!(shown["document"]["type"], "noul");
    success(&run(root, &["presets", "lint", "--json"], b"", &[]));
}

#[test]
fn preset_input_defaults_and_static_context_flow_into_exact_offline_plans() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("xdg/loupe/presets")).unwrap();
    std::fs::write(root.join("xdg/loupe/presets/records.toml"),"name='records'\ntype='noul'\ninstructions='Does this say {{arg.query}}?'\n[input]\nmode='records'\nrecords='^EVENT'\n[context]\nreference='A static rubric'\n").unwrap();
    let output = run(
        root,
        &[
            "keep",
            "-p",
            "records",
            "--arg",
            "query=outage",
            "--dry-run",
        ],
        b"EVENT first\n trace\nEVENT next\n",
        &[],
    );
    success(&output);
    let plans: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0]["request"]["state"], "EVENT first\n trace\n");
    assert_eq!(
        plans[0]["request"]["questions"]["records"]["instructions"],
        json!({"question":"Does this say outage?","context":{"reference":"A static rubric"}})
    );
    assert_eq!(plans[0]["counts"]["exact"], true);
    let output = run(
        root,
        &[
            "keep",
            "-p",
            "records",
            "--arg",
            "query=outage",
            "--dry-run",
            "--records",
            "^NEXT",
        ],
        b"EVENT first\nNEXT second\n",
        &[],
    );
    success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 2);
}

#[test]
fn untrusted_live_lint_does_not_execute_even_a_trusted_credential_command() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    success(&run(
        root,
        &["presets", "new", "rubric", "--project"],
        b"",
        &[],
    ));
    let config = root.join("auth.toml");
    let marker = root.join("credential-ran");
    std::fs::write(
        &config,
        format!(
            "[profile.auth]\nbase_url='http://127.0.0.1:1'\napi_key_cmd='touch {}'\n",
            marker.display()
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "auth",
            "presets",
            "lint",
            "rubric",
            "--live",
        ])
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(64),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("untrusted"));
    assert!(!marker.exists());
}

#[test]
fn offline_preset_commands_and_estimated_plans_do_not_need_hosted_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(dir.path())
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "jev",
            "presets",
            "lint",
            "secret",
            "--json",
        ])
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("LOUPE_API_KEY")
        .output()
        .unwrap();
    success(&output);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_live_preset_examples_apply_thresholds_uncertainty_and_identity_checks() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("xdg/loupe/presets")).unwrap();
    let path = root.join("xdg/loupe/presets/outage.json");
    let mut preset = json!({"name":"outage","version":1,"type":"noul","instructions":"Is checkout currently unavailable?","context":{"reference":"Judge the current service state, rather than historical failures."},"policy":{"threshold":0.5},"examples":[{"unit":"Checkout is down. Customers cannot place orders.","expect":true},{"unit":"Checkout is healthy. All checks pass.","expect":false}]});
    std::fs::write(&path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let output = run(
        root,
        &[
            "--url", &endpoint, "presets", "lint", "outage", "--live", "--json",
        ],
        b"",
        &[],
    );
    success(&output);
    let reports: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reports[0]["examples_passed"], true);
    let plan = run(
        root,
        &["tokens", "-p", "outage"],
        b"Checkout is down. Customers cannot place orders.",
        &[],
    );
    success(&plan);
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(
        reports[0]["examples"][0]["response"]["usage"]["input_tokens"],
        plan["counts"]["input_tokens"]
    );
    assert!(
        reports[0]["examples"][0]["response"]["usage"]["input_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    preset["policy"] = json!({"margin":1.0});
    for example in preset["examples"].as_array_mut().unwrap() {
        example["expect"] = json!("uncertain");
    }
    std::fs::write(&path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let output = run(
        root,
        &[
            "--url", &endpoint, "presets", "lint", "outage", "--live", "--json",
        ],
        b"",
        &[],
    );
    success(&output);
    let reports: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        reports[0]["examples"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| result["uncertain"] == true)
    );
    preset["policy"] = json!({"model_identity":format!("clef-flash@{};quantization=nf4;compute=bf16;server={}","a".repeat(40),"b".repeat(64))});
    std::fs::write(&path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let output = run(
        root,
        &[
            "--url", &endpoint, "presets", "lint", "outage", "--live", "--json",
        ],
        b"",
        &[],
    );
    assert_eq!(output.status.code(), Some(64));
    let reports: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reports[0]["valid"], false);
    assert!(
        reports[0]["error"]
            .as_str()
            .unwrap()
            .contains("identity is unavailable")
    );
    assert!(reports[0].get("examples").is_none());
}
