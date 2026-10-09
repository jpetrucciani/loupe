#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn setup(root: &Path) -> (PathBuf, PathBuf) {
    let config = root.join("config.toml");
    std::fs::write(&config, include_str!("../../../examples/config.toml")).unwrap();
    let questions = root.join("features.toml");
    std::fs::write(
        &questions,
        include_str!("../../../examples/postmortem-features.toml"),
    )
    .unwrap();
    (config, questions)
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
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn ensemble_questions(root: &Path) -> (PathBuf, PathBuf) {
    let (config, questions) = setup(root);
    std::fs::write(
        &questions,
        include_str!("../../../examples/ensemble-features.toml"),
    )
    .unwrap();
    (config, questions)
}

#[test]
fn ensemble_feature_plans_keep_primary_columns_and_count_all_window_heads() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = ensemble_questions(root);
    let output = run(
        root,
        &config,
        &[
            "--url",
            "http://127.0.0.1:1",
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0.75",
            "--pack",
            "window:2",
            "--dry-run",
        ],
        &source(),
    );
    code(&output, 0);
    let plan = records(&output).remove(0);
    assert_eq!(plan["feature_columns"], json!(COLUMNS));
    assert_eq!(plan["ensemble"]["min_agreement"], 0.75);
    let request: loupe_core::wire::Request =
        serde_json::from_value(plan["request"].clone()).unwrap();
    assert_eq!(request.questions.len(), 12);
    assert!(request.questions.contains_key("R2/ensemble:urgency/direct"));
    assert!(request.questions.contains_key("R1/signal:recovered"));
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_feature_ensembles_project_means_preserve_raw_answers_and_bind_resume() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = ensemble_questions(root);
    for pack in ["unit", "window:2"] {
        let journal = root.join(format!("{pack}.jsonl"));
        let mut args = vec![
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0.75",
            "--pack",
            pack,
            "--json",
            "--why",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
            "--failed",
            journal.to_str().unwrap(),
        ];
        let output = run(root, &config, &args, &source());
        code(&output, 0);
        let rows = records(&output);
        assert_eq!(rows.len(), 2);
        for (offset, row) in rows.iter().enumerate() {
            assert_eq!(row["ensembles"].as_object().unwrap().len(), 2);
            assert_eq!(row["features"].as_object().unwrap().len(), COLUMNS.len());
            for field in ["config_change", "urgency"] {
                let report = &row["ensembles"][field];
                let members = report["members"].as_array().unwrap();
                assert_eq!(members.len(), 2);
                assert_eq!(members[0]["answer"], row["answers"][field]);
                assert_eq!(report["min_agreement"], 0.75);
                assert_eq!(report["independent"], false);
                assert_eq!(report["calibrated"], false);
                let bound = if pack == "unit" {
                    field.to_owned()
                } else {
                    format!("R{}/{field}", offset + 1)
                };
                assert_eq!(members[0]["field"], json!(bound));
                if field == "config_change" {
                    let mean = members
                        .iter()
                        .map(|member| member["answer"]["noul"].as_f64().unwrap())
                        .sum::<f64>()
                        / 2.0;
                    assert!(
                        (row["features"]["p_config_change"].as_f64().unwrap() - mean).abs() < 1e-12
                    );
                } else {
                    let mut expectation = 0.0;
                    for level in 0..3 {
                        let mean = members
                            .iter()
                            .map(|member| {
                                let distribution =
                                    member["answer"]["probabilities"].as_object().unwrap();
                                let mass = distribution
                                    .values()
                                    .map(|value| value.as_f64().unwrap())
                                    .sum::<f64>();
                                distribution[&level.to_string()].as_f64().unwrap() / mass
                            })
                            .sum::<f64>()
                            / 2.0;
                        assert!(
                            (row["features"][format!("p_urgency_{level}")]
                                .as_f64()
                                .unwrap()
                                - mean)
                                .abs()
                                < 1e-12
                        );
                        expectation += level as f64 * mean;
                    }
                    assert!(
                        (row["features"]["s_urgency"].as_f64().unwrap() - expectation).abs()
                            < 1e-12
                    );
                }
            }
            assert_eq!(
                row["features"]["p_customer_impact"],
                row["answers"]["customer_impact"]["noul"]
            );
            assert!(row["signals"]["recovered"]["answer"].is_object());
        }
        let journal_flag = args.iter().position(|arg| *arg == "--failed").unwrap();
        args[journal_flag] = "--resume";
        args.extend(["--budget", "0"]);
        let resumed = run(root, &config, &args, &source());
        code(&resumed, 0);
        assert!(resumed.stdout.is_empty());
        assert!(String::from_utf8_lossy(&resumed.stderr).contains("http_attempts=0"));
        let gate = args.iter().position(|arg| *arg == "0.75").unwrap();
        args[gate] = "0.5";
        let changed = run(root, &config, &args, &source());
        code(&changed, 65);
        assert!(changed.stdout.is_empty());
    }
}

#[cfg(feature = "parquet")]
#[test]
#[ignore = "requires an actual LOUPE_TEST_URL endpoint"]
fn actual_server_parquet_schema_values_and_fatal_prefix_match_numeric_json() {
    server_parquet_matches_numeric_json(false);
}

#[cfg(feature = "parquet")]
#[test]
#[ignore = "requires an actual server endpoint; no fake backend"]
fn actual_server_ensemble_parquet_values_and_fatal_prefix_match_numeric_json() {
    server_parquet_matches_numeric_json(true);
}

#[cfg(feature = "parquet")]
fn server_parquet_matches_numeric_json(ensemble: bool) {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::RowAccessor;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = if ensemble {
        ensemble_questions(root)
    } else {
        setup(root)
    };
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let mut common = vec![
        "--url",
        &url,
        "featurize",
        "-f",
        questions.to_str().unwrap(),
        "--field",
        "id",
        "--strict",
        "--no-cache",
        "--deadline",
        "20s",
    ];
    if ensemble {
        common.push("--ensemble");
    }
    let mut args = common.to_vec();
    args.extend(["--json"]);
    let json = run(root, &config, &args, &source());
    code(&json, 0);
    let projected = records(&json);
    let mut args = common.to_vec();
    args.extend(["-O", "parquet"]);
    let encoded = run(root, &config, &args, &source());
    code(&encoded, 0);
    let path = root.join("features.parquet");
    std::fs::write(&path, &encoded.stdout).unwrap();
    let reader = SerializedFileReader::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(reader.metadata().file_metadata().num_rows(), 2);
    let rows = reader
        .get_row_iter(None)
        .unwrap()
        .map(|row| row.unwrap())
        .collect::<Vec<_>>();
    for (row, json) in rows.iter().zip(&projected) {
        assert_eq!(row.get_ulong(0).unwrap(), json["index"].as_u64().unwrap());
        assert_eq!(
            row.get_string(1).unwrap(),
            &json["row_id"]
                .as_str()
                .map_or_else(|| json["row_id"].to_string(), str::to_owned)
        );
        for (offset, column) in COLUMNS.iter().enumerate() {
            assert_eq!(
                row.get_double(offset + 2).unwrap(),
                json["features"][column].as_f64().unwrap()
            );
        }
    }
    let mut input = source();
    input.extend_from_slice(b"{invalid}\n");
    let failed = run(root, &config, &args, &input);
    code(&failed, 65);
    std::fs::write(&path, &failed.stdout).unwrap();
    let reader = SerializedFileReader::new(std::fs::File::open(path).unwrap()).unwrap();
    assert!(reader.metadata().file_metadata().num_rows() <= 2);
}
fn source() -> Vec<u8> {
    format!("{}\n{}\n",json!({"id":"é,\"a\"\nsecond", "text":"A configuration change broke checkout. Customers cannot pay."}),json!({"id":42,"text":"Checkout recovered; customers can pay normally."})).into_bytes()
}
const COLUMNS: &[&str] = &[
    "p_config_change",
    "p_customer_impact",
    "s_urgency",
    "p_urgency_0",
    "p_urgency_1",
    "p_urgency_2",
];

#[test]
fn multi_question_plans_keep_order_full_state_and_window_heads() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = setup(root);
    let input = source();
    let unit = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--field",
            "id",
            "--dry-run",
        ],
        &input,
    );
    code(&unit, 0);
    let plans = records(&unit);
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0]["feature_columns"], json!(COLUMNS));
    assert!(
        plans[0]["request"]["state"]["text"]
            .as_str()
            .unwrap()
            .contains("configuration")
    );
    assert_eq!(
        plans[0]["request"]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "config_change",
            "customer_impact",
            "urgency",
            "signal:recovered"
        ]
    );
    let window = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--field",
            "id",
            "--pack",
            "window:2",
            "--dry-run",
        ],
        &input,
    );
    code(&window, 0);
    let plans = records(&window);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0]["indices"], json!([1, 2]));
    let heads = plans[0]["request"]["questions"].as_object().unwrap();
    assert_eq!(heads.len(), 8);
    assert!(heads.contains_key("R2/urgency"));
    assert_eq!(plans[0]["feature_columns"], json!(COLUMNS));
    let preset_dir = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&preset_dir).unwrap();
    std::fs::copy(&questions, preset_dir.join("features.toml")).unwrap();
    let preset = run(
        root,
        &config,
        &["featurize", "-p", "features", "--dry-run"],
        &input,
    );
    code(&preset, 0);
    assert_eq!(records(&preset)[0]["request"], records(&unit)[0]["request"]);
    let one = root.join("one.json");
    std::fs::write(&one,json!({"questions":{"logical/impact":{"type":"noul","instructions":"Are customers affected?"}}}).to_string()).unwrap();
    let plan = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            one.to_str().unwrap(),
            "--pack",
            "window:2",
            "--dry-run",
            "-I",
            "lines",
        ],
        b"a\nb\n",
    );
    code(&plan, 0);
    assert_eq!(
        records(&plan)[0]["feature_columns"],
        json!(["p_logical/impact"])
    );
    assert!(
        records(&plan)[0]["request"]["questions"]
            .get("L1")
            .is_some()
    );
}

#[test]
fn incompatible_feature_sources_outputs_and_policy_flags_fail_before_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = setup(root);
    for extra in [
        vec!["-O", "plain"],
        vec!["-O", "annotate"],
        vec!["-O", "tsv"],
        vec!["--threshold", "0.7"],
        vec!["--on-uncertain", "keep"],
        vec!["--margin", "0.1"],
        vec!["--uncertain", "uncertain.txt"],
        vec!["--min-confidence", "0.5"],
        vec!["-c", "low,high"],
    ] {
        let mut args = vec![
            "--url",
            "http://127.0.0.1:1",
            "featurize",
            "-f",
            questions.to_str().unwrap(),
        ];
        args.extend(extra);
        let result = run(root, &config, &args, &source());
        code(&result, 64);
        assert!(result.stdout.is_empty());
    }
    let no_source = run(
        root,
        &config,
        &["--url", "http://127.0.0.1:1", "featurize", "question"],
        &source(),
    );
    code(&no_source, 64);
    let single = root.join("single.json");
    std::fs::write(
        &single,
        json!({"type":"noul","instructions":"test"}).to_string(),
    )
    .unwrap();
    code(
        &run(
            root,
            &config,
            &["featurize", "-f", single.to_str().unwrap(), "--dry-run"],
            &source(),
        ),
        64,
    );
    for document in [
        json!({"questions":{"label":{"type":"choice","criteria":{"a":"A"}}}}),
        json!({"questions":{"x":{"type":"score","criteria":["low","high"]},"x_0":{"type":"noul"}}}),
    ] {
        std::fs::write(&single, document.to_string()).unwrap();
        code(
            &run(
                root,
                &config,
                &["featurize", "-f", single.to_str().unwrap(), "--dry-run"],
                &source(),
            ),
            64,
        );
    }
    for command in ["keep", "find", "tokens"] {
        code(
            &run(
                root,
                &config,
                &[
                    "--url",
                    "http://127.0.0.1:1",
                    command,
                    "question",
                    "-O",
                    "csv",
                ],
                b"a\n",
            ),
            64,
        );
    }
    assert!(!root.join("uncertain.txt").exists());
}

#[test]
fn invalid_row_ids_and_global_indices_are_checked_in_actual_input_reading() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = setup(root);
    for id in [Value::Null, json!(true), json!(""), json!([]), json!({})] {
        let result = run(
            root,
            &config,
            &[
                "featurize",
                "-f",
                questions.to_str().unwrap(),
                "--field",
                "id",
                "--dry-run",
            ],
            format!("{}\n", json!({"id":id,"text":"test"})).as_bytes(),
        );
        code(&result, 65);
        assert!(result.stdout.is_empty());
    }
    let left = root.join("left.jsonl");
    let right = root.join("right.jsonl");
    std::fs::write(&left, b"{\"text\":\"a\"}\n").unwrap();
    std::fs::write(&right, b"{\"text\":\"b\"}\n").unwrap();
    let result = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--dry-run",
        ],
        b"",
    );
    code(&result, 0);
    let plans = records(&result);
    assert_eq!(
        plans
            .iter()
            .map(|p| p["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_feature_json_windows_and_csv_rows_preserve_actual_probability_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = setup(root);
    let input = source();
    let journal = root.join("features.jsonl");
    let result = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--field",
            "id",
            "--pack",
            "window:2",
            "--json",
            "--why",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
            "--failed",
            journal.to_str().unwrap(),
        ],
        &input,
    );
    code(&result, 0);
    let rows = records(&result);
    assert_eq!(rows.len(), 2);
    for (offset, row) in rows.iter().enumerate() {
        assert_eq!(row["index"], json!(offset + 1));
        assert_eq!(
            row["features"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            COLUMNS
        );
        assert_eq!(
            row["features"]["p_config_change"],
            row["answers"]["config_change"]["noul"]
        );
        assert_eq!(
            row["features"]["s_urgency"],
            row["answers"]["urgency"]["score"]
        );
        for level in 0..3 {
            assert_eq!(
                row["features"][format!("p_urgency_{level}")],
                row["answers"]["urgency"]["probabilities"][level.to_string()]
            );
        }
        assert_eq!(
            row["bindings"]["urgency"],
            json!(format!("R{}/urgency", offset + 1))
        );
        assert_eq!(
            row["signals"]["recovered"]["field"],
            json!(format!("R{}/signal:recovered", offset + 1))
        );
        assert_eq!(
            row["signals"]["recovered"]["probability"],
            row["signals"]["recovered"]["answer"]["noul"]
        );
    }
    assert_eq!(rows[0]["row_id"], json!("é,\"a\"\nsecond"));
    assert_eq!(rows[1]["row_id"], json!(42));
    assert!(
        rows[0]["features"]["p_customer_impact"].as_f64().unwrap()
            > rows[1]["features"]["p_customer_impact"].as_f64().unwrap()
    );
    let resume = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--field",
            "id",
            "--pack",
            "window:2",
            "--json",
            "--why",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
        ],
        &input,
    );
    code(&resume, 0);
    assert!(resume.stdout.is_empty());
    assert!(String::from_utf8_lossy(&resume.stderr).contains("http_attempts=0"));
    let simple = b"{\"text\":\"Checkout is down.\"}\n{\"text\":\"Checkout recovered.\"}\n";
    let journal = root.join("csv.jsonl");
    let csv = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
            "--failed",
            journal.to_str().unwrap(),
        ],
        simple,
    );
    code(&csv, 0);
    let text = String::from_utf8(csv.stdout).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[0],
        "index,row_id,p_config_change,p_customer_impact,s_urgency,p_urgency_0,p_urgency_1,p_urgency_2,error"
    );
    assert!(lines[1].starts_with("1,R1,"));
    assert!(lines[2].starts_with("2,R2,"));
    let resume = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            questions.to_str().unwrap(),
            "--strict",
            "--no-cache",
            "--deadline",
            "60s",
            "--resume",
            journal.to_str().unwrap(),
            "--budget",
            "0",
        ],
        simple,
    );
    code(&resume, 0);
    assert!(resume.stdout.is_empty());
    let one = root.join("one.json");
    std::fs::write(&one,json!({"questions":{"logical/impact":{"type":"noul","instructions":"Are customers unable to pay?"}}}).to_string()).unwrap();
    let result = run(
        root,
        &config,
        &[
            "featurize",
            "-f",
            one.to_str().unwrap(),
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
        b"Customers cannot pay.\nCustomers can pay.\n",
    );
    code(&result, 0);
    let rows = records(&result);
    assert_eq!(rows[0]["row_id"], json!("L1"));
    assert_eq!(rows[1]["bindings"]["logical/impact"], json!("L2"));
    assert_eq!(
        rows[1]["features"]["p_logical/impact"],
        rows[1]["answers"]["logical/impact"]["noul"]
    );
}

#[test]
#[ignore = "requires actual server System One model; no fake backend"]
fn server_feature_errors_keep_blank_columns_and_resume_only_the_missing_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, questions) = setup(root);
    let limited = std::fs::read_to_string(&config).unwrap().replace(
        "max_length = 16384",
        "max_length = 16384\nmax_state_tokens = 128",
    );
    std::fs::write(&config, limited).unwrap();
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"text":"Checkout is down."}),
        json!({"text":"large ".repeat(400)}),
        json!({"text":"Checkout recovered."})
    );
    let journal = root.join("partial.jsonl");
    let args = [
        "featurize",
        "-f",
        questions.to_str().unwrap(),
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let mut initial = args.to_vec();
    initial.extend(["--failed", journal.to_str().unwrap()]);
    let first = run(root, &config, &initial, input.as_bytes());
    code(&first, 65);
    let text = String::from_utf8(first.stdout).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().nth(1).unwrap().starts_with("1,R1,"));
    let mut resume = args.to_vec();
    resume.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
    let repeated = run(root, &config, &resume, input.as_bytes());
    code(&repeated, 65);
    assert!(repeated.stdout.is_empty());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("http_attempts=0"));
    let mut keep = args.to_vec();
    keep.extend(["--keep-going", "--on-error", "keep", "--json"]);
    let result = run(root, &config, &keep, input.as_bytes());
    code(&result, 0);
    let rows = records(&result);
    assert_eq!(rows.len(), 3);
    assert!(
        rows[1]["features"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null)
    );
    assert_eq!(rows[1]["error"]["code"], json!(65));
    assert!(rows[2]["answers"]["urgency"].is_object());
    let mut keep = args.to_vec();
    keep.extend(["--keep-going", "--on-error", "keep"]);
    let result = run(root, &config, &keep, input.as_bytes());
    code(&result, 0);
    let text = String::from_utf8(result.stdout).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(lines[2].starts_with("2,R2,,,,,,,"));
}
