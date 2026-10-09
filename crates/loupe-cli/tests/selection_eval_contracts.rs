#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const SUITES: &[&str] = &[
    "find-pick/find",
    "find-pick/pick",
    "find-pick/independent",
    "out-of-scope/selection",
    "extract/dates",
    "extract/money",
    "extract/semver",
    "extract/emails",
    "extract/hostnames",
    "extract/tickers",
];
fn fixture(root: &Path) {
    let source = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../evals/suites"));
    for name in SUITES {
        let target = root.join(name);
        std::fs::create_dir_all(&target).unwrap();
        for file in ["cases.jsonl", "suite.toml"] {
            std::fs::copy(source.join(name).join(file), target.join(file)).unwrap();
        }
    }
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
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_CONCURRENCY")
        .output()
        .unwrap()
}
fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn pinned_seeds_have_fixed_disjoint_labels_and_explicit_draft_provenance() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    for name in SUITES {
        let suite =
            loupe_core::suites::Suite::load(&root.path().join(name).join("suite.toml")).unwrap();
        let splits = suite.planned_splits().unwrap();
        assert!(!splits.test.is_empty());
        assert!(splits.calibrate.iter().all(|id| !splits.test.contains(id)));
        assert!(
            suite
                .cases
                .iter()
                .all(|case| !case.reviewed && case.source == "handwritten-fictional")
        );
        assert_eq!(suite.manifest.provenance.license, "MIT");
        assert!(splits.test.iter().any(|id|suite.cases.iter().any(|case|&case.id==id&&matches!(&case.label,loupe_core::suites::Label::Ids(ids) if ids.is_empty()))));
    }
}
#[test]
fn full_offline_plans_keep_labels_out_of_requests_and_original_unicode_spans() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let result = report(&run(
        root.path(),
        &[
            "eval",
            "--suite",
            "all",
            "--suites-dir",
            ".",
            "--init-splits",
            "--dry-run",
            "--json",
        ],
    ));
    assert_eq!(result["status"], "plan");
    assert_eq!(result["token_usage"]["attempts"], 0);
    assert_eq!(result["suites"].as_array().unwrap().len(), 10);
    for suite in result["suites"].as_array().unwrap() {
        let cases = suite["selection"]["cases"].as_array().unwrap();
        assert_eq!(
            cases.len(),
            suite["selected_case_ids"].as_array().unwrap().len()
        );
        for case in cases {
            assert_eq!(case["complete"], false);
            assert!(case["requests"].as_array().unwrap().is_empty());
            for span in case["expected_spans"].as_array().unwrap() {
                let text = case["text"].as_str().unwrap();
                let start = span["byte_start"].as_u64().unwrap() as usize;
                let end = span["byte_end"].as_u64().unwrap() as usize;
                assert_eq!(&text[start..end], span["text"].as_str().unwrap());
                assert!(case["catalog"].as_array().unwrap().iter().any(
                    |candidate| candidate["id"] == span["id"]
                        && candidate["verbatim"] == span["text"]
                        && candidate["location"]["byte_start"] == span["byte_start"]
                        && candidate["location"]["byte_end"] == span["byte_end"]
                ));
            }
            let plan = case["plan"].to_string();
            assert!(
                !plan.contains("expected_spans")
                    && !plan.contains("normalized")
                    && !plan.contains("handwritten-fictional")
            );
        }
        assert!(suite["selection"]["retrieval"]["exact_match"]["value"].is_null());
    }
}
#[test]
fn incompatible_modes_and_unknown_gold_fail_before_split_publication() {
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
            "find-pick/find",
            "--init-splits",
            "--dry-run",
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
        assert!(!root.path().join("find-pick/find/splits.json").exists());
    }
    let rootcase = root.path().join("find-pick/find");
    let path = rootcase.join("cases.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"label\":[\"L2\"]", "\"label\":[\"L99\"]");
    std::fs::write(&path, text).unwrap();
    let pin = loupe_core::suites::Pin::capture(
        &rootcase,
        Path::new("cases.jsonl"),
        "fictional-selection-v1",
    )
    .unwrap();
    let manifest = rootcase.join("suite.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let old = text
        .split("blake3 = \"")
        .nth(1)
        .unwrap()
        .split('\"')
        .next()
        .unwrap();
    std::fs::write(&manifest, text.replace(old, &pin.blake3)).unwrap();
    let result = run(
        root.path(),
        &[
            "eval",
            "--suite",
            "find-pick/find",
            "--init-splits",
            "--json",
        ],
    );
    assert_eq!(
        result.status.code(),
        Some(65),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!rootcase.join("splits.json").exists());
}
#[test]
#[ignore = "actual server selection, no fake inference"]
fn actual_selection_suite_keeps_raw_rounds_normalization_and_uncached_latency() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let result = report(&run(
        root.path(),
        &[
            "eval",
            "--suite",
            "extract/semver",
            "--suite",
            "find-pick/independent",
            "--init-splits",
            "--allow-unverified",
            "--strict",
            "--no-cache",
            "--warmup",
            "0",
            "--deadline",
            "60s",
            "--json",
        ],
    ));
    assert_eq!(result["status"], "provisional");
    assert_eq!(
        result["token_usage"]["spent_input"],
        result["token_usage"]["reported_input"]
    );
    assert_eq!(result["token_usage"]["cache_hits"], 0);
    for suite in result["suites"].as_array().unwrap() {
        assert_eq!(suite["selection"]["retrieval"]["cases"], 2);
        assert_eq!(suite["phases"][1]["name"], "latency");
        assert_eq!(suite["phases"][1]["cache_requested"], false);
        for case in suite["selection"]["cases"].as_array().unwrap() {
            assert_eq!(case["complete"], true);
            for request in case["requests"].as_array().unwrap() {
                assert_eq!(
                    request["counts"]["input_tokens"],
                    request["evaluation"]["response"]["usage"]["input_tokens"]
                );
                assert_eq!(request["evaluation"]["cached"], false);
            }
        }
    }
    let uncertain = report(&run(
        root.path(),
        &[
            "eval",
            "--suite",
            "extract/semver",
            "--allow-unverified",
            "--strict",
            "--no-cache",
            "--warmup",
            "0",
            "--margin",
            "1",
            "--deadline",
            "30s",
            "--json",
        ],
    ));
    let suite = &uncertain["suites"][0]["selection"];
    assert_eq!(suite["coverage"]["value"], 0.0);
    assert_eq!(suite["absence_accuracy"]["value"], 0.0);
    assert!(
        suite["cases"]
            .as_array()
            .unwrap()
            .iter()
            .all(|case| case["uncertain"] == true)
    );
}
