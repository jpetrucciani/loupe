#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], text: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args(["--config", "config.toml", "--profile", "server"])
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_API_KEY")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn setup(root: &Path) {
    std::fs::write(
        root.join("config.toml"),
        include_str!("../../../examples/config.toml"),
    )
    .unwrap();
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

#[test]
fn invalid_normalization_configuration_fails_before_credentials_or_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let mut config = std::fs::read_to_string(root.join("config.toml")).unwrap();
    config.push_str("\n[profile.server-auth]\nbase_url='http://127.0.0.1:1'\napi_key_cmd='touch forbidden; printf secret'\n");
    std::fs::write(root.join("config.toml"), config).unwrap();
    for (kind, flags) in [
        ("money", vec!["--now", "invalid"]),
        ("dates", vec!["--tz", "Not/AZone"]),
        ("semver", vec!["--currency", "USD"]),
        ("money", vec!["--currency", "ZZZ"]),
        ("dates", vec!["--now", "2026-10-06"]),
    ] {
        let mut args = vec![
            "--profile",
            "server-auth",
            "extract",
            "Question",
            "--candidates",
            kind,
            "--normalize",
        ];
        args.extend(flags);
        code(&run(root, &args, "source"), 64);
    }
    assert!(!root.join("forbidden").exists());
}

#[test]
#[ignore = "requires an actual LOUPE_TEST_URL endpoint"]
fn actual_server_normalizes_selected_spans_and_preserves_original_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    for (kind, text, query, flags, expected) in [
        (
            "dates",
            "Release date: tomorrow.\n",
            "Which date is the release date?",
            vec![
                "--now",
                "2026-10-05T23:30:00-04:00",
                "--tz",
                "America/New_York",
            ],
            json!("2026-10-06T00:00:00-04:00"),
        ),
        (
            "money",
            "Invoice total: USD 9007199254740993.01.\n",
            "Which amount is the invoice total?",
            vec![],
            json!({"amount":"9007199254740993.01","currency":"USD"}),
        ),
        (
            "semver",
            "Current release: v1.2.3-rc.1+build.01.\n",
            "Which version is the current release?",
            vec![],
            json!("1.2.3-rc.1+build.01"),
        ),
    ] {
        let mut args = vec![
            "--url",
            &url,
            "extract",
            query,
            "--candidates",
            kind,
            "--normalize",
            "--json",
            "--strict",
            "--no-cache",
            "--budget",
            "2000",
            "--deadline",
            "20s",
        ];
        args.extend(flags);
        let output = run(root, &args, text);
        code(&output, 0);
        let selected: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(selected["normalized"]["value"], expected, "{selected}");
        let start = selected["location"]["byte_start"].as_u64().unwrap() as usize;
        let end = selected["location"]["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(&text[start..end], selected["unit"].as_str().unwrap());
    }
}
