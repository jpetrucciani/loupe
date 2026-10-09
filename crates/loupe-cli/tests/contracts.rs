#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
        ])
        .args(args)
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

#[test]
fn offline_token_plan_is_exact_and_does_not_connect() {
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout unavailable?",
            "--dry-run",
            "-I",
            "lines",
        ],
        b"Checkout is down.\r\n\nAll systems healthy.\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["index"], 1);
    assert_eq!(records[1]["index"], 3);
    assert_eq!(records[0]["request"]["state"], "Checkout is down.");
    assert_eq!(records[0]["counts"]["exact"], true);
    assert!(output.stderr.is_empty());
}

#[test]
fn grammar_rejects_wrong_type_flags_removed_aliases_and_bad_policy() {
    for args in [
        vec!["tag", "-p", "severity", "--threshold", "0.5"],
        vec!["keep", "question", "--min-confidence", "0.8"],
        vec!["keep", "question", "--jsonl"],
        vec!["keep", "question", "--tsv"],
        vec!["keep", "question", "--threshold", "NaN"],
        vec!["is", "question", "--on-uncertain", "keep"],
    ] {
        let output = run(&args, b"");
        assert_eq!(
            output.status.code(),
            Some(64),
            "args={args:?} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn completions_man_version_and_broken_pipes_need_no_backend() {
    for args in [vec!["--version"], vec!["completions", "bash"], vec!["man"]] {
        let output = run(&args, b"");
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
    }
    let manual = run(&["man"], b"");
    let rendered = String::from_utf8(manual.stdout).unwrap();
    for command in [
        "LOUPE PICK",
        "LOUPE MCP",
        "LOUPE PRESETS TRUST",
        "LOUPE HOOK INSTALL",
    ] {
        assert!(rendered.contains(command));
    }
    assert!(rendered.contains("max\\-rounds"));
    assert!(rendered.contains("allow\\-unverified"));
    let selected = run(&["man", "pick"], b"");
    assert!(selected.status.success());
    let selected = String::from_utf8(selected.stdout).unwrap();
    assert!(selected.contains("max\\-rounds"));
    assert!(!selected.contains("LOUPE HOOK INSTALL"));
    let nested = run(&["man", "presets", "trust"], b"");
    assert!(nested.status.success());
    assert!(String::from_utf8_lossy(&nested.stdout).contains("NAME"));
    let invalid = run(&["man", "pick", "bogus"], b"");
    assert_eq!(invalid.status.code(), Some(64));
    assert!(invalid.stdout.is_empty());
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args(["completions", "bash"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(child.wait_with_output().unwrap().status.success());
}

#[test]
fn structured_question_file_and_argument_context_are_counted_offline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("question.json");
    std::fs::write(&path, "7").unwrap();
    let malformed = run(
        &[
            "tag",
            "-f",
            path.to_str().unwrap(),
            "-c",
            "a,b",
            "--dry-run",
        ],
        b"",
    );
    assert_eq!(malformed.status.code(), Some(64));
    std::fs::write(&path, r#"{"type":"noul","instructions":{"query":"{{arg.ref}}","weight":1e16},"criteria":{"true":"Present","false":null}}"#).unwrap();
    let output = run(
        &["tokens", "-f", path.to_str().unwrap(), "--arg", "ref=東京"],
        b"original state",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        record["request"]["questions"]["decision"]["instructions"]["query"],
        "東京"
    );
    assert_eq!(record["counts"]["exact"], true);
}

#[test]
fn blank_file_indices_null_units_and_offline_oversize_remain_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("one.txt");
    let second = dir.path().join("two.txt");
    std::fs::write(&first, b"first\n\n").unwrap();
    std::fs::write(&second, b"second\n").unwrap();
    let output = run(
        &[
            "keep",
            "question",
            "--dry-run",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
        b"",
    );
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records[1]["index"], 3);
    let output = run(
        &["keep", "question", "--dry-run", "-I", "null"],
        b"first\0second\0",
    );
    assert!(output.status.success());
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records[1]["request"]["state"], "second");
    let output = run(
        &["is", "question", "--dry-run"],
        " event".repeat(20_000).as_bytes(),
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("total="));
}

#[test]
fn structured_inputs_send_objects_and_preserve_selected_subjects_and_context() {
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("uncertain.csv");
    let output = run(
        &[
            "keep",
            "question",
            "--dry-run",
            "--uncertain",
            output_path.to_str().unwrap(),
        ],
        b"a unit\n",
    );
    assert!(output.status.success());
    assert!(!output_path.exists());
    let output = run(
        &[
            "keep",
            "Does msg describe an outage?",
            "--dry-run",
            "-I",
            "jsonl",
            "--field",
            "msg",
        ],
        b"{\"msg\":\"outage\",\"id\":1267650600228229401496703205376}\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        plan["request"]["state"]["id"].to_string(),
        "1267650600228229401496703205376"
    );
    assert_eq!(plan["request"]["state"]["msg"], "outage");
    let output = run(
        &[
            "keep",
            "question",
            "--dry-run",
            "-I",
            "csv",
            "--select",
            "msg",
        ],
        b"id,msg\r\n1,\"first\nsecond\"\r\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        plan["request"]["state"],
        serde_json::json!({"msg":"first\nsecond"})
    );
    let output = run(
        &["keep", "question", "--dry-run", "--with-context", "1"],
        b"earlier\n\ncurrent\n",
    );
    assert!(output.status.success());
    let plans: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(plans[1]["index"], 3);
    assert_eq!(
        plans[1]["request"]["state"]["previous_lines"][0]["id"],
        "L2"
    );
    assert_eq!(
        plans[1]["request"]["questions"]["decision"]["instructions"]["unit"],
        "`unit`"
    );
    for args in [
        vec!["keep", "q", "--dry-run", "-I", "records"],
        vec!["keep", "q", "--dry-run", "-I", "lines", "--select", "x"],
        vec![
            "keep",
            "q",
            "--dry-run",
            "-I",
            "jsonl",
            "--with-context",
            "0",
        ],
    ] {
        let output = run(&args, b"");
        assert_eq!(output.status.code(), Some(64));
    }
}

#[test]
fn files_mode_sends_content_but_identifies_the_file_as_the_subject() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("incident.txt");
    std::fs::write(&file, "an outage\n").unwrap();
    let output = run(
        &[
            "keep",
            "question",
            "--files",
            "--dry-run",
            file.to_str().unwrap(),
        ],
        b"",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["request"]["state"]["path"], file.to_str().unwrap());
    assert_eq!(plan["request"]["state"]["content"], "an outage\n");
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_structured_filters_preserve_csv_bytes_headers_and_jsonl_field_echo() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let input = b"id,msg\r\n1,\"Checkout is down.\nCustomers cannot place orders.\"\r\n2,All services healthy.\r\n";
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Does msg report checkout currently unavailable?",
            "-I",
            "csv",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"id,msg\r\n1,\"Checkout is down.\nCustomers cannot place orders.\"\r\n"
    );
    let output = run(&["--url", &endpoint, "keep", "Does msg report an ongoing checkout outage?", "-I", "jsonl", "--field", "msg", "--strict", "--no-cache"], b"{\"id\":1267650600228229401496703205376,\"msg\":\"Checkout is down.\"}\n{\"msg\":\"Checkout is healthy.\"}\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Checkout is down.\n");
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_ask_histogram_and_stable_sort_have_command_specific_contracts() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let question_file = dir.path().join("incident.toml");
    std::fs::write(&question_file, "[questions.outage]\ntype='noul'\ninstructions='Is checkout unavailable?'\n[questions.severity]\ntype='choice'\n[questions.severity.criteria]\npage='Service unavailable'\nnoise='Informational success'\n").unwrap();
    let input = b"Checkout is down. Customers cannot place orders.\n";
    let ask = run(
        &[
            "--url",
            &endpoint,
            "ask",
            "-f",
            question_file.to_str().unwrap(),
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert!(
        ask.status.success(),
        "{}",
        String::from_utf8_lossy(&ask.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&ask.stdout).unwrap();
    assert_eq!(response.as_object().unwrap().len(), 3);
    assert_eq!(response["answers"].as_object().unwrap().len(), 2);
    assert!(response["answers"]["outage"]["noul"].as_f64().unwrap() > 0.5);
    let input = b"This is a wonderful successful day.\nThis is a terrible painful disaster.\nThis is a wonderful successful day.\n";
    let sorted = run(
        &[
            "--url",
            &endpoint,
            "sort",
            "-p",
            "sentiment",
            "--json",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert!(sorted.status.success());
    let records: Vec<serde_json::Value> = String::from_utf8(sorted.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        records
            .iter()
            .map(|value| value["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2, 1, 3]
    );
    let count = run(
        &[
            "--url",
            &endpoint,
            "count",
            "Is this positive?",
            "--json",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert!(count.status.success());
    let histogram: serde_json::Value = serde_json::from_slice(&count.stdout).unwrap();
    assert_eq!(histogram["counts"]["yes"], 2);
    assert_eq!(histogram["counts"]["no"], 1);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_fatal_oversize_flushes_only_completed_prefix() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let mut input = b"Checkout is down. Customers cannot place orders.\n".to_vec();
    input.extend(" event".repeat(20_000).as_bytes());
    input.extend(b"\nCheckout remains down.\n");
    let output = run(
        &[
            "--url",
            &endpoint,
            "-j",
            "1",
            "keep",
            "Is checkout unavailable?",
            "--strict",
            "--no-cache",
        ],
        &input,
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(
        output.stdout,
        b"Checkout is down. Customers cannot place orders.\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("total="));
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_filters_partition_preserve_bytes_and_reject_budget_before_submission() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let input = b"Checkout is down. Customers cannot place orders.\r\nAll services healthy. Checkout passed every test.\n";
    let keep = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently unavailable?",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    let drop = run(
        &[
            "--url",
            &endpoint,
            "drop",
            "Is checkout currently unavailable?",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert!(
        keep.status.success(),
        "{}",
        String::from_utf8_lossy(&keep.stderr)
    );
    assert!(
        drop.status.success(),
        "{}",
        String::from_utf8_lossy(&drop.stderr)
    );
    assert_eq!(
        keep.stdout,
        b"Checkout is down. Customers cannot place orders.\r\n"
    );
    assert_eq!(
        drop.stdout,
        b"All services healthy. Checkout passed every test.\n"
    );
    let limited = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently unavailable?",
            "--budget",
            "1",
            "--no-cache",
        ],
        input,
    );
    assert_eq!(limited.status.code(), Some(4));
    assert!(limited.stdout.is_empty());
    let uncertain = run(
        &[
            "--url",
            &endpoint,
            "is",
            "Is checkout currently unavailable?",
            "--margin",
            "1",
            "--strict",
            "--no-cache",
        ],
        &input[..48],
    );
    assert_eq!(uncertain.status.code(), Some(2));
    assert!(uncertain.stdout.is_empty());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_tag_rate_json_order_distribution_and_unsupported_models() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let input = b"Checkout is down. Customers cannot place orders.\nScheduled heartbeat completed successfully.\n";
    for args in [
        vec![
            "--url",
            &endpoint,
            "tag",
            "-p",
            "severity",
            "--json",
            "--strict",
            "--no-cache",
        ],
        vec![
            "--url",
            &endpoint,
            "rate",
            "-p",
            "sentiment",
            "--json",
            "--strict",
            "--no-cache",
        ],
    ] {
        let output = run(&args, input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["index"], 1);
        assert_eq!(records[1]["index"], 2);
        assert_eq!(records[0].as_object().unwrap().len(), 5);
        assert!(records[0]["answer"]["probabilities"].is_object());
        assert_eq!(records[0]["cached"], false);
    }
    let models = run(&["--url", &endpoint, "models"], b"");
    assert_eq!(models.status.code(), Some(69));
    assert!(String::from_utf8_lossy(&models.stderr).contains("does not support"));
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_uncertainty_csv_appends_header_once_and_refuses_input_aliases() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let uncertain = dir.path().join("uncertain.csv");
    let input = b"id,msg\r\n1,Checkout is down.\r\n";
    for _ in 0..2 {
        let output = run(
            &[
                "--url",
                &endpoint,
                "keep",
                "Is checkout unavailable?",
                "-I",
                "csv",
                "--margin",
                "1",
                "--uncertain",
                uncertain.to_str().unwrap(),
                "--strict",
                "--no-cache",
            ],
            input,
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    assert_eq!(
        std::fs::read(&uncertain).unwrap(),
        b"id,msg\r\n1,Checkout is down.\r\n1,Checkout is down.\r\n"
    );
    std::fs::write(&uncertain, b"different,header\n").unwrap();
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout unavailable?",
            "-I",
            "csv",
            "--margin",
            "1",
            "--uncertain",
            uncertain.to_str().unwrap(),
            "--strict",
            "--no-cache",
        ],
        input,
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(std::fs::read(&uncertain).unwrap(), b"different,header\n");
    let source = dir.path().join("input.csv");
    let alias = dir.path().join("alias.csv");
    std::fs::write(&source, input).unwrap();
    std::fs::hard_link(&source, &alias).unwrap();
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout unavailable?",
            "-I",
            "csv",
            "--uncertain",
            alias.to_str().unwrap(),
            source.to_str().unwrap(),
            "--no-cache",
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(64));
    assert_eq!(std::fs::read(&source).unwrap(), input);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_unit_batches_preserve_order_and_flush_valid_prefix_before_preflight_failure() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let input = b"Checkout is down.\nCheckout is healthy.\nCheckout remains down.\nAll checkout checks pass.\n";
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout unavailable?",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"Checkout is down.\nCheckout remains down.\n"
    );
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(
        summary.contains("units=4 attempts=4 http_attempts=1"),
        "{summary}"
    );
    let mut input = b"Checkout is down.\n".to_vec();
    input.extend(" event".repeat(20_000).as_bytes());
    input.extend(b"\nCheckout remains down.\n");
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout unavailable?",
            "--strict",
            "--no-cache",
        ],
        &input,
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(output.stdout, b"Checkout is down.\n");
}

#[cfg(unix)]
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
#[allow(clippy::panic)]
fn server_idle_pipe_flushes_before_eof_and_sigterm_cancels_with_a_clean_prefix() {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;
    use std::time::Duration;
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
            "--url",
            &endpoint,
            "keep",
            "Is checkout unavailable?",
            "--strict",
            "--no-cache",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = BufReader::new(stdout);
        let mut line = Vec::new();
        output.read_until(b'\n', &mut line).unwrap();
        sender.send(line).unwrap();
        let mut rest = Vec::new();
        std::io::Read::read_to_end(&mut output, &mut rest).unwrap();
        rest
    });
    input.write_all(b"Checkout is down.\n").unwrap();
    input.flush().unwrap();
    let result = receiver.recv_timeout(Duration::from_secs(10));
    if result.is_err() {
        let _ = child.kill();
        drop(input);
        let stopped = child.wait_with_output().unwrap();
        panic!(
            "no idle output before EOF: {}",
            String::from_utf8_lossy(&stopped.stderr)
        );
    }
    assert_eq!(result.unwrap(), b"Checkout is down.\n");
    assert!(child.try_wait().unwrap().is_none());
    assert!(
        Command::new("sh")
            .args([
                "-c",
                "kill -TERM \"$1\"",
                "loupe-test",
                &child.id().to_string()
            ])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(143),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(reader.join().unwrap().is_empty());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_real_classification_broken_pipe_exits_successfully() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
            "--url",
            &endpoint,
            "tag",
            "-p",
            "severity",
            "--json",
            "--strict",
            "--no-cache",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down.\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn window_plans_keep_physical_ids_context_paths_and_emit_one_plan_per_request() {
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is this an active checkout failure?",
            "--dry-run",
            "--pack",
            "window:4",
            "--with-context",
            "1",
        ],
        b"earlier\n\ncurrent\nlast\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["indices"], serde_json::json!([1, 3, 4]));
    assert_eq!(plan["request"]["state"]["units"]["L3"]["unit"], "current");
    assert_eq!(
        plan["request"]["state"]["units"]["L3"]["previous_lines"][0]["id"],
        "L2"
    );
    assert_eq!(
        plan["request"]["questions"]["L3"]["instructions"]["unit"],
        "`units.L3.unit`"
    );
    assert_eq!(
        plan["request"]["questions"]["L3"]["instructions"]["context"],
        "`units.L3.previous_lines`"
    );
    assert_eq!(
        plan["request"]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["L1", "L3", "L4"]
    );
    assert_eq!(plan["counts"]["exact"], true);
    let output = run(
        &["keep", "question", "--dry-run", "--pack", "auto"],
        b"one\ntwo\n",
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 2);
    for args in [
        vec!["keep", "q", "--pack", "window:0"],
        vec!["is", "q", "--pack", "window:4"],
    ] {
        assert_eq!(run(&args, b"").status.code(), Some(64));
    }
}

#[test]
fn window_halving_preserves_prefix_and_final_single_unit_failure() {
    let unit = " event".repeat(5000) + "\n";
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "tokens",
            "--pack",
            "window:4",
            "-I",
            "lines",
        ],
        unit.repeat(4).as_bytes(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plans: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0]["indices"], serde_json::json!([1, 2]));
    assert_eq!(plans[1]["indices"], serde_json::json!([3, 4]));
    assert!(
        plans
            .iter()
            .all(|plan| plan["counts"]["input_tokens"].as_u64().unwrap() <= 16384)
    );
    let mut input = b"short unit\n".to_vec();
    input.extend((" event".repeat(20_000) + "\n").as_bytes());
    let output = run(&["tokens", "--pack", "window:4", "-I", "lines"], &input);
    assert_eq!(output.status.code(), Some(65));
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["indices"], serde_json::json!([1]));
    assert!(String::from_utf8_lossy(&output.stderr).contains("size 1"));
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_joint_windows_map_each_raw_answer_back_to_its_original_subject() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let input=b"Checkout is down.\r\n\nCheckout is healthy.\nCheckout remains unavailable.\nCheckout works normally.\n";
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently unavailable?",
            "--pack",
            "window:4",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"Checkout is down.\r\nCheckout remains unavailable.\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("units=4 attempts=1 http_attempts=1"));
    for args in [
        vec!["tag", "-p", "severity"],
        vec!["rate", "-p", "sentiment"],
    ] {
        let mut command = vec!["--url", &endpoint];
        command.extend(args);
        command.extend(["--pack", "window:4", "--json", "--strict", "--no-cache"]);
        let output = run(
            &command,
            b"Checkout is down.\nHeartbeat completed successfully.\n",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let units: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0]["index"], 1);
        assert_eq!(units[1]["index"], 2);
        assert_eq!(units[0]["unit"], "Checkout is down.");
        assert_eq!(units[1]["unit"], "Heartbeat completed successfully.");
        assert_eq!(units[0].as_object().unwrap().len(), 5);
        assert!(
            units
                .iter()
                .all(|unit| unit["answer"]["probabilities"].is_object())
        );
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_windows_use_structured_records_and_flush_a_valid_prefix_before_overflow() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Does msg describe an active checkout outage?",
            "--pack",
            "window:4",
            "-I",
            "csv",
            "--strict",
            "--no-cache",
        ],
        b"id,msg\r\n1,Checkout is down.\r\n2,Checkout is healthy.\r\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"id,msg\r\n1,Checkout is down.\r\n");
    let mut input = b"Checkout is down.\n".to_vec();
    input.extend((" event".repeat(20_000) + "\n").as_bytes());
    input.extend(b"Checkout remains down.\n");
    let output = run(
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently unavailable?",
            "--pack",
            "window:4",
            "--strict",
            "--no-cache",
        ],
        &input,
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(output.stdout, b"Checkout is down.\n");
}
