#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const MANIFEST: &str = include_str!("../../../evals/suites/join/suite.toml");
const CASES: &[u8] = include_bytes!("../../../evals/suites/join/cases.jsonl");

fn fixture(root: &Path) {
    std::fs::write(root.join("suite.toml"), MANIFEST).unwrap();
    std::fs::write(root.join("cases.jsonl"), CASES).unwrap();
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
        ])
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_CONFIG")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_API_KEY")
        .output()
        .unwrap()
}
fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn seed_has_exact_pins_and_four_disjoint_scenario_strata() {
    let declared = MANIFEST
        .lines()
        .find_map(|line| {
            line.strip_prefix("blake3 = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .unwrap();
    assert_eq!(declared, blake3::hash(CASES).to_hex().as_str());
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let suite = loupe_core::suites::Suite::load(&root.path().join("suite.toml")).unwrap();
    let splits = suite.planned_splits().unwrap();
    assert_eq!(splits.calibrate.len(), 4);
    assert_eq!(splits.test.len(), 4);
    assert!(splits.calibrate.iter().all(|id| !splits.test.contains(id)));
    for kind in ["duplicate-", "abbreviation-", "cap-", "negative-"] {
        assert_eq!(
            splits.test.iter().filter(|id| id.starts_with(kind)).count(),
            1
        );
        assert_eq!(
            splits
                .calibrate
                .iter()
                .filter(|id| id.starts_with(kind))
                .count(),
            1
        );
    }
    assert!(
        suite
            .cases
            .iter()
            .all(|case| !case.reviewed && case.source == "handwritten-fictional")
    );
}
#[test]
fn offline_join_plan_exposes_full_pairs_and_preserves_fixed_partitions() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let plan = report(&run(
        root.path(),
        &[
            "eval",
            "--suite",
            "suite.toml",
            "--dry-run",
            "--init-splits",
            "--json",
        ],
    ));
    assert_eq!(plan["status"], "plan");
    assert_eq!(plan["token_usage"]["attempts"], 0);
    let suite = &plan["suites"][0];
    let cases = suite["join"]["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    assert_eq!(suite["partition"], "test");
    assert_eq!(suite["join"]["blocking_recall"]["support"], 7);
    assert_eq!(suite["join"]["blocking_recall"]["value"], 5.0 / 7.0);
    for case in cases {
        assert_eq!(case["pairs"].as_array().unwrap().len(), 6);
        assert!(case["assignments"].as_array().unwrap().is_empty());
        for pair in case["pairs"].as_array().unwrap() {
            assert!(pair["evaluation"].is_null());
            assert_eq!(pair["request"]["state"].as_object().unwrap().len(), 2);
            assert!(pair["request"]["state"].get("label").is_none());
            assert!(pair["counts"]["input_tokens"].as_u64().unwrap() > 0);
        }
    }
    let split = std::fs::read(root.path().join("splits.json")).unwrap();
    report(&run(
        root.path(),
        &["eval", "--suite", "suite.toml", "--dry-run", "--json"],
    ));
    assert_eq!(
        split,
        std::fs::read(root.path().join("splits.json")).unwrap()
    );
}

#[test]
fn invalid_catalogs_and_inapplicable_modes_fail_before_credentials_or_split_writes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    for flags in [
        vec!["--pack", "unit"],
        vec!["--sweep-pack", "unit,window:4"],
        vec!["--ensemble"],
    ] {
        let mut args = vec![
            "eval",
            "--suite",
            "suite.toml",
            "--dry-run",
            "--init-splits",
            "--json",
        ];
        args.extend(flags);
        let result = run(root.path(), &args);
        assert_eq!(
            result.status.code(),
            Some(64),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!root.path().join("splits.json").exists());
    }
    let capped = MANIFEST.replace("max_pairs = 10000", "max_pairs = 1");
    std::fs::write(root.path().join("suite.toml"), capped).unwrap();
    let result = run(
        root.path(),
        &[
            "eval",
            "--suite",
            "suite.toml",
            "--dry-run",
            "--init-splits",
            "--json",
        ],
    );
    assert_eq!(result.status.code(), Some(65));
    assert!(!root.path().join("splits.json").exists());
    std::fs::write(root.path().join("suite.toml"), MANIFEST).unwrap();
    let mut cases = CASES
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    for case in &mut cases {
        case["unit"]["right"] = serde_json::json!([]);
    }
    let bytes = cases
        .iter()
        .map(|case| serde_json::to_string(case).unwrap() + "\n")
        .collect::<String>();
    std::fs::write(root.path().join("cases.jsonl"), &bytes).unwrap();
    std::fs::write(
        root.path().join("suite.toml"),
        MANIFEST.replace(
            blake3::hash(CASES).to_hex().as_str(),
            blake3::hash(bytes.as_bytes()).to_hex().as_str(),
        ),
    )
    .unwrap();
    let result = run(
        root.path(),
        &["eval", "--suite", "suite.toml", "--init-splits", "--json"],
    );
    assert_eq!(
        result.status.code(),
        Some(65),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!root.path().join("splits.json").exists());
}

#[test]
#[ignore = "actual server inference, no fake backend"]
fn actual_join_suite_preserves_all_pairs_and_uncached_cardinality_pipelines() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let output = run(
        root.path(),
        &[
            "eval",
            "--suite",
            "suite.toml",
            "--init-splits",
            "--allow-unverified",
            "--strict",
            "--no-cache",
            "--warmup",
            "0",
            "--deadline",
            "180s",
            "--json",
        ],
    );
    let report = report(&output);
    assert_eq!(report["status"], "provisional");
    assert_eq!(
        report["token_usage"]["spent_input"],
        report["token_usage"]["reported_input"]
    );
    assert_eq!(report["token_usage"]["cache_hits"], 0);
    let suite = &report["suites"][0];
    assert_eq!(suite["join"]["all_pair_quality"]["cases"], 24);
    for cardinality in suite["join"]["assignments"].as_array().unwrap() {
        assert_eq!(cardinality["rows"], 8);
    }
    let latency = &suite["phases"][1];
    assert_eq!(latency["name"], "latency");
    assert_eq!(latency["cache_requested"], false);
    assert_eq!(latency["groups"].as_array().unwrap().len(), 12);
    assert!(
        latency["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|group| group["complete"] == true)
    );
    let first = &suite["join"]["cases"][0]["pairs"][0]["counts"]["input_tokens"];
    let budget = first.as_u64().unwrap().to_string();
    let partial = run(
        root.path(),
        &[
            "eval",
            "--suite",
            "suite.toml",
            "--allow-unverified",
            "--strict",
            "--no-cache",
            "--warmup",
            "0",
            "--budget",
            &budget,
            "--json",
        ],
    );
    assert_eq!(partial.status.code(), Some(4));
    let partial: Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(partial["status"], "incomplete");
    assert_eq!(partial["token_usage"]["attempts"], 1);
    assert_eq!(
        partial["token_usage"]["spent_input"],
        budget.parse::<u64>().unwrap()
    );
    assert_eq!(
        partial["token_usage"]["reported_input"],
        partial["token_usage"]["spent_input"]
    );
    assert_eq!(partial["suites"][0]["join"]["all_pair_quality"]["cases"], 1);
    assert_eq!(partial["suites"][0]["join"]["assignments"][0]["rows"], 0);
}
