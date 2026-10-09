#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn config(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("config.toml");
    let source = include_str!("../../../examples/config.toml")
        .replace("max_length = 16384", "max_length = 1400");
    std::fs::write(&path, source).unwrap();
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
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn long_text() -> String {
    "A routine heartbeat completed. ".repeat(500)
        + "\n\nCheckout is down and customers cannot pay.\n"
}

#[test]
fn split_flags_require_valid_explicit_semantics_and_never_split_structured_records() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    for args in [
        vec!["keep", "question", "--dry-run", "--split"],
        vec!["keep", "question", "--dry-run", "--combine", "any"],
        vec!["keep", "question", "--dry-run", "--overlap", "0"],
        vec![
            "tag",
            "question",
            "-c",
            "a,b",
            "--dry-run",
            "--split",
            "--combine",
            "any",
        ],
        vec![
            "keep",
            "question",
            "--dry-run",
            "--split",
            "--combine",
            "any",
            "--max-chunks",
            "0",
        ],
        vec![
            "keep",
            "question",
            "--dry-run",
            "--split",
            "--combine",
            "any",
            "-I",
            "jsonl",
        ],
        vec![
            "keep",
            "question",
            "--dry-run",
            "--split",
            "--combine",
            "any",
            "-I",
            "csv",
        ],
        vec![
            "keep",
            "question",
            "--dry-run",
            "--split",
            "--combine",
            "any",
            "-I",
            "tsv",
        ],
    ] {
        let output = run(&config, &args, b"");
        assert_eq!(
            output.status.code(),
            Some(64),
            "args={args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn whole_split_plans_bound_chunks_preserve_original_utf8_and_report_overlap_weights() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let input = long_text() + &"東京🙂 café\n".repeat(100);
    let rejected = run(&config, &["tokens", "Is checkout down?"], input.as_bytes());
    assert_eq!(rejected.status.code(), Some(65));
    assert!(rejected.stdout.is_empty());
    let planned = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "tokens",
            "Is checkout down?",
            "--split",
            "--combine",
            "any",
        ],
        input.as_bytes(),
    );
    success(&planned);
    assert!(planned.stderr.is_empty());
    let plan: Value = serde_json::from_slice(&planned.stdout).unwrap();
    assert_eq!(plan["request"]["state"], input);
    let chunks = plan["split"]["chunks"].as_array().unwrap();
    assert!(chunks.len() > 1 && chunks.len() <= 64);
    for chunk in chunks {
        let range = &chunk["range"];
        let start = range["byte_start"].as_u64().unwrap() as usize;
        let end = range["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(chunk["request"]["state"], &input[start..end]);
        assert!(chunk["counts"]["input_tokens"].as_u64().unwrap() <= 1400);
        assert_eq!(chunk["counts"]["exact"], true);
        if end < input.len() {
            assert!(range["text_tokens"].as_u64().unwrap() >= 512);
        }
        assert!(range["unique_tokens"].as_u64().unwrap() > 0);
    }
    assert_eq!(chunks.last().unwrap()["range"]["byte_end"], input.len());
    let capped = run(
        &config,
        &[
            "tokens",
            "Is checkout down?",
            "--split",
            "--combine",
            "any",
            "--max-chunks",
            "1",
        ],
        input.as_bytes(),
    );
    assert_eq!(capped.status.code(), Some(65));
    assert!(capped.stdout.is_empty());
    assert!(String::from_utf8_lossy(&capped.stderr).contains("max-chunks"));
}

#[test]
fn window_single_unit_recovery_and_file_content_splitting_keep_source_ids() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let long = " event".repeat(2500);
    let input = format!("short\n{long}\nlast\n");
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "-I",
            "lines",
            "--pack",
            "window:4",
            "--split",
            "--combine",
            "any",
        ],
        input.as_bytes(),
    );
    success(&output);
    let plans = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(plans.len(), 3);
    assert_eq!(plans[0]["indices"], json!([1]));
    assert_eq!(plans[1]["indices"], json!([2]));
    assert_eq!(plans[2]["indices"], json!([3]));
    assert_eq!(plans[1]["split"]["combine"], json!({"L2":"any"}));
    assert!(plans[1]["split"]["chunks"][0]["request"]["state"]["units"]["L2"].is_string());
    let path = dir.path().join("document.txt");
    std::fs::write(&path, &long).unwrap();
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "--files",
            "--split",
            "--combine",
            "any",
            path.to_str().unwrap(),
        ],
        b"",
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    for chunk in plan["split"]["chunks"].as_array().unwrap() {
        assert_eq!(chunk["request"]["state"]["path"], path.to_str().unwrap());
        assert!(chunk["request"]["state"]["content"].is_string());
    }
}

fn mixed_document() -> Value {
    json!({"questions":{
        "present":{"type":"noul","instructions":"Is checkout currently down?"},
        "severity":{"type":"choice","instructions":"Classify the report","criteria":{"healthy":"Only successful routine operation","failure":"Active checkout failure"}},
        "urgency":{"type":"score","instructions":"Rate response urgency","criteria":["Can wait","Soon","Immediately"]}
    },"policy":{"splittable":true,"combine":{"present":"any","severity":"mean","urgency":"max"}}})
}

#[test]
fn mixed_ask_requires_a_reducer_per_question_and_fit_input_keeps_ordinary_plan() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let path = dir.path().join("questions.json");
    let mut document = mixed_document();
    std::fs::write(&path, document.to_string()).unwrap();
    let output = run(
        &config,
        &["ask", "-f", path.to_str().unwrap(), "--dry-run"],
        long_text().as_bytes(),
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["split"]["combine"], document["policy"]["combine"]);
    document["policy"]["combine"]
        .as_object_mut()
        .unwrap()
        .remove("urgency");
    std::fs::write(&path, document.to_string()).unwrap();
    let output = run(
        &config,
        &["ask", "-f", path.to_str().unwrap(), "--dry-run"],
        b"short",
    );
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("urgency"));
    let output = run(
        &config,
        &["tokens", "question", "--split", "--combine", "any"],
        b"short",
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(plan.get("split").is_none());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_split_uses_real_chunk_answers_exact_counts_derived_provenance_and_hard_budget() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let input = long_text();
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently down?",
            "-I",
            "whole",
            "--split",
            "--combine",
            "any",
            "--strict",
            "--json",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result.as_object().unwrap().len(), 5);
    assert_eq!(result["unit"], input);
    assert_eq!(result["answer"]["derived"], true);
    let provenance = &result["answer"]["provenance"];
    let responses = provenance["responses"].as_array().unwrap();
    assert!(responses.len() > 1);
    let largest = responses
        .iter()
        .map(|response| response["answers"]["decision"]["noul"].as_f64().unwrap())
        .fold(0.0, f64::max);
    assert_eq!(result["answer"]["noul"], json!(largest));
    assert_eq!(
        responses.len(),
        provenance["ranges"].as_array().unwrap().len()
    );
    let cost = responses
        .iter()
        .map(|response| response["usage"]["input_tokens"].as_u64().unwrap())
        .sum::<u64>();
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "is",
            "Is checkout currently down?",
            "--split",
            "--combine",
            "any",
            "--strict",
            "--budget",
            &(cost - 1).to_string(),
            "--summary",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("budget"), "{stderr}");
    let path = dir.path().join("questions.json");
    std::fs::write(&path, mixed_document().to_string()).unwrap();
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "ask",
            "-f",
            path.to_str().unwrap(),
            "--strict",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result.as_object().unwrap().len(), 3);
    assert_eq!(result["answers"].as_object().unwrap().len(), 3);
    for id in ["present", "severity", "urgency"] {
        assert_eq!(result["answers"][id]["derived"], true);
        assert!(
            result["answers"][id]["provenance"]["responses"]
                .as_array()
                .unwrap()
                .len()
                > 1
        );
    }
}

#[test]
fn truncation_plans_retain_declared_views_and_only_change_the_selected_field() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let input = format!("BEGIN{}END", " 東京🙂 event".repeat(900));
    for mode in ["head", "tail", "middle"] {
        let output = run(
            &config,
            &[
                "--url",
                "http://127.0.0.1:1",
                "tokens",
                "question",
                "--truncate",
                mode,
            ],
            input.as_bytes(),
        );
        success(&output);
        let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["truncate"]["mode"], mode);
        assert!(plan["counts"]["input_tokens"].as_u64().unwrap() <= 1400);
        assert!(
            plan["truncate"]["original_counts"]["input_tokens"]
                .as_u64()
                .unwrap()
                > 1400
        );
        let retained = plan["truncate"]["retained"].as_array().unwrap();
        let value = retained
            .iter()
            .map(|range| {
                &input[range["byte_start"].as_u64().unwrap() as usize
                    ..range["byte_end"].as_u64().unwrap() as usize]
            })
            .collect::<Vec<_>>()
            .join(plan["truncate"]["separator"].as_str().unwrap());
        assert_eq!(plan["request"]["state"], value);
        if mode != "tail" {
            assert!(value.starts_with("BEGIN"));
        }
        if mode != "head" {
            assert!(value.ends_with("END"));
        }
    }
    let original =
        json!({"body":{"message":input,"id":18446744073709551615u64},"a/b~c":"untouched"});
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "-I",
            "jsonl",
            "--truncate-field",
            "body.message",
        ],
        original.to_string().as_bytes(),
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        plan["request"]["state"]["body"]["id"],
        original["body"]["id"]
    );
    assert_eq!(plan["request"]["state"]["a/b~c"], "untouched");
    assert_eq!(plan["truncate"]["path"], "/body/message");
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "-I",
            "jsonl",
            "--pack",
            "window:4",
            "--truncate-field",
            "/body/message",
        ],
        original.to_string().as_bytes(),
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["indices"], json!([1]));
    assert_eq!(plan["truncate"]["path"], "/units/R1/body/message");
    assert_eq!(
        plan["request"]["state"]["units"]["R1"]["body"]["id"],
        original["body"]["id"]
    );
}

#[test]
fn invalid_truncation_targets_formats_and_conflicting_policies_fail_before_inference() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    for args in [
        vec!["tokens", "question", "--truncate", "invalid"],
        vec![
            "tokens",
            "question",
            "--truncate",
            "head",
            "--split",
            "--combine",
            "any",
        ],
        vec!["tokens", "question", "--truncate-field", "message"],
        vec!["tokens", "question", "--truncate", "head", "-I", "jsonl"],
        vec![
            "tokens",
            "question",
            "--truncate-field",
            "message",
            "--truncate",
            "head",
            "-I",
            "jsonl",
        ],
        vec![
            "tokens",
            "question",
            "--truncate-field",
            "/missing",
            "-I",
            "jsonl",
        ],
        vec![
            "tokens",
            "question",
            "--truncate-field",
            "number",
            "-I",
            "jsonl",
        ],
    ] {
        let output = run(&config, &args, b"{\"message\":\"short\",\"number\":42}\n");
        assert_eq!(
            output.status.code(),
            Some(64),
            "args={args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    let oversized = json!({"message":"short","other":" event".repeat(3000)});
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "-I",
            "jsonl",
            "--truncate-field",
            "message",
        ],
        oversized.to_string().as_bytes(),
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    let output = run(
        &config,
        &["tokens", "question", "--truncate", "head"],
        b"short",
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(plan.get("truncate").is_none());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_lossy_views_have_exact_usage_original_output_and_explicit_provenance() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let input =
        "Checkout is down and customers cannot pay. ".to_owned() + &" routine detail".repeat(2000);
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout down?",
            "-I",
            "whole",
            "--truncate",
            "head",
            "--strict",
            "--json",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record.as_object().unwrap().len(), 5);
    assert_eq!(record["unit"], input);
    assert_eq!(record["answer"]["lossy"], true);
    assert_eq!(record["answer"]["provenance"]["operation"], "truncate");
    assert_eq!(record["answer"]["provenance"]["mode"], "head");
    let csv = format!("id,message\r\nA,{input}\r\n");
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout down?",
            "-I",
            "csv",
            "--truncate-field",
            "message",
            "--strict",
            "--no-cache",
        ],
        csv.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, csv.as_bytes());
    let tsv = format!("id\tmessage\nA\t{input}\n");
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout down?",
            "-I",
            "tsv",
            "--truncate-field",
            "message",
            "--strict",
            "--no-cache",
        ],
        tsv.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, tsv.as_bytes());
}
