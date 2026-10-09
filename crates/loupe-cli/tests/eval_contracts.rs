#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn run(root: &Path, args: &[&str]) -> Output {
    let local = root.join("config.toml");
    let config = if local.exists() {
        local
    } else {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/config.toml"
        ))
        .to_owned()
    };
    Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args(["--config", config.to_str().unwrap(), "--profile", "server"])
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_CONFIG")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("LOUPE_API_KEY")
        .output()
        .unwrap()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}
fn report(output: &Output) -> Value {
    success(output);
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture(root: &Path, mixed: bool) {
    std::fs::create_dir(root.join(".git")).unwrap();
    let presets = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&presets).unwrap();
    let document = if mixed {
        "version=1\n[questions.available]\ntype='noul'\ninstructions='Is checkout unavailable?'\n[questions.topic]\ntype='choice'\ninstructions='Choose the main topic.'\n[questions.topic.criteria]\ncommerce='Checkout and payments'\nweather='Weather and clouds'\n[questions.severity]\ntype='score'\ninstructions='Rate checkout impact from zero to two.'\ncriteria=['No impact','Minor impact','Checkout is entirely unavailable']\n"
    } else {
        "version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n"
    };
    std::fs::write(presets.join("rubric.toml"), document).unwrap();
    let mut data = String::new();
    for index in 0..8 {
        if index == 1 {
            data.push('\n');
        }
        let label = if mixed {
            json!({"available":index%2==0,"topic":if index%2==0{"commerce"}else{"weather"},"severity":if index%2==0{2}else{0}})
        } else {
            json!(index % 2 == 0)
        };
        let unit = if index % 2 == 0 {
            "Checkout is down and customers cannot pay."
        } else {
            "Blue skies and sunshine all day."
        };
        data.push_str(&serde_json::to_string(&json!({"id":format!("case-{index}"),"unit":unit,"label":label,"notes":"LABEL_SECRET_DO_NOT_SEND"})).unwrap());
        data.push('\n');
    }
    data.push_str(&serde_json::to_string(&json!({"id":"review","unit":"Reviewed example","label":if mixed {json!({"available":true,"topic":"commerce","severity":2})}else{json!(true)},"source":"review","reviewed":true,"sampling":"uniform-audit"})).unwrap());
    data.push('\n');
    std::fs::write(root.join("cases.jsonl"), data).unwrap();
}
fn args(extra: &[&str]) -> Vec<String> {
    let mut args = [
        "eval",
        "cases.jsonl",
        "--preset",
        "rubric",
        "--dataset-revision",
        "reviewed-v1",
        "--json",
    ]
    .map(str::to_owned)
    .to_vec();
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    args
}
fn execute(root: &Path, extra: &[&str]) -> Output {
    let args = args(extra);
    run(root, &args.iter().map(String::as_str).collect::<Vec<_>>())
}

#[test]
fn offline_ensemble_comparison_preserves_splits_raw_schema_counts_and_trusted_credential_boundary()
{
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, true);
    let path = root.join("xdg/loupe/presets/rubric.toml");
    let mut document = loupe_core::presets::read_document(&path).unwrap();
    document["variants"] = json!([
        {"field":"available","id":"direct","instructions":"Are customers unable to check out?"},
        {"field":"topic","id":"direct","instructions":"Which subject does this discuss?"},
        {"field":"severity","id":"direct","instructions":"Rate the customer impact from zero to two."}
    ]);
    std::fs::write(&path, loupe_core::presets::to_toml(&document).unwrap()).unwrap();
    let marker = root.join("credential-executed");
    let config = include_str!("../../../examples/config.toml").replace(
        "[profile.server]",
        &format!(
            "[profile.server]\napi_key_cmd=\"touch {}\"",
            marker.display()
        ),
    );
    std::fs::write(root.join("config.toml"), config).unwrap();
    let actual = report(&execute(
        root,
        &[
            "--compare",
            "standalone",
            "ensemble",
            "--ensemble",
            "--min-agreement",
            "0.6",
            "--dry-run",
            "--init-splits",
            "--budget",
            "0",
        ],
    ));
    assert_eq!(actual["kind"], "comparison");
    assert_eq!(actual["status"], "plan");
    assert!(!marker.exists());
    let variants = actual["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    let left = &variants[0]["report"]["suites"][0];
    let right = &variants[1]["report"]["suites"][0];
    assert_eq!(left["selected_case_ids"], right["selected_case_ids"]);
    assert_eq!(left["splits"], right["splits"]);
    assert_eq!(left["partition"], "test");
    for (left, right) in left["records"]
        .as_array()
        .unwrap()
        .iter()
        .zip(right["records"].as_array().unwrap())
    {
        assert_eq!(left["case_id"], right["case_id"]);
        assert_eq!(left["label"], right["label"]);
        assert!(left.get("ensemble").is_none());
        assert_eq!(right["ensemble"]["settings"]["min_agreement"], 0.6);
        assert_eq!(left["request"]["questions"].as_object().unwrap().len(), 3);
        assert_eq!(right["request"]["questions"].as_object().unwrap().len(), 6);
        assert_eq!(right["counts"]["exact"], true);
        assert!(
            right["counts"]["input_tokens"].as_u64().unwrap()
                > left["counts"]["input_tokens"].as_u64().unwrap()
        );
        assert!(
            !right["request"]
                .to_string()
                .contains("LABEL_SECRET_DO_NOT_SEND")
        );
    }
    let bad = execute(root, &["--ensemble", "--min-agreement", "1.1", "--dry-run"]);
    assert_eq!(bad.status.code(), Some(64));
    assert!(!marker.exists());
    let bad = run(
        root,
        &[
            "calibrate",
            "cases.jsonl",
            "--preset",
            "rubric",
            "--dataset-revision",
            "reviewed-v1",
            "--ensemble",
            "--target",
            "precision=0.9",
            "--dry-run",
        ],
    );
    assert_eq!(bad.status.code(), Some(64));
    assert!(!marker.exists());
}

#[test]
#[ignore = "requires a reachable actual server model; no fake backend"]
fn actual_server_ensemble_comparison_measures_raw_primary_shift_and_keeps_fixed_test_cases() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, false);
    let path = root.join("xdg/loupe/presets/rubric.toml");
    let mut document = loupe_core::presets::read_document(&path).unwrap();
    document["variants"] =
        json!([{"id":"direct","instructions":"Are customers currently unable to check out?"}]);
    std::fs::write(&path, loupe_core::presets::to_toml(&document).unwrap()).unwrap();
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let arguments = args(&[
        "--compare",
        "standalone",
        "ensemble",
        "--init-splits",
        "--warmup",
        "0",
        "--strict",
        "--no-cache",
        "--allow-unverified",
        "--budget",
        "12000",
        "--deadline",
        "60s",
    ]);
    let result = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
            "--url",
            &url,
        ])
        .args(arguments)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_CONFIG")
        .output()
        .unwrap();
    let result = report(&result);
    let variants = result["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    let left = &variants[0]["report"]["suites"][0];
    let right = &variants[1]["report"]["suites"][0];
    assert_eq!(left["selected_case_ids"], right["selected_case_ids"]);
    assert_eq!(left["splits"], right["splits"]);
    let mut raw_max: f64 = 0.0;
    let mut raw_total = 0.0;
    let mut cases = 0;
    for record in left["records"].as_array().unwrap() {
        let joint = right["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["case_id"] == record["case_id"])
            .unwrap();
        let p = record["evaluation"]["response"]["answers"]["rubric"]["noul"]
            .as_f64()
            .unwrap();
        let q = joint["evaluation"]["response"]["answers"]["rubric"]["noul"]
            .as_f64()
            .unwrap();
        let delta = (p - q).abs();
        raw_max = raw_max.max(delta);
        raw_total += delta;
        cases += 1;
        let aggregate = &joint["ensemble"]["reports"]["rubric"];
        assert_eq!(aggregate["independent"], false);
        assert_eq!(
            aggregate["members"][0]["answer"],
            joint["evaluation"]["response"]["answers"]["rubric"]
        );
        assert_eq!(aggregate["members"].as_array().unwrap().len(), 2);
    }
    let measured = &result["comparisons"][0]["fields"][0]["raw_primary_shift"];
    assert_eq!(measured["cases"].as_u64().unwrap(), cases);
    assert!((measured["max_probability_difference"].as_f64().unwrap() - raw_max).abs() < 1e-12);
    assert!(
        (measured["mean_probability_difference"].as_f64().unwrap() - raw_total / cases as f64)
            .abs()
            < 1e-12
    );
    assert_eq!(result["token_usage"]["unknown_attempts"], 0);
    assert_eq!(
        result["token_usage"]["spent_input"],
        result["token_usage"]["reported_input"]
    );
    assert_eq!(result["token_usage"]["cache_hits"], 0);
    assert!(result["token_usage"]["spent_input"].as_u64().unwrap() <= 12000);
    for variant in variants {
        let phases = variant["report"]["suites"][0]["phases"].as_array().unwrap();
        assert!(phases.iter().any(|phase| phase["name"] == "latency"));
        assert!(phases.iter().all(|phase| phase["cache_hits"] == 0));
        // Legacy metadata diagnostics do not establish a full-identity baseline.
        if variant["report"]["identity"]["verified"] == false {
            assert_eq!(variant["report"]["status"], "provisional");
        }
    }
}

#[test]
fn offline_rubric_comparison_keeps_one_fixed_split_and_never_executes_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    std::fs::write(
        root.join("xdg/loupe/presets/alternate.toml"),
        "version=2\ntype='noul'\ninstructions='Does this describe an unavailable checkout?'\n",
    )
    .unwrap();
    let marker = root.join("credential-executed");
    std::fs::write(root.join("config.toml"), format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n", marker.display())).unwrap();
    let actual = report(&execute(
        root,
        &[
            "--compare",
            "preset:rubric",
            "preset:alternate",
            "--dry-run",
            "--init-splits",
            "--budget",
            "0",
            "--report",
            "reports",
        ],
    ));
    assert_eq!(actual["kind"], "comparison");
    assert_eq!(actual["status"], "plan");
    assert_eq!(actual["token_usage"]["attempts"], 0);
    assert_eq!(actual["variants"].as_array().unwrap().len(), 2);
    let left = &actual["variants"][0]["report"]["suites"][0];
    let right = &actual["variants"][1]["report"]["suites"][0];
    assert_eq!(left["splits"], right["splits"]);
    assert_eq!(left["selected_case_ids"], right["selected_case_ids"]);
    assert!(left["records"][0]["questions"]["rubric"].is_object());
    assert!(right["records"][0]["questions"]["alternate"].is_object());
    assert!(!marker.exists());
    let bundle = std::fs::read_dir(root.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved: Value =
        serde_json::from_slice(&loupe_core::report_store::read(&bundle).unwrap()).unwrap();
    assert_eq!(saved, actual);
}

#[test]
fn offline_sweeps_use_only_calibration_and_validate_all_variants_before_split_writes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    for flags in [
        vec!["--sweep-pack", "unit,window:4", "--partition", "test"],
        vec!["--sweep-pack", "auto"],
        vec!["--sweep-combine", "any,vote"],
    ] {
        let mut arguments = flags;
        arguments.extend(["--dry-run", "--init-splits"]);
        assert_eq!(execute(root, &arguments).status.code(), Some(64));
        assert!(!root.join("cases.jsonl.splits.json").exists());
    }
    let actual = report(&execute(
        root,
        &[
            "--sweep-pack",
            "unit,window:4",
            "--sweep-combine",
            "any,mean",
            "--sweep-focus",
            "0,3",
            "--dry-run",
            "--init-splits",
            "--overlap",
            "0",
        ],
    ));
    assert_eq!(actual["kind"], "sweep");
    assert_eq!(actual["status"], "plan");
    assert_eq!(actual["variants"].as_array().unwrap().len(), 6);
    assert_eq!(actual["token_usage"]["attempts"], 0);
    for variant in actual["variants"].as_array().unwrap() {
        let suite = &variant["report"]["suites"][0];
        assert_eq!(suite["partition"], "calibrate");
        assert_eq!(suite["selected_case_ids"], suite["splits"]["calibrate"]);
        assert!(
            !suite["selected_case_ids"]
                .as_array()
                .unwrap()
                .contains(&json!("review"))
        );
        assert!(
            suite["selected_case_ids"]
                .as_array()
                .unwrap()
                .iter()
                .all(|id| !suite["splits"]["test"].as_array().unwrap().contains(id))
        );
    }
}

#[test]
fn saved_real_report_comparison_is_offline_and_refuses_corrupt_or_unverified_bundles() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let evidence: Value =
        serde_json::from_str(include_str!("../../loupe-core/testdata/captured-eval.json")).unwrap();
    let store = loupe_core::report_store::Store::open(&root.join("saved")).unwrap();
    let left = store
        .publish(
            &serde_json::to_vec(&evidence["checks"][2]["report"]).unwrap(),
            b"Captured actual unit report\n",
        )
        .unwrap();
    let right = store
        .publish(
            &serde_json::to_vec(&evidence["checks"][3]["report"]).unwrap(),
            b"Captured actual window report\n",
        )
        .unwrap();
    let actual = report(&run(
        root,
        &[
            "eval",
            "--compare",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--allow-unverified",
            "--budget",
            "0",
            "--json",
            "--report",
            "comparisons",
        ],
    ));
    assert_eq!(actual["status"], "provisional");
    assert_eq!(actual["token_usage"]["attempts"], 0);
    assert_eq!(actual["token_usage"]["spent_input"], 0);
    assert_eq!(actual["comparisons"][0]["paired_fields"], 12);
    assert_eq!(
        actual["variants"][0]["report"],
        evidence["checks"][2]["report"]
    );
    let strict = run(
        root,
        &[
            "eval",
            "--compare",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(strict.status.code(), Some(69));
    assert!(strict.stdout.is_empty());
    let overridden = run(
        root,
        &[
            "eval",
            "--compare",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--allow-unverified",
            "--question-id-source",
            "lines",
            "--json",
        ],
    );
    assert_eq!(overridden.status.code(), Some(64));
    assert!(overridden.stdout.is_empty());
    std::fs::write(right.join("report.md"), b"Changed report\n").unwrap();
    assert_eq!(
        run(
            root,
            &[
                "eval",
                "--compare",
                left.to_str().unwrap(),
                right.to_str().unwrap(),
                "--allow-unverified",
                "--json"
            ]
        )
        .status
        .code(),
        Some(65)
    );
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_live_rubric_comparison_retains_separate_requests_identities_and_metrics() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    std::fs::write(
        root.join("xdg/loupe/presets/alternate.toml"),
        "version=2\ntype='noul'\ninstructions='Does this describe an unavailable checkout?'\n",
    )
    .unwrap();
    let actual = report(&execute(
        root,
        &[
            "--compare",
            "preset:rubric",
            "preset:alternate",
            "--init-splits",
            "--warmup",
            "0",
            "--allow-unverified",
            "--no-cache",
            "--strict",
        ],
    ));
    assert_eq!(actual["comparisons"][0]["paired_fields"], 4);
    assert_eq!(
        actual["token_usage"]["spent_input"].as_u64().unwrap(),
        actual["token_usage"]["reported_input"].as_u64().unwrap()
            + actual["token_usage"]["unknown_input"].as_u64().unwrap()
    );
    let usage = actual["variants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            variant["report"]["token_usage"]["spent_input"]
                .as_u64()
                .unwrap()
        })
        .sum::<u64>();
    assert_eq!(actual["token_usage"]["spent_input"], usage);
    assert_eq!(actual["token_usage"]["cache_hits"], 0);
    for variant in actual["variants"].as_array().unwrap() {
        let suite = &variant["report"]["suites"][0];
        assert_eq!(suite["questions"][0]["metrics"]["cases"], 4);
        for phase in suite["phases"].as_array().unwrap() {
            assert_eq!(phase["usage"]["cache_hits"], 0);
            assert_eq!(phase["timing"]["completed_units"], 4);
        }
        for record in suite["records"].as_array().unwrap() {
            assert_eq!(
                record["evaluation"]["response"]["usage"]["input_tokens"],
                record["counts"]["input_tokens"]
            );
        }
    }
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_calibration_packing_sweep_shares_budget_and_retains_completed_variants() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, true);
    let flags = [
        "--sweep-pack",
        "unit,window:4",
        "--init-splits",
        "--warmup",
        "0",
        "--allow-unverified",
        "--no-cache",
        "--strict",
    ];
    let actual = report(&execute(root, &flags));
    assert_eq!(actual["kind"], "sweep");
    assert_eq!(actual["comparisons"][0]["paired_fields"], 12);
    assert_eq!(
        actual["token_usage"]["spent_input"].as_u64().unwrap(),
        actual["token_usage"]["reported_input"].as_u64().unwrap()
            + actual["token_usage"]["unknown_input"].as_u64().unwrap()
    );
    for variant in actual["variants"].as_array().unwrap() {
        let suite = &variant["report"]["suites"][0];
        assert_eq!(suite["partition"], "calibrate");
        assert_eq!(suite["selected_case_ids"], suite["splits"]["calibrate"]);
        assert!(
            suite["phases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|phase| phase["usage"]["cache_hits"] == 0)
        );
    }
    let saved = loupe_core::report_store::Store::open(&root.join("saved"))
        .unwrap()
        .publish(
            &serde_json::to_vec(&actual["variants"][0]["report"]).unwrap(),
            b"Completed actual calibration variant\n",
        )
        .unwrap();
    let output = execute(
        root,
        &[
            "--compare",
            saved.to_str().unwrap(),
            "profile:server",
            "--partition",
            "calibrate",
            "--budget",
            "0",
            "--warmup",
            "0",
            "--allow-unverified",
            "--no-cache",
            "--strict",
        ],
    );
    assert_eq!(output.status.code(), Some(4));
    let partial: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(partial["status"], "incomplete");
    assert_eq!(partial["variants"].as_array().unwrap().len(), 2);
    assert_ne!(partial["variants"][0]["report"]["status"], "incomplete");
    assert_eq!(partial["variants"][1]["report"]["status"], "incomplete");
    assert_eq!(
        partial["variants"][1]["report"]["token_usage"]["attempts"],
        0
    );
    assert_eq!(partial["token_usage"]["spent_input"], 0);
    assert_eq!(
        partial["variants"][0]["report"],
        actual["variants"][0]["report"]
    );
}

#[test]
fn offline_eval_preserves_fixed_test_ids_source_locations_and_private_reports() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    let first = report(&execute(
        root,
        &[
            "--init-splits",
            "--dry-run",
            "--url",
            "http://127.0.0.1:1",
            "--report",
            "reports",
        ],
    ));
    assert_eq!(first["status"], "plan");
    assert_eq!(first["identity"]["verified"], false);
    assert_eq!(first["token_usage"]["attempts"], 0);
    assert!(first["suites"][0]["phases"][0]["timing"]["p95_ms"]["value"].is_null());
    let suite = &first["suites"][0];
    assert_eq!(suite["selected_case_ids"].as_array().unwrap().len(), 4);
    assert_eq!(suite["splits"]["captured"], json!(["review"]));
    assert!(
        suite["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["case_id"] != "review")
    );
    for record in suite["records"].as_array().unwrap() {
        let index = record["case_id"]
            .as_str()
            .unwrap()
            .strip_prefix("case-")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let physical = index + 1 + u64::from(index >= 1);
        assert_eq!(record["index"], physical);
        assert_eq!(record["provenance"]["source"]["line_start"], physical);
        assert!(record["evaluation"].is_null());
        assert!(!record["request"].to_string().contains("LABEL_SECRET"));
        assert!(record["counts"]["input_tokens"].as_u64().unwrap() > 0);
    }
    let second = report(&execute(root, &["--dry-run"]));
    assert_eq!(second["suites"][0]["splits"], suite["splits"]);
    assert_eq!(
        second["suites"][0]["selected_case_ids"],
        suite["selected_case_ids"]
    );
    let labeled = report(&run(
        root,
        &[
            "eval",
            "--labels",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "reviewed-v1",
            "--dry-run",
            "--json",
        ],
    ));
    assert_eq!(labeled["suites"][0]["splits"], suite["splits"]);
    let bundles = std::fs::read_dir(root.join("reports"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(bundles.len(), 1);
    let published: Value =
        serde_json::from_slice(&std::fs::read(bundles[0].join("report.json")).unwrap()).unwrap();
    assert_eq!(published, first);
    assert!(
        std::fs::read_to_string(bundles[0].join("report.md"))
            .unwrap()
            .contains("Verified identity: false")
    );
    assert!(bundles[0].join("manifest.json").is_file());
}

#[test]
fn invalid_eval_does_not_publish_splits_and_changed_data_never_replaces_them() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    let denied = execute(
        root,
        &["--init-splits", "--dry-run", "--min-confidence", "0.9"],
    );
    assert_eq!(denied.status.code(), Some(64));
    assert!(!root.join("cases.jsonl.splits.json").exists());
    success(&execute(root, &["--init-splits", "--dry-run"]));
    let path = root.join("cases.jsonl.splits.json");
    let split_bytes = std::fs::read(&path).unwrap();
    let mut bytes = std::fs::read(root.join("cases.jsonl")).unwrap();
    bytes.extend_from_slice(b"\n");
    std::fs::write(root.join("cases.jsonl"), bytes).unwrap();
    let changed = execute(root, &["--init-splits", "--dry-run"]);
    assert_eq!(changed.status.code(), Some(65));
    assert_eq!(std::fs::read(path).unwrap(), split_bytes);
    assert_eq!(run(root, &["eval", "--dry-run"]).status.code(), Some(64));
}

#[test]
fn mixed_window_plans_map_every_label_and_reject_structured_split() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, true);
    let plan = report(&execute(
        root,
        &[
            "--init-splits",
            "--dry-run",
            "--pack",
            "window:4",
            "--margin",
            "0.1",
            "--min-gap",
            "0.1",
        ],
    ));
    let records = plan["suites"][0]["records"].as_array().unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records[0]["request"]["questions"]
            .as_object()
            .unwrap()
            .len(),
        12
    );
    for record in records {
        assert_eq!(record["binding"].as_object().unwrap().len(), 3);
        for binding in record["binding"].as_object().unwrap().values() {
            assert!(
                record["request"]["questions"]
                    .get(binding.as_str().unwrap())
                    .is_some()
            );
        }
    }
    let new_root = tempfile::tempdir().unwrap();
    fixture(new_root.path(), false);
    let text = std::fs::read_to_string(new_root.path().join("cases.jsonl")).unwrap();
    let text = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut case: Value = serde_json::from_str(line).unwrap();
            case["unit"] = json!({"text":case["unit"]});
            serde_json::to_string(&case).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(new_root.path().join("cases.jsonl"), text).unwrap();
    let denied = execute(
        new_root.path(),
        &["--init-splits", "--dry-run", "--split", "--combine", "any"],
    );
    assert_eq!(denied.status.code(), Some(64));
    assert!(!new_root.path().join("cases.jsonl.splits.json").exists());
}

#[test]
fn query_context_and_capture_partitions_remain_separate_from_model_labels() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    std::fs::write(root.join("xdg/loupe/presets/rubric.toml"), "version=1\ntype='noul'\ninstructions='Does this discuss {{arg.query}}?'\n[context]\npolicy='Use the incident details only'\n").unwrap();
    let input = std::fs::read_to_string(root.join("cases.jsonl")).unwrap();
    let mut rows = Vec::new();
    for line in input.lines().filter(|line| !line.is_empty()) {
        let mut case: Value = serde_json::from_str(line).unwrap();
        case["query"] = case["id"].clone();
        case["context"] = json!({"reference":"CASE_SPECIFIC_CONTEXT"});
        rows.push(serde_json::to_string(&case).unwrap());
    }
    std::fs::write(root.join("cases.jsonl"), rows.join("\n")).unwrap();
    let planned = report(&execute(
        root,
        &["--init-splits", "--dry-run", "--pack", "window:4"],
    ));
    let cases = planned["suites"][0]["records"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let request = case["request"].to_string();
        assert!(request.contains(case["case_id"].as_str().unwrap()));
        assert!(request.contains("CASE_SPECIFIC_CONTEXT"));
        assert!(request.contains("Use the incident details only"));
        assert!(!request.contains("LABEL_SECRET"));
        assert_eq!(case["request"]["questions"].as_object().unwrap().len(), 1);
    }
    let conflict = execute(root, &["--dry-run", "--arg", "query=override"]);
    assert_eq!(conflict.status.code(), Some(64));
    let captured = report(&execute(root, &["--dry-run", "--partition", "captured"]));
    assert_eq!(
        captured["suites"][0]["selected_case_ids"],
        json!(["review"])
    );
    assert_eq!(
        captured["suites"][0]["records"][0]["sampling"],
        "uniform-audit"
    );
    assert!(
        !planned["suites"][0]["selected_case_ids"]
            .as_array()
            .unwrap()
            .contains(&json!("review"))
    );
}

#[test]
fn project_presets_and_symlink_report_roots_are_rejected_without_output_writes() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    let project = root.join(".loupe/presets");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::rename(
        root.join("xdg/loupe/presets/rubric.toml"),
        project.join("rubric.toml"),
    )
    .unwrap();
    let untrusted = execute(root, &["--init-splits", "--dry-run"]);
    assert_eq!(untrusted.status.code(), Some(64));
    assert!(!root.join("cases.jsonl.splits.json").exists());
    success(&run(root, &["presets", "trust", "rubric"]));
    let outside = root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, root.join("reports")).unwrap();
    let refused = execute(root, &["--init-splits", "--dry-run", "--report", "reports"]);
    assert_eq!(refused.status.code(), Some(74));
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_eval_reports_actual_mixed_answers_and_separate_uncached_measurements() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, true);
    let strict = execute(root, &["--init-splits", "--url", &url]);
    assert_eq!(
        strict.status.code(),
        Some(69),
        "{}",
        String::from_utf8_lossy(&strict.stderr)
    );
    assert!(String::from_utf8_lossy(&strict.stderr).contains("fresh full model identity"));
    for pack in ["unit", "window:4"] {
        let actual = report(&execute(
            root,
            &[
                "--allow-unverified",
                "--strict",
                "--url",
                &url,
                "--pack",
                pack,
                "--report",
                "reports",
            ],
        ));
        assert_eq!(actual["status"], "provisional");
        assert_eq!(actual["identity"]["verified"], false);
        let suite = &actual["suites"][0];
        assert_eq!(suite["records"].as_array().unwrap().len(), 4);
        assert_eq!(suite["questions"].as_array().unwrap().len(), 3);
        let phases = suite["phases"].as_array().unwrap();
        assert_eq!(phases.len(), 3);
        assert_eq!(
            phases
                .iter()
                .map(|phase| phase["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["accuracy", "warmup", "latency"]
        );
        let logical = if pack == "unit" { 4 } else { 1 };
        for phase in phases {
            assert_eq!(phase["logical_requests"], logical);
            assert_eq!(phase["usage"]["cache_hits"], 0);
            assert_eq!(
                phase["usage"]["spent_input"],
                phase["usage"]["reported_input"]
            );
            assert_eq!(phase["timing"]["completed_units"], 4);
            assert!(phase["timing"]["p95_ms"]["value"].as_f64().unwrap() > 0.0);
        }
        assert_eq!(
            actual["token_usage"]["spent_input"],
            phases
                .iter()
                .map(|phase| phase["usage"]["spent_input"].as_u64().unwrap())
                .sum::<u64>()
        );
        for record in suite["records"].as_array().unwrap() {
            assert_eq!(record["evaluation"]["cached"], false);
            assert_eq!(
                record["evaluation"]["response"]["usage"]["input_tokens"],
                record["counts"]["input_tokens"]
            );
            assert!(
                record["evaluation"]["response"]["answers"]
                    .as_object()
                    .unwrap()
                    .len()
                    >= 3
            );
        }
        if pack == "unit" {
            let parity = root.join("parity");
            std::fs::create_dir(&parity).unwrap();
            let mut rows = Vec::new();
            for record in suite["records"].as_array().unwrap() {
                rows.push(serde_json::to_string(&json!({
                    "id":record["case_id"],"unit":record["request"]["state"],"label":record["label"],
                    "reference":{"request":record["request"],"response":record["evaluation"]["response"],"encoder_identity":actual["identity"]["local_encoder"]}
                })).unwrap());
            }
            let bytes = rows.join("\n");
            std::fs::write(parity.join("cases.jsonl"), &bytes).unwrap();
            let digest = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let manifest = format!(
                "schema_version=1\nid='saved-real-response-parity'\nrevision='capture-v1'\npreset='exact-reference'\ntask='parity'\nsplits='splits.json'\nmetrics=['probability-mean-error','argmax-agreement']\n[cases]\npath='cases.jsonl'\nrevision='real-server-capture-v1'\nblake3='{digest}'\n[split]\nseed=42\ncalibrate_percent=50\n[provenance]\nsource='real requests captured in this test; no Python/BF16 equivalence claimed'\nlicense='synthetic test data, MIT'\n"
            );
            std::fs::write(parity.join("suite.toml"), manifest).unwrap();
            let repeated = report(&run(
                root,
                &[
                    "eval",
                    "--suite",
                    "parity/suite.toml",
                    "--init-splits",
                    "--allow-unverified",
                    "--strict",
                    "--url",
                    &url,
                    "--warmup",
                    "0",
                    "--json",
                ],
            ));
            assert_eq!(repeated["status"], "provisional");
            assert_eq!(
                repeated["suites"][0]["records"].as_array().unwrap().len(),
                2
            );
            assert!(
                repeated["suites"][0]["parity"]["mean_absolute_error"]["value"]
                    .as_f64()
                    .unwrap()
                    .is_finite()
            );
            assert!(
                repeated["suites"][0]["warnings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|warning| warning.as_str().unwrap().contains("saved reference"))
            );
            for record in repeated["suites"][0]["records"].as_array().unwrap() {
                assert_eq!(record["request"], record["reference"]["request"]);
            }
        }
    }
    let exhausted = execute(
        root,
        &[
            "--allow-unverified",
            "--strict",
            "--url",
            &url,
            "--budget",
            "0",
        ],
    );
    assert_eq!(exhausted.status.code(), Some(4));
    let partial: Value = serde_json::from_slice(&exhausted.stdout).unwrap();
    assert_eq!(partial["status"], "incomplete");
    assert_eq!(partial["token_usage"]["http_attempts"], 0);
    assert_eq!(partial["suites"][0]["records"], json!([]));
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_eval_long_views_reuse_the_shared_planner_and_charge_every_uncached_phase() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, false);
    std::fs::write(root.join("config.toml"),"[profile.server]\nbase_url='http://127.0.0.1:8015'\npath='/v1/systemone'\nmodel='clef-flash'\nencoder='clef-v1'\ntokenizer='hf:Cloudflare/clef-flash'\ntokenizer_revision='17f0b0ad64efb65d273590632833508766b2aae6'\nconcurrency=2\nmax_length=1400\n").unwrap();
    let text = "Observed checkout failure and customers cannot pay. ".repeat(180);
    let bytes = (0..4)
        .map(|index| {
            serde_json::to_string(&json!({"id":format!("long-{index}"),"unit":text,"label":true}))
                .unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(root.join("cases.jsonl"), bytes).unwrap();
    success(&execute(
        root,
        &["--init-splits", "--dry-run", "--truncate", "head"],
    ));
    let no_view = execute(
        root,
        &[
            "--allow-unverified",
            "--strict",
            "--url",
            &url,
            "--warmup",
            "0",
        ],
    );
    assert_eq!(no_view.status.code(), Some(65));
    let no_view: Value = serde_json::from_slice(&no_view.stdout).unwrap();
    assert_eq!(no_view["token_usage"]["attempts"], 0);
    for (options, provenance) in [
        (
            vec![
                "--split",
                "--combine",
                "any",
                "--overlap",
                "64",
                "--max-chunks",
                "8",
            ],
            "split",
        ),
        (
            vec!["--focus", "1", "--overlap", "64", "--max-chunks", "8"],
            "focus",
        ),
        (vec!["--truncate", "head"], "truncate"),
    ] {
        let mut flags = vec![
            "--allow-unverified",
            "--strict",
            "--url",
            &url,
            "--warmup",
            "0",
        ];
        flags.extend(options);
        let actual = report(&execute(root, &flags));
        assert_eq!(actual["status"], "provisional");
        let suite = &actual["suites"][0];
        assert_eq!(suite["records"].as_array().unwrap().len(), 2);
        for record in suite["records"].as_array().unwrap() {
            assert!(!record["provenance"][provenance].is_null());
            assert_eq!(record["evaluation"]["cached"], false);
            let counted = match provenance {
                "split" => {
                    assert_eq!(record["request_role"], "original-split-input");
                    record["provenance"]["split"]["chunks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|chunk| chunk["counts"]["input_tokens"].as_u64().unwrap())
                        .sum::<u64>()
                }
                "focus" => {
                    assert_eq!(record["request_role"], "focus-final");
                    let plan = &record["provenance"]["focus"];
                    assert_eq!(record["request"], plan["final"]["request"]);
                    assert_eq!(record["counts"], plan["final"]["counts"]);
                    plan["relevance"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|phase| {
                            assert_eq!(
                                phase["counts"]["input_tokens"],
                                phase["response"]["usage"]["input_tokens"]
                            );
                            phase["counts"]["input_tokens"].as_u64().unwrap()
                        })
                        .sum::<u64>()
                        + plan["final"]["counts"]["input_tokens"].as_u64().unwrap()
                }
                _ => {
                    assert_eq!(record["request_role"], "evaluated");
                    record["counts"]["input_tokens"].as_u64().unwrap()
                }
            };
            assert_eq!(
                record["evaluation"]["response"]["usage"]["input_tokens"]
                    .as_u64()
                    .unwrap(),
                counted
            );
        }
        let phases = suite["phases"].as_array().unwrap();
        assert_eq!(phases.len(), 2);
        for phase in phases {
            assert_eq!(phase["usage"]["cache_hits"], 0);
            assert_eq!(
                phase["usage"]["spent_input"],
                phase["usage"]["reported_input"]
            );
            assert_eq!(phase["timing"]["completed_units"], 2);
        }
        assert_eq!(
            actual["token_usage"]["spent_input"],
            phases
                .iter()
                .map(|phase| phase["usage"]["spent_input"].as_u64().unwrap())
                .sum::<u64>()
        );
    }
}
