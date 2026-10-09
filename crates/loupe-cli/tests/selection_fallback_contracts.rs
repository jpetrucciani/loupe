#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const QUERY: &str = "Which line describes a current checkout outage?";
const INPUT: &[u8] = b"Checkout is unavailable and customers cannot pay.\r\nCheckout is unavailable and customers cannot pay.\r\n";

fn config(root: &Path, options: usize) -> PathBuf {
    let path = root.join("config.toml");
    std::fs::write(
        &path,
        include_str!("../../../examples/config.toml")
            .replace("max_options = 512", &format!("max_options = {options}"))
            .replace("concurrency = 4", "concurrency = 1"),
    )
    .unwrap();
    path
}
fn run(root: &Path, config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            "http://127.0.0.1:8015",
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
    child.stdin.take().unwrap().write_all(input).unwrap();
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
fn metric(output: &Output, key: &str) -> u64 {
    let prefix = format!("{key}=");
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_choice_fallback_keeps_one_original_span_with_null_probability_and_replay() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = config(root, 512);
    for (verb, input, extras, label, expected) in [
        (
            "find",
            INPUT,
            Vec::new(),
            "L2",
            "Checkout is unavailable and customers cannot pay.",
        ),
        (
            "pick",
            INPUT,
            vec!["-n", "2"],
            "L2",
            "Checkout is unavailable and customers cannot pay.",
        ),
        (
            "extract",
            b"Either v1.2.3 or v4.5.6 may have introduced the regression.\r\n".as_slice(),
            vec!["--candidates", "semver", "--normalize"],
            "C2",
            "v4.5.6",
        ),
    ] {
        let journal = root.join(format!("{verb}.jsonl"));
        let marker = root.join(format!("{verb}-calls"));
        let view = root.join(format!("{verb}-state"));
        let fallback = format!(
            "cat > '{}'; printf '%s\\n' \"$LOUPE_QUESTION_ID\" >> '{}'; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection) printf {label};; *) exit 13;; esac",
            view.display(),
            marker.display()
        );
        let mut args = vec![
            verb,
            if verb == "extract" {
                "Which version should be checked for the regression?"
            } else {
                QUERY
            },
            "--margin",
            "1",
            "--min-confidence",
            "1",
            "--min-gap",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            &fallback,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ];
        args.extend(extras);
        let mut first = args.clone();
        first.extend(["--failed", journal.to_str().unwrap()]);
        let output = run(root, &config, &first, input);
        code(&output, 0);
        let values = rows(&output);
        assert_eq!(values.len(), 1);
        let row = &values[0];
        assert_eq!(row["id"], json!(label));
        assert_eq!(row["unit"], json!(expected));
        assert!(row["probability"].is_null());
        assert!(row["model_probability"].is_number());
        assert_eq!(
            row["selection_decisions"]["selection"]["source"],
            json!("fallback")
        );
        assert!(row["selection_decisions"]["selection"]["confidence"].is_null());
        let history = &row["provenance"]["history"];
        let request = &history["plans"][0]["brackets"][0]["request"];
        let response = &history["evaluations"][0][0]["response"];
        assert_eq!(row["answer"], response["answers"]["selection"]);
        assert_eq!(
            row["model_probability"],
            response["answers"]["selection"]["probabilities"][label]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&view).unwrap()).unwrap(),
            request["state"]
        );
        assert_eq!(
            history["plans"][0]["brackets"][0]["counts"]["input_tokens"],
            response["usage"]["input_tokens"]
        );
        assert_eq!(metric(&output, "fallback_attempts"), 2);
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "selection-present\nselection\n"
        );
        if verb == "extract" {
            assert_eq!(row["normalized"]["text"], json!("4.5.6"));
        }
        let mut resume = args;
        resume.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
        let replay = run(root, &config, &resume, input);
        code(&replay, 0);
        assert!(replay.stdout.is_empty());
        assert_eq!(metric(&replay, "http_attempts"), 0);
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "selection-present\nselection\n"
        );
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_selection_fatal_choice_replays_its_successful_gate_without_new_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = config(root, 512);
    let journal = root.join("run.jsonl");
    let fallback = "cat > /dev/null; printf '%s\\n' \"$LOUPE_QUESTION_ID\" >> calls; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; *) printf INVALID;; esac";
    let args = [
        "pick",
        QUERY,
        "--margin",
        "1",
        "--min-confidence",
        "1",
        "--min-gap",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        fallback,
        "--on-fallback-error",
        "fail",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    for flag in ["--failed", "--resume"] {
        let mut run_args = args.to_vec();
        run_args.extend([flag, journal.to_str().unwrap()]);
        if flag == "--resume" {
            run_args.extend(["--budget", "0"]);
        }
        let output = run(root, &config, &run_args, INPUT);
        code(&output, 69);
        assert!(output.stdout.is_empty());
        assert_eq!(
            metric(&output, "http_attempts"),
            u64::from(flag == "--failed")
        );
        assert_eq!(
            metric(&output, "fallback_attempts"),
            2 * u64::from(flag == "--failed")
        );
        assert_eq!(
            metric(&output, "fallback_replayed"),
            2 * u64::from(flag == "--resume")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("calls")).unwrap(),
            "selection-present\nselection\n"
        );
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_independent_fallback_scores_drive_top_n_and_partial_response_resume() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = config(root, 512);
    let journal = root.join("run.jsonl");
    let fallback = "IFS= read -r state; printf '%s\\n' \"$LOUPE_QUESTION_ID\" >> calls; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection-relevance) case \"$state\" in *L1*) printf 0.5;; *) printf 1.5;; esac;; selection) printf none;; *) exit 13;; esac";
    let base = [
        "pick",
        QUERY,
        "--independent",
        "-n",
        "2",
        "--margin",
        "1",
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
    let plan = run(
        root,
        &config,
        &["pick", QUERY, "--independent", "--dry-run"],
        INPUT,
    );
    code(&plan, 0);
    let allowance = rows(&plan)[0]["plan"]["items"][0]["bracket"]["counts"]["input_tokens"]
        .as_u64()
        .unwrap()
        .to_string();
    let mut first = base.to_vec();
    first.extend([
        "--failed",
        journal.to_str().unwrap(),
        "--budget",
        &allowance,
    ]);
    let failed = run(root, &config, &first, INPUT);
    code(&failed, 4);
    assert!(failed.stdout.is_empty());
    assert_eq!(metric(&failed, "http_attempts"), 1);
    assert!(!root.join("calls").exists());
    let mut resume = base.to_vec();
    resume.extend(["--resume", journal.to_str().unwrap()]);
    let output = run(root, &config, &resume, INPUT);
    code(&output, 0);
    assert_eq!(metric(&output, "http_attempts"), 1);
    let values = rows(&output);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["id"], json!("L2"));
    assert_eq!(values[0]["ranking_score"], json!(1.5));
    assert_eq!(values[1]["ranking_score"], json!(0.5));
    for row in values {
        assert_eq!(row["score_calculation"], json!("external_score"));
        assert_eq!(
            row["selection_decisions"]["selection-relevance"]["source"],
            json!("fallback")
        );
        assert!(row["selection_decisions"]["selection-relevance"]["confidence"].is_null());
        let index = if row["id"] == "L1" { 0 } else { 1 };
        let history = &row["provenance"]["history"];
        assert_eq!(
            row["answer"],
            history["evaluations"][0][index]["response"]["answers"]["selection-relevance"]
        );
        assert_eq!(
            history["plans"][0]["items"][index]["bracket"]["counts"]["input_tokens"],
            history["evaluations"][0][index]["response"]["usage"]["input_tokens"]
        );
    }
    assert_eq!(metric(&output, "fallback_attempts"), 4);
    let calls = std::fs::read(root.join("calls")).unwrap();
    resume.extend(["--budget", "0"]);
    let done = run(root, &config, &resume, INPUT);
    code(&done, 0);
    assert!(done.stdout.is_empty());
    assert_eq!(metric(&done, "http_attempts"), 0);
    assert_eq!(std::fs::read(root.join("calls")).unwrap(), calls);
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_selection_none_default_failures_and_deadline_do_not_invent_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = config(root, 512);
    for command in [
        "cat > /dev/null; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; *) printf none;; esac",
        "cat > /dev/null; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; *) printf INVALID;; esac",
        "cat > /dev/null; sleep 2; printf yes",
    ] {
        let output = run(
            root,
            &config,
            &[
                "find",
                QUERY,
                "--margin",
                "1",
                "--min-confidence",
                "1",
                "--min-gap",
                "1",
                "--on-uncertain",
                "fallback",
                "--fallback",
                command,
                "--fallback-timeout",
                "50ms",
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
            ],
            INPUT,
        );
        code(&output, 1);
        assert!(output.stdout.is_empty());
        if command.contains("INVALID") || command.contains("sleep") {
            assert!(String::from_utf8_lossy(&output.stderr).contains("decision remains uncertain"));
            assert_eq!(metric(&output, "uncertain"), 1);
        }
    }
    let output = run(
        root,
        &config,
        &[
            "pick",
            QUERY,
            "--margin",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            "cat > /dev/null; sleep 2; printf yes",
            "--deadline",
            "1s",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        INPUT,
    );
    code(&output, 69);
    assert!(output.stdout.is_empty());
}

#[test]
#[ignore = "requires actual server model and real child commands; no fake backend"]
fn server_selection_overflow_carries_only_external_winners_between_rounds() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = config(root, 3);
    let input = b"Checkout is unavailable and customers cannot pay.\n".repeat(6);
    let fallback = "cat > /dev/null; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection) python3 -c 'import json,os; q=json.loads(os.environ[\"LOUPE_QUESTION\"]); print(sorted(k for k in q[\"criteria\"] if k != \"none\")[0])';; *) exit 13;; esac";
    let output = run(
        root,
        &config,
        &[
            "pick",
            QUERY,
            "--margin",
            "1",
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
        ],
        &input,
    );
    code(&output, 0);
    let values = rows(&output);
    assert_eq!(values.len(), 1);
    assert_eq!(values[0]["id"], json!("L1"));
    assert!(values[0]["probability"].is_null());
    let history = &values[0]["provenance"]["history"];
    assert_eq!(history["plans"].as_array().unwrap().len(), 3);
    assert_eq!(metric(&output, "http_attempts"), 6);
    assert_eq!(
        metric(&output, "input_counted"),
        metric(&output, "input_reported")
    );
    for (round, expected) in [["L1", "L3", "L5"].as_slice(), ["L1", "L5"].as_slice()]
        .iter()
        .enumerate()
    {
        let ids = history["plans"][round + 1]["brackets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|bracket| bracket["candidate_ids"].as_array().unwrap())
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, *expected);
    }
}
