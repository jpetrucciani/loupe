#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const QUESTION: &str =
    "Do the two records mention at least one identical incident label (ALPHA or BRAVO)?";
const BRIDGE: &[u8] = b"Incident ALPHA: checkout is unavailable.\nIncident ALPHA: checkout is unavailable. Incident BRAVO: email delivery is delayed.\nIncident BRAVO: email delivery is delayed.\n";

fn setup(root: &Path) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        include_str!("../../../examples/config.toml").replace("concurrency = 4", "concurrency = 1"),
    )
    .unwrap();
    config
}
fn command(root: &Path, config: &Path, args: &[&str], live: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
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
        .stderr(Stdio::piped());
    command
}
fn run(root: &Path, config: &Path, args: &[&str], input: &[u8], live: bool) -> Output {
    let mut child = command(root, config, args, live).spawn().unwrap();
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
fn attempts(output: &Output) -> u64 {
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("http_attempts="))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn finite_pair_plan_preserves_physical_indices_original_state_and_locations() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let result = run(
        root,
        &config,
        &["dedupe", QUESTION, "--dry-run"],
        "alpha\r\n\nβeta\nlast".as_bytes(),
        false,
    );
    code(&result, 0);
    let planned = rows(&result);
    assert_eq!(
        planned
            .iter()
            .map(|row| row["pair"].clone())
            .collect::<Vec<_>>(),
        vec![json!([1, 3]), json!([1, 4]), json!([3, 4])]
    );
    assert_eq!(
        planned[0]["request"]["state"]["left"],
        json!({"id":"L1","record":"alpha"})
    );
    assert_eq!(
        planned[0]["request"]["state"]["right"],
        json!({"id":"L3","record":"βeta"})
    );
    assert_eq!(planned[0]["counts"]["exact"], json!(true));
    let input = b"{\"id\":\"one\",\"text\":\"ALPHA\"}\n{\"id\":\"two\",\"text\":\"ALPHA\"}\n";
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            QUESTION,
            "-I",
            "jsonl",
            "--field",
            "id",
            "--dry-run",
        ],
        input,
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)[0]["request"]["state"]["left"]["record"]["text"],
        json!("ALPHA")
    );
}

#[test]
fn finite_catalog_waits_for_eof_before_any_pair_output() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let mut child = command(root, &config, &["dedupe", QUESTION, "--dry-run"], false)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        let result = reader.read_until(b'\n', &mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    stdin.write_all(b"ALPHA\nBRAVO\n").unwrap();
    stdin.flush().unwrap();
    assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
    drop(stdin);
    let bytes = receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    code(&child.wait_with_output().unwrap(), 0);
    assert_eq!(
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
            .count(),
        1
    );
}

#[test]
fn finite_validation_pair_caps_and_empty_singleton_need_no_backend() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    for extra in [
        vec!["--keep", "other"],
        vec!["--split"],
        vec!["--keep-going"],
        vec!["--pack", "window"],
        vec!["--dry-run", "--failed", "run.jsonl"],
        vec!["--min-confidence", "0.8"],
        vec!["-O", "csv"],
        vec!["--fallback", "printf yes"],
        vec!["--on-uncertain", "fallback"],
        vec![
            "--on-uncertain",
            "fallback",
            "--fallback",
            "printf yes",
            "--fallback-timeout",
            "0s",
        ],
    ] {
        let mut args = vec!["dedupe", QUESTION];
        args.extend(extra);
        code(&run(root, &config, &args, b"", false), 64);
    }
    code(
        &run(
            root,
            &config,
            &["dedupe", QUESTION, "--max-pairs", "2"],
            BRIDGE,
            false,
        ),
        65,
    );
    let result = run(root, &config, &["dedupe"], b"", false);
    code(&result, 1);
    assert!(result.stdout.is_empty());
    let result = run(root, &config, &["dedupe"], b"only\r\n", false);
    code(&result, 0);
    assert_eq!(result.stdout, b"only\r\n");
}

#[test]
fn finite_multifile_header_mismatch_and_journal_aliases_preserve_sources() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let a = root.join("a.csv");
    let b = root.join("b.csv");
    std::fs::write(&a, b"id,text\na,ALPHA\n").unwrap();
    std::fs::write(&b, b"text,id\nALPHA,b\n").unwrap();
    code(
        &run(
            root,
            &config,
            &[
                "dedupe",
                QUESTION,
                a.to_str().unwrap(),
                b.to_str().unwrap(),
                "-I",
                "csv",
                "--dry-run",
            ],
            b"",
            false,
        ),
        65,
    );
    let file = root.join("one.txt");
    std::fs::write(&file, b"only\n").unwrap();
    code(
        &run(
            root,
            &config,
            &[
                "dedupe",
                QUESTION,
                file.to_str().unwrap(),
                "--failed",
                file.to_str().unwrap(),
            ],
            b"",
            false,
        ),
        74,
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"only\n");
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            "--files",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--dry-run",
        ],
        b"",
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)[0]["request"]["state"]["left"]["id"],
        json!("F1")
    );
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_finite_bridge_uses_negative_endpoint_pair_and_all_representative_rules() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    for (keep, index) in [("first", 1), ("last", 3), ("longest", 2)] {
        let result = run(
            root,
            &config,
            &[
                "dedupe",
                QUESTION,
                "--keep",
                keep,
                "--json",
                "--no-cache",
                "--strict",
                "--summary",
                "--deadline",
                "60s",
            ],
            BRIDGE,
            true,
        );
        code(&result, 0);
        let rows = rows(&result);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["index"], json!(index));
        assert_eq!(
            rows[0]["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|member| member["index"].clone())
                .collect::<Vec<_>>(),
            vec![json!(1), json!(2), json!(3)]
        );
        assert_eq!(
            rows[0]["links"]
                .as_array()
                .unwrap()
                .iter()
                .map(|link| json!([link["left"], link["right"]]))
                .collect::<Vec<_>>(),
            vec![json!([1, 2]), json!([2, 3])]
        );
        assert_eq!(attempts(&result), 3);
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_partial_pair_resume_emits_only_after_all_pairs_and_refuses_changed_sources() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let journal = root.join("run.jsonl");
    let plan = run(
        root,
        &config,
        &["dedupe", QUESTION, "--dry-run"],
        BRIDGE,
        false,
    );
    code(&plan, 0);
    let allowance = rows(&plan)[0]["counts"]["input_tokens"]
        .as_u64()
        .unwrap()
        .to_string();
    let base = vec![
        "dedupe",
        QUESTION,
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut args = base.clone();
    args.extend([
        "--budget",
        &allowance,
        "--failed",
        journal.to_str().unwrap(),
    ]);
    let result = run(root, &config, &args, BRIDGE, true);
    code(&result, 4);
    assert!(result.stdout.is_empty());
    assert_eq!(attempts(&result), 1);
    let mut args = base.clone();
    args.extend(["--budget", "0", "--resume", journal.to_str().unwrap()]);
    let result = run(root, &config, &args, BRIDGE, true);
    code(&result, 4);
    assert!(result.stdout.is_empty());
    assert_eq!(attempts(&result), 0);
    let mut args = base;
    args.extend(["--resume", journal.to_str().unwrap()]);
    let result = run(root, &config, &args, BRIDGE, true);
    code(&result, 0);
    assert_eq!(rows(&result).len(), 1);
    assert_eq!(attempts(&result), 2);
    let result = run(root, &config, &args, BRIDGE, true);
    code(&result, 0);
    assert!(result.stdout.is_empty());
    assert_eq!(attempts(&result), 0);
    let result = run(root, &config, &args, b"changed\n", true);
    code(&result, 65);
    assert!(result.stdout.is_empty());
    assert_eq!(attempts(&result), 0);
}

#[test]
fn streaming_flags_capacity_and_retained_bytes_fail_without_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    for extra in [
        vec!["--window", "0"],
        vec!["--window", "2", "--window-time", "1s"],
        vec!["--window", "2", "--keep", "last"],
        vec!["--window-time", "0s"],
        vec!["--window", "2", "--max-pairs", "3"],
        vec!["--index-capacity", "2"],
        vec!["--max-index-bytes", "1000"],
        vec!["--max-candidates", "3"],
        vec!["--window", "2", "--dry-run", "--failed", "journal.jsonl"],
        vec!["--window", "2", "--fallback", "printf yes"],
        vec!["--window", "2", "--on-uncertain", "fallback"],
    ] {
        let mut args = vec!["dedupe", QUESTION];
        args.extend(extra);
        code(&run(root, &config, &args, b"", false), 64);
    }
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            "--window-time",
            "60s",
            "--index-capacity",
            "1",
            "--summary",
        ],
        b"alpha\nbravo\n",
        false,
    );
    code(&result, 65);
    assert_eq!(result.stdout, b"alpha\n");
    assert_eq!(attempts(&result), 0);
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            "--window",
            "2",
            "--max-index-bytes",
            "1",
            "--summary",
        ],
        b"alpha\n",
        false,
    );
    code(&result, 65);
    assert!(result.stdout.is_empty());
    assert_eq!(attempts(&result), 0);
}

#[test]
fn streaming_first_output_precedes_eof_and_time_expiry_uses_a_real_clock() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let mut child = command(
        root,
        &config,
        &["dedupe", "--window-time", "100ms", "--summary"],
        false,
    )
    .spawn()
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    if sender.send(bytes).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    stdin.write_all(b"ALPHA\n").unwrap();
    stdin.flush().unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
        b"ALPHA\n"
    );
    std::thread::sleep(Duration::from_millis(200));
    stdin.write_all(b"ALPHA\n").unwrap();
    stdin.flush().unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"ALPHA\n"
    );
    drop(stdin);
    let result = child.wait_with_output().unwrap();
    code(&result, 0);
    assert_eq!(attempts(&result), 0);
}

#[test]
fn streaming_dry_run_reports_prospective_candidates_expiry_and_bounded_history() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            "--window",
            "2",
            "--dry-run",
            "--max-candidates",
            "1",
        ],
        b"ALPHA\nALPHA\nALPHA\nALPHA\n",
        false,
    );
    code(&result, 0);
    let planned = rows(&result);
    assert_eq!(planned.len(), 4);
    assert_eq!(planned[0]["pairs"], json!([]));
    assert_eq!(planned[2]["candidates"]["eligible"], json!(2));
    assert_eq!(planned[2]["pairs"].as_array().unwrap().len(), 1);
    assert_eq!(planned[3]["expired"], json!([1]));
    assert_eq!(planned[3]["history"]["representatives"], json!(3));
    assert_eq!(planned[3]["history"]["band_postings"], json!(24));
    let result = run(
        root,
        &config,
        &["dedupe", "--window", "1", "--json", "--summary"],
        b"ALPHA\n\nALPHA\n",
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)
            .iter()
            .map(|row| row["index"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(3)]
    );
    assert_eq!(attempts(&result), 0);
}

const STREAM_QUESTION: &str = "Do both records report an outage of the same service (ALPHA or BRAVO)? A service described as healthy is not in outage. Ignore the unrelated node. A record with both services down matches a record with either service down.";
const STREAM_BRIDGE: &[u8] = b"Incident ALPHA is down. Incident BRAVO is healthy. The unrelated node is healthy.\nIncident ALPHA is down. Incident BRAVO is down. The unrelated node is healthy.\nIncident ALPHA is healthy. Incident BRAVO is down. The unrelated node is healthy.\n";

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_streaming_dropped_bridge_does_not_merge_representatives_retroactively() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            STREAM_QUESTION,
            "--window",
            "10",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        STREAM_BRIDGE,
        true,
    );
    code(&result, 0);
    let streamed = rows(&result);
    assert_eq!(
        streamed
            .iter()
            .map(|row| row["index"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(3)]
    );
    assert_eq!(attempts(&result), 2);
    assert_eq!(
        streamed[1]["pairs"][0]["assessment"]["selected"]["boolean"],
        json!(false)
    );
    assert_eq!(
        streamed[1]["pairs"][0]["counts"]["input_tokens"],
        streamed[1]["pairs"][0]["response"]["usage"]["input_tokens"]
    );
    let finite = run(
        root,
        &config,
        &[
            "dedupe",
            STREAM_QUESTION,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        STREAM_BRIDGE,
        true,
    );
    code(&finite, 0);
    assert_eq!(rows(&finite).len(), 1);
    assert_eq!(attempts(&finite), 3);
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_streaming_unit_expiry_and_uncertainty_keep_only_first_representatives() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let input=b"Incident ALPHA is delayed.\r\nIncident ALPHA is delayed.\r\nIncident ALPHA is delayed.\r\n";
    let result = run(
        root,
        &config,
        &[
            "dedupe",
            QUESTION,
            "--window",
            "1",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        input,
        true,
    );
    code(&result, 0);
    assert_eq!(
        result.stdout,
        b"Incident ALPHA is delayed.\r\nIncident ALPHA is delayed.\r\n"
    );
    assert_eq!(attempts(&result), 1);
    for (action, expected, output_count) in [("keep", 0, 1), ("drop", 0, 2), ("fail", 3, 1)] {
        let result = run(
            root,
            &config,
            &[
                "dedupe",
                QUESTION,
                "--window",
                "2",
                "--margin",
                "1",
                "--on-uncertain",
                action,
                "--json",
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
            ],
            b"ALPHA\nALPHA\n",
            true,
        );
        code(&result, expected);
        assert_eq!(rows(&result).len(), output_count);
        assert_eq!(attempts(&result), 1);
    }
}

#[test]
#[ignore = "requires actual server metadata/inference; no fake backend"]
fn server_streaming_resume_reuses_partial_pairs_and_refuses_changed_source_and_policy() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let journal = root.join("stream.jsonl");
    let input = b"ALPHA\nALPHA\nALPHA\n";
    let base = vec![
        "dedupe",
        QUESTION,
        "--window",
        "10",
        "--margin",
        "1",
        "--on-uncertain",
        "drop",
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut args = base.clone();
    args.push("--dry-run");
    let plan = run(root, &config, &args, input, false);
    code(&plan, 0);
    let count = rows(&plan)[1]["pairs"][0]["counts"]["input_tokens"]
        .as_u64()
        .unwrap();
    let allowance = (count * 2).to_string();
    let mut args = base.clone();
    args.extend([
        "--budget",
        &allowance,
        "--failed",
        journal.to_str().unwrap(),
    ]);
    let first = run(root, &config, &args, input, true);
    code(&first, 4);
    assert_eq!(rows(&first).len(), 2);
    assert_eq!(attempts(&first), 2);
    let mut args = base.clone();
    args.extend(["--budget", "0", "--resume", journal.to_str().unwrap()]);
    let repeat = run(root, &config, &args, input, true);
    code(&repeat, 4);
    assert!(repeat.stdout.is_empty());
    assert_eq!(attempts(&repeat), 0);
    let before = std::fs::read(&journal).unwrap();
    let changed = run(root, &config, &args, b"BRAVO\nALPHA\nALPHA\n", true);
    code(&changed, 65);
    assert!(changed.stdout.is_empty());
    assert_eq!(attempts(&changed), 0);
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    let mut args = base.clone();
    args.extend(["--threshold", "0.6", "--resume", journal.to_str().unwrap()]);
    let changed = run(root, &config, &args, input, true);
    code(&changed, 65);
    assert!(changed.stdout.is_empty());
    assert_eq!(attempts(&changed), 0);
    let allowance = count.to_string();
    let mut args = base.clone();
    args.extend([
        "--budget",
        &allowance,
        "--resume",
        journal.to_str().unwrap(),
    ]);
    let resumed = run(root, &config, &args, input, true);
    code(&resumed, 0);
    let resumed_rows = rows(&resumed);
    assert_eq!(resumed_rows.len(), 1);
    assert_eq!(resumed_rows[0]["index"], json!(3));
    assert_eq!(resumed_rows[0]["pairs"].as_array().unwrap().len(), 2);
    assert_eq!(attempts(&resumed), 1);
    let before = std::fs::read(&journal).unwrap();
    let mut args = base;
    args.extend(["--budget", "0", "--resume", journal.to_str().unwrap()]);
    let done = run(root, &config, &args, input, true);
    code(&done, 0);
    assert!(done.stdout.is_empty());
    assert_eq!(attempts(&done), 0);
    assert_eq!(std::fs::read(&journal).unwrap(), before);
}

#[test]
#[ignore = "requires actual server metadata/inference; no fake backend"]
fn server_streaming_uncertain_failure_and_csv_resume_preserve_raw_answers_and_header() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let journal = root.join("uncertain.jsonl");
    let base = vec![
        "dedupe",
        QUESTION,
        "--window",
        "2",
        "--margin",
        "1",
        "--on-uncertain",
        "fail",
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut args = base.clone();
    args.extend(["--failed", journal.to_str().unwrap()]);
    let first = run(root, &config, &args, b"ALPHA\nALPHA\n", true);
    code(&first, 3);
    assert_eq!(rows(&first).len(), 1);
    assert_eq!(attempts(&first), 1);
    let mut args = base;
    args.extend(["--budget", "0", "--resume", journal.to_str().unwrap()]);
    let replay = run(root, &config, &args, b"ALPHA\nALPHA\n", true);
    code(&replay, 3);
    assert!(replay.stdout.is_empty());
    assert_eq!(attempts(&replay), 0);
    let journal = root.join("csv.jsonl");
    let input = b"id,text\r\na,ALPHA\r\nb,ALPHA\r\n";
    let base = vec![
        "dedupe",
        QUESTION,
        "--window",
        "2",
        "-I",
        "csv",
        "--select",
        "text",
        "--margin",
        "1",
        "--on-uncertain",
        "drop",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut args = base.clone();
    args.extend(["--budget", "0", "--failed", journal.to_str().unwrap()]);
    let first = run(root, &config, &args, input, true);
    code(&first, 4);
    assert_eq!(first.stdout, b"id,text\r\na,ALPHA\r\n");
    assert_eq!(attempts(&first), 0);
    let mut args = base;
    args.extend(["--resume", journal.to_str().unwrap()]);
    let replay = run(root, &config, &args, input, true);
    code(&replay, 0);
    assert_eq!(replay.stdout, b"b,ALPHA\r\n");
    assert_eq!(attempts(&replay), 1);
}

#[test]
#[ignore = "requires actual server metadata and real SIGTERM; no fake backend"]
fn server_streaming_time_window_resume_expires_downtime_history_before_new_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let journal = root.join("time.jsonl");
    let base = vec![
        "dedupe",
        QUESTION,
        "--window-time",
        "100ms",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut args = base.clone();
    args.extend(["--failed", journal.to_str().unwrap()]);
    let mut child = command(root, &config, &args, true).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        let result = reader.read_until(b'\n', &mut line).map(|_| line);
        let _ = sender.send(result);
    });
    stdin.write_all(b"ALPHA\n").unwrap();
    stdin.flush().unwrap();
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap(),
        b"ALPHA\n"
    );
    let acknowledged = std::time::Instant::now();
    loop {
        let frames = std::fs::read_to_string(&journal).unwrap();
        if frames
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|frame| frame["record"]["record"] == json!("handled"))
        {
            break;
        }
        assert!(acknowledged.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let stopped = child.wait_with_output().unwrap();
    code(&stopped, 143);
    drop(stdin);
    std::thread::sleep(Duration::from_millis(200));
    let mut args = base;
    args.extend(["--budget", "0", "--resume", journal.to_str().unwrap()]);
    let resumed = run(root, &config, &args, b"ALPHA\nALPHA\n", true);
    code(&resumed, 0);
    assert_eq!(resumed.stdout, b"ALPHA\n");
    assert_eq!(attempts(&resumed), 0);
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_dedupe_fallback_preserves_pair_evidence_and_replays_finite_and_streaming() {
    for streaming in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = setup(root);
        let journal = root.join("run.jsonl");
        let input = b"ALPHA\r\nALPHA\r\n";
        let fallback = "cat > pair.json; printf once >> calls; printf yes";
        let mut base = vec![
            "dedupe",
            QUESTION,
            "--margin",
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
        if streaming {
            base.extend(["--window", "2"]);
        }
        let mut args = base.clone();
        args.extend(["--failed", journal.to_str().unwrap()]);
        let first = run(root, &config, &args, input, true);
        code(&first, 0);
        let output = rows(&first);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["index"], json!(1));
        let state: Value =
            serde_json::from_slice(&std::fs::read(root.join("pair.json")).unwrap()).unwrap();
        assert_eq!(state["left"]["id"], json!("L1"));
        assert_eq!(state["right"]["id"], json!("L2"));
        assert_eq!(state["left"]["record"], json!("ALPHA"));
        assert_eq!(state["right"]["record"], json!("ALPHA"));
        let pair = if streaming {
            std::fs::read_to_string(&journal)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find_map(|frame| {
                    let record = &frame["record"];
                    (record["record"] == "ready" && record["index"] == 2)
                        .then(|| record["completed"]["row"]["pairs"][0].clone())
                })
                .unwrap()
        } else {
            output[0]["links"][0].clone()
        };
        assert_eq!(pair["assessment"]["uncertain"], json!(true));
        assert_eq!(pair["decision"]["selected"], json!({"boolean":true}));
        assert_eq!(pair["decision"]["uncertain"], json!(false));
        assert_eq!(pair["decision"]["source"], json!("fallback"));
        assert!(pair["decision"]["confidence"].is_null());
        assert_eq!(pair["fallback"]["status"], json!("resolved"));
        assert_eq!(attempts(&first), 1);
        assert_eq!(std::fs::read(root.join("calls")).unwrap(), b"once");
        let mut args = base;
        args.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
        let repeated = run(root, &config, &args, input, true);
        code(&repeated, 0);
        assert!(repeated.stdout.is_empty());
        assert_eq!(attempts(&repeated), 0);
        assert_eq!(std::fs::read(root.join("calls")).unwrap(), b"once");
        if streaming {
            assert_eq!(
                pair["counts"]["input_tokens"],
                pair["response"]["usage"]["input_tokens"]
            );
        }
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_dedupe_fallback_failure_is_durable_for_finite_and_streaming_resume() {
    for streaming in [false, true] {
        for fatal in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path();
            let config = setup(root);
            let journal = root.join("run.jsonl");
            let input = b"ALPHA\nALPHA\n";
            let fallback = "cat > /dev/null; printf once >> calls; printf invalid";
            let mut base = vec![
                "dedupe",
                QUESTION,
                "--margin",
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
            if fatal {
                base.extend(["--on-fallback-error", "fail"]);
            }
            if streaming {
                base.extend(["--window", "2"]);
            }
            for flag in ["--failed", "--resume"] {
                let mut args = base.clone();
                args.extend([flag, journal.to_str().unwrap()]);
                if flag == "--resume" {
                    args.extend(["--budget", "0"]);
                }
                let result = run(root, &config, &args, input, true);
                code(&result, if fatal { 69 } else { 0 });
                assert_eq!(
                    rows(&result).len(),
                    if flag == "--resume" {
                        0
                    } else if fatal {
                        usize::from(streaming)
                    } else {
                        2
                    }
                );
                assert_eq!(attempts(&result), u64::from(flag == "--failed"));
                assert_eq!(std::fs::read(root.join("calls")).unwrap(), b"once");
                if fatal {
                    assert!(
                        String::from_utf8_lossy(&result.stderr)
                            .contains("fallback failed under --on-fallback-error fail")
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires actual server model and real process signals; no fake backend"]
#[allow(clippy::panic)]
fn server_dedupe_fallback_sigterm_cleans_child_and_preserves_stream_prefix() {
    for streaming in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = setup(root);
        let fallback = "cat > /dev/null; printf '%s' \"$$\" > child.pid; exec sleep 30";
        let mut args = vec![
            "dedupe",
            QUESTION,
            "--margin",
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
            "30s",
        ];
        if streaming {
            args.extend(["--window", "2"]);
        }
        let mut child = command(root, &config, &args, true).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"ALPHA\nALPHA\n")
            .unwrap();
        let expires = std::time::Instant::now() + Duration::from_secs(15);
        let marker = root.join("child.pid");
        while !marker.exists() && std::time::Instant::now() < expires {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !marker.exists() {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "fallback never started: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let owned_pid = std::fs::read_to_string(&marker).unwrap();
        assert!(owned_pid.trim().parse::<u32>().unwrap() > 1);
        let signal = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(signal.success());
        let expires = std::time::Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() && std::time::Instant::now() < expires {
            std::thread::sleep(Duration::from_millis(10));
        }
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
        }
        let result = child.wait_with_output().unwrap();
        code(&result, 143);
        assert_eq!(rows(&result).len(), usize::from(streaming));
        assert!(!Path::new(&format!("/proc/{}", owned_pid.trim())).exists());
        assert!(String::from_utf8_lossy(&result.stderr).contains("fallback_cancelled=1"));
    }
}
