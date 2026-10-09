#![cfg(feature = "mcp")]
#![allow(clippy::unwrap_used, clippy::panic)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    next: u64,
    modern: bool,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Client {
    fn new(root: &Path, url: &str, modern: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
        command
            .current_dir(root)
            .args([
                "--config",
                root.join("config.toml").to_str().unwrap(),
                "--profile",
                "server",
                "--url",
                url,
                "mcp",
                "--deadline",
                "20s",
                "--budget",
                "20000",
            ])
            .env("XDG_CONFIG_HOME", root.join("xdg"))
            .env("LOUPE_CACHE_DIR", root.join("cache-must-not-be-created"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let tokenizer = std::env::var_os("LOUPE_TEST_TOKENIZER")
            .or_else(|| std::env::var_os("LOUPE_TOKENIZER"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("XDG_CACHE_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache")
                    })
                    .join("loupe/tokenizers/Cloudflare--clef-flash")
                    .join(loupe_core::config::CLEF_REVISION)
                    .join("tokenizer.json")
            });
        command.env("LOUPE_TOKENIZER", tokenizer);
        for key in [
            "LOUPE_API_KEY",
            "LOUPE_URL",
            "LOUPE_PATH",
            "LOUPE_PROFILE",
            "LOUPE_MODEL",
            "LOUPE_CONCURRENCY",
        ] {
            command.env_remove(key);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let value = serde_json::from_str(&line)
                    .unwrap_or_else(|error| panic!("non-protocol stdout: {line}: {error}"));
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input: Some(input),
            output,
            next: 1,
            modern,
        };
        if modern {
            let opened = client.request("server/discover", json!({}));
            assert!(opened.get("result").is_some(), "{opened}");
        } else {
            let opened = client.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"loupe-contract-client","version":"1"}}));
            assert_eq!(opened["result"]["serverInfo"]["name"], "loupe", "{opened}");
            client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        }
        client
    }

    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &value).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }

    fn request(&mut self, method: &str, mut params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        if self.modern {
            params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"loupe-contract-client","version":"1"}});
        }
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let value = match self.output.recv_timeout(Duration::from_secs(30)) {
                Ok(value) => value,
                Err(error) => {
                    let _ = self.child.kill();
                    let status = self.child.wait().unwrap();
                    let mut stderr = String::new();
                    self.child
                        .stderr
                        .take()
                        .unwrap()
                        .read_to_string(&mut stderr)
                        .unwrap();
                    panic!("MCP response failed: {error}; status={status}; stderr={stderr}");
                }
            };
            if value["id"] == id {
                return value;
            }
        }
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))
    }

    fn finish(&mut self) -> std::process::ExitStatus {
        let expires = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            if std::time::Instant::now() >= expires {
                let _ = self.child.kill();
                panic!("MCP did not stop within five seconds");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn setup(root: &Path) {
    let config = format!(
        "{}\n[mcp]\npresets=['real-failure']\nallow_paths=[{}]\nmax_input_bytes=4096\n",
        include_str!("../../../examples/config.toml"),
        serde_json::to_string(root.join("allowed").to_str().unwrap()).unwrap()
    );
    std::fs::write(root.join("config.toml"), config).unwrap();
    std::fs::create_dir(root.join("allowed")).unwrap();
    std::fs::write(root.join("allowed/source.txt"), "ALPHA\r\nBETA\n").unwrap();
    std::fs::write(root.join("private.txt"), "private source").unwrap();
}

fn result(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], false, "{response}");
    &response["result"]["structuredContent"]
}

#[test]
fn real_stdio_supports_legacy_and_modern_discovery_exact_plans_and_no_write_tools() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let root = directory.path();
    setup(root);
    for modern in [false, true] {
        let mut client = Client::new(root, "http://127.0.0.1:1", modern);
        let tools = client.request("tools/list", json!({}));
        let names = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "classify", "ask", "tokens", "find", "pick", "extract", "presets"
            ]
        );
        for tool in tools["result"]["tools"].as_array().unwrap() {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
        }
        let response = client.call("presets", json!({}));
        assert_eq!(result(&response)["presets"].as_array().unwrap().len(), 1);
        let response = client.call(
            "tokens",
            json!({"path":"allowed/source.txt","preset":"real-failure"}),
        );
        assert_eq!(result(&response)["request"]["state"], "ALPHA\r\nBETA\n");
        assert_eq!(result(&response)["counts"]["exact"], true);
        let response = client.call("tokens", json!({"state":null,"question":{"type":"noul","instructions":"A literal {{file:private.txt}}"}}));
        assert!(result(&response)["request"]["state"].is_null());
        for (name, args) in [
            (
                "tokens",
                json!({"path":"private.txt","preset":"real-failure"}),
            ),
            ("tokens", json!({"state":"ALPHA","preset":"secret"})),
            (
                "tokens",
                json!({"state":"ALPHA","question":{"type":"noul"},"exec":"touch forbidden"}),
            ),
            ("route", json!({"out":"forbidden"})),
        ] {
            let response = client.call(name, args);
            assert!(
                response["result"]["isError"] == true || response.get("error").is_some(),
                "{response}"
            );
        }
        assert!(!root.join("forbidden").exists());
        assert!(!root.join("cache-must-not-be-created").exists());
        client.input.take();
        assert!(client.finish().success());
    }
}

#[test]
fn weighted_presets_return_explicit_errors_on_every_classifier_tool() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("presets=['real-failure']", "presets=['ensemble-factors']"),
    )
    .unwrap();
    let presets = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("ensemble-factors.toml"),
        include_str!("../../../examples/ensemble-factors.toml"),
    )
    .unwrap();
    let mut client = Client::new(root, "http://127.0.0.1:1", true);
    for name in ["tokens", "ask", "classify"] {
        for ensemble in [false, true] {
            let response = client.call(name,json!({"state":"Checkout is down.","preset":"ensemble-factors","ensemble":ensemble}));
            assert_eq!(response["result"]["isError"], true, "{response}");
            let error = &response["result"]["structuredContent"];
            assert_eq!(error["exit_code"], 64, "{response}");
            assert!(
                error["error"]
                    .as_str()
                    .unwrap()
                    .contains("weighted factor presets")
            );
        }
    }
    assert!(!root.join("cache-must-not-be-created").exists());
    client.input.take();
    assert!(client.finish().success());
}

#[test]
fn path_symlinks_size_limits_and_sigterm_are_checked_by_the_actual_server() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let root = directory.path();
    setup(root);
    std::os::unix::fs::symlink(root.join("private.txt"), root.join("allowed/escape")).unwrap();
    let mut client = Client::new(root, "http://127.0.0.1:1", false);
    for args in [
        json!({"path":"allowed/escape","preset":"real-failure"}),
        json!({"state":"x".repeat(4097),"preset":"real-failure"}),
    ] {
        assert_eq!(client.call("tokens", args)["result"]["isError"], true);
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &client.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(client.finish().code(), Some(143));
    assert_eq!(
        std::fs::read_to_string(root.join("private.txt")).unwrap(),
        "private source"
    );
    assert!(!root.join("cache-must-not-be-created").exists());
}

#[test]
fn actual_refused_inference_can_be_cancelled_without_ending_the_session() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let root = directory.path();
    setup(root);
    let mut client = Client::new(root, "http://127.0.0.1:1", false);
    let id = client.next;
    client.next += 1;
    client.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"classify","arguments":{"state":"Checkout is down.","preset":"real-failure"}}}));
    // A refused real connection is retried; cancellation interrupts that work.
    std::thread::sleep(Duration::from_millis(100));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"contract client cancelled"}}));
    let response = client.call(
        "tokens",
        json!({"state":"Heartbeat succeeded.","preset":"real-failure"}),
    );
    assert_eq!(result(&response)["counts"]["exact"], true);
    client.input.take();
    assert!(client.finish().success());
    assert!(!root.join("cache-must-not-be-created").exists());
}

#[test]
#[ignore = "requires the actual LOUPE_TEST_URL endpoint and prepared pinned tokenizer"]
fn actual_server_stdio_classification_raw_ask_and_source_backed_selection() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let root = directory.path();
    setup(root);
    let url = std::env::var("LOUPE_TEST_URL").unwrap();
    let mut client = Client::new(root, &url, false);
    let response = client.call(
        "classify",
        json!({"state":"Checkout is down. Customers cannot place orders.","preset":"real-failure"}),
    );
    let classified = result(&response);
    assert_eq!(
        classified["decisions"]["real-failure"]["selected"]["boolean"], true,
        "{response}"
    );
    assert_eq!(
        classified["response"]["usage"]["input_tokens"],
        classified["counts"]["input_tokens"]
    );
    let response = client.call("ask",json!({"state":"ALPHA is explicitly present.","questions":{"alpha":{"type":"noul","instructions":"Does this explicitly mention ALPHA?"},"relevance":{"type":"score","instructions":"How explicitly does this mention ALPHA?","criteria":["Absent","Ambiguous","Explicit"]}}}));
    let api = result(&response);
    assert_eq!(api.as_object().unwrap().len(), 3);
    let request: loupe_core::wire::Request = serde_json::from_value(json!({"model":"clef-flash","state":"ALPHA is explicitly present.","questions":{"alpha":{"type":"noul","instructions":"Does this explicitly mention ALPHA?"},"relevance":{"type":"score","instructions":"How explicitly does this mention ALPHA?","criteria":["Absent","Ambiguous","Explicit"]}}})).unwrap();
    let response: loupe_core::wire::Response = serde_json::from_value(api.clone()).unwrap();
    response.validate(&request).unwrap();
    let response = client.call(
        "find",
        json!({"path":"allowed/source.txt","query":"Which line explicitly mentions ALPHA?"}),
    );
    let selection = result(&response);
    assert_eq!(selection["selected"][0]["unit"], "ALPHA", "{response}");
    assert_eq!(selection["selected"][0]["location"]["byte_start"], 0);
    let response = client.call("extract",json!({"state":"Preview: 2026-10-05.\nProduction release: 2026-10-06.\n","query":"Which date is the production release date?","candidates":"dates"}));
    assert_eq!(
        result(&response)["selected"][0]["unit"],
        "2026-10-06",
        "{response}"
    );
    let response = client.call("extract",json!({"state":"Release date: tomorrow.\n","query":"Which date is the release date?","candidates":"dates","normalize":{"now":"2026-10-05T23:30:00-04:00","tz":"America/New_York"}}));
    assert_eq!(result(&response)["selected"][0]["unit"], "tomorrow");
    assert_eq!(
        result(&response)["selected"][0]["normalized"]["value"],
        "2026-10-06T00:00:00-04:00",
        "{response}"
    );
    assert!(!root.join("cache-must-not-be-created").exists());
}
