#![allow(clippy::unwrap_used)]

use loupe_core::review::{Queue, Task};
use loupe_core::suites::Sampling;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn setup(root: &Path) {
    std::fs::write(
        root.join("config.toml"),
        include_str!("../../../examples/config.toml"),
    )
    .unwrap();
}
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
    command
        .current_dir(root)
        .args(["--config", "config.toml", "--profile", "server"])
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_PROFILE");
    command
}
fn run(root: &Path, args: &[&str]) -> Output {
    let configured = command(root);
    let mut command = Command::new("python");
    command.args(["-c","import subprocess,sys; p=subprocess.run(sys.argv[1:],stdin=subprocess.DEVNULL,capture_output=True,start_new_session=True); sys.stdout.buffer.write(p.stdout); sys.stderr.buffer.write(p.stderr); sys.exit(p.returncode)",env!("CARGO_BIN_EXE_loupe")]);
    command
        .args(configured.get_args())
        .args(args)
        .current_dir(root);
    for (key, value) in configured.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    command.output().unwrap()
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

fn retained_task(root: &Path) -> Task {
    let evidence: Value = serde_json::from_str(include_str!(
        "../../loupe-core/testdata/captured-response.json"
    ))
    .unwrap();
    let text = evidence["request"]["state"]
        .as_str()
        .map_or_else(|| evidence["request"]["state"].to_string(), str::to_owned);
    let mut unit = loupe_core::input::Reader::new(
        std::io::Cursor::new(text),
        loupe_core::input::InputFormat::Whole,
        None,
        false,
    )
    .next_unit()
    .unwrap()
    .unwrap();
    unit.state = evidence["request"]["state"].clone();
    let task = Task {
        id: String::new(),
        run: "a".repeat(64),
        preset: "outage".into(),
        unit,
        request: serde_json::from_value(evidence["request"].clone()).unwrap(),
        response: serde_json::from_value(evidence["response"].clone()).unwrap(),
        field: "outage".into(),
        encoder_identity: "retained-real-clef-response".into(),
        sampling: Sampling::UniformAudit,
    }
    .seal()
    .unwrap();
    let mut queue = Queue::open(&root.join("queue.jsonl"), true).unwrap();
    queue.insert(task.clone()).unwrap();
    task
}

#[test]
fn noninteractive_review_and_inapplicable_flags_fail_before_authentication_or_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    for args in [
        vec![
            "--url",
            "http://127.0.0.1:1",
            "keep",
            "question",
            "--review",
        ],
        vec!["review", "absent.jsonl"],
        vec!["tokens", "question", "--review=defer"],
        vec!["count", "question", "--review=defer"],
        vec!["keep", "question", "--review=defer", "--audit-rate", "NaN"],
        vec!["keep", "question", "--review=defer", "--dry-run"],
    ] {
        code(&run(root, &args), 64);
    }
    assert!(!root.join(".loupe").exists());
}

#[cfg(feature = "review")]
fn pty(root: &Path, args: &[String], action: &str) -> Value {
    let mut argv = vec![env!("CARGO_BIN_EXE_loupe").to_owned()];
    argv.extend_from_slice(args);
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/review_pty.py");
    let output = Command::new("python")
        .args([
            worker.to_str().unwrap(),
            &serde_json::to_string(&argv).unwrap(),
            root.to_str().unwrap(),
            action,
        ])
        .output()
        .unwrap();
    code(&output, 0);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(feature = "review")]
#[test]
fn actual_terminal_review_captures_once_and_restores_on_keys_and_sigterm() {
    for (action, status) in [("79", 0), ("73", 0), ("03", 130), ("TERM", 143)] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        setup(root);
        let task = retained_task(root);
        let arguments = vec![
            "review".into(),
            "queue.jsonl".into(),
            "--labels-dir".into(),
            root.join("labels").to_str().unwrap().into(),
        ];
        let response = pty(root, &arguments, action);
        assert_eq!(response["exit_code"], status, "{response}");
        assert_eq!(response["terminal_restored"], true);
        let labels = root.join("labels/outage.jsonl");
        if action == "79" {
            let output: Value = serde_json::from_str(response["stdout"].as_str().unwrap()).unwrap();
            assert_eq!(output["decision"]["source"], "review");
            assert!(output["decision"]["confidence"].is_null());
            assert_eq!(output["answer"], task.response.answers["outage"]);
            let case: loupe_core::suites::Case =
                serde_json::from_str(&std::fs::read_to_string(&labels).unwrap()).unwrap();
            assert_eq!(case.source, "review");
            assert_eq!(case.sampling, Some(Sampling::UniformAudit));
            assert_eq!(
                Queue::open(&root.join("queue.jsonl"), false)
                    .unwrap()
                    .pending()
                    .count(),
                0
            );
            let second = pty(root, &arguments, "NONE");
            assert_eq!(second["exit_code"], 0);
            assert_eq!(second["stdout"], "");
            assert_eq!(std::fs::read_to_string(labels).unwrap().lines().count(), 1);
        } else if status != 0 {
            assert_eq!(
                Queue::open(&root.join("queue.jsonl"), false)
                    .unwrap()
                    .pending()
                    .count(),
                1
            );
            assert!(!labels.exists() || std::fs::read_to_string(labels).unwrap().is_empty());
        }
    }
}

#[cfg(feature = "review")]
#[test]
#[ignore = "requires the actual LOUPE_TEST_URL endpoint"]
fn actual_server_pipe_reviews_only_tty_keys_and_deferred_resume_reuses_answers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let args = vec![
        "--config".into(),
        "config.toml".into(),
        "--profile".into(),
        "server".into(),
        "--url".into(),
        url.clone(),
        "keep".into(),
        "-p".into(),
        "real-failure".into(),
        "--margin".into(),
        "0.49".into(),
        "--review".into(),
        "--review-queue".into(),
        "inline.jsonl".into(),
        "--review-labels".into(),
        "labels".into(),
        "--audit-rate".into(),
        "100%".into(),
        "--json".into(),
        "--strict".into(),
        "--no-cache".into(),
        "--deadline".into(),
        "20s".into(),
    ];
    let response = pty(root, &args, "79");
    assert_eq!(response["exit_code"], 0, "{response}");
    let row: Value = serde_json::from_str(response["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(
        row["unit"],
        "Checkout is down. Customers cannot place orders."
    );
    assert_eq!(row["decision"]["source"], "review");
    let deferred = [
        "--url",
        &url,
        "keep",
        "-p",
        "real-failure",
        "--margin",
        "0.49",
        "--review=defer",
        "--review-queue",
        "deferred.jsonl",
        "--review-labels",
        "deferred-labels",
        "--audit-rate",
        "100%",
        "--failed",
        "run.jsonl",
        "--strict",
        "--no-cache",
    ];
    let mut child = command(root)
        .args(deferred)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down.\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    code(&output, 1);
    assert_eq!(
        Queue::open(&root.join("deferred.jsonl"), false)
            .unwrap()
            .pending()
            .count(),
        1
    );
    let before = std::fs::read(root.join("deferred.jsonl")).unwrap();
    let mut resumed = deferred.to_vec();
    let position = resumed
        .iter()
        .position(|value| *value == "--failed")
        .unwrap();
    resumed[position] = "--resume";
    resumed.extend(["--budget", "0"]);
    let mut child = command(root)
        .args(resumed)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Checkout is down.\n")
        .unwrap();
    code(&child.wait_with_output().unwrap(), 1);
    assert_eq!(std::fs::read(root.join("deferred.jsonl")).unwrap(), before);
}
