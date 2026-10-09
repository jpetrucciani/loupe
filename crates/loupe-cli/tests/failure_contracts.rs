#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
fn run(config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args(["--config", config.to_str().unwrap(), "--profile", "server"])
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
fn metrics(output: &Output) -> BTreeMap<String, String> {
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    stderr
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
fn json_rows(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn oversized() -> Vec<u8> {
    [b"word ".repeat(2000), "東京🙂\r\n".as_bytes().to_vec()].concat()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn failure_flags_reject_inapplicable_commands_and_require_keep_going() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    for command in ["is", "ask", "tokens"] {
        assert_eq!(
            run(
                &config,
                &[command, "Is checkout down?", "--keep-going"],
                b"x\n"
            )
            .status
            .code(),
            Some(64)
        );
    }
    for args in [
        vec!["keep", "Is checkout down?", "--on-error", "keep"],
        vec!["keep", "Is checkout down?", "--keep-going", "--dry-run"],
        vec![
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--on-error",
            "yes",
        ],
    ] {
        assert_eq!(run(&config, &args, b"x\n").status.code(), Some(64));
    }
}

#[test]
fn preflight_kept_failures_preserve_raw_units_and_never_create_probabilities() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let unit = oversized();
    let input = [unit.as_slice(), unit.as_slice()].concat();
    for pack in ["unit", "window:4"] {
        for command in ["keep", "drop", "tag", "rate", "sort"] {
            let mut args = vec![
                "--url",
                "http://127.0.0.1:1",
                command,
                "Is checkout down?",
                "--pack",
                pack,
                "--keep-going",
                "--on-error",
                "keep",
                "--json",
                "--summary",
            ];
            if matches!(command, "tag" | "rate" | "sort") {
                args.extend(["-c", "low,high"]);
            }
            let output = run(&config, &args, &input);
            success(&output);
            let rows = json_rows(&output);
            assert_eq!(rows.len(), 2);
            for (position, row) in rows.iter().enumerate() {
                assert_eq!(row["index"], position + 1);
                assert!(row["answer"].is_null());
                assert!(row["model"].is_null());
                assert_eq!(row["cached"], false);
                assert_eq!(row["error"]["code"], 65);
                assert!(row["error"]["message"].is_string());
            }
            let summary = metrics(&output);
            assert_eq!(summary["errors"], "2");
            assert_eq!(summary["uncertain"], "0");
            assert_eq!(summary["attempts"], "0");
        }
    }
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--on-error",
            "keep",
        ],
        &input,
    );
    success(&output);
    assert_eq!(output.stdout, input);
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "tag",
            "Is checkout down?",
            "-c",
            "down,up",
            "--keep-going",
            "--on-error",
            "keep",
            "-O",
            "tsv",
        ],
        &unit,
    );
    success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    let columns: Vec<_> = text.trim_end().split('\t').collect();
    assert_eq!(columns.len(), 4);
    assert_eq!(columns[1], "error");
    assert!(columns[2].is_empty());
    assert!(columns[3].contains("東京🙂"));
}

#[test]
fn drop_fail_counts_and_structured_failures_remain_distinct_from_uncertainty() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let unit = oversized();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--summary",
        ],
        &unit,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(metrics(&output)["errors"], "1");
    for keep in [false, true] {
        let mut args = vec![
            "--url",
            "http://127.0.0.1:1",
            "count",
            "Is checkout down?",
            "--keep-going",
            "--json",
        ];
        if keep {
            args.extend(["--on-error", "keep"]);
        }
        let output = run(&config, &args, &unit);
        success(&output);
        assert_eq!(
            json_rows(&output),
            vec![serde_json::json!({"counts":{},"errors":1})]
        );
    }
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--on-error",
            "fail",
        ],
        &unit,
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    let csv = [b"id,text\r\n1,".as_slice(), &unit].concat();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "drop",
            "Is checkout down?",
            "-I",
            "csv",
            "--select",
            "text",
            "--keep-going",
            "--on-error",
            "keep",
        ],
        &csv,
    );
    success(&output);
    assert_eq!(output.stdout, csv);
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "-I",
            "jsonl",
            "--keep-going",
            "--on-error",
            "keep",
            "--summary",
        ],
        b"{bad json}\n",
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    assert_eq!(metrics(&output)["errors"], "0");
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--on-error",
            "keep",
            "--budget",
            "0",
            "--summary",
        ],
        b"Short input.\n",
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert_eq!(metrics(&output)["attempts"], "0");
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout down?",
            "--keep-going",
            "--on-error",
            "keep",
            "--deadline",
            "100ms",
        ],
        b"Short input.\n",
    );
    assert_eq!(output.status.code(), Some(69));
    assert!(output.stdout.is_empty());
}

#[test]
fn refused_real_transport_retains_every_failed_unit_without_a_fake_backend() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    for pack in ["unit", "window:4"] {
        let output = run(
            &config,
            &[
                "--url",
                "http://127.0.0.1:1",
                "-j",
                "4",
                "tag",
                "Classify the service state",
                "-c",
                "down=Unavailable,up=Healthy",
                "--pack",
                pack,
                "--keep-going",
                "--on-error",
                "keep",
                "--json",
                "--summary",
            ],
            b"First.\nSecond.\nThird.\n",
        );
        success(&output);
        let rows = json_rows(&output);
        assert_eq!(rows.len(), 3);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["index"], index + 1);
            assert_eq!(row["error"]["code"], 69);
            assert!(row["answer"].is_null());
        }
        let summary = metrics(&output);
        assert_eq!(summary["errors"], "3");
        assert_eq!(summary["uncertain"], "0");
        let records = if pack == "unit" { 3 } else { 1 };
        assert_eq!(summary["logical_records"], records.to_string());
        assert_eq!(summary["attempts"], (records * 5).to_string());
        assert_eq!(summary["unknown_attempts"], summary["attempts"]);
        assert_eq!(summary["cache_hits"], "0");
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_keep_going_preserves_order_after_preflight_and_separates_uncertainty() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let overflow = oversized();
    let input = [
        b"Checkout is down and customers cannot pay.\n".as_slice(),
        &overflow,
        b"Checkout is down again.\n",
    ]
    .concat();
    for pack in ["unit", "window:4"] {
        let output = run(
            &config,
            &[
                "--url",
                &url,
                "-j",
                "4",
                "keep",
                "Is checkout unavailable?",
                "--pack",
                pack,
                "--keep-going",
                "--on-error",
                "keep",
                "--strict",
                "--no-cache",
                "--json",
                "--summary",
            ],
            &input,
        );
        success(&output);
        let rows = json_rows(&output);
        assert_eq!(rows.len(), 3);
        assert!(rows[0]["answer"]["noul"].is_number());
        assert_eq!(rows[1]["error"]["code"], 65);
        assert!(rows[2]["answer"]["noul"].is_number());
        assert_eq!(
            rows.iter()
                .map(|row| row["index"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let summary = metrics(&output);
        assert_eq!(summary["errors"], "1");
        assert_eq!(summary["units"], "2");
        assert_eq!(summary["uncertain"], "0");
        assert_eq!(summary["logical_records"], "2");
        assert_eq!(summary["failed_records"], "0");
        let output = run(
            &config,
            &[
                "--url",
                &url,
                "-j",
                "4",
                "keep",
                "Is checkout unavailable?",
                "--pack",
                pack,
                "--keep-going",
                "--strict",
                "--no-cache",
                "--json",
            ],
            &input,
        );
        success(&output);
        assert_eq!(
            json_rows(&output)
                .iter()
                .map(|row| row["index"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
    }
    let capture = root.path().join("uncertain");
    let output = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "1",
            "keep",
            "Is checkout unavailable?",
            "--keep-going",
            "--on-error",
            "keep",
            "--margin",
            "1",
            "--uncertain",
            capture.to_str().unwrap(),
            "--summary",
            "--strict",
            "--no-cache",
            "--json",
        ],
        &input,
    );
    success(&output);
    assert_eq!(json_rows(&output).len(), 1);
    let summary = metrics(&output);
    assert_eq!(summary["errors"], "1");
    assert_eq!(summary["uncertain"], "2");
    assert_eq!(
        std::fs::read(&capture).unwrap(),
        b"Checkout is down and customers cannot pay.\nCheckout is down again.\n"
    );
    let output = run(
        &config,
        &[
            "--url",
            &url,
            "-j",
            "1",
            "keep",
            "Is checkout unavailable?",
            "--keep-going",
            "--on-error",
            "keep",
            "--margin",
            "1",
            "--on-uncertain",
            "fail",
            "--summary",
            "--strict",
            "--no-cache",
        ],
        &input,
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert_eq!(metrics(&output)["uncertain"], "1");
    assert_eq!(metrics(&output)["errors"], "0");
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_split_focus_and_sort_continue_only_at_original_unit_boundaries() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let overflow = oversized();
    let input = [
        b"Checkout is down.\n".as_slice(),
        &overflow,
        b"Checkout is still down.\n",
    ]
    .concat();
    for mode in [
        vec!["--split", "--combine", "any", "--max-chunks", "1"],
        vec!["--focus", "1", "--max-chunks", "1"],
    ] {
        for pack in ["unit", "window:4"] {
            let mut args = vec![
                "--url",
                &url,
                "keep",
                "Is checkout unavailable?",
                "--pack",
                pack,
                "--keep-going",
                "--on-error",
                "keep",
                "--no-cache",
                "--strict",
                "--json",
                "--summary",
            ];
            args.extend(mode.iter().copied());
            let output = run(&config, &args, &input);
            success(&output);
            let rows = json_rows(&output);
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[1]["error"]["code"], 65);
            assert_eq!(rows[2]["index"], 3);
            assert!(rows[0]["answer"]["noul"].is_number());
            assert!(rows[2]["answer"]["noul"].is_number());
            assert_eq!(metrics(&output)["errors"], "1");
        }
    }
    for reverse in [false, true] {
        let mut args = vec![
            "--url",
            &url,
            "sort",
            "Rate checkout urgency",
            "-c",
            "low,high",
            "--keep-going",
            "--on-error",
            "keep",
            "--no-cache",
            "--strict",
            "--json",
        ];
        if reverse {
            args.push("--reverse");
        }
        let output = run(&config, &args, &input);
        success(&output);
        let rows = json_rows(&output);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2]["index"], 2);
        assert_eq!(rows[2]["error"]["code"], 65);
        let a = rows[0]["answer"]["score"].as_f64().unwrap();
        let b = rows[1]["answer"]["score"].as_f64().unwrap();
        assert!(if reverse { a >= b } else { a <= b });
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_journals_kept_errors_and_skipped_errors_without_uncertainty_capture() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let overflow = oversized();
    let input = [
        b"Checkout is down.\n".as_slice(),
        &overflow,
        b"Checkout is down again.\n",
    ]
    .concat();
    for keep in [false, true] {
        let out = root.path().join(if keep { "kept" } else { "dropped" });
        let capture = out.join("uncertain");
        let action = if keep { "keep" } else { "drop" };
        let args = [
            "--url",
            &url,
            "route",
            "Classify service state",
            "-c",
            "down=Checkout is unavailable,up=Service is healthy",
            "--out",
            out.to_str().unwrap(),
            "--uncertain",
            capture.to_str().unwrap(),
            "--keep-going",
            "--on-error",
            action,
            "--json",
            "--no-cache",
            "--strict",
            "--summary",
        ];
        let output = run(&config, &args, &input);
        success(&output);
        assert!(output.stdout.is_empty());
        assert_eq!(metrics(&output)["errors"], "1");
        assert_eq!(metrics(&output)["uncertain"], "0");
        if keep {
            let error: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&capture).unwrap()).unwrap();
            assert_eq!(error["index"], 2);
            assert_eq!(error["error"]["code"], 65);
            assert!(error["answer"].is_null());
        } else {
            assert!(!capture.exists());
        }
        let journal = out.join(".loupe-route.journal");
        let before = std::fs::read(&journal).unwrap();
        let manifest = out.join(".loupe-route.jsonl");
        let mut replay_args = args.to_vec();
        replay_args.extend(["--resume", manifest.to_str().unwrap(), "--budget", "0"]);
        let replay = run(&config, &replay_args, &input);
        success(&replay);
        assert_eq!(metrics(&replay)["attempts"], "0");
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        let changed_action = if keep { "drop" } else { "keep" };
        let position = replay_args
            .iter()
            .position(|value| *value == action)
            .unwrap();
        replay_args[position] = changed_action;
        assert_eq!(run(&config, &replay_args, &input).status.code(), Some(65));
    }
}
