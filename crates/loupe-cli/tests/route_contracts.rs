#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const QUESTION: &str = "Route service outages to page and harmless heartbeats to ticket.";
fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
        ])
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
fn rows(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path.join(".loupe-route.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn summary(output: &Output) -> BTreeMap<String, String> {
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    let line = stderr
        .lines()
        .find(|line| line.starts_with("loupe: units="))
        .unwrap();
    line[7..]
        .split_whitespace()
        .map(|field| {
            let (key, value) = field.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect()
}
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
fn route_offline_plans_preserve_shared_packing_and_do_not_create_files_or_execute_processes() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("absent");
    let marker = root.path().join("never-executed");
    let command = format!("page=touch {}", quote(&marker));
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out.to_str().unwrap(),
            "--to",
            &command,
            "--pack",
            "window:4",
            "--dry-run",
            "--truncate",
        ],
        b"Checkout is down.\nHeartbeat complete.\n",
    );
    assert_success(&output);
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["indices"], serde_json::json!([1, 2]));
    assert_eq!(plan["route"]["reset"], true);
    assert_eq!(plan["route"]["labels"]["names"]["page"], "page");
    assert!(!out.exists());
    assert!(!marker.exists());
    let focus = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out.to_str().unwrap(),
            "--focus",
            "1",
            "--truncate",
            "--dry-run",
        ],
        b"Checkout is down.\n",
    );
    assert_success(&focus);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&focus.stdout)
            .unwrap()
            .get("focus")
            .is_some()
    );
    assert!(!out.exists());
}
#[test]
fn route_grammar_rejects_inapplicable_destinations_and_preserves_text_truncation_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("absent");
    let out = out.to_str().unwrap();
    let resume = root.path().join("elsewhere/.loupe-route.jsonl");
    for flags in [
        vec!["route", QUESTION, "-c", "page,ticket"],
        vec!["route", QUESTION, "-c", "PAGE,page", "--out", out],
        vec!["route", QUESTION, "-c", "page,ticket", "--to", "page=cat"],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--to",
            "unknown=cat",
        ],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--to",
            "page=",
        ],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--to",
            "page=cat",
            "--to",
            "page=cat",
        ],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--to-timeout",
            "1s",
        ],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--resume",
            resume.to_str().unwrap(),
        ],
        vec![
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out,
            "--uncertain",
            resume.to_str().unwrap(),
        ],
        vec!["keep", "question", "--focus", "1", "--truncate", "head"],
    ] {
        let output = run(&flags, b"");
        assert_eq!(
            output.status.code(),
            Some(64),
            "{flags:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!Path::new(out).exists());
}
#[test]
fn route_real_symlinks_and_input_aliases_are_denied_before_reset_or_model_submission() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"preserve\n").unwrap();
    symlink(&outside, out.join("page")).unwrap();
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out.to_str().unwrap(),
            "--truncate",
            "--budget",
            "0",
            "--no-cache",
        ],
        b"Checkout is down.\n",
    );
    assert_eq!(output.status.code(), Some(74));
    assert_eq!(std::fs::read(&outside).unwrap(), b"preserve\n");
    std::fs::remove_file(out.join("page")).unwrap();
    std::fs::write(out.join("page"), b"source\n").unwrap();
    let source = out.join("page");
    let output = run(
        &[
            "--url",
            "http://127.0.0.1:1",
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--out",
            out.to_str().unwrap(),
            "--truncate",
            "--budget",
            "0",
            "--no-cache",
            source.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(64));
    assert_eq!(std::fs::read(source).unwrap(), b"source\n");
    assert!(!out.join(".loupe-route.journal").exists());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_appends_resets_and_replays_exact_file_decisions_without_inference() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    let path = out.to_str().unwrap();
    let input: Vec<_> = (1..=9)
        .map(|index| format!("Checkout is down and customers cannot pay. Event {index}.\r\n"))
        .collect();
    let data = input.concat();
    let args = [
        "--url",
        &url,
        "route",
        QUESTION,
        "-c",
        "page,ticket",
        "--out",
        path,
        "--strict",
        "--no-cache",
        "--summary",
    ];
    let first = run(&args, data.as_bytes());
    assert_success(&first);
    assert!(first.stdout.is_empty());
    let observed = rows(&out);
    assert_eq!(observed.len(), 9);
    let metrics = summary(&first);
    assert_eq!(metrics["logical_records"], "9");
    assert_eq!(metrics["input_counted"], metrics["input_reported"]);
    let mut expected = BTreeMap::<String, String>::new();
    for (index, row) in observed.iter().enumerate() {
        assert_eq!(row["index"], index + 1);
        let name = row["target"]["name"].as_str().unwrap();
        assert!(matches!(name, "page" | "ticket"));
        expected
            .entry(name.to_owned())
            .or_default()
            .push_str(&input[index]);
    }
    for (name, bytes) in &expected {
        assert_eq!(std::fs::read(out.join(name)).unwrap(), bytes.as_bytes());
    }
    let journal = std::fs::read(out.join(".loupe-route.journal")).unwrap();
    let manifest = out.join(".loupe-route.jsonl");
    let mut resume = args.to_vec();
    resume.extend(["--resume", manifest.to_str().unwrap(), "--budget", "0"]);
    let replay = run(&resume, data.as_bytes());
    assert_success(&replay);
    assert_eq!(summary(&replay)["attempts"], "0");
    assert_eq!(summary(&replay)["logical_records"], "0");
    assert_eq!(
        std::fs::read(out.join(".loupe-route.journal")).unwrap(),
        journal
    );
    assert_eq!(rows(&out), observed);
    let changed = run(&resume, b"changed input\n");
    assert_eq!(changed.status.code(), Some(65));
    assert_eq!(summary(&changed)["attempts"], "0");
    let short = run(&resume, input[0].as_bytes());
    assert_eq!(short.status.code(), Some(65));
    let extra = run(&resume, (data.clone() + "extra\n").as_bytes());
    assert_eq!(extra.status.code(), Some(65));
    let append = run(&args, data.as_bytes());
    assert_success(&append);
    assert_eq!(rows(&out).len(), 18);
    std::fs::write(out.join("unrelated.keep"), b"keep\n").unwrap();
    let mut reset = args.to_vec();
    reset.push("--truncate");
    let reset = run(&reset, data.as_bytes());
    assert_success(&reset);
    assert_eq!(rows(&out).len(), 9);
    assert_eq!(rows(&out)[0]["run"], 1);
    assert_eq!(
        std::fs::read(out.join("unrelated.keep")).unwrap(),
        b"keep\n"
    );
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_recovers_failed_prefix_and_rejects_changed_policy_before_new_inference() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    let first = b"Checkout is down and customers cannot pay.\r\n";
    let input = [first.as_slice(), &b"word ".repeat(20_000), b"\n"].concat();
    let args = [
        "--url",
        &url,
        "-j",
        "1",
        "route",
        QUESTION,
        "-c",
        "page,ticket",
        "--out",
        out.to_str().unwrap(),
        "--strict",
        "--no-cache",
        "--summary",
    ];
    let failed = run(&args, &input);
    assert_eq!(failed.status.code(), Some(65));
    assert_eq!(rows(&out).len(), 1);
    let label = rows(&out)[0]["target"]["name"].as_str().unwrap().to_owned();
    assert_eq!(std::fs::read(out.join(label)).unwrap(), first);
    let manifest = out.join(".loupe-route.jsonl");
    let mut resume = args.to_vec();
    resume.extend(["--resume", manifest.to_str().unwrap()]);
    let mut changed = resume.clone();
    changed.extend(["--min-confidence", "0.75"]);
    let policy = run(&changed, first);
    assert_eq!(policy.status.code(), Some(65));
    assert_eq!(summary(&policy)["attempts"], "0");
    let repaired = [first.as_slice(), b"A harmless heartbeat completed.\r\n"].concat();
    let resumed = run(&resume, &repaired);
    assert_success(&resumed);
    assert_eq!(summary(&resumed)["logical_records"], "1");
    assert_eq!(rows(&out).len(), 2);
    assert_eq!(rows(&out)[1]["index"], 2);
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_csv_headers_and_uncertainty_capture_are_part_of_durable_payloads() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    let csv =
        b"message,id\r\n\"Checkout is down\",one\r\n\"A harmless heartbeat completed\",two\r\n";
    let args = [
        "--url",
        &url,
        "route",
        QUESTION,
        "-c",
        "page,ticket",
        "--out",
        out.to_str().unwrap(),
        "-I",
        "csv",
        "--strict",
        "--no-cache",
        "--summary",
    ];
    assert_success(&run(&args, csv));
    let observed = rows(&out);
    assert_eq!(observed.len(), 2);
    let mut expected = BTreeMap::<String, Vec<u8>>::new();
    for (row, record) in observed.iter().zip([
        b"\"Checkout is down\",one\r\n".as_slice(),
        b"\"A harmless heartbeat completed\",two\r\n",
    ]) {
        let label = row["target"]["name"].as_str().unwrap();
        expected
            .entry(label.to_owned())
            .or_insert_with(|| b"message,id\r\n".to_vec())
            .extend(record);
    }
    for (label, data) in &expected {
        assert_eq!(std::fs::read(out.join(label)).unwrap(), *data);
    }
    let manifest = out.join(".loupe-route.jsonl");
    let mut resume = args.to_vec();
    resume.extend(["--resume", manifest.to_str().unwrap(), "--budget", "0"]);
    assert_success(&run(&resume, csv));
    for (label, data) in &expected {
        assert_eq!(std::fs::read(out.join(label)).unwrap(), *data);
    }
    let uncertain = root.path().join("uncertain");
    let capture = uncertain.join("uncertain");
    let forced = run(
        &[
            "--url",
            &url,
            "route",
            "Choose either similar option.",
            "-c",
            "page,ticket",
            "--min-gap",
            "1",
            "--out",
            uncertain.to_str().unwrap(),
            "--uncertain",
            capture.to_str().unwrap(),
            "--strict",
            "--json",
            "--no-cache",
        ],
        b"This is ambiguous between both choices.\r\n",
    );
    assert_success(&forced);
    assert_eq!(rows(&uncertain)[0]["label"], "uncertain");
    assert_eq!(
        std::fs::read(capture).unwrap(),
        b"This is ambiguous between both choices.\r\n"
    );
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_real_process_sinks_stream_once_and_report_nonzero_exit_without_retry() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let page = root.path().join("page");
    let ticket = root.path().join("ticket");
    let page_starts = root.path().join("page-starts");
    let ticket_starts = root.path().join("ticket-starts");
    let to_page = format!(
        "page=printf x >> {}; cat > {}",
        quote(&page_starts),
        quote(&page)
    );
    let to_ticket = format!(
        "ticket=printf x >> {}; cat > {}",
        quote(&ticket_starts),
        quote(&ticket)
    );
    let input = b"Checkout is down and customers cannot pay.\r\nCheckout is down and customers cannot pay.\r\n";
    let result = run(
        &[
            "--url",
            &url,
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--to",
            &to_page,
            "--to",
            &to_ticket,
            "--strict",
            "--no-cache",
            "--to-timeout",
            "2s",
        ],
        input,
    );
    assert_success(&result);
    let mut actual = Vec::new();
    for (output, starts) in [(&page, &page_starts), (&ticket, &ticket_starts)] {
        if output.exists() {
            actual.extend(std::fs::read(output).unwrap());
            assert_eq!(std::fs::read(starts).unwrap(), b"x");
        }
    }
    assert_eq!(actual, input);
    let fail_page = "page=cat >/dev/null; exit 7";
    let fail_ticket = "ticket=cat >/dev/null; exit 7";
    let failed = run(
        &[
            "--url",
            &url,
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--to",
            fail_page,
            "--to",
            fail_ticket,
            "--strict",
            "--no-cache",
            "--summary",
            "--to-timeout",
            "2s",
        ],
        &input[..input.len() / 2],
    );
    assert_eq!(failed.status.code(), Some(69));
    assert_eq!(summary(&failed)["logical_records"], "1");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("7"));
}
#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_sigterm_during_process_shutdown_cancels_owned_descendants_promptly() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let started = root.path().join("started");
    let leaked = root.path().join("leaked");
    let delayed = format!("sleep 0.5; printf leaked > {}", quote(&leaked));
    let command = format!(
        "printf ready > {}; sh -c '{}' & cat >/dev/null; sleep 30",
        quote(&started),
        delayed.replace('\'', "'\\''")
    );
    let page = format!("page={command}");
    let ticket = format!("ticket={command}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .args([
            "--config",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
            "--profile",
            "server",
            "--url",
            &url,
            "route",
            QUESTION,
            "-c",
            "page,ticket",
            "--to",
            &page,
            "--to",
            &ticket,
            "--to-timeout",
            "10s",
            "--summary",
            "--no-cache",
        ])
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down and customers cannot pay.\n")
        .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !started.exists() && std::time::Instant::now() < until {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(started.exists(), "process did not start");
    let started_cancel = std::time::Instant::now();
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(143));
    assert!(started_cancel.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(summary(&output)["succeeded_records"], "1");
    std::thread::sleep(std::time::Duration::from_millis(550));
    assert!(!leaked.exists());
}

#[test]
#[ignore = "requires real LOUPE_TEST_URL System One service"]
fn server_route_joint_windows_keep_json_shapes_and_replay_without_repacking_inference() {
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let root = tempfile::tempdir().unwrap();
    let window = root.path().join("window-json");
    let input: Vec<_> = (1..=9)
        .map(|index| format!("Checkout is down and customers cannot pay. Event {index}.\r\n"))
        .collect();
    let data = input.concat();
    let args = [
        "--url",
        &url,
        "route",
        QUESTION,
        "-c",
        "page,ticket",
        "--out",
        window.to_str().unwrap(),
        "--pack",
        "window:4",
        "--json",
        "--strict",
        "--no-cache",
        "--summary",
    ];
    let classified = run(&args, data.as_bytes());
    assert_success(&classified);
    let counts = summary(&classified);
    assert_eq!(counts["logical_records"], "3");
    assert_eq!(counts["input_counted"], counts["input_reported"]);
    let observed = rows(&window);
    assert_eq!(observed.len(), 9);
    let mut by_index = BTreeMap::new();
    for label in ["page", "ticket"] {
        if let Ok(file) = std::fs::read_to_string(window.join(label)) {
            for line in file.lines() {
                let unit: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(unit["answer"]["choice"], label);
                assert_eq!(unit["cached"], false);
                by_index.insert(unit["index"].as_u64().unwrap(), unit);
            }
        }
    }
    assert_eq!(by_index.len(), 9);
    for (index, original) in input.iter().enumerate() {
        assert_eq!(
            by_index[&(index as u64 + 1)]["unit"],
            original.trim_end_matches(['\r', '\n'])
        );
    }
    let manifest = window.join(".loupe-route.jsonl");
    let mut replay = args.to_vec();
    replay.extend(["--resume", manifest.to_str().unwrap(), "--budget", "0"]);
    let replay = run(&replay, data.as_bytes());
    assert_success(&replay);
    assert_eq!(summary(&replay)["attempts"], "0");
    assert_eq!(rows(&window), observed);
}
