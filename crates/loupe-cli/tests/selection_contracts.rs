#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn config(root: &Path, options: usize) -> PathBuf {
    let path = root.join("config.toml");
    let text = include_str!("../../../examples/config.toml")
        .replace("max_options = 512", &format!("max_options = {options}"));
    std::fs::write(&path, text).unwrap();
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
fn json(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn summary(output: &Output, name: &str) -> u64 {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let prefix = format!("{name}=");
    stderr
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}
fn positive() -> &'static [u8] {
    "東京🙂\r\n\r\nCheckout works normally.\r\nCheckout is unavailable and customers cannot pay.\r\nBirds sing outside.\r\nThe garden needs water.\r\nBlue skies and sunshine.\r\nA football team won its match.\r\nThe database backup completed.\r\nClouds will clear tonight.\r\nTraffic is moving smoothly.".as_bytes()
}
const QUERY: &str = "Which line describes a current checkout outage?";

#[test]
fn find_offline_plans_preserve_physical_ids_context_criteria_and_conditional_rounds() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let rubric = directory.path().join("query.json");
    std::fs::write(&rubric,r#"{"type":"noul","instructions":"Which line describes a current checkout outage?","criteria":{"true":"There is a matching candidate.","false":"There is no matching candidate."}}"#).unwrap();
    let uncertain = directory.path().join("uncertain.txt");
    let result = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "find",
            "-f",
            rubric.to_str().unwrap(),
            "--dry-run",
            "--with-context",
            "1",
            "--uncertain",
            uncertain.to_str().unwrap(),
        ],
        positive(),
    );
    code(&result, 0);
    assert!(!uncertain.exists());
    assert!(result.stderr.is_empty());
    let records = json(&result);
    assert_eq!(records.len(), 1);
    let plan = &records[0];
    assert_eq!(plan["catalog"][0]["id"], "L1");
    assert_eq!(plan["catalog"][1]["id"], "L3");
    assert_eq!(plan["catalog"][2]["id"], "L4");
    assert_eq!(plan["round"]["capacity"], 3);
    assert_eq!(plan["round"]["final_round"], false);
    let first = &plan["round"]["brackets"][0];
    assert_eq!(first["counts"]["exact"], true);
    assert_eq!(
        first["request"]["questions"]["selection-present"]["criteria"]["true"],
        "There is a matching candidate."
    );
    assert_eq!(
        first["request"]["questions"]["selection"]["criteria"]["L3"]["previous_lines"][0]["id"],
        "L2"
    );
    assert_eq!(
        first["request"]["questions"]["selection"]["criteria"]["L3"]["previous_lines"][0]["text"],
        ""
    );
    assert!(
        plan["future_rounds"]
            .as_str()
            .unwrap()
            .contains("conditional")
    );
}

#[test]
fn find_rejects_inapplicable_flags_bad_types_nonreducing_tournaments_and_aliases() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    for flags in [
        vec!["-I", "jsonl"],
        vec!["--pack", "window:2"],
        vec!["--split", "--combine", "any"],
        vec!["--focus", "2"],
        vec!["--records", "^"],
        vec!["--keep-going"],
        vec!["--take", "0"],
        vec!["--max-rounds", "0"],
        vec!["--min-confidence", "NaN"],
        vec!["--min-gap", "1.1"],
        vec!["--threshold", "2"],
        vec!["--review=defer"],
        vec!["--ensemble"],
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
        let mut args = vec!["find", QUERY, "--dry-run"];
        args.extend(flags);
        let output = run(&config, &args, b"one\ntwo");
        code(&output, 64);
        assert!(output.stdout.is_empty());
    }
    let result = run(&config, &["find", "-p", "severity", "--dry-run"], b"one");
    code(&result, 64);
    let result = run(
        &config,
        &["find", QUERY, "--dry-run", "--take", "3"],
        positive(),
    );
    code(&result, 65);
    let input = directory.path().join("document.txt");
    std::fs::write(&input, positive()).unwrap();
    let alias = directory.path().join("alias.txt");
    std::fs::hard_link(&input, &alias).unwrap();
    let result = run(
        &config,
        &[
            "find",
            QUERY,
            input.to_str().unwrap(),
            "--dry-run",
            "--uncertain",
            alias.to_str().unwrap(),
        ],
        b"",
    );
    code(&result, 64);
    assert_eq!(std::fs::read(&input).unwrap(), positive());
    let result = run(&config, &["find", QUERY, "--dry-run", "-", "-"], b"one");
    code(&result, 64);
}

#[test]
fn find_empty_documents_and_real_input_failures_need_no_model_decision() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let result = run(
        &config,
        &["--url", "http://127.0.0.1:1", "find", QUERY, "--dry-run"],
        b"",
    );
    code(&result, 0);
    assert!(
        json(&result)[0]["round"]["brackets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let result = run(&config, &["find", QUERY, "--dry-run"], &[0xff]);
    code(&result, 65);
    let missing = directory.path().join("missing.txt");
    let result = run(
        &config,
        &["find", QUERY, missing.to_str().unwrap(), "--dry-run"],
        b"",
    );
    code(&result, 74);
}

#[test]
fn find_deadline_bounds_a_real_stdin_pipe_wait() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "find",
            QUERY,
            "--dry-run",
            "--deadline",
            "50ms",
            "--summary",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held_pipe = child.stdin.take().unwrap();
    let started = std::time::Instant::now();
    let output = child.wait_with_output().unwrap();
    drop(held_pipe);
    code(&output, 69);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(output.stdout.is_empty());
    assert_eq!(summary(&output, "http_attempts"), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn find_sigint_cancels_a_real_stdin_pipe_after_an_in_order_plan_prefix() {
    use std::io::BufRead;
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let input = directory.path().join("document.txt");
    std::fs::write(&input, "one line\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "find",
            QUERY,
            input.to_str().unwrap(),
            "-",
            "--dry-run",
            "--summary",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held_pipe = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut prefix = String::new();
    stdout.read_line(&mut prefix).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&prefix).unwrap()["document_index"],
        1
    );
    let status_path = format!("/proc/{}/status", child.id());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = std::fs::read_to_string(&status_path).unwrap();
        let mask = status
            .lines()
            .find_map(|line| line.strip_prefix("SigCgt:"))
            .unwrap()
            .trim();
        if u64::from_str_radix(mask, 16).unwrap() & 2 != 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "SIGINT handler did not become ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let signal = Command::new("sh")
        .args(["-c", "kill -INT \"$1\"", "kill", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(signal.success());
    let output = child.wait_with_output().unwrap();
    drop(held_pipe);
    code(&output, 130);
    assert_eq!(summary(&output, "http_attempts"), 0);
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_find_preserves_utf8_offsets_raw_answers_and_real_license_locations() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let result = run(
        &config,
        &[
            "find",
            QUERY,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        positive(),
    );
    code(&result, 0);
    let output = json(&result);
    assert_eq!(output.len(), 1);
    let selected = &output[0];
    assert_eq!(selected["id"], "L4");
    assert_eq!(selected["index"], 4);
    let span = selected["unit"].as_str().unwrap();
    let start = selected["location"]["byte_start"].as_u64().unwrap() as usize;
    let end = selected["location"]["byte_end"].as_u64().unwrap() as usize;
    assert_eq!(&positive()[start..end], span.as_bytes());
    assert_eq!(
        selected["answer"]["probabilities"]["L4"],
        selected["probability"]
    );
    assert_eq!(selected["probability_scope"], "final-surviving-bracket");
    assert!(
        selected["provenance"]["history"]["plans"]
            .as_array()
            .unwrap()
            .len()
            > 1
    );
    assert_eq!(
        summary(&result, "input_counted"),
        summary(&result, "input_reported") + summary(&result, "unknown_input_counted")
    );
    let license = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE");
    let result = run(
        &config,
        &[
            "find",
            "Which line says the software is provided AS IS?",
            license.to_str().unwrap(),
            "--json",
            "--strict",
            "--no-cache",
        ],
        b"",
    );
    code(&result, 0);
    let output = json(&result);
    assert_eq!(output[0]["location"]["line_start"], 15);
    assert!(
        output[0]["unit"]
            .as_str()
            .unwrap()
            .starts_with("THE SOFTWARE IS PROVIDED")
    );
    assert_eq!(output[0]["location"]["source"], license.to_str().unwrap());
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_find_absence_uncertainty_plain_tsv_and_budget_have_distinct_contracts() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let simple=b"Checkout works normally.\r\nCheckout is unavailable and customers cannot pay.\r\nBirds sing outside.";
    let result = run(&config, &["find", QUERY, "--no-cache"], simple);
    code(&result, 0);
    assert_eq!(
        result.stdout,
        b"2:Checkout is unavailable and customers cannot pay.\r\n"
    );
    let result = run(
        &config,
        &[
            "find",
            "-p",
            "answers",
            "--arg",
            "query=Which line describes a current checkout outage?",
            "--json",
            "--no-cache",
            "--strict",
        ],
        simple,
    );
    code(&result, 0);
    assert_eq!(json(&result)[0]["id"], "L2");
    let result = run(&config, &["find", QUERY, "-O", "tsv", "--no-cache"], simple);
    code(&result, 0);
    assert_eq!(
        String::from_utf8(result.stdout)
            .unwrap()
            .trim_end()
            .split('\t')
            .count(),
        4
    );
    let result = run(
        &config,
        &["find", QUERY, "--no-cache"],
        b"Blue skies and sunshine.\nBirds sing outside.",
    );
    code(&result, 1);
    assert!(result.stdout.is_empty());
    let uncertain = directory.path().join("uncertain.txt");
    for (action, expected) in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)] {
        let result = run(
            &config,
            &[
                "find",
                QUERY,
                "--no-cache",
                "--min-confidence",
                "1",
                "--on-uncertain",
                action,
                "--uncertain",
                uncertain.to_str().unwrap(),
                "--json",
            ],
            simple,
        );
        code(&result, expected);
        if expected == 0 {
            assert_eq!(json(&result)[0]["uncertain"], true);
        } else {
            assert!(result.stdout.is_empty());
        }
        if action == "stderr" {
            assert!(String::from_utf8_lossy(&result.stderr).contains("uncertain document="));
        }
    }
    assert_eq!(std::fs::read(&uncertain).unwrap(), simple.repeat(4));
    let result = run(
        &config,
        &["find", QUERY, "--no-cache", "--budget", "0", "--summary"],
        simple,
    );
    code(&result, 4);
    assert_eq!(summary(&result, "http_attempts"), 0);
    assert!(result.stdout.is_empty());
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_find_journal_resumes_finished_and_policy_failed_responses_without_inference() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let input = b"Checkout is unavailable and customers cannot pay.\nBirds sing outside.";
    let journal = directory.path().join("finished.jsonl");
    let result = run(
        &config,
        &[
            "find",
            QUERY,
            "--no-cache",
            "--failed",
            journal.to_str().unwrap(),
            "--json",
        ],
        input,
    );
    code(&result, 0);
    let result = run(
        &config,
        &[
            "find",
            QUERY,
            "--no-cache",
            "--resume",
            journal.to_str().unwrap(),
            "--json",
            "--budget",
            "0",
            "--summary",
        ],
        input,
    );
    code(&result, 0);
    assert!(result.stdout.is_empty());
    assert_eq!(summary(&result, "http_attempts"), 0);
    let result = run(
        &config,
        &[
            "find",
            QUERY,
            "--no-cache",
            "--resume",
            journal.to_str().unwrap(),
            "--json",
            "--budget",
            "0",
        ],
        b"changed input",
    );
    code(&result, 65);
    assert!(result.stdout.is_empty());
    let journal = directory.path().join("uncertain.jsonl");
    let args = [
        "find",
        QUERY,
        "--no-cache",
        "--min-confidence",
        "1",
        "--on-uncertain",
        "fail",
        "--json",
    ];
    let mut first = args.to_vec();
    first.extend(["--failed", journal.to_str().unwrap()]);
    let result = run(&config, &first, input);
    code(&result, 3);
    let mut resume = args.to_vec();
    resume.extend([
        "--resume",
        journal.to_str().unwrap(),
        "--budget",
        "0",
        "--summary",
    ]);
    let result = run(&config, &resume, input);
    code(&result, 3);
    assert_eq!(summary(&result, "http_attempts"), 0);
    assert!(result.stdout.is_empty());
}

#[test]
fn pick_independent_plans_isolate_candidates_and_preserve_score_presets_context_and_flag_rules() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let result = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "pick",
            QUERY,
            "--independent",
            "--dry-run",
            "--with-context",
            "1",
        ],
        positive(),
    );
    code(&result, 0);
    let plan = &json(&result)[0]["plan"];
    let items = plan["items"].as_array().unwrap();
    assert_eq!(items.len(), 10);
    for item in items {
        let id = item["id"].as_str().unwrap();
        let bracket = &item["bracket"];
        assert_eq!(bracket["candidate_ids"].as_array().unwrap().len(), 1);
        let criteria = bracket["request"]["questions"]["selection"]["criteria"]
            .as_object()
            .unwrap();
        assert_eq!(criteria.len(), 2);
        assert!(criteria.contains_key(id));
        assert!(criteria.contains_key("none"));
        assert_eq!(bracket["counts"]["exact"], true);
        assert_eq!(
            bracket["request"]["questions"]["selection-relevance"]["type"],
            "score"
        );
    }
    let result = run(
        &config,
        &[
            "pick",
            "-p",
            "relevant",
            "--arg",
            "query=Which line describes a current checkout outage?",
            "--independent",
            "--dry-run",
        ],
        b"one\ntwo",
    );
    code(&result, 0);
    assert_eq!(
        json(&result)[0]["plan"]["criteria"],
        serde_json::json!(["Unrelated", "Somewhat related", "Directly relevant"])
    );
    for args in [
        vec![
            "pick",
            QUERY,
            "--independent",
            "--max-rounds",
            "64",
            "--dry-run",
        ],
        vec!["find", QUERY, "--independent", "--dry-run"],
        vec!["pick", QUERY, "--independent", "--split", "--dry-run"],
    ] {
        let result = run(&config, &args, b"one");
        code(&result, 64);
        assert!(result.stdout.is_empty());
    }
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_pick_choice_top_n_uses_only_the_final_marginals_and_preserves_original_lines() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let input="東京🙂\nCheckout is unavailable and customers cannot pay.\nCheckout works normally.\nBirds sing outside.".as_bytes();
    let result = run(
        &config,
        &[
            "pick",
            QUERY,
            "-n",
            "2",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    code(&result, 0);
    let records = json(&result);
    assert_eq!(records.len(), 2);
    let distribution = records[0]["answer"]["probabilities"].as_object().unwrap();
    let mut ranked = distribution
        .iter()
        .filter(|(id, _)| id.as_str() != "none")
        .map(|(id, p)| (id, p.as_f64().unwrap()))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    for (record, (id, p)) in records.iter().zip(ranked) {
        assert_eq!(record["id"], id.as_str());
        assert_eq!(record["probability"], p);
        assert_eq!(record["probability_scope"], "all-candidates");
        assert_eq!(record["answer"]["type"], "choice");
        let start = record["location"]["byte_start"].as_u64().unwrap() as usize;
        let end = record["location"]["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(
            &input[start..end],
            record["unit"].as_str().unwrap().as_bytes()
        );
    }
    let result = run(&config, &["pick", QUERY, "--no-cache"], input);
    code(&result, 0);
    assert_eq!(
        result.stdout,
        b"Checkout is unavailable and customers cannot pay.\n"
    );
    let result = run(
        &config,
        &["pick", QUERY, "--no-cache"],
        b"Birds sing outside.\nBlue skies.",
    );
    code(&result, 1);
    assert!(result.stdout.is_empty());
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_pick_independent_ranks_own_score_distributions_and_keeps_pair_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let input=b"Checkout is unavailable and customers cannot pay.\nCheckout requests fail with errors for every customer.\nCheckout works normally.\nBirds sing outside.";
    let result = run(
        &config,
        &[
            "pick",
            QUERY,
            "--independent",
            "-n",
            "2",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    code(&result, 0);
    let records = json(&result);
    assert_eq!(records.len(), 2);
    let ids = records
        .iter()
        .map(|value| value["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids, std::collections::BTreeSet::from(["L1", "L2"]));
    let mut previous = f64::INFINITY;
    for record in &records {
        assert_eq!(record["answer"]["type"], "score");
        assert!(record.get("probability").is_none());
        assert_eq!(record["probability_scope"], "independent-candidate-score");
        let probabilities = record["answer"]["probabilities"].as_object().unwrap();
        let sum = probabilities
            .values()
            .map(|p| p.as_f64().unwrap())
            .sum::<f64>();
        let mean = probabilities
            .iter()
            .map(|(key, p)| key.parse::<f64>().unwrap() * p.as_f64().unwrap())
            .sum::<f64>()
            / sum;
        let value = record["ranking_score"].as_f64().unwrap();
        assert!((value - mean).abs() < 1e-12);
        assert!(value <= previous);
        previous = value;
        let evaluations = record["provenance"]["history"]["evaluations"][0]
            .as_array()
            .unwrap();
        assert_eq!(evaluations.len(), 4);
        let plans = record["provenance"]["history"]["plans"][0]["items"]
            .as_array()
            .unwrap();
        let index = plans
            .iter()
            .position(|plan| plan["id"] == record["id"])
            .unwrap();
        assert_eq!(
            record["answer"],
            evaluations[index]["response"]["answers"]["selection-relevance"]
        );
        assert_eq!(
            plans[index]["bracket"]["candidate_ids"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    assert_eq!(
        summary(&result, "input_counted"),
        summary(&result, "input_reported") + summary(&result, "unknown_input_counted")
    );
    let result = run(
        &config,
        &[
            "pick",
            "-p",
            "relevant",
            "--arg",
            "query=Which line describes a current checkout outage?",
            "--independent",
            "--json",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    code(&result, 0);
    assert!(matches!(
        json(&result)[0]["id"].as_str().unwrap(),
        "L1" | "L2"
    ));
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_pick_independent_uncertainty_absence_budget_and_journal_replay_are_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let input = b"Checkout is unavailable and customers cannot pay.\nBirds sing outside.";
    let uncertain = directory.path().join("uncertain.txt");
    for (action, expected) in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)] {
        let result = run(
            &config,
            &[
                "pick",
                QUERY,
                "--independent",
                "--margin",
                "1",
                "--on-uncertain",
                action,
                "--uncertain",
                uncertain.to_str().unwrap(),
                "--json",
                "--strict",
                "--no-cache",
            ],
            input,
        );
        code(&result, expected);
        if expected == 0 {
            assert_eq!(json(&result)[0]["uncertain"], true);
        } else {
            assert!(result.stdout.is_empty());
        }
    }
    assert_eq!(std::fs::read(&uncertain).unwrap(), input.repeat(4));
    let result = run(
        &config,
        &["pick", QUERY, "--independent", "--no-cache"],
        b"Birds sing outside.\nBlue skies.",
    );
    code(&result, 1);
    assert!(result.stdout.is_empty());
    let result = run(
        &config,
        &[
            "pick",
            QUERY,
            "--independent",
            "--budget",
            "0",
            "--summary",
            "--no-cache",
        ],
        input,
    );
    code(&result, 4);
    assert_eq!(summary(&result, "http_attempts"), 0);
    let journal = directory.path().join("independent.jsonl");
    let flags = [
        "pick",
        QUERY,
        "--independent",
        "--margin",
        "1",
        "--on-uncertain",
        "fail",
        "--json",
        "--no-cache",
    ];
    let mut first = flags.to_vec();
    first.extend(["--failed", journal.to_str().unwrap()]);
    let result = run(&config, &first, input);
    code(&result, 3);
    let mut replay = flags.to_vec();
    replay.extend([
        "--resume",
        journal.to_str().unwrap(),
        "--budget",
        "0",
        "--summary",
    ]);
    let result = run(&config, &replay, input);
    code(&result, 3);
    assert_eq!(summary(&result, "http_attempts"), 0);
}

#[test]
fn extract_plans_keep_parsed_spans_containing_lines_context_and_conditional_limits() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let input =
        "東京🙂\r\nYesterday's note.\r\nThe offer was USD 12.50; the settlement was USD 40.00.\r\n"
            .as_bytes();
    let result = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "extract",
            "Which amount was paid as the settlement?",
            "--candidates",
            "money",
            "--with-context",
            "1",
            "--dry-run",
        ],
        input,
    );
    code(&result, 0);
    let record = &json(&result)[0];
    let catalog = record["catalog"].as_array().unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0]["id"], "C1");
    assert_eq!(catalog[0]["verbatim"], "USD 12.50");
    assert_eq!(catalog[1]["id"], "C2");
    assert_eq!(catalog[1]["verbatim"], "USD 40.00");
    for candidate in catalog {
        let start = candidate["location"]["byte_start"].as_u64().unwrap() as usize;
        let end = candidate["location"]["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(
            &input[start..end],
            candidate["verbatim"].as_str().unwrap().as_bytes()
        );
    }
    let bracket = &record["round"]["brackets"][0];
    assert_eq!(bracket["counts"]["exact"], true);
    let descriptor = &bracket["request"]["questions"]["selection"]["criteria"]["C2"];
    assert_eq!(descriptor["verbatim"], "USD 40.00");
    assert_eq!(
        descriptor["source_line"]["unit"],
        "The offer was USD 12.50; the settlement was USD 40.00."
    );
    assert_eq!(descriptor["source_line"]["previous_lines"][0]["id"], "L2");
    assert_eq!(
        descriptor["source_line"]["previous_lines"][0]["text"],
        "Yesterday's note."
    );
    assert!(!bracket["request"]["questions"]["selection"]["criteria"]["none"].is_null());
    for (kind, input, expected) in [
        ("dates", "2026-10-06 tomorrow", 2),
        ("semver", "v1.2.3 v2.0.0", 2),
        ("emails", "a@example.com b@example.net", 2),
        ("hostnames", "a.example.com b.example.net", 2),
        ("tickers", "$AAPL $MSFT", 2),
    ] {
        let result = run(
            &config,
            &[
                "extract",
                "Which candidate answers the question?",
                "--candidates",
                kind,
                "--dry-run",
            ],
            input.as_bytes(),
        );
        code(&result, 0);
        assert_eq!(
            json(&result)[0]["catalog"].as_array().unwrap().len(),
            expected
        );
    }
    let result = run(
        &config,
        &[
            "extract",
            "Which email is the contact?",
            "--candidates",
            "emails",
            "--dry-run",
        ],
        b"No address is listed.",
    );
    code(&result, 0);
    assert_eq!(json(&result)[0]["catalog"], serde_json::json!([]));
}

#[test]
fn extract_rejects_inapplicable_flags_and_bad_rubrics_before_inference() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    for flags in [
        vec!["--keep-blank"],
        vec!["--independent"],
        vec!["--split"],
        vec!["-I", "jsonl"],
        vec!["--pack", "unit"],
    ] {
        let mut args = vec![
            "extract",
            "Which amount was paid?",
            "--candidates",
            "money",
            "--dry-run",
        ];
        args.extend(flags);
        code(&run(&config, &args, b"USD 12.50"), 64);
    }
    code(
        &run(
            &config,
            &["extract", "Which amount was paid?", "--dry-run"],
            b"USD 12.50",
        ),
        64,
    );
    code(
        &run(
            &config,
            &[
                "extract",
                "Which amount was paid?",
                "--candidates",
                "imaginary",
                "--dry-run",
            ],
            b"USD 12.50",
        ),
        64,
    );
    let result = run(
        &config,
        &[
            "extract",
            "-p",
            "relevant",
            "--arg",
            "query=Which amount was paid?",
            "--candidates",
            "money",
            "--dry-run",
        ],
        b"USD 12.50",
    );
    code(&result, 64);
    let result = run(
        &config,
        &[
            "extract",
            "Which email is the contact?",
            "--candidates",
            "emails",
            "--dry-run",
        ],
        b"\xff",
    );
    code(&result, 65);
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_extract_all_candidate_families_keep_verbatim_offsets_and_raw_choice_answers() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 512);
    let cases = [
        (
            "dates",
            "The meeting is 2026-10-06; the settlement date is 2026-10-09.",
            "Which date is the settlement date?",
            "2026-10-09",
        ),
        (
            "money",
            "The offer was USD 12.50; the settlement paid was USD 40.00.",
            "Which amount was paid as the settlement?",
            "USD 40.00",
        ),
        (
            "semver",
            "The installed version is v1.2.3; the regression first appeared in v2.0.0-rc.1+build.7.",
            "Which version first introduced the regression?",
            "v2.0.0-rc.1+build.7",
        ),
        (
            "emails",
            "Support: help@example.com; billing: bills@example.org.",
            "Which email address is for billing?",
            "bills@example.org",
        ),
        (
            "hostnames",
            "Development: dev.example.com; production: api.example.net.",
            "Which hostname serves production?",
            "api.example.net",
        ),
        (
            "tickers",
            "The watchlist includes $MSFT; the purchased symbol was NASDAQ:AAPL.",
            "Which ticker was purchased?",
            "NASDAQ:AAPL",
        ),
    ];
    for (kind, line, query, expected) in cases {
        let input = format!("東京🙂\r\n{line}\r\n");
        let result = run(
            &config,
            &[
                "extract",
                query,
                "--candidates",
                kind,
                "--json",
                "--strict",
                "--no-cache",
                "--summary",
            ],
            input.as_bytes(),
        );
        code(&result, 0);
        let record = &json(&result)[0];
        assert_eq!(record["unit"], expected, "{kind}");
        assert_eq!(record["id"], "C2", "{kind}");
        assert_eq!(record["candidate_type"], kind);
        assert_eq!(record["location"]["line_start"], 2);
        let start = record["location"]["byte_start"].as_u64().unwrap() as usize;
        let end = record["location"]["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(&input.as_bytes()[start..end], expected.as_bytes());
        assert_eq!(record["answer"]["type"], "choice");
        assert_eq!(
            record["probability"],
            record["answer"]["probabilities"]["C2"]
        );
        assert_eq!(
            summary(&result, "input_counted"),
            summary(&result, "input_reported") + summary(&result, "unknown_input_counted")
        );
    }
}

#[test]
#[ignore = "requires real server System One model; no fake backend"]
fn server_extract_presence_uncertainty_budget_and_journal_replay_use_the_document_contract() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), 4);
    let input = b"Support: help@example.com; billing: bills@example.org.";
    let query = "Which email address is for billing?";
    let absent = run(
        &config,
        &[
            "extract",
            query,
            "--candidates",
            "emails",
            "--json",
            "--strict",
            "--no-cache",
        ],
        b"Birds sing outside.",
    );
    code(&absent, 1);
    assert!(absent.stdout.is_empty());
    let zero = run(
        &config,
        &[
            "extract",
            query,
            "--candidates",
            "emails",
            "--budget",
            "0",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    code(&zero, 4);
    assert_eq!(summary(&zero, "http_attempts"), 0);
    let uncertain = directory.path().join("uncertain.txt");
    for (action, expected) in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)] {
        let result = run(
            &config,
            &[
                "extract",
                query,
                "--candidates",
                "emails",
                "--margin",
                "1",
                "--on-uncertain",
                action,
                "--uncertain",
                uncertain.to_str().unwrap(),
                "--json",
                "--strict",
                "--no-cache",
            ],
            input,
        );
        code(&result, expected);
        if expected == 0 {
            assert_eq!(json(&result)[0]["uncertain"], true);
        } else {
            assert!(result.stdout.is_empty());
        }
    }
    assert_eq!(std::fs::read(uncertain).unwrap(), input.repeat(4));
    let journal = directory.path().join("extract.jsonl");
    let flags = [
        "extract",
        query,
        "--candidates",
        "emails",
        "--margin",
        "1",
        "--on-uncertain",
        "fail",
        "--json",
        "--no-cache",
    ];
    let mut first = flags.to_vec();
    first.extend(["--failed", journal.to_str().unwrap()]);
    code(&run(&config, &first, input), 3);
    let mut replay = flags.to_vec();
    replay.extend([
        "--resume",
        journal.to_str().unwrap(),
        "--budget",
        "0",
        "--summary",
    ]);
    let result = run(&config, &replay, input);
    code(&result, 3);
    assert_eq!(summary(&result, "http_attempts"), 0);
    let changed = run(
        &config,
        &[
            "extract",
            query,
            "--candidates",
            "hostnames",
            "--margin",
            "1",
            "--on-uncertain",
            "fail",
            "--json",
            "--no-cache",
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
        ],
        input,
    );
    code(&changed, 65);
}
