#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args([
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--profile",
            "server",
        ])
        .args(arguments)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_CONFIG")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_PRESET_PATH")
        .output()
        .unwrap()
}

fn fixture(root: &Path, live: bool) {
    std::fs::create_dir(root.join(".git")).unwrap();
    let directory = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("rubric.toml"),
        "version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n",
    )
    .unwrap();
    let mut cases = Vec::new();
    for index in 0..8 {
        serde_json::to_writer(&mut cases,&json!({"id":format!("case-{index}"),"unit":if index%2==0{"Checkout is down and customers cannot pay."}else{"The sky is blue today."},"label":index%2==0})).unwrap();
        cases.push(b'\n');
    }
    std::fs::write(root.join("cases.jsonl"), cases).unwrap();
    std::fs::write(root.join("config.toml"),if live {include_str!("../../../examples/config.toml").to_owned()}else{format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n",root.join("credential-executed").display())}).unwrap();
}

fn publish(root: &Path, report: &Value) -> Value {
    let bytes = serde_json::to_vec_pretty(report).unwrap();
    let store = loupe_core::report_store::Store::open(&root.join("reports")).unwrap();
    let bundle = store
        .publish(&bytes, b"Retained actual server report\n")
        .unwrap();
    let record = json!({"schema_version":1,"report":bundle.strip_prefix(root).unwrap(),"report_blake3":blake3::hash(&bytes).to_hex().as_str(),"ratification":{"by":"codec-test fixture, no production ratification","at":"2026-10-06T22:00:00Z","justification":"File/grammar test only; provisional responses must never become an accepted baseline."},"checks":[{"suite":report["suites"][0]["manifest"]["id"],"field":"rubric","minimum_support":1,"gate":{"metric":"accuracy","minimum":0.8,"max_drop":0.05}}]});
    std::fs::write(
        root.join("baseline.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    record
}

#[test]
fn offline_baseline_flags_and_ratification_reject_before_credentials_or_split_publication() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    let saved: Value =
        serde_json::from_str(include_str!("../../loupe-core/testdata/captured-eval.json")).unwrap();
    let record = publish(root, &saved["checks"][2]["report"]);
    for flags in [
        vec!["--partition", "calibrate"],
        vec!["--partition", "captured"],
        vec!["--sweep-pack", "unit,window:4"],
        vec!["--compare", "standalone", "ensemble"],
    ] {
        let mut arguments = vec![
            "eval",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "functional-v1",
            "--baseline",
            "baseline.json",
            "--dry-run",
            "--init-splits",
        ];
        arguments.extend(flags);
        assert_eq!(run(root, &arguments).status.code(), Some(64));
    }
    assert_eq!(
        run(
            root,
            &[
                "calibrate",
                "cases.jsonl",
                "-p",
                "rubric",
                "--dataset-revision",
                "functional-v1",
                "--baseline",
                "baseline.json",
                "--target",
                "precision=0.9",
                "--dry-run",
                "--init-splits"
            ]
        )
        .status
        .code(),
        Some(64)
    );
    for field in ["by", "at", "justification"] {
        let mut changed = record.clone();
        changed["ratification"][field] = json!("");
        std::fs::write(
            root.join("baseline.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert_eq!(
            run(
                root,
                &[
                    "eval",
                    "cases.jsonl",
                    "-p",
                    "rubric",
                    "--dataset-revision",
                    "functional-v1",
                    "--baseline",
                    "baseline.json",
                    "--dry-run",
                    "--init-splits"
                ]
            )
            .status
            .code(),
            Some(65)
        );
    }
    std::fs::write(
        root.join("baseline.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let result = run(
        root,
        &[
            "eval",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "functional-v1",
            "--baseline",
            "baseline.json",
            "--allow-unverified",
            "--dry-run",
            "--init-splits",
        ],
    );
    assert_eq!(
        result.status.code(),
        Some(69),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(!root.join("credential-executed").exists());
    assert!(!root.join("cases.jsonl.splits.json").exists());
}

#[test]
#[ignore = "requires actual server inference; no fake model/HTTP backend"]
fn real_server_held_out_reports_cannot_be_ratified_or_used_as_baselines_without_full_identity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, true);
    let output = run(
        root,
        &[
            "eval",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "authored-functional-v1",
            "--allow-unverified",
            "--strict",
            "--no-cache",
            "--init-splits",
            "--warmup",
            "0",
            "--deadline",
            "60s",
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "provisional");
    assert_eq!(report["suites"][0]["partition"], "test");
    assert_eq!(
        report["suites"][0]["selected_case_ids"],
        report["suites"][0]["splits"]["test"]
    );
    assert!(report.get("acceptance").is_none());
    assert_eq!(
        report["token_usage"]["spent_input"],
        report["token_usage"]["reported_input"]
    );
    for record in report["suites"][0]["records"].as_array().unwrap() {
        assert_eq!(
            record["counts"]["input_tokens"],
            record["evaluation"]["response"]["usage"]["input_tokens"]
        );
    }
    publish(root, &report);
    let preset = root.join("xdg/loupe/presets/rubric.toml");
    let original = std::fs::read(&preset).unwrap();
    for dry in [false, true] {
        let mut arguments = vec![
            "eval",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "authored-functional-v1",
            "--baseline",
            "baseline.json",
            "--allow-unverified",
            "--budget",
            "0",
            "--warmup",
            "0",
            "--json",
        ];
        if dry {
            arguments.push("--dry-run");
        }
        let result = run(root, &arguments);
        assert_eq!(
            result.status.code(),
            Some(69),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
    }
    assert_eq!(std::fs::read(&preset).unwrap(), original);
}
