#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn config(root: &Path) -> std::path::PathBuf {
    let path = root.join("config.toml");
    std::fs::write(
        &path,
        include_str!("../../../examples/config.toml")
            .replace("max_length = 16384", "max_length = 1400"),
    )
    .unwrap();
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
fn document() -> String {
    "An inventory label was printed successfully. ".repeat(500)
        + "\n\nCheckout is down and customers cannot pay. 東京🙂\n"
}
fn questions() -> Value {
    json!({"questions":{
        "present":{"type":"noul","instructions":"Is checkout currently down?"},
        "severity":{"type":"choice","instructions":"Classify the checkout status in the excerpt","criteria":{"healthy":"Checkout is currently working","failure":"Checkout is currently unavailable"}},
        "urgency":{"type":"score","instructions":"Rate response urgency","criteria":["Can wait","Soon","Immediately"]}
    }})
}

#[test]
fn focus_plans_expose_exact_first_pass_and_conditional_final_without_inference() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let input = document();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "tokens",
            "Is checkout down?",
            "--focus",
            "1",
        ],
        input.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(plan["counts"]["input_tokens"].as_u64().unwrap() > 1400);
    assert_eq!(plan["request"]["state"], input);
    assert_eq!(plan["focus"]["operation"], "focus");
    assert_eq!(plan["focus"]["k"], 1);
    assert_eq!(plan["focus"]["final_template"]["state"], "");
    assert!(
        plan["focus"]["final_status"]
            .as_str()
            .unwrap()
            .contains("conditional")
    );
    let chunks = plan["focus"]["relevance"].as_array().unwrap();
    assert!(chunks.len() > 1 && chunks.len() <= 64);
    for chunk in chunks {
        assert!(chunk["counts"]["input_tokens"].as_u64().unwrap() <= 1400);
        assert_eq!(chunk["request"]["questions"].as_object().unwrap().len(), 1);
        assert_eq!(
            chunk["request"]["questions"]["focus-relevance"]["type"],
            "score"
        );
        let start = chunk["range"]["byte_start"].as_u64().unwrap() as usize;
        let end = chunk["range"]["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(chunk["request"]["state"], &input[start..end]);
    }
    for input in [b"short".as_slice(), b""] {
        let output = run(&config, &["tokens", "question", "--focus", "3"], input);
        success(&output);
        let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["focus"]["relevance"].as_array().unwrap().len(), 1);
    }
    let output = run(
        &config,
        &["tokens", "question", "--focus", "1", "--max-chunks", "1"],
        document().as_bytes(),
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
}

#[test]
fn focus_flags_reject_conflicts_structured_records_and_estimated_tokenizers() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    for args in [
        vec!["tokens", "q", "--focus", "0"],
        vec!["tokens", "q", "--focus", "1", "--split"],
        vec!["tokens", "q", "--focus", "1", "--combine", "mean"],
        vec!["tokens", "q", "--focus", "1", "--truncate", "head"],
        vec!["tokens", "q", "--focus", "1", "--truncate-field", "message"],
        vec!["tokens", "q", "--focus", "1", "-I", "jsonl"],
        vec!["tokens", "q", "--focus", "1", "-I", "csv"],
        vec!["tokens", "q", "--focus", "1", "-I", "tsv"],
    ] {
        let output = run(&config, &args, b"");
        assert_eq!(
            output.status.code(),
            Some(64),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    let source = std::fs::read_to_string(&config)
        .unwrap()
        .replace("encoder = \"clef-v1\"", "encoder = \"estimate\"");
    std::fs::write(&config, source).unwrap();
    let output = run(&config, &["tokens", "q", "--focus", "1"], b"text");
    assert_eq!(output.status.code(), Some(64));
    assert!(output.stdout.is_empty());
}

#[test]
fn focus_overrides_split_defaults_and_mixed_ask_needs_no_combine_map() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let path = dir.path().join("questions.json");
    std::fs::write(&path, questions().to_string()).unwrap();
    let output = run(
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "ask",
            "-f",
            path.to_str().unwrap(),
            "--focus",
            "1",
            "--dry-run",
        ],
        document().as_bytes(),
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        plan["focus"]["final_template"]["questions"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert!(plan.get("split").is_none());
    std::fs::write(&path,json!({"type":"noul","instructions":"Is checkout down?","policy":{"splittable":true,"combine":"any"}}).to_string()).unwrap();
    let output = run(
        &config,
        &["tokens", "-f", path.to_str().unwrap(), "--focus", "1"],
        document().as_bytes(),
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(plan.get("focus").is_some() && plan.get("split").is_none());
}

#[test]
fn focus_window_context_and_file_plans_keep_original_target_and_source_identity() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "-I",
            "lines",
            "--pack",
            "window:4",
            "--with-context",
            "1",
            "--focus",
            "1",
        ],
        b"earlier\n\ncurrent\n",
    );
    success(&output);
    let plans = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[1]["indices"], json!([3]));
    assert_eq!(plans[1]["focus"]["path"], "/units/L3/unit");
    assert_eq!(
        plans[1]["focus"]["relevance"][0]["request"]["state"]["units"]["L3"]["previous_lines"][0]["id"],
        "L2"
    );
    assert_eq!(
        plans[1]["focus"]["final_template"]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["L3"]
    );
    let path = dir.path().join("document.txt");
    std::fs::write(&path, document()).unwrap();
    let output = run(
        &config,
        &[
            "tokens",
            "question",
            "--files",
            "--focus",
            "1",
            path.to_str().unwrap(),
        ],
        b"",
    );
    success(&output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["focus"]["path"], "/content");
    assert_eq!(
        plan["focus"]["final_template"]["state"]["path"],
        path.to_str().unwrap()
    );
    assert_eq!(plan["focus"]["final_template"]["state"]["content"], "");
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_focus_reasks_mixed_questions_with_raw_phase_counts_and_separate_budget_admission() {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let path = dir.path().join("questions.json");
    std::fs::write(&path, questions().to_string()).unwrap();
    let input = document();
    let args = [
        "--url",
        &endpoint,
        "ask",
        "-f",
        path.to_str().unwrap(),
        "--focus",
        "1",
        "--strict",
        "--no-cache",
    ];
    let output = run(&config, &args, input.as_bytes());
    success(&output);
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result.as_object().unwrap().len(), 3);
    assert_eq!(result["answers"].as_object().unwrap().len(), 3);
    let provenance = &result["answers"]["present"]["provenance"];
    let relevance = provenance["relevance"].as_array().unwrap();
    assert!(relevance.len() > 1);
    let first_cost = relevance
        .iter()
        .map(|chunk| {
            assert_eq!(
                chunk["counts"]["input_tokens"],
                chunk["response"]["usage"]["input_tokens"]
            );
            chunk["counts"]["input_tokens"].as_u64().unwrap()
        })
        .sum::<u64>();
    assert_eq!(
        provenance["final"]["counts"]["input_tokens"],
        provenance["final"]["response"]["usage"]["input_tokens"]
    );
    let final_cost = provenance["final"]["counts"]["input_tokens"]
        .as_u64()
        .unwrap();
    let ranges = provenance["ranges"].as_array().unwrap();
    let selected = ranges
        .iter()
        .map(|range| {
            &input[range["byte_start"].as_u64().unwrap() as usize
                ..range["byte_end"].as_u64().unwrap() as usize]
        })
        .collect::<Vec<_>>()
        .join(provenance["separator"].as_str().unwrap());
    assert_eq!(provenance["final"]["request"]["state"], selected);
    assert_eq!(result["usage"]["input_tokens"], first_cost + final_cost);
    let best = relevance
        .iter()
        .enumerate()
        .max_by(|(a, x), (b, y)| {
            x["response"]["answers"]["focus-relevance"]["score"]
                .as_f64()
                .unwrap()
                .total_cmp(
                    &y["response"]["answers"]["focus-relevance"]["score"]
                        .as_f64()
                        .unwrap(),
                )
                .then_with(|| b.cmp(a))
        })
        .unwrap()
        .0
        + 1;
    assert_eq!(provenance["selected_chunks"], json!([best]));
    for id in ["present", "severity", "urgency"] {
        assert_eq!(result["answers"][id]["focused"], true);
        let raw = &provenance["final"]["response"]["answers"][id];
        for (key, value) in raw.as_object().unwrap() {
            assert_eq!(&result["answers"][id][key], value);
        }
    }
    assert!(
        result["answers"]["present"]["noul"].as_f64().unwrap() > 0.5,
        "{result}"
    );
    for (limit, phase) in [(1, "focus relevance"), (first_cost, "focus final request")] {
        let mut args = args.to_vec();
        let limit = limit.to_string();
        args.extend(["--budget", &limit]);
        let output = run(&config, &args, input.as_bytes());
        assert_eq!(output.status.code(), Some(4));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(phase),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "ask",
            "-f",
            path.to_str().unwrap(),
            "--focus",
            "64",
            "--strict",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    assert_eq!(output.status.code(), Some(65));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Final request was not submitted"));
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_focus_windows_preserve_ids_halve_final_requests_and_flush_a_prefix_before_context_failure()
 {
    let endpoint = std::env::var("LOUPE_TEST_URL").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let positive = "Checkout is down and customers cannot pay. ".to_owned()
        + &" unrelated inventory label".repeat(700);
    let negative = "Checkout works normally and customers can pay. ".to_owned()
        + &" unrelated inventory label".repeat(700);
    let input = format!("{positive}\r\n\r\n{negative}\r\n{positive}\r\n");
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently down?",
            "--pack",
            "window:4",
            "--focus",
            "1",
            "--strict",
            "--json",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    success(&output);
    assert!(output.stderr.is_empty());
    let records = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .map(|record| record["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 4]
    );
    for (record, id) in records.iter().zip(["L1", "L4"]) {
        assert_eq!(record["unit"], positive);
        assert_eq!(record["answer"]["focused"], true);
        let provenance = &record["answer"]["provenance"];
        assert_eq!(provenance["path"], format!("/units/{id}"));
        assert!(
            provenance["final"]["counts"]["input_tokens"]
                .as_u64()
                .unwrap()
                <= 1400
        );
        assert_eq!(
            provenance["final"]["response"]["answers"]
                .as_object()
                .unwrap()
                .len(),
            1
        );
    }
    let input = format!("Checkout is healthy.\n\n{positive}\nordinary\n");
    let output = run(
        &config,
        &[
            "--url",
            &endpoint,
            "keep",
            "Is checkout currently down?",
            "--pack",
            "window:4",
            "--with-context",
            "1",
            "--focus",
            "1",
            "--strict",
            "--no-cache",
        ],
        input.as_bytes(),
    );
    assert_eq!(output.status.code(), Some(65));
    assert_eq!(output.stdout, format!("{positive}\n").as_bytes());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unit 4 focus plan"));
}
