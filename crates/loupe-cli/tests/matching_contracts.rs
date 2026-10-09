#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

fn setup(root: &Path) -> PathBuf {
    let config = root.join("config.toml");
    std::fs::write(&config, include_str!("../../../examples/config.toml")).unwrap();
    config
}
fn command(root: &Path, config: &Path, args: &[&str]) -> Command {
    command_url(root, config, args, "http://127.0.0.1:1")
}
fn command_url(root: &Path, config: &Path, args: &[&str], url: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            url,
        ])
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
        .stderr(Stdio::piped());
    command
}
fn run(root: &Path, config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command(root, config, args).spawn().unwrap();
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

#[test]
fn join_blocking_reads_right_first_and_emits_stable_capped_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(
        &right,
        (0..25)
            .map(|id| {
                format!(
                    "{}\n",
                    json!({"id":id,"name":"École Alpha, Inc.","location":"New York"})
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    let input = format!(
        "{}\n{}\n",
        json!({"name":"ALPHA INC ÉCOLE","location":"new-york"}),
        json!({"name":"unrelated","location":"Berlin"})
    );
    let result = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name,location",
            "--dry-run",
        ],
        input.as_bytes(),
    );
    code(&result, 0);
    let rows = records(&result);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["eligible_records"], json!(25));
    assert_eq!(rows[0]["candidates"].as_array().unwrap().len(), 20);
    for (offset, candidate) in rows[0]["candidates"].as_array().unwrap().iter().enumerate() {
        assert_eq!(candidate["blocking"]["right_index"], json!(offset + 1));
        assert_eq!(candidate["blocking"]["matching_lsh_bands"], json!(8));
        assert_eq!(candidate["right"]["unit"]["id"], json!(offset));
        assert!(candidate["right"]["location"]["byte_end"].as_u64().unwrap() > 0);
    }
    assert!(rows[1]["candidates"].as_array().unwrap().is_empty());
    assert_eq!(rows[0]["right_index"]["scratch_slots"], json!(25));
    let capped = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name,location",
            "--max-candidates",
            "2",
            "--blocking",
            "lsh",
            "--dry-run",
        ],
        input.as_bytes(),
    );
    code(&capped, 0);
    assert_eq!(
        records(&capped)[0]["candidates"].as_array().unwrap().len(),
        2
    );
    std::fs::write(&right, b"{\"name\":[]}\n").unwrap();
    let result = run(
        root,
        &config,
        &[
            "join",
            "missing-left",
            right.to_str().unwrap(),
            "--on",
            "name",
            "--dry-run",
        ],
        b"",
    );
    code(&result, 65);
}

#[test]
fn join_table_rows_errors_empty_sides_and_modes_use_real_readers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.csv");
    let left = root.join("left.csv");
    std::fs::write(
        &right,
        b"name,location,note\n\"Alpha, Inc\",London,\"first\nsecond\"\nBeta,Paris,note\n",
    )
    .unwrap();
    std::fs::write(&left, b"name,location\nAlpha Inc,London\n").unwrap();
    let result = run(
        root,
        &config,
        &[
            "join",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--on",
            "name,location",
            "-I",
            "csv",
            "--dry-run",
        ],
        b"",
    );
    code(&result, 0);
    let rows = records(&result);
    assert_eq!(
        rows[0]["candidates"][0]["right"]["unit"]["note"],
        json!("first\nsecond")
    );
    assert_eq!(
        rows[0]["candidates"][0]["right"]["location"]["line_end"],
        json!(3)
    );
    for extra in [
        vec!["-I", "lines"],
        vec!["--blocking", "bad"],
        vec!["--max-candidates", "0"],
    ] {
        let mut args = vec![
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "--dry-run",
        ];
        args.extend(extra);
        code(&run(root, &config, &args, b""), 64);
    }
    code(
        &run(
            root,
            &config,
            &["join", "-", "-", "--on", "name", "--dry-run"],
            b"",
        ),
        64,
    );
    code(
        &run(
            root,
            &config,
            &[
                "join",
                "-",
                right.to_str().unwrap(),
                "--on",
                "name",
                "-O",
                "plain",
            ],
            b"",
        ),
        64,
    );
    code(
        &run(
            root,
            &config,
            &[
                "join",
                "-",
                right.to_str().unwrap(),
                "--on",
                "name,/name",
                "--dry-run",
            ],
            b"",
        ),
        64,
    );
    std::fs::write(&right, b"").unwrap();
    let result = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "--dry-run",
        ],
        b"{\"name\":\"Alpha\"}\n",
    );
    code(&result, 0);
    assert!(
        records(&result)[0]["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    code(
        &run(
            root,
            &config,
            &[
                "join",
                "-",
                right.to_str().unwrap(),
                "--on",
                "name",
                "--dry-run",
            ],
            b"",
        ),
        1,
    );
}

#[test]
fn left_stdin_produces_a_complete_candidate_row_before_eof() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, b"{\"name\":\"Alpha\"}\n").unwrap();
    let mut child = command(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "--dry-run",
        ],
    )
    .spawn()
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    stdin.write_all(b"{\"name\":\"Alpha\"}\n").unwrap();
    stdin.flush().unwrap();
    let observed = receiver.recv_timeout(Duration::from_secs(5));
    drop(stdin);
    if observed.is_err() {
        let _ = child.kill();
    }
    let result = child.wait_with_output().unwrap();
    code(&result, 0);
    let row: Value = serde_json::from_str(&observed.unwrap().unwrap()).unwrap();
    assert_eq!(row["candidates"][0]["right"]["index"], json!(1));
}

fn live(root: &Path, config: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command_url(root, config, args, "http://127.0.0.1:8015")
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    child.wait_with_output().unwrap()
}
fn json_lines(values: &[Value]) -> Vec<u8> {
    values
        .iter()
        .map(|value| format!("{value}\n"))
        .collect::<String>()
        .into_bytes()
}
fn summary(output: &Output, key: &str) -> u64 {
    let prefix = format!("{key}=");
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}
fn join_args(right: &Path) -> Vec<&str> {
    vec![
        "join",
        "-",
        right.to_str().unwrap(),
        "--on",
        "name,location",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ]
}
fn entity(id: &str) -> Value {
    json!({"id":id,"name":"Acme Dental Clinic","location":"Portland Maine"})
}

#[test]
fn pair_rubrics_flags_schema_limits_and_journal_aliases_validate_before_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, json_lines(&[entity("r")])).unwrap();
    let questions = root.join("questions.json");
    for rubric in [
        json!({"type":"noul"}),
        json!({"questions":{"similarity":{"type":"score","criteria":["only"]},"same_entity":{"type":"noul"}}}),
        json!({"questions":{"similarity":{"type":"score","criteria":["no","yes"]},"same_entity":{"type":"choice","criteria":{"yes":"yes"}}}}),
    ] {
        std::fs::write(&questions, rubric.to_string()).unwrap();
        let result = run(
            root,
            &config,
            &[
                "join",
                "-",
                right.to_str().unwrap(),
                "--on",
                "name",
                "-f",
                questions.to_str().unwrap(),
                "--dry-run",
            ],
            &json_lines(&[entity("l")]),
        );
        code(&result, 64);
    }
    for extra in [
        vec!["--cardinality", "bad"],
        vec!["--threshold", "2"],
        vec!["--margin", "-1"],
        vec!["--min-confidence", "-0.1"],
        vec!["--min-gap", "-0.1"],
        vec!["--dry-run", "--failed", "run.jsonl"],
        vec!["--dry-run", "--keep-going"],
        vec!["--fallback", "printf yes"],
        vec!["--fallback-timeout", "1s"],
        vec!["--on-fallback-error", "fail"],
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
        let mut args = vec!["join", "-", right.to_str().unwrap(), "--on", "name"];
        args.extend(extra);
        code(&run(root, &config, &args, b""), 64);
    }
    let limited = std::fs::read_to_string(&config)
        .unwrap()
        .replace("max_questions = 128", "max_questions = 2");
    std::fs::write(&config, limited).unwrap();
    code(
        &run(
            root,
            &config,
            &[
                "join",
                "-",
                right.to_str().unwrap(),
                "--on",
                "name,location",
                "--dry-run",
            ],
            &json_lines(&[entity("l")]),
        ),
        65,
    );
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_join_cardinalities_keep_pair_distributions_and_enforce_reuse() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(
        &right,
        json_lines(&[
            entity("r-a"),
            entity("r-b"),
            json!({"id":"r-noise","name":"Acme Hardware","location":"Portland Oregon"}),
        ]),
    )
    .unwrap();
    let input = json_lines(&[
        entity("left-a"),
        entity("left-b"),
        json!({"id":"left-none","name":"Unlisted Workshop","location":"Berlin Germany"}),
    ]);
    for cardinality in ["1:1", "1:n", "n:m"] {
        let journal = root.join(format!("{}.jsonl", cardinality.replace(':', "-")));
        let mut args = join_args(&right);
        args.extend([
            "--cardinality",
            cardinality,
            "--unmatched",
            "both",
            "--why",
            "--failed",
            journal.to_str().unwrap(),
        ]);
        let result = live(root, &config, &args, &input);
        code(&result, 0);
        let rows = records(&result);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3]["kind"], json!("unmatched_right"));
        assert_eq!(rows[3]["right"]["unit"]["id"], json!("r-noise"));
        let counts = rows
            .iter()
            .take(3)
            .map(|row| row["matches"].as_array().unwrap().len())
            .collect::<Vec<_>>();
        assert_eq!(
            counts,
            match cardinality {
                "1:1" => vec![1, 1, 0],
                "1:n" => vec![2, 0, 0],
                _ => vec![2, 2, 0],
            }
        );
        if cardinality == "1:1" {
            assert_ne!(
                rows[0]["matches"][0]["right"]["index"],
                rows[1]["matches"][0]["right"]["index"]
            );
        }
        for row in rows.iter().take(3) {
            for pair in row["evaluated_pairs"].as_array().unwrap() {
                assert_eq!(
                    pair["counts"]["input_tokens"],
                    pair["response"]["usage"]["input_tokens"]
                );
                assert_eq!(
                    pair["request"]["state"]["left"]["id"],
                    json!(format!("L{}", row["index"].as_u64().unwrap()))
                );
                assert_eq!(
                    pair["question_order"],
                    json!(["similarity", "same_entity", "field_1", "field_2"])
                );
                let distribution = &pair["response"]["answers"]["similarity"]["probabilities"];
                let p0 = distribution["0"].as_f64().unwrap();
                let p1 = distribution["1"].as_f64().unwrap();
                let p2 = distribution["2"].as_f64().unwrap();
                assert!(
                    (pair["decision"]["ranking_score"].as_f64().unwrap()
                        - (p1 + 2.0 * p2) / (p0 + p1 + p2) / 2.0)
                        .abs()
                        < 1e-12
                );
                assert_eq!(pair["decision"]["probabilities"], *distribution);
            }
        }
        assert_eq!(
            summary(&result, "input_counted"),
            summary(&result, "input_reported")
        );
        assert_eq!(summary(&result, "unknown_input_counted"), 0);
        let mut args = join_args(&right);
        args.extend([
            "--cardinality",
            cardinality,
            "--unmatched",
            "both",
            "--why",
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
        ]);
        let result = live(root, &config, &args, &input);
        code(&result, 0);
        assert!(result.stdout.is_empty());
        assert_eq!(summary(&result, "http_attempts"), 0);
    }
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_join_input_order_and_budget_resume_preserve_arriving_left_assignment() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, json_lines(&[entity("right-only")])).unwrap();
    for ids in [["alpha", "beta"], ["beta", "alpha"]] {
        let input = json_lines(&[entity(ids[0]), entity(ids[1])]);
        let mut args = join_args(&right);
        args.extend(["--unmatched", "both", "--max-candidates", "1"]);
        let result = live(root, &config, &args, &input);
        code(&result, 0);
        let rows = records(&result);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["kind"], json!("matched"));
        assert_eq!(rows[0]["left"]["unit"]["id"], json!(ids[0]));
        assert_eq!(rows[1]["kind"], json!("unmatched_left"));
        assert_eq!(rows[1]["blocking"]["unavailable_records"], json!(1));
        assert!(rows[1]["evaluated_pairs"].as_array().unwrap().is_empty());
    }
    let input = json_lines(&[entity("alpha"), entity("beta"), entity("gamma")]);
    let plan = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name,location",
            "--cardinality",
            "n:m",
            "--dry-run",
        ],
        &input,
    );
    code(&plan, 0);
    let allowance = records(&plan)[0]["candidates"][0]["pair_plan"]["counts"]["input_tokens"]
        .as_u64()
        .unwrap()
        .to_string();
    let journal = root.join("partial.jsonl");
    let mut args = join_args(&right);
    args.extend([
        "--cardinality",
        "n:m",
        "--budget",
        &allowance,
        "--failed",
        journal.to_str().unwrap(),
    ]);
    let first = live(root, &config, &args, &input);
    code(&first, 4);
    assert_eq!(records(&first).len(), 1);
    let mut args = join_args(&right);
    args.extend([
        "--cardinality",
        "n:m",
        "--budget",
        "0",
        "--resume",
        journal.to_str().unwrap(),
    ]);
    let repeat = live(root, &config, &args, &input);
    code(&repeat, 4);
    assert!(repeat.stdout.is_empty());
    assert_eq!(summary(&repeat, "http_attempts"), 0);
    let mut args = join_args(&right);
    args.extend([
        "--cardinality",
        "n:m",
        "--resume",
        journal.to_str().unwrap(),
    ]);
    let resumed = live(root, &config, &args, &input);
    code(&resumed, 0);
    let rows = records(&resumed);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["left"]["unit"]["id"], json!("beta"));
    assert_eq!(rows[1]["left"]["unit"]["id"], json!("gamma"));
    assert_eq!(summary(&resumed, "http_attempts"), 2);
    let mutated = json_lines(&[entity("changed"), entity("beta"), entity("gamma")]);
    let refused = live(root, &config, &args, &mutated);
    code(&refused, 65);
    assert!(refused.stdout.is_empty());
    assert_eq!(summary(&refused, "http_attempts"), 0);
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_join_uncertainty_policies_replay_without_new_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, json_lines(&[entity("right")])).unwrap();
    let input = json_lines(&[entity("left")]);
    for action in ["drop", "keep", "stderr"] {
        let mut args = join_args(&right);
        args.extend([
            "--margin",
            "1",
            "--on-uncertain",
            action,
            "--unmatched",
            "both",
        ]);
        let result = live(root, &config, &args, &input);
        code(&result, 0);
        let rows = records(&result);
        assert_eq!(
            rows[0]["kind"],
            json!(if action == "keep" {
                "matched"
            } else {
                "unmatched_left"
            })
        );
        assert_eq!(
            rows[0]["evaluated_pairs"][0]["decision"]["uncertain"],
            json!(true)
        );
        assert_eq!(
            rows[0]["evaluated_pairs"][0]["decision"]["eligible"],
            json!(action == "keep")
        );
    }
    let journal = root.join("uncertain.jsonl");
    let mut args = join_args(&right);
    args.extend([
        "--margin",
        "1",
        "--on-uncertain",
        "fail",
        "--failed",
        journal.to_str().unwrap(),
    ]);
    let failed = live(root, &config, &args, &input);
    code(&failed, 3);
    assert!(failed.stdout.is_empty());
    assert_eq!(summary(&failed, "http_attempts"), 1);
    let mut args = join_args(&right);
    args.extend([
        "--margin",
        "1",
        "--on-uncertain",
        "fail",
        "--budget",
        "0",
        "--resume",
        journal.to_str().unwrap(),
    ]);
    let repeated = live(root, &config, &args, &input);
    code(&repeated, 3);
    assert!(repeated.stdout.is_empty());
    assert_eq!(summary(&repeated, "http_attempts"), 0);
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_join_fallback_orders_pair_heads_preserves_evidence_and_replays() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, json_lines(&[entity("right-a"), entity("right-b")])).unwrap();
    let input = json_lines(&[entity("left")]);
    let journal = root.join("fallback.jsonl");
    let fallback = r#"IFS= read -r state; printf '%s\n' "$state" >> states.jsonl; printf '%s\n' "$LOUPE_QUESTION_ID" >> fields.txt; printf '%s\n' "$LOUPE_QUESTION" >> questions.jsonl; printf '%s\n' "$LOUPE_ANSWER_JSON" >> answers.jsonl; case "$LOUPE_QUESTION_ID" in same_entity) printf yes;; similarity) printf 1.5;; *) exit 13;; esac"#;
    let mut args = join_args(&right);
    args.extend([
        "--cardinality",
        "n:m",
        "--margin",
        "1",
        "--min-confidence",
        "1",
        "--min-gap",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        fallback,
        "--failed",
        journal.to_str().unwrap(),
    ]);
    let result = live(root, &config, &args, &input);
    code(&result, 0);
    let rows = records(&result);
    let pairs = rows[0]["evaluated_pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(rows[0]["matches"].as_array().unwrap().len(), 2);
    let states: Vec<Value> = std::fs::read_to_string(root.join("states.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let fields = std::fs::read_to_string(root.join("fields.txt")).unwrap();
    let questions: Vec<Value> = std::fs::read_to_string(root.join("questions.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let answers: Vec<Value> = std::fs::read_to_string(root.join("answers.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut cursor = 0;
    let mut expected_fields = Vec::new();
    for pair in pairs {
        assert_eq!(
            pair["counts"]["input_tokens"],
            pair["response"]["usage"]["input_tokens"]
        );
        assert_eq!(pair["decision"]["gate"]["uncertain"], json!(true));
        assert_eq!(
            pair["decision"]["score"]["uncertain"],
            json!(true),
            "fixture must exercise the Score cascade"
        );
        assert_eq!(pair["decision"]["uncertain"], json!(false));
        assert_eq!(pair["decision"]["eligible"], json!(true));
        assert_eq!(pair["decision"]["ranking_score"], json!(0.75));
        assert_eq!(
            pair["decision"]["probabilities"],
            pair["response"]["answers"]["similarity"]["probabilities"]
        );
        for field in ["same_entity", "similarity"] {
            expected_fields.push(field);
            assert_eq!(states[cursor], pair["request"]["state"]);
            assert_eq!(questions[cursor], pair["request"]["questions"][field]);
            assert_eq!(answers[cursor], pair["response"]["answers"][field]);
            assert_eq!(
                pair["fallback"][field]["decision"]["source"],
                json!("fallback")
            );
            assert!(pair["fallback"][field]["decision"]["confidence"].is_null());
            cursor += 1;
        }
    }
    assert_eq!(fields.lines().collect::<Vec<_>>(), expected_fields);
    assert_eq!(cursor, states.len());
    assert_eq!(summary(&result, "fallback_attempts"), 4);
    assert_eq!(summary(&result, "fallback_resolved"), 4);
    assert!(String::from_utf8_lossy(&result.stderr).contains("fallback_fraction=1.000000"));
    assert_eq!(
        summary(&result, "input_counted"),
        summary(&result, "input_reported")
    );
    let mut resume = join_args(&right);
    resume.extend([
        "--cardinality",
        "n:m",
        "--margin",
        "1",
        "--min-confidence",
        "1",
        "--min-gap",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        fallback,
        "--resume",
        journal.to_str().unwrap(),
        "--budget",
        "0",
    ]);
    let replayed = live(root, &config, &resume, &input);
    code(&replayed, 0);
    assert!(replayed.stdout.is_empty());
    assert_eq!(summary(&replayed, "http_attempts"), 0);
    assert_eq!(summary(&replayed, "fallback_attempts"), 0);
    assert_eq!(summary(&replayed, "fallback_replayed"), 4);
    assert_eq!(
        std::fs::read_to_string(root.join("fields.txt")).unwrap(),
        fields
    );
    let changed = resume
        .iter()
        .map(|arg| if *arg == fallback { "printf no" } else { *arg })
        .collect::<Vec<_>>();
    let refused = live(root, &config, &changed, &input);
    code(&refused, 65);
    assert!(refused.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("fields.txt")).unwrap(),
        fields
    );
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_join_fallback_rejections_timeouts_and_fatal_replay_do_not_run_score() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, json_lines(&[entity("right")])).unwrap();
    let input = json_lines(&[entity("left")]);
    for (name, command, expected_status) in [
        ("reject", "cat > /dev/null; printf no", "resolved"),
        ("timeout", "cat > /dev/null; sleep 2; printf yes", "failed"),
        ("invalid", "cat > /dev/null; printf maybe", "failed"),
    ] {
        let mut args = join_args(&right);
        args.extend([
            "--margin",
            "1",
            "--min-confidence",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            command,
            "--fallback-timeout",
            "50ms",
            "--unmatched",
            "both",
        ]);
        let result = live(root, &config, &args, &input);
        code(&result, 0);
        let rows = records(&result);
        let pair = &rows[0]["evaluated_pairs"][0];
        assert_eq!(rows[0]["kind"], json!("unmatched_left"), "{name}");
        assert_eq!(
            pair["fallback"]["same_entity"]["execution"]["status"],
            json!(expected_status)
        );
        assert!(pair["fallback"].get("similarity").is_none());
        assert_eq!(pair["decision"]["eligible"], json!(false));
        assert_eq!(
            pair["decision"]["uncertain"],
            json!(expected_status == "failed")
        );
        assert_eq!(summary(&result, "fallback_attempts"), 1);
        assert_eq!(summary(&result, "http_attempts"), 1);
        assert_eq!(
            summary(&result, "input_counted"),
            summary(&result, "input_reported")
        );
        if name == "timeout" {
            assert_eq!(
                pair["fallback"]["same_entity"]["execution"]["kind"],
                json!("timeout")
            );
        }
    }
    for (filename, fallback, expected_calls, attempts) in [
        (
            "fatal-gate",
            "cat > /dev/null; printf '%s\n' \"$LOUPE_QUESTION_ID\" >> fatal-gate; exit 7",
            "same_entity\n",
            1,
        ),
        (
            "fatal-score",
            "cat > /dev/null; printf '%s\n' \"$LOUPE_QUESTION_ID\" >> fatal-score; if [ \"$LOUPE_QUESTION_ID\" = same_entity ]; then printf yes; else exit 7; fi",
            "same_entity\nsimilarity\n",
            2,
        ),
    ] {
        let journal = root.join(format!("{filename}.jsonl"));
        for flag in ["--failed", "--resume"] {
            let mut args = join_args(&right);
            args.extend([
                "--margin",
                "1",
                "--min-confidence",
                "1",
                "--min-gap",
                "1",
                "--on-uncertain",
                "fallback",
                "--fallback",
                fallback,
                "--on-fallback-error",
                "fail",
                flag,
                journal.to_str().unwrap(),
            ]);
            if flag == "--resume" {
                args.extend(["--budget", "0"]);
            }
            let result = live(root, &config, &args, &input);
            code(&result, 69);
            assert!(result.stdout.is_empty());
            assert_eq!(
                std::fs::read_to_string(root.join(filename)).unwrap(),
                expected_calls
            );
            assert_eq!(
                summary(&result, "http_attempts"),
                u64::from(flag == "--failed")
            );
            assert_eq!(
                summary(&result, "fallback_attempts"),
                attempts * u64::from(flag == "--failed")
            );
            assert_eq!(
                summary(&result, "fallback_replayed"),
                attempts * u64::from(flag == "--resume")
            );
        }
    }
}
