#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const INPUT: &[u8] = b"Checkout is unavailable and customers cannot pay.\r\nCheckout is unavailable and customers cannot pay.\r\n";

fn setup(root: &Path, options: usize) -> (PathBuf, PathBuf) {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        include_str!("../../../examples/config.toml")
            .replace("concurrency = 4", "concurrency = 1")
            .replace("max_options = 512", &format!("max_options = {options}")),
    )
    .unwrap();
    let question = root.join("query.json");
    std::fs::write(&question, json!({"type":"noul","instructions":"Which line describes a current checkout outage?","variants":[{"id":"alt","instructions":"Which listed candidate describes checkout currently being unavailable?"}]}).to_string()).unwrap();
    (config, question)
}
fn run(root: &Path, config: &Path, args: &[&str], input: &[u8], live: bool) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "--url",
            if live {
                "http://127.0.0.1:8015"
            } else {
                "http://127.0.0.1:1"
            },
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
    child.stdin.take().unwrap().write_all(input).unwrap();
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
fn metric(output: &Output, key: &str) -> u64 {
    let prefix = format!("{key}=");
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap_or("0")
        .parse()
        .unwrap()
}

#[test]
fn exact_selection_plans_expand_all_heads_and_independent_score_levels() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, question) = setup(root, 512);
    let context = root.join("context.txt");
    std::fs::write(&context, "Use the current status only.").unwrap();
    let contextual = run(
        root,
        &config,
        &[
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--context",
            context.to_str().unwrap(),
            "--dry-run",
        ],
        INPUT,
        false,
    );
    code(&contextual, 0);
    let value = rows(&contextual).remove(0);
    let questions = &value["round"]["brackets"][0]["request"]["questions"];
    assert_eq!(
        questions["selection"]["instructions"]["context"],
        "Use the current status only."
    );
    assert_eq!(
        questions["ensemble:selection/alt"]["instructions"]["context"],
        "Use the current status only."
    );
    for independent in [false, true] {
        let mut args = vec![
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0.75",
            "--dry-run",
        ];
        if independent {
            args.push("--independent");
        }
        let output = run(root, &config, &args, INPUT, false);
        code(&output, 0);
        let value = rows(&output).remove(0);
        let bracket = if independent {
            &value["plan"]["items"][0]["bracket"]
        } else {
            &value["round"]["brackets"][0]
        };
        assert_eq!(bracket["ensemble_min_agreement"], 0.75);
        assert_eq!(bracket["counts"]["exact"], true);
        let request: loupe_core::wire::Request =
            serde_json::from_value(bracket["request"].clone()).unwrap();
        assert_eq!(request.questions.len(), if independent { 6 } else { 4 });
        assert_eq!(
            request.questions["selection"].options(),
            request.questions["ensemble:selection/alt"].options()
        );
        assert!(!request.questions.contains_key("ensemble:decision/alt"));
        if independent {
            assert_eq!(request.questions["selection-relevance"].options().len(), 3);
            assert_eq!(
                request.questions["ensemble:selection-relevance/alt"]
                    .options()
                    .len(),
                3
            );
        }
    }
    std::fs::write(&question, json!({"type":"score","instructions":"How directly does this answer the query?","criteria":["None","Related","Direct"],"variants":[{"id":"alt","instructions":"Rate the relevance to the question.","criteria":["Absent","Context","Answer"]}]}).to_string()).unwrap();
    let planned = run(
        root,
        &config,
        &[
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--independent",
            "--dry-run",
        ],
        INPUT,
        false,
    );
    code(&planned, 0);
    let value = rows(&planned).remove(0);
    let request = &value["plan"]["items"][0]["bracket"]["request"];
    assert_eq!(
        request["questions"]["selection-relevance"]["criteria"],
        json!(["None", "Related", "Direct"])
    );
    assert_eq!(
        request["questions"]["ensemble:selection-relevance/alt"]["criteria"],
        json!(["Absent", "Context", "Answer"])
    );
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_selection_means_drive_choice_and_independent_scores_without_external_decisions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, question) = setup(root, 512);
    for independent in [false, true] {
        let mut args = vec![
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0",
            "--on-uncertain",
            "keep",
            "-n",
            "2",
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ];
        if independent {
            args.push("--independent");
        }
        let output = run(root, &config, &args, INPUT, true);
        code(&output, 0);
        let values = rows(&output);
        assert_eq!(values.len(), 2);
        for row in &values {
            assert!(row.get("fallback").is_none());
            let field = if independent {
                "selection-relevance"
            } else {
                "selection"
            };
            let report = &row["ensembles"][field];
            assert_eq!(report["members"][0]["answer"], row["answer"]);
            assert_eq!(row["selection_decisions"][field]["source"], "ensemble");
            if independent {
                assert_eq!(row["score_calculation"], "equal_mean_level_probabilities");
                let mean = report["mean_probabilities"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(level, p)| level.parse::<usize>().unwrap() as f64 * p.as_f64().unwrap())
                    .sum::<f64>();
                assert!((row["ranking_score"].as_f64().unwrap() - mean).abs() < 1e-12);
            } else {
                assert_eq!(row["probability_source"], "ensemble");
                let candidate = row["id"].as_str().unwrap();
                let members = report["members"].as_array().unwrap();
                let mean = members
                    .iter()
                    .map(|member| {
                        let distribution = member["answer"]["probabilities"].as_object().unwrap();
                        let mass = distribution
                            .values()
                            .map(|value| value.as_f64().unwrap())
                            .sum::<f64>();
                        distribution[candidate].as_f64().unwrap() / mass
                    })
                    .sum::<f64>()
                    / members.len() as f64;
                assert!((row["probability"].as_f64().unwrap() - mean).abs() < 1e-12);
            }
        }
        if independent {
            assert!(
                values[0]["ranking_score"].as_f64().unwrap()
                    >= values[1]["ranking_score"].as_f64().unwrap()
            );
        } else {
            assert!(
                values[0]["probability"].as_f64().unwrap()
                    >= values[1]["probability"].as_f64().unwrap()
            );
        }
        assert_eq!(
            metric(&output, "input_counted"),
            metric(&output, "input_reported")
        );
        assert_eq!(metric(&output, "fallback_attempts"), 0);
    }
}

#[test]
#[ignore = "requires actual server model and real shell fallback; no fake backend"]
fn server_ensemble_tournament_resumes_partial_brackets_and_counts_every_variant() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, question) = setup(root, 3);
    let source = std::fs::read_to_string(&config)
        .unwrap()
        .replace("max_total_options = 2048", "max_total_options = 10");
    std::fs::write(&config, source).unwrap();
    let input = INPUT.repeat(3);
    let journal = root.join("overflow.jsonl");
    let command = "cat > /dev/null; printf '%s\\n' \"$LOUPE_QUESTION_ID\" >> calls; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection) python3 -c 'import json,os; q=json.loads(os.environ[\"LOUPE_QUESTION\"]); print(sorted(k for k in q[\"criteria\"] if k != \"none\")[0])';; *) exit 13;; esac";
    let base = vec![
        "pick",
        "-f",
        question.to_str().unwrap(),
        "--ensemble",
        "--margin",
        "1",
        "--min-confidence",
        "1",
        "--min-gap",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        command,
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let planned = run(
        root,
        &config,
        &[
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--dry-run",
        ],
        &input,
        false,
    );
    code(&planned, 0);
    let value = rows(&planned).remove(0);
    let brackets = value["round"]["brackets"].as_array().unwrap();
    assert_eq!(brackets.len(), 3);
    let budget = brackets[0]["counts"]["input_tokens"]
        .as_u64()
        .unwrap()
        .to_string();
    let mut args = base.clone();
    args.extend(["--failed", journal.to_str().unwrap(), "--budget", &budget]);
    let partial = run(root, &config, &args, &input, true);
    code(&partial, 4);
    assert!(partial.stdout.is_empty());
    assert_eq!(metric(&partial, "http_attempts"), 1);
    assert!(!root.join("calls").exists());
    let mut args = base;
    args.extend(["--resume", journal.to_str().unwrap()]);
    let output = run(root, &config, &args, &input, true);
    code(&output, 0);
    assert_eq!(metric(&output, "http_attempts"), 5);
    assert_eq!(
        metric(&output, "input_counted"),
        metric(&output, "input_reported")
    );
    let value = rows(&output).remove(0);
    assert_eq!(value["id"], "L1");
    let history = &value["provenance"]["history"];
    assert_eq!(history["plans"].as_array().unwrap().len(), 3);
    for plan in history["plans"].as_array().unwrap() {
        for bracket in plan["brackets"].as_array().unwrap() {
            assert_eq!(
                bracket["request"]["questions"].as_object().unwrap().len(),
                4
            );
            assert_eq!(bracket["ensemble_min_agreement"], 1.0);
        }
    }
    let calls = std::fs::read(root.join("calls")).unwrap();
    args.extend(["--budget", "0"]);
    let replay = run(root, &config, &args, &input, true);
    code(&replay, 0);
    assert!(replay.stdout.is_empty());
    assert_eq!(metric(&replay, "http_attempts"), 0);
    assert_eq!(std::fs::read(root.join("calls")).unwrap(), calls);
}

#[test]
#[ignore = "requires actual server model and real shell fallback; no fake backend"]
fn server_selection_ensemble_fallback_keeps_one_span_and_raw_mean_provenance_on_resume() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, question) = setup(root, 512);
    for verb in ["find", "pick", "extract"] {
        let (content, label, extra) = if verb == "extract" {
            (
                b"Either v1.2.3 or v4.5.6 introduced the regression.\r\n".as_slice(),
                "C2",
                vec!["--candidates", "semver", "--normalize"],
            )
        } else {
            (
                INPUT,
                "L2",
                if verb == "pick" {
                    vec!["-n", "2"]
                } else {
                    vec![]
                },
            )
        };
        if verb == "extract" {
            std::fs::write(&question,json!({"type":"noul","instructions":"Which version should be checked for the regression?","variants":[{"id":"alt","instructions":"Which version might have introduced the regression?"}]}).to_string()).unwrap();
        }
        let journal = root.join(format!("{verb}.jsonl"));
        let command = format!(
            "cat > /dev/null; printf '%s\\n' \"$LOUPE_ENSEMBLE_JSON\" >> {verb}-members; printf '%s\\n' \"$LOUPE_ANSWER_JSON\" >> {verb}-raw; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection) printf {label};; *) exit 13;; esac"
        );
        let mut args = vec![
            verb,
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--margin",
            "1",
            "--min-confidence",
            "1",
            "--min-gap",
            "1",
            "--on-uncertain",
            "fallback",
            "--fallback",
            &command,
            "--json",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
            "--failed",
            journal.to_str().unwrap(),
        ];
        args.extend(extra);
        let output = run(root, &config, &args, content, true);
        code(&output, 0);
        let value = rows(&output).remove(0);
        assert_eq!(value["id"], json!(label));
        assert!(value["probability"].is_null());
        assert_eq!(value["probability_source"], "fallback");
        assert_eq!(
            value["model_probability"],
            value["answer"]["probabilities"][label]
        );
        assert_eq!(value["ensembles"].as_object().unwrap().len(), 2);
        assert_eq!(
            value["selection_decisions"]["selection"]["source"],
            "fallback"
        );
        assert!(value["selection_decisions"]["selection"]["confidence"].is_null());
        for field in ["selection-present", "selection"] {
            let report = &value["ensembles"][field];
            assert_eq!(report["members"].as_array().unwrap().len(), 2);
            assert_eq!(report["min_agreement"], 1.0);
            assert_eq!(report["independent"], false);
            let raw =
                &value["provenance"]["history"]["evaluations"][0][0]["response"]["answers"][field];
            assert_eq!(report["members"][0]["answer"], *raw);
        }
        assert_eq!(
            metric(&output, "input_counted"),
            metric(&output, "input_reported")
        );
        assert_eq!(metric(&output, "fallback_attempts"), 2);
        let reports = std::fs::read(root.join(format!("{verb}-members"))).unwrap();
        let raw = std::fs::read(root.join(format!("{verb}-raw"))).unwrap();
        for (report, answer) in String::from_utf8_lossy(&reports)
            .lines()
            .zip(String::from_utf8_lossy(&raw).lines())
        {
            let report: Value = serde_json::from_str(report).unwrap();
            let answer: Value = serde_json::from_str(answer).unwrap();
            assert_eq!(report["members"][0]["answer"], answer);
        }
        let flag = args.iter().position(|arg| *arg == "--failed").unwrap();
        args[flag] = "--resume";
        args.extend(["--budget", "0"]);
        let replay = run(root, &config, &args, content, true);
        code(&replay, 0);
        assert!(replay.stdout.is_empty());
        assert_eq!(metric(&replay, "http_attempts"), 0);
        assert_eq!(
            std::fs::read(root.join(format!("{verb}-members"))).unwrap(),
            reports
        );
        args.extend(["--min-agreement", "0.5"]);
        code(&run(root, &config, &args, content, true), 65);
    }
}

#[test]
#[ignore = "requires actual server model and real shell fallback; no fake backend"]
fn server_independent_ensemble_scores_rank_external_values_and_resume_partial_responses() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, question) = setup(root, 512);
    let journal = root.join("independent.jsonl");
    let command = "IFS= read -r state; printf '%s\\n' \"$LOUPE_ENSEMBLE_JSON\" >> reports; case \"$LOUPE_QUESTION_ID\" in selection-present) printf yes;; selection-relevance) case \"$state\" in *L1*) printf 0.5;; *) printf 1.5;; esac;; selection) printf none;; *) exit 13;; esac";
    let base = vec![
        "pick",
        "-f",
        question.to_str().unwrap(),
        "--ensemble",
        "--independent",
        "-n",
        "2",
        "--margin",
        "1",
        "--min-confidence",
        "1",
        "--min-gap",
        "1",
        "--on-uncertain",
        "fallback",
        "--fallback",
        command,
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
        "--deadline",
        "60s",
    ];
    let planned = run(
        root,
        &config,
        &[
            "pick",
            "-f",
            question.to_str().unwrap(),
            "--ensemble",
            "--independent",
            "--dry-run",
        ],
        INPUT,
        false,
    );
    code(&planned, 0);
    let budget = rows(&planned)[0]["plan"]["items"][0]["bracket"]["counts"]["input_tokens"]
        .as_u64()
        .unwrap()
        .to_string();
    let mut first = base.clone();
    first.extend(["--failed", journal.to_str().unwrap(), "--budget", &budget]);
    let partial = run(root, &config, &first, INPUT, true);
    code(&partial, 4);
    assert!(partial.stdout.is_empty());
    assert_eq!(metric(&partial, "http_attempts"), 1);
    assert!(!root.join("reports").exists());
    let mut args = base;
    args.extend(["--resume", journal.to_str().unwrap()]);
    let completed = run(root, &config, &args, INPUT, true);
    code(&completed, 0);
    assert_eq!(metric(&completed, "http_attempts"), 1);
    let values = rows(&completed);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["id"], "L2");
    assert_eq!(values[0]["ranking_score"], 1.5);
    assert_eq!(values[1]["ranking_score"], 0.5);
    for row in values {
        assert_eq!(row["score_calculation"], "external_score");
        assert_eq!(row["ensembles"].as_object().unwrap().len(), 3);
        assert_eq!(
            row["selection_decisions"]["selection-relevance"]["source"],
            "fallback"
        );
        assert_eq!(
            row["ensembles"]["selection-relevance"]["members"][0]["answer"],
            row["answer"]
        );
    }
    assert_eq!(
        metric(&completed, "input_counted"),
        metric(&completed, "input_reported")
    );
    let reports = std::fs::read(root.join("reports")).unwrap();
    args.extend(["--budget", "0"]);
    let replay = run(root, &config, &args, INPUT, true);
    code(&replay, 0);
    assert!(replay.stdout.is_empty());
    assert_eq!(metric(&replay, "http_attempts"), 0);
    assert_eq!(std::fs::read(root.join("reports")).unwrap(), reports);
}
