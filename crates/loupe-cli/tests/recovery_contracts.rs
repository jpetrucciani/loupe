#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

const QUESTION: &str = "Is checkout unavailable?";
fn config(root: &Path) -> std::path::PathBuf {
    let path = root.join("config.toml");
    std::fs::write(
        &path,
        include_str!("../../../examples/config.toml")
            .replace("max_length = 16384", "max_length = 1400"),
    )
    .unwrap();
    path
}
fn command(config: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .args(["--config", config.to_str().unwrap(), "--profile", "server"])
        .args(args)
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn run(config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command(config, args).spawn().unwrap();
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
fn frames(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .flat_map(|line| {
            let frame:Value=serde_json::from_str(line).unwrap();
            if frame["record"]["record"]=="ready_many" {
                frame["record"]["completions"].as_array().unwrap().iter().map(|completion|json!({"record":{"record":"ready","index":completion["index"],"completed":completion["completed"]}})).collect::<Vec<_>>()
            } else {vec![frame]}
        })
        .collect()
}
fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn summary(output: &Output) -> BTreeMap<String, String> {
    String::from_utf8(output.stderr.clone())
        .unwrap()
        .lines()
        .find(|line| line.starts_with("loupe: units="))
        .unwrap()[7..]
        .split_whitespace()
        .map(|field| {
            let (key, value) = field.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect()
}
fn oversized() -> Vec<u8> {
    ("word ".repeat(2000) + "東京🙂\r\n").into_bytes()
}

#[test]
fn recovery_grammar_and_existing_artifacts_are_validated_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    for args in [
        vec!["tokens", QUESTION, "--failed", journal.to_str().unwrap()],
        vec![
            "keep",
            QUESTION,
            "--dry-run",
            "--failed",
            journal.to_str().unwrap(),
        ],
        vec![
            "keep",
            QUESTION,
            "--failed",
            journal.to_str().unwrap(),
            "--resume",
            journal.to_str().unwrap(),
        ],
    ] {
        assert_eq!(run(&config, &args, b"").status.code(), Some(64));
        assert!(!journal.exists());
    }
    std::fs::write(&journal, b"unrelated data\n").unwrap();
    assert_eq!(
        run(
            &config,
            &[
                "--url",
                "http://127.0.0.1:1",
                "keep",
                QUESTION,
                "--failed",
                journal.to_str().unwrap()
            ],
            b""
        )
        .status
        .code(),
        Some(74)
    );
    assert_eq!(std::fs::read(&journal).unwrap(), b"unrelated data\n");
    assert_eq!(
        run(
            &config,
            &[
                "--url",
                "http://127.0.0.1:1",
                "keep",
                QUESTION,
                "--resume",
                journal.to_str().unwrap()
            ],
            b""
        )
        .status
        .code(),
        Some(74)
    );
    assert_eq!(std::fs::read(&journal).unwrap(), b"unrelated data\n");
}

#[test]
fn completed_error_run_replays_only_after_full_raw_input_and_semantic_identity_validation() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let unit = oversized();
    let input = [unit.as_slice(), b"\n", unit.as_slice()].concat();
    let common = [
        "--url",
        "http://127.0.0.1:1",
        "keep",
        QUESTION,
        "--keep-going",
        "--on-error",
        "keep",
        "--json",
        "--summary",
    ];
    let mut args = common.to_vec();
    args.extend(["--failed", journal.to_str().unwrap()]);
    let first = run(&config, &args, &input);
    success(&first);
    assert_eq!(
        rows(&first)
            .iter()
            .map(|row| row["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 3]
    );
    let events = frames(&journal);
    assert_eq!(
        events
            .iter()
            .filter(|frame| frame["record"]["record"] == "admit")
            .count(),
        2
    );
    assert_eq!(events.last().unwrap()["record"]["record"], "finish");
    let before = std::fs::read(&journal).unwrap();
    let mut args = common.to_vec();
    args.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
    let replay = run(&config, &args, &input);
    success(&replay);
    assert!(replay.stdout.is_empty());
    assert_eq!(summary(&replay)["attempts"], "0");
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    for changed in [
        unit.clone(),
        [input.as_slice(), b"\n"].concat(),
        [input.as_slice(), unit.as_slice()].concat(),
        input
            .iter()
            .copied()
            .map(|byte| if byte == b'w' { b'x' } else { byte })
            .collect(),
    ] {
        let output = run(&config, &args, &changed);
        assert_eq!(output.status.code(), Some(65));
        assert!(output.stdout.is_empty());
        assert_eq!(summary(&output)["attempts"], "0");
        assert_eq!(std::fs::read(&journal).unwrap(), before);
    }
    let mut changed = args.clone();
    changed.extend(["--threshold", "0.7"]);
    assert_eq!(run(&config, &changed, &input).status.code(), Some(65));
    assert_eq!(std::fs::read(&journal).unwrap(), before);
}

#[test]
fn reader_fatal_and_input_output_inode_aliases_preserve_the_original_artifact() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            QUESTION,
            "-I",
            "jsonl",
            "--failed",
            journal.to_str().unwrap(),
            "--summary",
        ],
        b"{bad json}\n",
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    let events = frames(&journal);
    assert!(
        events
            .iter()
            .any(|frame| frame["record"]["record"] == "failed")
    );
    assert!(
        !events
            .iter()
            .any(|frame| frame["record"]["record"] == "admit")
    );
    let before = std::fs::read(&journal).unwrap();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            QUESTION,
            "-I",
            "jsonl",
            "--resume",
            journal.to_str().unwrap(),
            journal.to_str().unwrap(),
        ],
        b"",
    );
    assert!(matches!(output.status.code(), Some(64 | 65)));
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    let alias = root.path().join("input-alias");
    std::fs::hard_link(&journal, &alias).unwrap();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            QUESTION,
            "--resume",
            journal.to_str().unwrap(),
            alias.to_str().unwrap(),
        ],
        b"",
    );
    assert!(matches!(output.status.code(), Some(64 | 65 | 74)));
    assert_eq!(std::fs::read(&journal).unwrap(), before);
}

#[test]
fn actual_sigterm_preserves_bounded_read_ahead_and_requires_original_stdin_prefix() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let input = b"First.\nSecond.\nThird.\nFourth.\nFifth.\n";
    let mut child = command(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "-j",
            "1",
            "-vv",
            "keep",
            QUESTION,
            "--failed",
            journal.to_str().unwrap(),
            "--summary",
        ],
    )
    .spawn()
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).unwrap();
    let stderr = child.stderr.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let line = line.unwrap();
            let _ = send.send(line);
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut submitted = false;
    while std::time::Instant::now() < deadline {
        if let Ok(line) = receive.recv_timeout(Duration::from_millis(50))
            && line.contains("attempt=1")
        {
            submitted = true;
            break;
        }
    }
    if !submitted {
        child.kill().unwrap();
        let _ = child.wait();
        panic!("no real submitted attempt before timeout");
    }
    rustix_signal(&child, 15);
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    reader.join().unwrap();
    assert_eq!(output.status.code(), Some(143));
    assert!(output.stdout.is_empty());
    let original = frames(&journal);
    let indices: Vec<_> = original
        .iter()
        .filter(|frame| frame["record"]["record"] == "admit")
        .map(|frame| frame["record"]["unit"]["index"].as_u64().unwrap())
        .collect();
    assert!(indices.len() >= 2);
    assert!(
        !original
            .iter()
            .any(|frame| frame["record"]["record"] == "input_end")
    );
    let before = std::fs::read(&journal).unwrap();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "-j",
            "1",
            "keep",
            QUESTION,
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
            "--summary",
        ],
        b"First.\nCHANGED.\nThird.\nFourth.\nFifth.\n",
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    assert_eq!(summary(&output)["attempts"], "0");
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "-j",
            "1",
            "keep",
            QUESTION,
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
            "--summary",
        ],
        input,
    );
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(summary(&output)["attempts"], "0");
}
fn rustix_signal(child: &std::process::Child, signal: u8) {
    assert!(
        Command::new("kill")
            .args([format!("-{signal}"), child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_recovery_resumes_budget_failure_and_completed_receipts_without_cache_or_new_tokens() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let input = b"Checkout is down.\nCheckout is down again.\nCheckout is unavailable.\n";
    let first = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "1",
            "keep",
            QUESTION,
            "--failed",
            journal.to_str().unwrap(),
            "--budget",
            "200",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    assert_eq!(first.status.code(), Some(4));
    assert_eq!(rows(&first).len(), 1);
    let replay = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "2",
            "keep",
            QUESTION,
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "2000",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    success(&replay);
    assert_eq!(
        rows(&replay)
            .iter()
            .map(|row| row["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(summary(&replay)["logical_records"], "2");
    assert_eq!(summary(&replay)["cache_hits"], "0");
    let before = std::fs::read(&journal).unwrap();
    let replay = run(
        &config,
        &[
            "--url",
            &url,
            "keep",
            QUESTION,
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    success(&replay);
    assert!(replay.stdout.is_empty());
    assert_eq!(summary(&replay)["attempts"], "0");
    assert_eq!(std::fs::read(&journal).unwrap(), before);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_completed_but_unemitted_result_reuses_its_actual_request_and_distribution() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let input = [
        ("An inventory label was printed successfully. ".repeat(2000) + "\n").as_bytes(),
        b"Checkout is down and customers cannot pay.\n",
    ]
    .concat();
    let first = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "4",
            "keep",
            QUESTION,
            "--split",
            "--combine",
            "any",
            "--threshold",
            "0",
            "--overlap",
            "0",
            "--failed",
            journal.to_str().unwrap(),
            "--budget",
            "12000",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        &input,
    );
    assert_eq!(first.status.code(), Some(4));
    assert!(first.stdout.is_empty());
    let events = frames(&journal);
    let saved = events
        .iter()
        .find(|frame| frame["record"]["record"] == "ready" && frame["record"]["index"] == 2)
        .unwrap_or_else(||panic!("no completed unit 2; stderr={}; events={}",String::from_utf8_lossy(&first.stderr),json!(events.iter().map(|frame|json!({"record":frame["record"]["record"],"index":frame["record"]["index"],"indices":frame["record"]["indices"],"code":frame["record"]["code"],"message":frame["record"]["message"]})).collect::<Vec<_>>() )))["record"]["completed"]
        .clone();
    assert!(
        !events
            .iter()
            .any(|frame| frame["record"]["record"] == "handled" && frame["record"]["index"] == 2)
    );
    let chunk_count = events
        .iter()
        .find(|frame| {
            frame["record"]["record"] == "plan" && frame["record"]["indices"] == json!([1])
        })
        .unwrap()["record"]["provenance"]["provenance"]["chunks"]
        .as_array()
        .unwrap()
        .len();
    let replay = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "4",
            "keep",
            QUESTION,
            "--split",
            "--combine",
            "any",
            "--threshold",
            "0",
            "--overlap",
            "0",
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "1000000",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        &input,
    );
    success(&replay);
    let result = rows(&replay);
    assert_eq!(result.len(), 2);
    let id = saved["answer_id"].as_str().unwrap();
    assert_eq!(
        result[1]["answer"],
        saved["evaluation"]["response"]["answers"][id]
    );
    assert_eq!(result[1]["model"], saved["evaluation"]["response"]["model"]);
    assert_eq!(result[1]["cached"], false);
    assert_eq!(summary(&replay)["logical_records"], chunk_count.to_string());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_unfinished_sort_and_count_recover_the_complete_aggregate_from_saved_results() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let input = b"Checkout is down.\nCheckout is down again.\nCheckout is unavailable.\n";
    for kind in ["sort", "count"] {
        let journal = root.path().join(format!("{kind}.jsonl"));
        let mut common = vec![
            "--url",
            &url,
            "-j",
            "1",
            kind,
            QUESTION,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ];
        if kind == "sort" {
            common.extend(["-c", "low,high"]);
        }
        let mut first = common.clone();
        first.extend(["--failed", journal.to_str().unwrap(), "--budget", "200"]);
        let first = run(&config, &first, input);
        assert_eq!(first.status.code(), Some(4));
        assert!(first.stdout.is_empty());
        let events = frames(&journal);
        assert!(
            events
                .iter()
                .any(|frame| frame["record"]["record"] == "ready" && frame["record"]["index"] == 1)
        );
        assert!(
            !events.iter().any(
                |frame| frame["record"]["record"] == "handled" && frame["record"]["index"] == 1
            )
        );
        let mut retry = common.clone();
        retry.extend(["--resume", journal.to_str().unwrap(), "--budget", "2000"]);
        let retry = run(&config, &retry, input);
        success(&retry);
        assert_eq!(summary(&retry)["logical_records"], "2");
        let output = rows(&retry);
        if kind == "count" {
            assert_eq!(output, vec![json!({"counts":{"yes":3}})]);
        } else {
            assert_eq!(output.len(), 3);
            let mut indices: Vec<_> = output
                .iter()
                .map(|row| row["index"].as_u64().unwrap())
                .collect();
            indices.sort();
            assert_eq!(indices, [1, 2, 3]);
            assert!(
                output
                    .windows(2)
                    .all(|pair| pair[0]["answer"]["score"].as_f64().unwrap()
                        <= pair[1]["answer"]["score"].as_f64().unwrap())
            );
        }
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_saved_joint_request_keeps_its_original_packing_and_per_unit_answers() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let journal = root.path().join("failed.jsonl");
    let input = [
        ("An inventory label was printed successfully. ".repeat(2000) + "\n").as_bytes(),
        b"Checkout is down.\nCheckout is down again.\n",
    ]
    .concat();
    let common = [
        "--url",
        &url,
        "-j",
        "4",
        "keep",
        QUESTION,
        "--pack",
        "window:3",
        "--split",
        "--combine",
        "any",
        "--overlap",
        "0",
        "--threshold",
        "0",
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
    ];
    let mut first = common.to_vec();
    first.extend(["--failed", journal.to_str().unwrap(), "--budget", "12000"]);
    let first = run(&config, &first, &input);
    assert_eq!(first.status.code(), Some(4));
    assert!(first.stdout.is_empty());
    let events = frames(&journal);
    let saved: Vec<_> = events
        .iter()
        .filter(|frame| frame["record"]["record"] == "ready")
        .collect();
    assert_eq!(saved.len(), 2);
    assert!(
        saved
            .iter()
            .all(|frame| frame["record"]["completed"]["packed_indices"] == json!([2, 3]))
    );
    let original_request = saved[0]["record"]["completed"]["request"].clone();
    assert_eq!(original_request, saved[1]["record"]["completed"]["request"]);
    let chunks = events
        .iter()
        .find(|frame| {
            frame["record"]["record"] == "plan" && frame["record"]["indices"] == json!([1])
        })
        .unwrap()["record"]["provenance"]["provenance"]["chunks"]
        .as_array()
        .unwrap()
        .len();
    let mut retry = common.to_vec();
    retry.extend(["--resume", journal.to_str().unwrap(), "--budget", "1000000"]);
    let retry = run(&config, &retry, &input);
    success(&retry);
    let output = rows(&retry);
    assert_eq!(output.len(), 3);
    assert_eq!(summary(&retry)["logical_records"], chunks.to_string());
    for frame in saved {
        let index = frame["record"]["index"].as_u64().unwrap();
        let snapshot = &frame["record"]["completed"];
        let id = snapshot["answer_id"].as_str().unwrap();
        let row = output.iter().find(|row| row["index"] == index).unwrap();
        assert_eq!(
            row["answer"],
            snapshot["evaluation"]["response"]["answers"][id]
        );
        assert_eq!(row["cached"], false);
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_recovery_preserves_sort_count_ask_and_route_output_contracts() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let input = b"Checkout is down.\nCheckout is down again.\n";
    let question_file = root.path().join("questions.json");
    std::fs::write(&question_file,serde_json::to_vec(&json!({"questions":{"present":{"type":"noul","instructions":QUESTION},"state":{"type":"choice","instructions":"Classify checkout","criteria":{"down":"Checkout is down","up":"Checkout is healthy"}}}})).unwrap()).unwrap();
    for kind in ["sort", "count", "ask", "is", "route"] {
        let journal = root.path().join(format!("{kind}.jsonl"));
        let out = root.path().join("out");
        let mut args = vec![
            "--url",
            &url,
            "-j",
            "1",
            kind,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ];
        if kind == "ask" {
            args.extend(["-f", question_file.to_str().unwrap()]);
        } else {
            args.push(QUESTION);
        }
        if kind == "sort" {
            args.extend(["-c", "low,high"]);
        }
        if kind == "route" {
            args.extend(["-c", "down,up", "--out", out.to_str().unwrap()]);
        }
        let mut first = args.clone();
        first.extend(["--failed", journal.to_str().unwrap()]);
        let first = run(&config, &first, input);
        success(&first);
        let before = std::fs::read(&journal).unwrap();
        let mut resume = args.clone();
        resume.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
        let replay = run(&config, &resume, input);
        success(&replay);
        assert!(replay.stdout.is_empty());
        assert_eq!(summary(&replay)["attempts"], "0");
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        if kind == "ask" {
            let rows = rows(&first);
            assert_eq!(rows[0].as_object().unwrap().len(), 3);
        }
        if kind == "count" {
            assert_eq!(rows(&first), vec![json!({"counts":{"yes":2}})]);
        }
        if kind == "route" {
            let routed: Vec<Value> = std::fs::read_to_string(out.join("down"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(
                routed
                    .iter()
                    .map(|row| row["index"].as_u64().unwrap())
                    .collect::<Vec<_>>(),
                [1, 2]
            );
            assert_eq!(
                routed
                    .iter()
                    .map(|row| row["unit"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["Checkout is down.", "Checkout is down again."]
            );
        }
    }
}
