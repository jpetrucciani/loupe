#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn setup(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        include_str!("../../../examples/config.toml").replace("concurrency = 4", "concurrency = 1"),
    )
    .unwrap();
    let join = root.join("join.json");
    std::fs::write(&join, json!({"questions":{
        "same_entity":{"type":"noul","instructions":"Are the names in left.record and right.record identical?"},
        "similarity":{"type":"score","instructions":"How similar are the two names?","criteria":["Different","Related","Identical"]}},
        "variants":[{"id":"alt","field":"same_entity","instructions":"Do both records have exactly the same name?"},{"id":"alt","field":"similarity","instructions":"Rate agreement between the names."}]}).to_string()).unwrap();
    let dedupe = root.join("dedupe.json");
    std::fs::write(&dedupe, json!({"type":"noul","instructions":"Are left.record and right.record identical?","variants":[{"id":"alt","instructions":"Do both records contain the same incident label?"}]}).to_string()).unwrap();
    (config, join, dedupe)
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
fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn assert_mean(report: &Value, response: &Value, field: &str) {
    assert_eq!(report["members"][0]["answer"], response["answers"][field]);
    let members = report["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    for (option, mean) in report["mean_probabilities"].as_object().unwrap() {
        let expected = members
            .iter()
            .map(|member| member["probabilities"][option].as_f64().unwrap())
            .sum::<f64>()
            / 2.0;
        assert!((mean.as_f64().unwrap() - expected).abs() < 1e-12);
    }
}

#[test]
fn pair_ensemble_plans_include_all_heads_context_and_exact_counts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, join, dedupe) = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, b"{\"name\":\"ALPHA\"}\n").unwrap();
    let context = root.join("context.txt");
    std::fs::write(&context, "Compare names only.").unwrap();
    let planned = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "-f",
            join.to_str().unwrap(),
            "--ensemble",
            "--context",
            context.to_str().unwrap(),
            "--dry-run",
        ],
        b"{\"name\":\"ALPHA\"}\n",
        false,
    );
    code(&planned, 0);
    let pair = rows(&planned).remove(0)["candidates"][0]["pair_plan"].clone();
    assert_eq!(pair["counts"]["exact"], true);
    assert_eq!(pair["request"]["questions"].as_object().unwrap().len(), 4);
    for question in pair["request"]["questions"].as_object().unwrap().values() {
        assert_eq!(question["instructions"]["context"], "Compare names only.");
    }
    let planned = run(
        root,
        &config,
        &[
            "dedupe",
            "-f",
            dedupe.to_str().unwrap(),
            "--ensemble",
            "--dry-run",
        ],
        b"ALPHA\nALPHA\n",
        false,
    );
    code(&planned, 0);
    let pair = rows(&planned).remove(0);
    assert_eq!(pair["counts"]["exact"], true);
    assert_eq!(pair["request"]["questions"].as_object().unwrap().len(), 2);
    for args in [
        vec![
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "--ensemble",
            "--dry-run",
        ],
        vec!["dedupe", "same?", "--ensemble", "--dry-run"],
        vec![
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "-f",
            join.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "1.1",
            "--dry-run",
        ],
    ] {
        code(&run(root, &config, &args, b"", false), 64);
    }
}

#[test]
#[ignore = "requires actual server model; no fake backend"]
fn server_pair_means_drive_join_ranking_and_finite_streaming_dedupe() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (config, join, dedupe) = setup(root);
    let right = root.join("right.jsonl");
    std::fs::write(&right, b"{\"name\":\"ALPHA\"}\n").unwrap();
    let output = run(
        root,
        &config,
        &[
            "join",
            "-",
            right.to_str().unwrap(),
            "--on",
            "name",
            "-f",
            join.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0",
            "--threshold",
            "0",
            "--on-uncertain",
            "keep",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ],
        b"{\"name\":\"ALPHA\"}\n",
        true,
    );
    code(&output, 0);
    let pair = rows(&output).remove(0)["evaluated_pairs"][0].clone();
    for field in ["same_entity", "similarity"] {
        assert_mean(
            &pair["decision"]["ensembles"][field],
            &pair["response"],
            field,
        );
    }
    let expected = pair["decision"]["ensembles"]["similarity"]["mean_probabilities"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(level, p)| level.parse::<usize>().unwrap() as f64 * p.as_f64().unwrap() / 2.0)
        .sum::<f64>();
    assert!((pair["decision"]["ranking_score"].as_f64().unwrap() - expected).abs() < 1e-12);
    assert_eq!(
        pair["decision"]["probabilities"],
        pair["response"]["answers"]["similarity"]["probabilities"]
    );
    assert_eq!(
        metric(&output, "input_counted"),
        metric(&output, "input_reported")
    );
    for streaming in [false, true] {
        let journal = root.join(if streaming {
            "stream.jsonl"
        } else {
            "finite.jsonl"
        });
        let mut args = vec![
            "dedupe",
            "-f",
            dedupe.to_str().unwrap(),
            "--ensemble",
            "--min-agreement",
            "0",
            "--threshold",
            "0",
            "--on-uncertain",
            "keep",
            "--strict",
            "--no-cache",
            "--summary",
            "--json",
            "--deadline",
            "60s",
            "--failed",
            journal.to_str().unwrap(),
        ];
        if streaming {
            args.extend(["--window", "2"]);
        }
        let output = run(root, &config, &args, b"ALPHA\nALPHA\n", true);
        code(&output, 0);
        let values = rows(&output);
        assert_eq!(values.len(), 1);
        let pair = if streaming {
            lines(&journal)
                .iter()
                .find_map(|frame| {
                    let r = &frame["record"];
                    (r["record"] == "ready" && r["index"] == 2)
                        .then(|| r["completed"]["row"]["pairs"][0].clone())
                })
                .unwrap()
        } else {
            values[0]["links"][0].clone()
        };
        assert_eq!(pair["decision"]["source"], "ensemble");
        assert_eq!(pair["decision"]["selected"], json!({"boolean":true}));
        assert_eq!(pair["ensemble"]["members"].as_array().unwrap().len(), 2);
        assert_eq!(metric(&output, "http_attempts"), 1);
        assert_eq!(
            metric(&output, "input_counted"),
            metric(&output, "input_reported")
        );
    }
}

#[test]
#[ignore = "requires actual server model and real external commands; no fake backend"]
fn server_pair_ensemble_fallback_preserves_raw_mean_and_completed_replay() {
    for join_mode in [true, false] {
        for streaming in [false, true] {
            if join_mode && streaming {
                continue;
            }
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path();
            let (config, join, dedupe) = setup(root);
            let right = root.join("right.jsonl");
            std::fs::write(&right, b"{\"name\":\"ALPHA\"}\n").unwrap();
            let journal = root.join("run.jsonl");
            let fallback = r#"cat > /dev/null; printf '%s\n' "$LOUPE_ENSEMBLE_JSON" >> means.jsonl; printf '%s\n' "$LOUPE_ANSWER_JSON" >> answers.jsonl; printf '%s\n' "$LOUPE_QUESTION_ID" >> calls; if [ "$LOUPE_QUESTION_ID" = similarity ]; then printf 1.5; else printf yes; fi"#;
            let mut base = if join_mode {
                vec![
                    "join",
                    "-",
                    right.to_str().unwrap(),
                    "--on",
                    "name",
                    "-f",
                    join.to_str().unwrap(),
                    "--min-confidence",
                    "1",
                    "--min-gap",
                    "1",
                ]
            } else {
                vec!["dedupe", "-f", dedupe.to_str().unwrap(), "--json"]
            };
            base.extend([
                "--ensemble",
                "--margin",
                "1",
                "--on-uncertain",
                "fallback",
                "--fallback",
                fallback,
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
            ]);
            if streaming {
                base.extend(["--window", "2"]);
            }
            let input: &[u8] = if join_mode {
                b"{\"name\":\"ALPHA\"}\n"
            } else {
                b"ALPHA\nALPHA\n"
            };
            let mut args = base.clone();
            args.extend(["--failed", journal.to_str().unwrap()]);
            let output = run(root, &config, &args, input, true);
            code(&output, 0);
            let means = lines(&root.join("means.jsonl"));
            let answers = lines(&root.join("answers.jsonl"));
            assert_eq!(means.len(), if join_mode { 2 } else { 1 });
            for (mean, answer) in means.iter().zip(&answers) {
                assert_eq!(mean["members"][0]["answer"], *answer);
                assert_eq!(mean["members"].as_array().unwrap().len(), 2);
            }
            if join_mode {
                let pair = rows(&output).remove(0)["evaluated_pairs"][0].clone();
                assert_eq!(pair["decision"]["ranking_score"], 0.75);
                assert_eq!(pair["decision"]["eligible"], true);
                assert_eq!(
                    pair["decision"]["derived"]["same_entity"]["uncertain"],
                    true
                );
                assert_eq!(
                    pair["decision"]["probabilities"],
                    pair["response"]["answers"]["similarity"]["probabilities"]
                );
            } else {
                assert_eq!(rows(&output).len(), 1);
            }
            assert_eq!(
                metric(&output, "input_counted"),
                metric(&output, "input_reported")
            );
            let calls = std::fs::read(root.join("calls")).unwrap();
            let mut args = base.clone();
            args.extend(["--resume", journal.to_str().unwrap(), "--budget", "0"]);
            let replay = run(root, &config, &args, input, true);
            code(&replay, 0);
            assert!(replay.stdout.is_empty());
            assert_eq!(metric(&replay, "http_attempts"), 0);
            assert_eq!(metric(&replay, "fallback_attempts"), 0);
            assert_eq!(std::fs::read(root.join("calls")).unwrap(), calls);
            args.extend(["--min-agreement", "0.5"]);
            code(&run(root, &config, &args, input, true), 65);
            assert_eq!(std::fs::read(root.join("calls")).unwrap(), calls);
        }
    }
}
