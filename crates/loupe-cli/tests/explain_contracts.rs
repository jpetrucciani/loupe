#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn setup(root: &Path) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(&config, include_str!("../../../examples/config.toml")).unwrap();
    let presets = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("incident-factors.toml"),
        include_str!("../../../examples/incident-factors.toml"),
    )
    .unwrap();
    std::fs::write(
        presets.join("ensemble-factors.toml"),
        include_str!("../../../examples/ensemble-factors.toml"),
    )
    .unwrap();
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
        .env_remove("LOUPE_PATH")
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
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}

#[test]
fn factor_plans_bind_order_context_templates_and_windows_with_no_inference() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let output = run(
        root,
        &config,
        &["explain", "-p", "incident-factors", "--dry-run"],
        b"outage\n",
    );
    code(&output, 0);
    let rows = records(&output);
    let fields = rows[0]["request"]["questions"].as_object().unwrap();
    assert_eq!(
        fields.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![
            "incident-factors",
            "factor:customer_impact",
            "factor:scope",
            "factor:recovered"
        ]
    );
    assert_eq!(rows[0]["request"]["state"], "outage\n");
    assert_eq!(rows[0]["counts"]["exact"], true);
    let window = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "incident-factors",
            "-I",
            "lines",
            "--pack",
            "window:2",
            "--dry-run",
        ],
        b"outage\nrecovered\n",
    );
    code(&window, 0);
    let fields = records(&window)[0]["request"]["questions"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(fields.len(), 8);
    assert!(fields.contains_key("L2/factor:scope"));
    let context = root.join("context.txt");
    std::fs::write(&context, "Customer impact matters.").unwrap();
    let contextual = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "incident-factors",
            "--context",
            context.to_str().unwrap(),
            "--dry-run",
        ],
        b"outage",
    );
    code(&contextual, 0);
    assert!(
        records(&contextual)[0]["request"]["questions"]["factor:scope"]["instructions"].is_object()
    );
}

#[test]
fn factor_variants_bind_primary_and_factor_ids_context_and_full_schema_limits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let context = root.join("context.txt");
    std::fs::write(&context, "Only current customer impact matters.").unwrap();
    let output = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "ensemble-factors",
            "--ensemble",
            "--context",
            context.to_str().unwrap(),
            "--dry-run",
        ],
        b"outage",
    );
    code(&output, 0);
    let row = records(&output).remove(0);
    assert_eq!(row["counts"]["exact"], true);
    let fields = row["request"]["questions"].as_object().unwrap();
    assert_eq!(fields.len(), 7);
    assert!(fields.contains_key("ensemble:ensemble-factors/direct"));
    assert!(fields.contains_key("ensemble:factor:scope/direct"));
    for question in fields.values() {
        assert_eq!(
            question["instructions"]["context"],
            "Only current customer impact matters."
        );
    }
    let output = run(root, &config, &["presets", "lint", "ensemble-factors"], b"");
    code(&output, 0);
    let restricted = root.join("restricted.toml");
    std::fs::write(
        &restricted,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_questions = 128", "max_questions = 6"),
    )
    .unwrap();
    code(
        &run(
            root,
            &restricted,
            &[
                "explain",
                "-p",
                "ensemble-factors",
                "--ensemble",
                "--dry-run",
            ],
            b"outage",
        ),
        65,
    );
    let output = run(
        root,
        &config,
        &[
            "tokens",
            "-p",
            "ensemble-factors",
            "--ensemble",
            "--dry-run",
        ],
        b"outage",
    );
    code(&output, 0);
    assert_eq!(
        records(&output)[0]["request"]["questions"]
            .as_object()
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn invalid_composite_definitions_policy_flags_and_limits_fail_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    for args in [
        vec!["explain", "-p", "severity"],
        vec!["explain", "-p", "incident-factors", "--threshold", "0.7"],
        vec![
            "explain",
            "-p",
            "incident-factors",
            "--on-uncertain",
            "drop",
        ],
        vec!["explain", "-p", "incident-factors", "--keep-going"],
        vec!["is", "-p", "incident-factors"],
        vec!["featurize", "-p", "incident-factors"],
    ] {
        code(&run(root, &config, &args, b"outage"), 64);
    }
    let restricted = root.join("restricted.toml");
    std::fs::write(
        &restricted,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_questions = 128", "max_questions = 3"),
    )
    .unwrap();
    code(
        &run(
            root,
            &restricted,
            &["explain", "-p", "incident-factors", "--dry-run"],
            b"outage",
        ),
        65,
    );
    let path = root.join("bad.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"type":"noul","factors":[{"id":"risk","weight":0}]})).unwrap(),
    )
    .unwrap();
    code(
        &run(
            root,
            &config,
            &["explain", "-f", path.to_str().unwrap(), "--dry-run"],
            b"outage",
        ),
        64,
    );
}

fn verify(row: &Value) {
    let mut expected = 0.0;
    for factor in row["composite"]["factors"].as_object().unwrap().values() {
        let raw = &factor["answer"];
        assert_eq!(
            raw,
            &row["response"]["answers"][factor["field"].as_str().unwrap()]
        );
        let mut value = if let Some(ensemble) = factor.get("ensemble") {
            assert_eq!(ensemble["members"][0]["answer"], *raw);
            let members = ensemble["members"].as_array().unwrap();
            for (level, p) in ensemble["mean_probabilities"].as_object().unwrap() {
                let expected = members
                    .iter()
                    .map(|member| member["probabilities"][level].as_f64().unwrap())
                    .sum::<f64>()
                    / members.len() as f64;
                assert!((p.as_f64().unwrap() - expected).abs() < 1e-12);
            }
            assert_eq!(factor["assessment"], ensemble["assessment"]);
            assert!(factor["raw_assessment"].is_object());
            if raw["type"] == "noul" {
                ensemble["mean_noul"].as_f64().unwrap()
            } else {
                ensemble["mean_score"].as_f64().unwrap()
                    / (ensemble["mean_probabilities"].as_object().unwrap().len() - 1) as f64
            }
        } else if raw["type"] == "noul" {
            raw["noul"].as_f64().unwrap()
        } else {
            let probabilities = raw["probabilities"].as_object().unwrap();
            let mass: f64 = probabilities.values().map(|p| p.as_f64().unwrap()).sum();
            let weighted: f64 = probabilities
                .iter()
                .map(|(level, p)| level.parse::<f64>().unwrap() * p.as_f64().unwrap())
                .sum();
            weighted / mass / (probabilities.len() - 1) as f64
        };
        if factor["invert"] == true {
            value = 1.0 - value;
        }
        assert!((value - factor["value"].as_f64().unwrap()).abs() < 1e-12);
        let contribution = value * factor["normalized_weight"].as_f64().unwrap();
        assert!((contribution - factor["contribution"].as_f64().unwrap()).abs() < 1e-12);
        expected += contribution;
    }
    assert!((expected - row["composite"]["score"].as_f64().unwrap()).abs() < 1e-12);
}

#[test]
#[ignore = "requires actual server model and real journal files; no fake backend"]
fn server_factor_ensembles_use_means_preserve_window_provenance_and_pin_replay() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let input =
        b"Checkout is down for all customers.\nThe retry succeeded and service fully recovered.\n";
    for window in [false, true] {
        let journal = root.join(if window { "window.jsonl" } else { "unit.jsonl" });
        let mut base = vec![
            "explain",
            "-p",
            "ensemble-factors",
            "--ensemble",
            "-I",
            "lines",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ];
        if window {
            base.extend(["--pack", "window:2"]);
        }
        let mut args = base.clone();
        args.extend(["--failed", journal.to_str().unwrap()]);
        let output = run(root, &config, &args, input);
        code(&output, 0);
        let rows = records(&output);
        assert_eq!(rows.len(), 2);
        for (offset, row) in rows.iter().enumerate() {
            verify(row);
            assert_eq!(row["answer"], row["ensemble"]["members"][0]["answer"]);
            assert_eq!(
                row["counts"]["input_tokens"],
                row["response"]["usage"]["input_tokens"]
            );
            let scope = &row["composite"]["factors"]["scope"];
            assert_eq!(scope["ensemble"]["members"].as_array().unwrap().len(), 2);
            assert!(
                row["composite"]["factors"]["recovered"]
                    .get("ensemble")
                    .is_none()
            );
            if window {
                assert_eq!(scope["field"], format!("L{}/factor:scope", offset + 1));
                assert_eq!(
                    scope["ensemble"]["members"][1]["field"],
                    format!("L{}/ensemble:factor:scope/direct", offset + 1)
                );
            }
        }
        let mut args = base;
        args.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
        let replay = run(root, &config, &args, input);
        code(&replay, 0);
        assert!(replay.stdout.is_empty());
        assert!(String::from_utf8_lossy(&replay.stderr).contains("http_attempts=0"));
        args.extend(["--min-agreement", "0.5"]);
        code(&run(root, &config, &args, input), 65);
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_factor_values_reproduce_raw_distributions_and_preserve_window_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let output = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "incident-factors",
            "-I",
            "lines",
            "--pack",
            "window:2",
            "--json",
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"Checkout is down for all customers.\nThe retry succeeded and service fully recovered.\n",
    );
    code(&output, 0);
    let rows = records(&output);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        verify(row);
        assert_eq!(
            row["counts"]["input_tokens"],
            row["response"]["usage"]["input_tokens"]
        );
    }
    assert_eq!(
        rows[0]["composite"]["factors"]["scope"]["field"],
        "L1/factor:scope"
    );
    assert_eq!(
        rows[1]["composite"]["factors"]["scope"]["field"],
        "L2/factor:scope"
    );
}

#[test]
#[ignore = "requires actual server model and real journal files; no fake backend"]
fn server_composite_resume_keeps_original_responses_and_refuses_weight_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = setup(root);
    let source = root.join("input.txt");
    std::fs::write(&source, "Checkout is down for all customers.").unwrap();
    let journal = root.join("run.jsonl");
    let output = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "incident-factors",
            source.to_str().unwrap(),
            "--json",
            "--failed",
            journal.to_str().unwrap(),
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
        ],
        b"",
    );
    code(&output, 0);
    verify(&records(&output)[0]);
    let replay = run(
        root,
        &config,
        &[
            "explain",
            "-p",
            "incident-factors",
            source.to_str().unwrap(),
            "--json",
            "--resume",
            journal.to_str().unwrap(),
            "--strict",
            "--no-cache",
            "--budget",
            "0",
            "--deadline",
            "60s",
        ],
        b"",
    );
    code(&replay, 0);
    assert!(replay.stdout.is_empty());
    let preset = root.join("xdg/loupe/presets/incident-factors.toml");
    std::fs::write(
        &preset,
        std::fs::read_to_string(&preset)
            .unwrap()
            .replace("weight = 3", "weight = 4"),
    )
    .unwrap();
    code(
        &run(
            root,
            &config,
            &[
                "explain",
                "-p",
                "incident-factors",
                source.to_str().unwrap(),
                "--json",
                "--resume",
                journal.to_str().unwrap(),
                "--budget",
                "0",
            ],
            b"",
        ),
        65,
    );
}
