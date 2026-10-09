#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use loupe_core::encoder::ClefEncoder;
use loupe_core::wire::Request;
use serde_json::{Value, json};

fn setup(root: &Path) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(&config, include_str!("../../../examples/config.toml")).unwrap();
    let presets = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&presets).unwrap();
    for (name, document) in [
        (
            "outage",
            json!({"type":"noul","instructions":"Is checkout currently unavailable?","variants":[{"id":"direct","instructions":"Are customers currently unable to check out?"},{"id":"countercheck","instructions":"Does this describe an unresolved checkout outage?"}]}),
        ),
        (
            "severity-test",
            json!({"type":"choice","instructions":"Classify current checkout status.","criteria":{"page":"An unresolved outage","noise":"Healthy or recovered"},"variants":[{"id":"rephrase","instructions":"Select the current incident category.","criteria":{"page":"Checkout currently fails","noise":"Checkout works again"}}]}),
        ),
        (
            "impact-test",
            json!({"type":"score","instructions":"Rate the current impact.","criteria":["None","Partial","Complete outage"],"variants":[{"id":"rephrase","instructions":"How severe is current customer impact?"}]}),
        ),
    ] {
        std::fs::write(presets.join(format!("{name}.json")), document.to_string()).unwrap();
    }
    config
}

fn run(root: &Path, config: &Path, args: &[&str], input: &[u8], live: bool) -> Output {
    let url = if live {
        std::env::var("LOUPE_TEST_URL").unwrap()
    } else {
        String::new()
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            if live { &url } else { "http://127.0.0.1:1" },
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
fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn encoder() -> ClefEncoder {
    let tokenizer = std::env::var_os("LOUPE_TEST_TOKENIZER")
        .or_else(|| std::env::var_os("LOUPE_TOKENIZER"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache"))
                .join("loupe/tokenizers/Cloudflare--clef-flash")
                .join(loupe_core::config::CLEF_REVISION)
                .join("tokenizer.json")
        });
    ClefEncoder::load(&tokenizer, loupe_core::config::CLEF_REVISION).unwrap()
}

#[test]
fn joint_plans_use_stable_ids_exact_counts_and_configurable_agreement() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path());
    let baseline = run(
        dir.path(),
        &config,
        &["tokens", "-p", "outage"],
        b"Checkout is down.\n",
        false,
    );
    code(&baseline, 0);
    let planned = run(
        dir.path(),
        &config,
        &["tokens", "-p", "outage", "--ensemble"],
        b"Checkout is down.\n",
        false,
    );
    code(&planned, 0);
    let plan = rows(&planned).remove(0);
    let request: Request = serde_json::from_value(plan["request"].clone()).unwrap();
    assert_eq!(
        request
            .questions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "outage",
            "ensemble:outage/direct",
            "ensemble:outage/countercheck"
        ]
    );
    let (_, counts) = encoder().encode(&request).unwrap();
    assert_eq!(serde_json::to_value(counts).unwrap(), plan["counts"]);
    assert!(
        plan["counts"]["input_tokens"].as_u64().unwrap()
            > rows(&baseline)[0]["counts"]["input_tokens"]
                .as_u64()
                .unwrap()
    );
    assert_eq!(plan["ensemble"]["min_agreement"], 1.0);
    assert_eq!(plan["ensemble"]["independent"], false);
    let changed = run(
        dir.path(),
        &config,
        &[
            "tokens",
            "-p",
            "outage",
            "--ensemble",
            "--min-agreement",
            "0.66",
        ],
        b"Checkout is down.\n",
        false,
    );
    code(&changed, 0);
    let changed = rows(&changed).remove(0);
    assert_eq!(changed["request"], plan["request"]);
    assert_eq!(changed["counts"], plan["counts"]);
    assert_eq!(changed["ensemble"]["min_agreement"], 0.66);
}

#[test]
fn windows_bind_every_variant_and_route_uses_only_primary_labels() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path());
    let result = run(
        dir.path(),
        &config,
        &[
            "tag",
            "-p",
            "severity-test",
            "--ensemble",
            "--pack",
            "window:2",
            "--dry-run",
        ],
        b"Checkout is down.\nCheckout recovered.\n",
        false,
    );
    code(&result, 0);
    let plan = rows(&result).remove(0);
    let request: Request = serde_json::from_value(plan["request"].clone()).unwrap();
    assert_eq!(
        request
            .questions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "L1/severity-test",
            "L1/ensemble:severity-test/rephrase",
            "L2/severity-test",
            "L2/ensemble:severity-test/rephrase"
        ]
    );
    assert_eq!(
        serde_json::to_value(encoder().encode(&request).unwrap().1).unwrap(),
        plan["counts"]
    );
    let output = dir.path().join("routed");
    let result = run(
        dir.path(),
        &config,
        &[
            "route",
            "-p",
            "severity-test",
            "--ensemble",
            "--out",
            output.to_str().unwrap(),
            "--dry-run",
        ],
        b"Checkout is down.\n",
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)[0]["request"]["questions"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        rows(&result)[0]["route"]["labels"],
        json!({"names":{"noise":"noise","page":"page","none":"none","uncertain":"uncertain"}})
    );
    assert!(!output.exists());
}

#[test]
fn schemas_and_flags_are_rejected_before_inference_or_actions() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path());
    for args in [
        vec!["keep", "Question?", "--ensemble", "--dry-run"],
        vec![
            "keep",
            "-p",
            "outage",
            "--min-agreement",
            "0.5",
            "--dry-run",
        ],
        vec![
            "keep",
            "-p",
            "outage",
            "--ensemble",
            "--min-agreement",
            "1.1",
            "--dry-run",
        ],
        vec![
            "keep",
            "-p",
            "outage",
            "--ensemble",
            "--min-agreement",
            "-0.1",
            "--dry-run",
        ],
        vec!["pick", "-p", "severity-test", "--ensemble", "--dry-run"],
    ] {
        code(
            &run(dir.path(), &config, &args, b"Checkout is down.\n", false),
            64,
        );
    }
    let restricted = dir.path().join("restricted.toml");
    std::fs::write(
        &restricted,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_questions = 128", "max_questions = 2"),
    )
    .unwrap();
    code(
        &run(
            dir.path(),
            &restricted,
            &["keep", "-p", "outage", "--ensemble", "--dry-run"],
            b"Checkout is down.\n",
            false,
        ),
        65,
    );
    let broken = dir.path().join("broken.json");
    std::fs::write(&broken, json!({"type":"choice","criteria":{"a":"A","b":"B"},"variants":[{"id":"rephrase","criteria":{"a":"A","c":"C"}}]}).to_string()).unwrap();
    code(
        &run(
            dir.path(),
            &config,
            &["tag", "-f", broken.to_str().unwrap(), "--dry-run"],
            b"Anything.\n",
            false,
        ),
        64,
    );
    let marker = dir.path().join("action");
    let command = format!("printf bad > '{}'", marker.display());
    let result = run(
        dir.path(),
        &config,
        &[
            "on",
            "-p",
            "outage",
            "--ensemble",
            "--when",
            "yes",
            "--exec",
            &command,
            "--dry-run",
        ],
        b"Checkout is down.\n",
        false,
    );
    code(&result, 0);
    assert_eq!(
        rows(&result)[0]["request"]["questions"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert!(!marker.exists());
}

#[test]
fn multi_question_split_reducers_are_inherited_before_joint_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path());
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_length = 16384", "max_length = 2000"),
    )
    .unwrap();
    let source = dir.path().join("multi.json");
    std::fs::write(&source, json!({"questions":{"present":{"type":"noul","instructions":"Is checkout down?"},"impact":{"type":"score","instructions":"Rate impact.","criteria":["None","Full"]}},"variants":[{"field":"present","id":"direct","instructions":"Is checkout unavailable?"},{"field":"impact","id":"direct","instructions":"How severe is impact?"}],"policy":{"splittable":true,"combine":{"present":"any","impact":"max"},"min_agreement":0.75}}).to_string()).unwrap();
    let input = "Checkout is down and customers cannot pay. ".repeat(900);
    code(
        &run(
            dir.path(),
            &config,
            &[
                "ask",
                "-f",
                source.to_str().unwrap(),
                "--ensemble",
                "--min-agreement",
                "0.5",
                "--dry-run",
            ],
            b"Checkout is down.",
            false,
        ),
        64,
    );
    for command in ["ask", "tokens"] {
        let result = run(
            dir.path(),
            &config,
            &[
                command,
                "-f",
                source.to_str().unwrap(),
                "--ensemble",
                "--dry-run",
            ],
            input.as_bytes(),
            false,
        );
        code(&result, 0);
        let plan = rows(&result).remove(0);
        assert_eq!(plan["ensemble"]["min_agreement"], 0.75);
        assert_eq!(
            plan["split"]["combine"],
            json!({"present":"any","impact":"max","ensemble:present/direct":"any","ensemble:impact/direct":"max"})
        );
        let chunks = plan["split"]["chunks"].as_array().unwrap();
        assert!(chunks.len() > 1);
        let encoder = encoder();
        for chunk in chunks {
            let request: Request = serde_json::from_value(chunk["request"].clone()).unwrap();
            assert_eq!(request.questions.len(), 4);
            assert_eq!(
                serde_json::to_value(encoder.encode(&request).unwrap().1).unwrap(),
                chunk["counts"]
            );
            assert!(chunk["counts"]["input_tokens"].as_u64().unwrap() <= 2000);
        }
    }
}

#[test]
#[ignore = "requires a reachable actual server model; no fake backend"]
fn actual_server_joint_variants_report_mean_votes_raw_answers_and_exact_usage() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path());
    for (command, preset) in [
        ("keep", "outage"),
        ("tag", "severity-test"),
        ("rate", "impact-test"),
    ] {
        let result = run(
            dir.path(),
            &config,
            &[
                command,
                "-p",
                preset,
                "--ensemble",
                "--json",
                "--strict",
                "--no-cache",
                "--on-uncertain",
                "keep",
            ],
            b"Checkout is unavailable and customers cannot pay.\n",
            true,
        );
        code(&result, 0);
        let record = rows(&result).remove(0);
        let report = &record["ensemble"];
        assert_eq!(report["min_agreement"], 1.0);
        assert_eq!(report["independent"], false);
        assert_eq!(record["decision"]["source"], "ensemble");
        let members = report["members"].as_array().unwrap();
        assert_eq!(members[0]["answer"], record["answer"]);
        assert_eq!(members.len(), if command == "keep" { 3 } else { 2 });
        for (option, mean) in report["mean_probabilities"].as_object().unwrap() {
            let sum: f64 = members
                .iter()
                .map(|member| member["probabilities"][option].as_f64().unwrap())
                .sum();
            assert!((mean.as_f64().unwrap() - sum / members.len() as f64).abs() < 1e-12);
        }
        let agreed = members
            .iter()
            .filter(|member| {
                !report["winning_option"].is_null() && member["vote"] == report["winning_option"]
            })
            .count();
        assert_eq!(
            report["agreement"].as_f64().unwrap(),
            agreed as f64 / members.len() as f64
        );
        if agreed < members.len() {
            assert_eq!(report["assessment"]["uncertain"], true);
        }
    }
}
