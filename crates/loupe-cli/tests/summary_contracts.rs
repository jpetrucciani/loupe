#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
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
fn summary(output: &Output) -> BTreeMap<String, String> {
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    let lines: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("loupe: units="))
        .collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    lines[0]
        .strip_prefix("loupe: ")
        .unwrap()
        .split_whitespace()
        .map(|field| {
            let (key, value) = field.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect()
}
fn number(summary: &BTreeMap<String, String>, key: &str) -> u64 {
    summary[key].parse().unwrap()
}
#[test]
fn planning_summary_is_explicit_and_never_invents_inference_or_latency() {
    let input = b"a line\n".repeat(500);
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "tokens",
            "question",
            "-I",
            "lines",
            "--summary",
        ],
        &input,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "units"), 500);
    assert_eq!(metrics["mode"], "plan");
    for key in [
        "attempts",
        "http_attempts",
        "logical_records",
        "latency_samples",
        "input_counted",
        "input_reported",
        "unknown_attempts",
    ] {
        assert_eq!(number(&metrics, key), 0);
    }
    assert_eq!(metrics["p50_ms"], "unavailable");
    assert_eq!(metrics["cache_hit_rate"], "unavailable");
    assert_eq!(metrics["gpu_passes"], "unavailable");
}
#[test]
fn fatal_unsent_budget_failure_still_prints_logical_outcome_and_zero_usage() {
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "Is checkout unavailable?",
            "--budget",
            "0",
            "--no-cache",
            "--summary",
        ],
        b"Checkout is down.\n",
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "logical_records"), 1);
    assert_eq!(number(&metrics, "failed_records"), 1);
    assert_eq!(number(&metrics, "latency_samples"), 0);
    for key in [
        "attempts",
        "http_attempts",
        "units",
        "input_counted",
        "input_reported",
        "unknown_attempts",
        "succeeded_records",
        "cancelled_records",
    ] {
        assert_eq!(number(&metrics, key), 0);
    }
    assert_eq!(metrics["p50_ms"], "unavailable");
}
#[test]
fn cancelled_genuine_transport_retains_unknown_attempt_usage_and_has_no_completed_latency() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let endpoint = format!("http://{address}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
            "--url",
            &endpoint,
            "-j",
            "1",
            "-vv",
            "keep",
            "Is checkout unavailable?",
            "--no-cache",
            "--summary",
        ])
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down.\n")
        .unwrap();
    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let mut captured = String::new();
    loop {
        let mut line = String::new();
        assert!(stderr.read_line(&mut line).unwrap() > 0, "{captured}");
        let submitted = line.starts_with("loupe: submitted attempt=");
        captured.push_str(&line);
        if submitted {
            break;
        }
    }
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let mut output = child.wait_with_output().unwrap();
    stderr.read_to_string(&mut captured).unwrap();
    output.stderr = captured.into_bytes();
    assert_eq!(output.status.code(), Some(143));
    assert!(output.stdout.is_empty());
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "logical_records"), 1);
    assert_eq!(number(&metrics, "cancelled_records"), 1);
    assert_eq!(number(&metrics, "latency_samples"), 0);
    let attempts = number(&metrics, "attempts");
    assert!((1..5).contains(&attempts));
    assert_eq!(number(&metrics, "unknown_attempts"), attempts);
    assert_eq!(number(&metrics, "http_attempts"), attempts);
    assert!(number(&metrics, "input_counted") > 0);
    assert_eq!(number(&metrics, "input_reported"), 0);
    assert_eq!(number(&metrics, "input_reserved"), 0);
    assert_eq!(metrics["p95_ms"], "unavailable");
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_summary_distinguishes_units_joint_windows_http_batches_and_focus_phases() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let input = b"Checkout is down and customers cannot pay.\n".repeat(9);
    for (pack, logical) in [("unit", 9), ("window:4", 3)] {
        let output = run(
            &[
                "--url",
                &url,
                "keep",
                "Is checkout unavailable?",
                "--pack",
                pack,
                "--strict",
                "--no-cache",
                "--summary",
                "--json",
            ],
            &input,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout.iter().filter(|byte| **byte == b'\n').count(),
            9
        );
        let metrics = summary(&output);
        assert_eq!(number(&metrics, "units"), 9);
        assert_eq!(number(&metrics, "logical_records"), logical);
        assert_eq!(number(&metrics, "succeeded_records"), logical);
        assert_eq!(number(&metrics, "attempts"), logical);
        assert_eq!(number(&metrics, "http_attempts"), 3);
        assert_eq!(number(&metrics, "latency_samples"), logical);
        assert_eq!(
            number(&metrics, "input_counted"),
            number(&metrics, "input_reported")
        );
        assert_eq!(number(&metrics, "unknown_attempts"), 0);
        assert_eq!(number(&metrics, "failed_records"), 0);
        assert_eq!(metrics["gpu_passes"], "unavailable");
        assert_eq!(metrics["latency_bounds"], "upper");
        assert!(
            metrics["p95_ms"].parse::<f64>().unwrap() >= metrics["p50_ms"].parse::<f64>().unwrap()
        );
    }
    let output = run(
        &[
            "--url",
            &url,
            "is",
            "Is checkout unavailable?",
            "--focus",
            "1",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        b"Checkout is down and customers cannot pay.",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "units"), 1);
    assert_eq!(number(&metrics, "logical_records"), 2);
    assert_eq!(number(&metrics, "attempts"), 2);
    assert_eq!(number(&metrics, "latency_samples"), 2);
    assert_eq!(
        number(&metrics, "input_counted"),
        number(&metrics, "input_reported")
    );
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_fatal_preflight_preserves_output_prefix_and_prints_its_summary() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let prefix = b"Checkout is down and customers cannot pay.\r\n";
    let input = [prefix.as_slice(), &b"word ".repeat(20_000), b"\n"].concat();
    let output = run(
        &[
            "--url",
            &url,
            "-j",
            "1",
            "keep",
            "Is checkout unavailable?",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        &input,
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(output.stdout, prefix);
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "units"), 1);
    assert_eq!(number(&metrics, "logical_records"), 1);
    assert_eq!(number(&metrics, "succeeded_records"), 1);
    assert_eq!(number(&metrics, "failed_records"), 0);
    assert_eq!(number(&metrics, "cancelled_records"), 0);
    assert_eq!(number(&metrics, "latency_samples"), 1);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_configured_rate_estimate_and_uncertainty_failure_preserve_real_usage() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        include_str!("../../../examples/config.toml").to_owned()
            + "\n[profile.server.pricing]\ninput_usd_per_million=2.0\noutput_usd_per_million=4.0\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            &url,
            "keep",
            "Is checkout unavailable?",
            "--margin",
            "0.51",
            "--on-uncertain",
            "fail",
            "--strict",
            "--summary",
            "--no-cache",
        ])
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down and customers cannot pay.\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let metrics = summary(&output);
    assert_eq!(number(&metrics, "units"), 1);
    assert_eq!(number(&metrics, "uncertain"), 1);
    assert_eq!(number(&metrics, "succeeded_records"), 1);
    assert_eq!(number(&metrics, "failed_records"), 0);
    let expected = number(&metrics, "input_reported") as f64 * 2.0 / 1_000_000.0
        + number(&metrics, "output_reported") as f64 * 4.0 / 1_000_000.0;
    assert!((metrics["cost_usd_estimate"].parse::<f64>().unwrap() - expected).abs() <= 0.00000051);
    assert_eq!(metrics["cost_unknown_output"], "false");
    assert_eq!(number(&metrics, "unknown_input_counted"), 0);
}
