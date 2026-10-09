#![allow(clippy::unwrap_used)]
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
fn setup(root: &Path) -> PathBuf {
    let path = root.join("config.toml");
    std::fs::write(&path, include_str!("../../../examples/config.toml")).unwrap();
    path
}
fn run(root: &Path, config: &Path, args: &[&str], input: &[u8], live: bool) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            if live {
                "http://127.0.0.1:8015"
            } else {
                "http://127.0.0.1:1"
            },
        ])
        .args(args)
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    child.wait_with_output().unwrap()
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
fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
const QUESTION: &str = "Does this text explicitly mention ALPHA?";
#[test]
fn ordered_action_groups_survive_mixed_equals_flags_and_dry_run_has_no_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let dest = root.join("out");
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when=yes",
            "--exec=printf first",
            "--append",
            dest.to_str().unwrap(),
            "--when",
            "no",
            "--exec",
            "printf second",
            "--dry-run",
        ],
        b"ALPHA\n",
        false,
    );
    code(&result, 0);
    let row = rows(&result).remove(0);
    assert_eq!(row["actions"].as_array().unwrap().len(), 3);
    assert_eq!(row["actions"][0]["matcher"]["kind"], json!("yes"));
    assert_eq!(row["actions"][1]["target"]["kind"], json!("append"));
    assert_eq!(row["actions"][2]["matcher"]["kind"], json!("no"));
    assert!(!dest.exists());
}
#[test]
fn invalid_selectors_types_order_flags_and_append_aliases_fail_before_inference() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    for tail in [
        vec!["--exec", "true"],
        vec!["--when", "yes"],
        vec!["--when", "yes", "--when", "no", "--exec", "true"],
        vec!["--when", "page", "--exec", "true"],
        vec!["--when", "yes", "--exec", "true", "--exec-timeout", "0s"],
        vec!["--when", "yes", "--exec", "true", "--split"],
        vec!["--when", "yes", "--exec", "true", "--max-fired", "1"],
        vec!["--when", "yes", "--exec", "true", "--failed", "log"],
        vec!["--when", "yes", "--exec", "true", "-O", "tsv"],
    ] {
        let mut args = vec!["on", QUESTION];
        args.extend(tail);
        code(&run(root, &config, &args, b"", false), 64);
    }
    let input = root.join("input");
    std::fs::write(&input, b"ALPHA\n").unwrap();
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            input.to_str().unwrap(),
            "--when",
            "yes",
            "--append",
            input.to_str().unwrap(),
        ],
        b"",
        false,
    );
    code(&result, 64);
    assert_eq!(std::fs::read(input).unwrap(), b"ALPHA\n");
}
#[test]
fn choice_plan_uses_declared_labels_and_score_presets_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let result = run(
        root,
        &config,
        &[
            "on",
            "-p",
            "severity",
            "--when",
            "page",
            "--exec",
            "true",
            "--when",
            "ticket",
            "--append",
            "tickets",
            "--dry-run",
        ],
        b"Service is down.\n",
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)[0]["actions"][0]["matcher"]["value"],
        json!("page")
    );
    code(
        &run(
            root,
            &config,
            &[
                "on",
                "-p",
                "sentiment",
                "--when",
                "uncertain",
                "--exec",
                "true",
                "--dry-run",
            ],
            b"",
            false,
        ),
        64,
    );
}

#[test]
fn durable_state_initialization_dry_run_identity_and_alias_guards_need_no_backend() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let state = root.join("state");
    let common = [
        "on",
        QUESTION,
        "--when",
        "yes",
        "--exec",
        "true",
        "--state",
        state.to_str().unwrap(),
    ];
    let mut dry = common.to_vec();
    dry.push("--dry-run");
    code(&run(root, &config, &dry, b"ALPHA\n", false), 0);
    assert!(!state.exists());
    code(&run(root, &config, &common, b"", false), 1);
    assert!(state.exists());
    code(&run(root, &config, &common, b"", false), 1);
    let changed = [
        "on",
        "A different question",
        "--when",
        "yes",
        "--exec",
        "true",
        "--state",
        state.to_str().unwrap(),
    ];
    code(&run(root, &config, &changed, b"", false), 65);
    let alias = [
        "on",
        QUESTION,
        "--when",
        "yes",
        "--append",
        state.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ];
    // Use a distinct state to keep the action identity unchanged for the alias check.
    let other = root.join("alias-state");
    let alias = alias
        .iter()
        .map(|arg| {
            if *arg == state.to_str().unwrap() {
                other.to_str().unwrap()
            } else {
                *arg
            }
        })
        .collect::<Vec<_>>();
    code(&run(root, &config, &alias, b"", false), 64);
    let input = root.join("input");
    std::fs::write(&input, b"ALPHA\n").unwrap();
    let alias = [
        "on",
        QUESTION,
        input.to_str().unwrap(),
        "--when",
        "yes",
        "--exec",
        "true",
        "--state",
        input.to_str().unwrap(),
    ];
    code(&run(root, &config, &alias, b"", false), 74);
    assert_eq!(std::fs::read(&input).unwrap(), b"ALPHA\n");
    let missing = root.join("missing");
    let output = root.join("new-output");
    let args = [
        "on",
        QUESTION,
        missing.to_str().unwrap(),
        "--when",
        "yes",
        "--append",
        output.to_str().unwrap(),
        "--state",
        missing.to_str().unwrap(),
    ];
    code(&run(root, &config, &args, b"", false), 74);
    assert!(!missing.exists());
    assert!(!output.exists());
}

#[test]
#[ignore = "requires actual server decisions and real durable state; no fake backend"]
fn server_durable_claims_suppress_restart_and_explicit_windows_expire() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let state = root.join("permanent");
    let effect = root.join("effect");
    let args = [
        "on",
        QUESTION,
        "--when",
        "yes",
        "--append",
        effect.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--json",
        "--strict",
        "--no-cache",
        "--deadline",
        "60s",
    ];
    code(&run(root, &config, &args, b"ALPHA\n", true), 0);
    let repeated = run(root, &config, &args, b"ALPHA\n", true);
    code(&repeated, 1);
    assert_eq!(rows(&repeated)[0]["actions"][0]["suppressed"], true);
    assert_eq!(std::fs::read(&effect).unwrap(), b"ALPHA\n");
    let state = root.join("window-state");
    let effect = root.join("window-effect");
    let args = [
        "on",
        QUESTION,
        "--when",
        "yes",
        "--append",
        effect.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--dedupe-window",
        "60s",
        "--json",
        "--strict",
        "--no-cache",
        "--deadline",
        "60s",
    ];
    code(&run(root, &config, &args, b"ALPHA\n", true), 0);
    code(&run(root, &config, &args, b"ALPHA\n", true), 1);
    let state = root.join("short-state");
    let effect = root.join("short-effect");
    let args = [
        "on",
        QUESTION,
        "--when",
        "yes",
        "--append",
        effect.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--dedupe-window",
        "1ms",
        "--strict",
        "--no-cache",
        "--deadline",
        "60s",
    ];
    code(&run(root, &config, &args, b"ALPHA\n", true), 0);
    std::thread::sleep(std::time::Duration::from_millis(2));
    code(&run(root, &config, &args, b"ALPHA\n", true), 0);
    assert_eq!(std::fs::read(&effect).unwrap(), b"ALPHA\nALPHA\n");
}
#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_on_fires_ordered_once_actions_preserves_append_bytes_and_reports_child_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let marker = root.join("exec");
    let append = root.join("append");
    let command = format!(
        "printf '%s:%s\\n' \"$LOUPE_INDEX\" \"$LOUPE_UNIT\" >> '{}'",
        marker.display()
    );
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--exec",
            &command,
            "--append",
            append.to_str().unwrap(),
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        b"ALPHA\r\nBRAVO\r\n",
        true,
    );
    code(&result, 0);
    let records = rows(&result);
    assert_eq!(records[0]["actions"].as_array().unwrap().len(), 2);
    assert!(records[1]["actions"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(marker).unwrap(), b"1:ALPHA\n");
    assert_eq!(std::fs::read(append).unwrap(), b"ALPHA\r\n");
    let count = root.join("count");
    let command = format!("printf once >> '{}'; exit 7", count.display());
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--exec",
            &command,
            "--json",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"ALPHA\n",
        true,
    );
    code(&result, 69);
    assert_eq!(rows(&result)[0]["actions"][0]["result"]["code"], json!(7));
    assert_eq!(std::fs::read(count).unwrap(), b"once");
}
#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_on_window_suppresses_repeats_and_timeout_stops_later_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let dest = root.join("out");
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--append",
            dest.to_str().unwrap(),
            "--dedupe-window",
            "60s",
            "--json",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"ALPHA\nALPHA\n",
        true,
    );
    code(&result, 0);
    assert_eq!(std::fs::read(dest).unwrap(), b"ALPHA\n");
    assert_eq!(rows(&result)[1]["actions"][0]["suppressed"], json!(true));
    let leaked = root.join("leaked");
    let command = format!(
        "(sleep 0.3; printf leaked > '{}') & sleep 30",
        leaked.display()
    );
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--exec",
            &command,
            "--exec-timeout",
            "50ms",
            "--json",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"ALPHA\n",
        true,
    );
    code(&result, 69);
    assert_eq!(
        rows(&result)[0]["actions"][0]["result"]["timed_out"],
        json!(true)
    );
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!leaked.exists());
}
#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_on_uncertainty_can_route_explicitly_and_failure_prevents_actions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let dest = root.join("out");
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--exec",
            "true",
            "--when",
            "uncertain",
            "--append",
            dest.to_str().unwrap(),
            "--margin",
            "1",
            "--json",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"ALPHA\n",
        true,
    );
    code(&result, 0);
    assert_eq!(rows(&result)[0]["actions"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read(&dest).unwrap(), b"ALPHA\n");
    let result = run(
        root,
        &config,
        &[
            "on",
            QUESTION,
            "--when",
            "yes",
            "--append",
            dest.to_str().unwrap(),
            "--margin",
            "1",
            "--on-uncertain",
            "fail",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"ALPHA other\n",
        true,
    );
    code(&result, 3);
    assert_eq!(std::fs::read(dest).unwrap(), b"ALPHA\n");
}
