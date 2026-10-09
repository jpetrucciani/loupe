#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use loupe_core::recovery::Store;
use serde_json::{Value, json};

const QUESTION: &str = "Does this text explicitly mention ALPHA?";

fn config(root: &Path) -> PathBuf {
    let path = root.join("config.toml");
    std::fs::write(&path, include_str!("../../../examples/config.toml")).unwrap();
    path
}

fn run(root: &Path, config: &Path, args: &[&str], input: &[u8], live: bool) -> Output {
    let live_url = if live {
        std::env::var("LOUPE_TEST_URL").unwrap()
    } else {
        String::new()
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            if live {
                &live_url
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

fn metrics(output: &Output) -> Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr
        .lines()
        .find(|line| line.starts_with("loupe: units="))
        .unwrap();
    let values = line
        .strip_prefix("loupe: ")
        .unwrap()
        .split_whitespace()
        .map(|field| {
            let (key, value) = field.split_once('=').unwrap();
            (key.to_owned(), json!(value))
        })
        .collect::<serde_json::Map<_, _>>();
    Value::Object(values)
}

#[test]
fn inapplicable_missing_and_zero_timeout_flags_fail_before_child_launch() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let marker = root.path().join("bad");
    let command = format!("printf bad > '{}'", marker.display());
    for args in [
        vec![
            "tokens",
            QUESTION,
            "--on-uncertain",
            "fallback",
            "--fallback",
            &command,
        ],
        vec!["keep", QUESTION, "--fallback", &command],
        vec!["keep", QUESTION, "--on-uncertain", "fallback"],
        vec![
            "keep",
            QUESTION,
            "--on-uncertain",
            "fallback",
            "--fallback",
            &command,
            "--fallback-timeout",
            "0s",
        ],
    ] {
        code(&run(root.path(), &config, &args, b"ALPHA\r\n", false), 64);
        assert!(!marker.exists());
    }
    code(
        &run(
            root.path(),
            &config,
            &[
                "keep",
                QUESTION,
                "--on-uncertain",
                "fallback",
                "--fallback",
                &command,
                "--dry-run",
            ],
            b"ALPHA\r\n",
            false,
        ),
        0,
    );
    assert!(!marker.exists());
}

/// The CLI first creates its actual journal with inference denied by budget.
/// Only an exact retained server request/count match can supply a saved result.
fn retained_journal(root: &Path, config: &Path, base: &[&str], input: &[u8]) -> PathBuf {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../loupe-core/testdata/captured-trigger.json"
    ))
    .unwrap();
    retained_result(root, config, base, input, &fixture, "decision")
}

fn retained_result(
    root: &Path,
    config: &Path,
    base: &[&str],
    input: &[u8],
    fixture: &Value,
    field: &str,
) -> PathBuf {
    let path = root.join("journal");
    let mut args = base.to_vec();
    args.extend(["--failed", path.to_str().unwrap()]);
    let output = run(root, config, &args, input, false);
    code(&output, 4);
    assert!(output.stdout.is_empty());
    assert_eq!(metrics(&output)["attempts"], "0");
    let text = std::fs::read_to_string(&path).unwrap();
    let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let identity = first["record"]["identity"].clone();
    assert_eq!(
        identity["questions"],
        json!([[field, fixture["request"]["questions"][field]]])
    );
    let mut store = Store::resume(&path, &identity, &[]).unwrap();
    let mut cursor = 0;
    while let Some(index) = store.next_index(cursor).unwrap() {
        let saved = store.saved(index).unwrap();
        assert_eq!(saved.unit.state, fixture["request"]["state"]);
        if let Some(plan) = saved.plan {
            assert_eq!(plan["request"], fixture["request"]);
            assert_eq!(plan["counts"], fixture["counts"]);
        }
        store.ready(index, json!({
            "request":fixture["request"],"question_order":[field],"answer_order":[field],
            "counts":fixture["counts"],"evaluation":{"response":fixture["response"],"warning":null,"cached":false},
            "answer_id":field,"packed_indices":null,"split_plan":null,"view":null,"focus_plan":null
        })).unwrap();
        cursor = index;
    }
    assert!(cursor > 0);
    path
}

#[test]
fn actual_choice_cascade_routes_external_labels_and_resumes_without_duplication() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../loupe-core/testdata/captured-fallback.json"
    ))
    .unwrap();
    let fixture = &fixtures[0];
    assert_eq!(fixture["field"], "severity");
    let input = fixture["input"].as_str().unwrap().as_bytes();
    for verb in ["tag", "route"] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let out = root.path().join("routed");
        let marker = root.path().join("attempts");
        let command = format!("printf once >> '{}'; printf noise", marker.display());
        let mut base = vec![
            verb,
            "-p",
            "severity",
            "--min-confidence",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            &command,
            "--json",
            "--summary",
            "--budget",
            "0",
        ];
        if verb == "route" {
            base.extend(["--out", out.to_str().unwrap()]);
        }
        let journal = retained_result(root.path(), &config, &base, input, fixture, "severity");
        let mut args = base.to_vec();
        args.extend(["--resume", journal.to_str().unwrap()]);
        let output = run(root.path(), &config, &args, input, false);
        code(&output, 0);
        let data = if verb == "route" {
            assert!(output.stdout.is_empty());
            assert!(!out.join("page").exists());
            std::fs::read(out.join("noise")).unwrap()
        } else {
            output.stdout
        };
        let row: Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(row["answer"], fixture["response"]["answers"]["severity"]);
        assert_eq!(row["decision"]["selected"], json!({"choice":"noise"}));
        assert!(row["decision"]["confidence"].is_null());
        assert_eq!(std::fs::read(&marker).unwrap(), b"once");
        let replay = run(root.path(), &config, &args, input, false);
        code(&replay, 0);
        assert!(replay.stdout.is_empty());
        assert_eq!(std::fs::read(marker).unwrap(), b"once");
        if verb == "route" {
            assert_eq!(std::fs::read(out.join("noise")).unwrap(), data);
        }
    }
}

#[test]
fn actual_cli_merges_retained_answers_in_order_preserves_raw_bytes_and_replays_no_child() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let marker = root.path().join("attempts");
    let command = format!(
        "printf '%s\\n' \"$LOUPE_INDEX\" >> '{}'; if [ \"$LOUPE_INDEX\" = 2 ]; then printf yes; else printf no; fi",
        marker.display()
    );
    let base = [
        "drop",
        QUESTION,
        "-I",
        "lines",
        "--margin",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        &command,
        "-O",
        "json",
        "--summary",
        "--budget",
        "0",
    ];
    let input = b"ALPHA\r\nALPHA\r\nALPHA\r\n";
    let path = retained_journal(root.path(), &config, &base, input);
    assert!(!marker.exists());
    let mut args = base.to_vec();
    args.extend(["--resume", path.to_str().unwrap()]);
    let result = run(root.path(), &config, &args, input, false);
    code(&result, 0);
    let rows: Vec<Value> = String::from_utf8(result.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .map(|row| row["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 3]
    );
    for row in rows {
        assert_eq!(row["answer"]["noul"], json!(0.9497));
        assert_eq!(row["decision"]["selected"], json!({"boolean":false}));
        assert!(row["decision"]["confidence"].is_null());
        assert_eq!(row["fallback"]["outcome"]["code"], 0);
    }
    assert_eq!(std::fs::read(&marker).unwrap(), b"1\n2\n3\n");
    let summary = metrics(&result);
    assert_eq!(summary["attempts"], "0");
    assert_eq!(summary["fallback_attempts"], "3");
    assert_eq!(summary["fallback_resolved"], "3");
    assert_eq!(summary["fallback_cost_usd"], "unavailable");
    let again = run(root.path(), &config, &args, input, false);
    code(&again, 0);
    assert!(again.stdout.is_empty());
    assert_eq!(std::fs::read(marker).unwrap(), b"1\n2\n3\n");
}

#[test]
fn actual_cli_fatal_failure_persists_the_outcome_and_resume_does_not_repeat_it() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let marker = root.path().join("attempts");
    let command = format!("printf once >> '{}'; printf yes; exit 7", marker.display());
    let base = [
        "keep",
        QUESTION,
        "--margin",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        &command,
        "--on-fallback-error",
        "fail",
        "--summary",
        "--budget",
        "0",
    ];
    let input = b"ALPHA\r\n";
    let path = retained_journal(root.path(), &config, &base, input);
    let mut args = base.to_vec();
    args.extend(["--resume", path.to_str().unwrap()]);
    for attempt in 0..2 {
        let output = run(root.path(), &config, &args, input, false);
        code(&output, 69);
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("decision remains uncertain"));
        assert_eq!(std::fs::read(&marker).unwrap(), b"once");
        let summary = metrics(&output);
        assert_eq!(
            summary["fallback_attempts"],
            if attempt == 0 { "1" } else { "0" }
        );
        if attempt == 1 {
            assert_eq!(summary["fallback_replayed"], "1");
        }
    }
}

#[test]
fn actual_cli_failure_capture_histogram_and_test_exit_keep_the_model_answer_separate() {
    for (verb, command, extra, expected, stdout) in [
        ("drop", "printf no", vec![], 0, b"ALPHA\r\n".as_slice()),
        ("keep", "printf invalid", vec![], 1, b"".as_slice()),
        (
            "keep",
            "sleep 1",
            vec!["--fallback-timeout", "20ms"],
            1,
            b"".as_slice(),
        ),
        ("count", "printf no", vec![], 0, b"1\tno\n".as_slice()),
        ("is", "printf no", vec![], 1, b"".as_slice()),
    ] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let capture = root.path().join("uncertain");
        let mut base = vec![
            verb,
            QUESTION,
            "--margin",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            command,
            "--summary",
            "--budget",
            "0",
        ];
        base.extend(extra);
        if verb == "keep" {
            base.extend(["--uncertain", capture.to_str().unwrap()]);
        }
        let input = if verb == "is" {
            b"ALPHA".as_slice()
        } else {
            b"ALPHA\r\n".as_slice()
        };
        let path = retained_journal(root.path(), &config, &base, input);
        let mut args = base.to_vec();
        args.extend(["--resume", path.to_str().unwrap()]);
        let output = run(root.path(), &config, &args, input, false);
        code(&output, expected);
        assert_eq!(output.stdout, stdout);
        if verb == "keep" {
            assert_eq!(std::fs::read(capture).unwrap(), b"ALPHA\r\n");
            assert!(String::from_utf8_lossy(&output.stderr).contains("decision remains uncertain"));
            assert_eq!(metrics(&output)["fallback_failed"], "1");
        } else {
            assert_eq!(metrics(&output)["fallback_resolved"], "1");
        }
    }
}

#[test]
#[ignore = "requires the real server endpoint and prepared pinned tokenizer"]
fn real_server_fallback_filters_raw_units_and_does_not_charge_external_model_tokens() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let input = b"ALPHA\r\nBRAVO\r\n";
    let output = run(
        root.path(),
        &config,
        &[
            "drop",
            QUESTION,
            "--margin",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            "printf no",
            "--summary",
            "--strict",
            "--no-cache",
        ],
        input,
        true,
    );
    code(&output, 0);
    assert_eq!(output.stdout, input);
    let summary = metrics(&output);
    assert_eq!(summary["fallback_attempts"], "2");
    assert_eq!(summary["fallback_resolved"], "2");
    assert_eq!(summary["fallback_cost_usd"], "unavailable");
    assert!(
        summary["input_counted"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(summary["input_counted"], summary["input_reported"]);
}

#[test]
#[ignore = "requires the real server endpoint and prepared pinned tokenizer"]
fn real_server_rate_sort_and_on_apply_external_decisions_without_model_confidence() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let input = b"ALPHA: the outage is frustrating, but the workaround is acceptable.\r\nBRAVO: the rollout is promising, although failures are concerning.\r\n";
    let fallback = "IFS= read -r line; case \"$line\" in ALPHA*) printf 0.5;; *) printf 1.5;; esac";
    for (verb, reverse) in [("rate", false), ("sort", false), ("sort", true)] {
        let mut args = vec![
            verb,
            "-p",
            "sentiment",
            "--min-confidence",
            "1",
            "--min-gap",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            fallback,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ];
        if reverse {
            args.push("-r");
        }
        let output = run(root.path(), &config, &args, input, true);
        code(&output, 0);
        let rows = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row["answer"]["type"], json!("score"));
            assert_eq!(row["decision"]["source"], json!("fallback"));
            assert!(row["decision"]["confidence"].is_null());
            let expected = if row["index"] == 1 { 0.5 } else { 1.5 };
            assert_eq!(row["decision"]["selected"], json!({"score":expected}));
        }
        assert_eq!(rows[0]["index"], json!(if reverse { 2 } else { 1 }));
        let summary = metrics(&output);
        assert_eq!(summary["fallback_attempts"], "2");
        assert_eq!(summary["input_counted"], summary["input_reported"]);
    }
    let action = "cat > action-input; printf '%s:%s' \"$LOUPE_DECISION_SOURCE\" \"$LOUPE_CONFIDENCE\" > action-meta";
    let output = run(
        root.path(),
        &config,
        &[
            "on",
            QUESTION,
            "--margin",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            "printf no",
            "--when",
            "no",
            "--exec",
            action,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        b"ALPHA\r\n",
        true,
    );
    code(&output, 0);
    assert_eq!(
        std::fs::read(root.path().join("action-input")).unwrap(),
        b"ALPHA\r\n"
    );
    assert_eq!(
        std::fs::read(root.path().join("action-meta")).unwrap(),
        b"fallback:"
    );
    let row: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["label"], json!("no"));
    assert_eq!(row["decision"]["selected"], json!({"boolean":false}));
    assert!(row["decision"]["confidence"].is_null());
    assert_eq!(row["answer"], row["response"]["answers"]["decision"]);
    assert_eq!(
        row["counts"]["input_tokens"],
        row["response"]["usage"]["input_tokens"]
    );
}
