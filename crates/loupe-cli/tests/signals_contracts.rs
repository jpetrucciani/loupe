#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn setup(root: &Path) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(&config, include_str!("../../../examples/config.toml")).unwrap();
    let directory = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&directory).unwrap();
    let signals = json!([
        {"id":"impact","instructions":"Does the state explicitly say customers cannot use checkout or pay?","policy":{"threshold":0.7,"margin":0.05}},
        {"id":"retried","instructions":"Does the state explicitly say a retry or recovery succeeded?"}
    ]);
    let presets = [
        (
            "severity-test",
            json!({"type":"choice","instructions":"Classify the current service condition.","criteria":{"page":"An unresolved outage prevents customers checking out.","noise":"No current outage, or the service recovered."},"signals":signals}),
        ),
        (
            "outage-test",
            json!({"type":"noul","instructions":"Is checkout currently unavailable to customers?","signals":signals}),
        ),
        (
            "retrieval-test",
            json!({"type":"noul","instructions":"Which line describes a current checkout outage?","signals":signals}),
        ),
        (
            "rank-test",
            json!({"type":"score","instructions":"How directly does this answer: Which line describes a current checkout outage?","criteria":["Unrelated","Direct evidence"],"signals":signals}),
        ),
        (
            "billing-test",
            json!({"type":"noul","instructions":"Which email address is for billing?","signals":signals}),
        ),
    ];
    for (name, preset) in presets {
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&preset).unwrap(),
        )
        .unwrap();
    }
    config
}

fn run(root: &Path, config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args(["--config", config.to_str().unwrap(), "--profile", "server"])
        .args(args)
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
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
fn records(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn metric(output: &Output, key: &str) -> u64 {
    let prefix = format!("{key}=");
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .find_map(|piece| piece.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn declared_signals_are_always_encoded_and_why_changes_only_output_identity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let without = run(
        root,
        &config,
        &["tag", "-p", "severity-test", "--dry-run"],
        b"Checkout is down.\n",
    );
    let with = run(
        root,
        &config,
        &["tag", "-p", "severity-test", "--dry-run", "--why"],
        b"Checkout is down.\n",
    );
    code(&without, 0);
    code(&with, 0);
    assert_eq!(records(&without), records(&with));
    let record = &records(&with)[0];
    let questions = record["request"]["questions"].as_object().unwrap();
    assert_eq!(
        questions.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["severity-test", "signal:impact", "signal:retried"]
    );
    assert_eq!(record["counts"]["exact"], true);
    let window = run(
        root,
        &config,
        &[
            "tag",
            "-p",
            "severity-test",
            "--pack",
            "window:2",
            "--dry-run",
            "--why",
        ],
        b"Checkout is down.\nCheckout recovered.\n",
    );
    code(&window, 0);
    let record = &records(&window)[0];
    let questions = record["request"]["questions"].as_object().unwrap();
    assert_eq!(questions.len(), 6);
    for index in [1, 2] {
        assert!(questions.contains_key(&format!("L{index}/signal:impact")));
    }
    let route = root.join("routes");
    let planned = run(
        root,
        &config,
        &[
            "route",
            "-p",
            "severity-test",
            "--out",
            route.to_str().unwrap(),
            "--dry-run",
            "--why",
        ],
        b"Checkout is down.\n",
    );
    code(&planned, 0);
    assert!(!route.exists());
}

#[test]
fn why_and_invalid_signals_fail_before_network_and_schema_limits_include_companions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    for args in [
        vec!["tokens", "-p", "severity-test", "--why"],
        vec!["count", "-p", "severity-test", "--why"],
        vec!["is", "Is checkout down?", "--why"],
        vec![
            "tag",
            "-p",
            "severity-test",
            "--threshold",
            "0.7",
            "--dry-run",
        ],
    ] {
        code(&run(root, &config, &args, b"Checkout is down."), 64);
    }
    let restricted = root.join("restricted.toml");
    std::fs::write(
        &restricted,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_questions = 128", "max_questions = 2"),
    )
    .unwrap();
    code(
        &run(
            root,
            &restricted,
            &["tag", "-p", "severity-test", "--dry-run"],
            b"Checkout is down.",
        ),
        65,
    );
    let broken = root.join("broken.json");
    std::fs::write(
        &broken,
        serde_json::to_vec(
            &json!({"type":"noul","instructions":"Down?","signals":[{"id":"same"},{"id":"same"}]}),
        )
        .unwrap(),
    )
    .unwrap();
    code(
        &run(
            root,
            &config,
            &["is", "-f", broken.to_str().unwrap(), "--dry-run"],
            b"Checkout is down.",
        ),
        64,
    );
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_why_keeps_primary_objects_and_binds_each_windows_own_signal_probabilities() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let input=b"Checkout is unavailable and customers cannot pay.\nCheckout recovered after a successful retry.\n";
    let result = run(
        root,
        &config,
        &[
            "tag",
            "-p",
            "severity-test",
            "--why",
            "--pack",
            "window:2",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
        ],
        input,
    );
    code(&result, 0);
    let data = records(&result);
    assert_eq!(data.len(), 2);
    for (position, record) in data.iter().enumerate() {
        assert_eq!(record["answer"]["type"], "choice");
        let signal = &record["signals"]["impact"];
        assert_eq!(signal["field"], format!("L{}/signal:impact", position + 1));
        let p = signal["answer"]["noul"].as_f64().unwrap();
        assert_eq!(signal["probability"], p);
        assert_eq!(signal["assessment"]["confidence"], p.max(1.0 - p));
        assert_eq!(signal["assessment"]["selected"], json!({"boolean":p>=0.7}));
        assert_eq!(signal["assessment"]["uncertain"], (p - 0.7).abs() < 0.05);
        assert_eq!(signal["threshold"], 0.7);
        assert_eq!(signal["margin"], 0.05);
    }
    assert_eq!(
        metric(&result, "input_counted"),
        metric(&result, "input_reported") + metric(&result, "unknown_input_counted")
    );
    let with = run(
        root,
        &config,
        &[
            "keep",
            "-p",
            "outage-test",
            "--why",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    code(&with, 0);
    assert_eq!(
        with.stdout,
        b"Checkout is unavailable and customers cannot pay.\n"
    );
    assert!(String::from_utf8_lossy(&with.stderr).contains("[impact "));
    let quiet = run(
        root,
        &config,
        &[
            "-q",
            "tag",
            "-p",
            "severity-test",
            "--why",
            "--json",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    code(&quiet, 0);
    assert!(quiet.stderr.is_empty());
    assert!(records(&quiet)[0].get("signals").is_some());
    let without = run(
        root,
        &config,
        &[
            "tag",
            "-p",
            "severity-test",
            "--json",
            "--strict",
            "--no-cache",
        ],
        input,
    );
    code(&without, 0);
    assert_eq!(records(&without)[0].as_object().unwrap().len(), 5);
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_selection_why_uses_each_final_choice_or_independent_score_request() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let source=b"Checkout works normally.\nCheckout is unavailable and customers cannot pay.\nBirds sing outside.";
    for flags in [
        vec!["find", "-p", "retrieval-test"],
        vec!["pick", "-p", "retrieval-test"],
        vec!["pick", "-p", "rank-test", "--independent"],
    ] {
        let mut args = flags;
        args.extend(["--why", "--json", "--strict", "--no-cache"]);
        let result = run(root, &config, &args, source);
        code(&result, 0);
        let record = &records(&result)[0];
        assert_eq!(record["id"], "L2");
        assert_eq!(record["signals"]["impact"]["field"], "signal:impact");
        let history = &record["provenance"]["history"];
        let evaluations = history["evaluations"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_array()
            .unwrap();
        let index = if record["probability_scope"] == "independent-candidate-score" {
            1
        } else {
            0
        };
        assert_eq!(
            record["signals"]["impact"]["answer"],
            evaluations[index]["response"]["answers"]["signal:impact"]
        );
    }
    let result = run(
        root,
        &config,
        &[
            "extract",
            "-p",
            "billing-test",
            "--candidates",
            "emails",
            "--why",
            "--json",
            "--strict",
            "--no-cache",
        ],
        b"Support: help@example.com; billing: bills@example.org.",
    );
    code(&result, 0);
    assert_eq!(records(&result)[0]["unit"], "bills@example.org");
    assert!(records(&result)[0].get("signals").is_some());
    let failure = run(
        root,
        &config,
        &[
            "find",
            "-p",
            "retrieval-test",
            "--why",
            "--json",
            "--margin",
            "1",
            "--on-uncertain",
            "fail",
            "--strict",
            "--no-cache",
        ],
        source,
    );
    code(&failure, 3);
    assert!(failure.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("[impact "));
}

fn dataset(root: &Path) -> PathBuf {
    let path = root.join("cases.jsonl");
    let cases=(0..8).map(|index| json!({"id":format!("case-{index}"),"unit":if index%2==0 {"Checkout is unavailable and customers cannot pay."} else {"Checkout recovered after a successful retry."},"label":index%2==0,"source":"synthetic functional signal fixture"})).map(|case|serde_json::to_string(&case).unwrap()).collect::<Vec<_>>().join("\n");
    std::fs::write(&path, format!("{cases}\n")).unwrap();
    path
}

#[test]
fn signal_eval_plans_pack_companions_without_requiring_extra_ground_truth() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let dataset = dataset(root);
    let result = run(
        root,
        &config,
        &[
            "eval",
            dataset.to_str().unwrap(),
            "-p",
            "outage-test",
            "--dataset-revision",
            "signals-fixture-v1",
            "--init-splits",
            "--dry-run",
            "--sweep-pack",
            "unit,window:2",
            "--json",
        ],
        b"",
    );
    code(&result, 0);
    assert!(!result.stdout.is_empty());
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_signal_packing_evaluation_preserves_all_head_answers_and_scores_only_labeled_fields() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let dataset = dataset(root);
    let report = root.join("report");
    let result = run(
        root,
        &config,
        &[
            "eval",
            dataset.to_str().unwrap(),
            "-p",
            "outage-test",
            "--dataset-revision",
            "signals-fixture-v1",
            "--init-splits",
            "--sweep-pack",
            "unit,window:2",
            "--warmup",
            "0",
            "--allow-unverified",
            "--no-cache",
            "--strict",
            "--json",
            "--report",
            report.to_str().unwrap(),
        ],
        b"",
    );
    code(&result, 0);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    let variants = value["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    for variant in variants {
        let suite = &variant["report"]["suites"][0];
        let metrics = suite["questions"].as_array().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0]["field"], "outage-test");
        let records = suite["records"].as_array().unwrap();
        assert!(!records.is_empty());
        for record in records {
            assert_eq!(record["questions"].as_object().unwrap().len(), 3);
            assert_eq!(record["base_question_order"].as_array().unwrap().len(), 3);
            for id in ["signal:impact", "signal:retried"] {
                let field = record["binding"][id].as_str().unwrap();
                assert_eq!(
                    record["evaluation"]["response"]["answers"][field]["type"],
                    "noul"
                );
            }
            assert_eq!(record["policies"]["signal:impact"]["threshold"], 0.7);
        }
    }
    assert_eq!(
        value["comparisons"][0]["paired_fields"].as_u64().unwrap(),
        variants[0]["report"]["suites"][0]["records"]
            .as_array()
            .unwrap()
            .len() as u64
    );
    assert!(report.exists());
    let preset_path = root.join("xdg/loupe/presets/mixed-signals.json");
    let mut preset: Value = serde_json::from_slice(
        &std::fs::read(root.join("xdg/loupe/presets/outage-test.json")).unwrap(),
    )
    .unwrap();
    let question = json!({"type":"noul","instructions":preset["instructions"]});
    preset.as_object_mut().unwrap().remove("type");
    preset.as_object_mut().unwrap().remove("instructions");
    preset["questions"] = json!({"outage":question});
    std::fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let labelled = root.join("labelled.jsonl");
    let lines = std::fs::read_to_string(&dataset)
        .unwrap()
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).unwrap();
            value["label"] = json!({"outage":value["label"],"signal:impact":value["label"]});
            serde_json::to_string(&value).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&labelled, format!("{lines}\n")).unwrap();
    let result = run(
        root,
        &config,
        &[
            "eval",
            labelled.to_str().unwrap(),
            "-p",
            "mixed-signals",
            "--dataset-revision",
            "labelled-signals-v1",
            "--init-splits",
            "--partition",
            "calibrate",
            "--pack",
            "window:2",
            "--threshold",
            "0.95",
            "--warmup",
            "0",
            "--allow-unverified",
            "--no-cache",
            "--strict",
            "--json",
        ],
        b"",
    );
    code(&result, 0);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    let suite = &value["suites"][0];
    let fields = suite["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["field"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        fields,
        std::collections::BTreeSet::from(["outage", "signal:impact"])
    );
    for record in suite["records"].as_array().unwrap() {
        assert_eq!(record["policies"]["outage"]["threshold"], 0.95);
        assert_eq!(record["policies"]["signal:impact"]["threshold"], 0.7);
    }
}
