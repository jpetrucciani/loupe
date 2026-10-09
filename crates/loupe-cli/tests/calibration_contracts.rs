#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn fixture(root: &Path, kind: &str) {
    std::fs::create_dir(root.join(".git")).unwrap();
    let directory = root.join("xdg/loupe/presets");
    std::fs::create_dir_all(&directory).unwrap();
    let document = match kind {
        "mixed" => {
            "version=1\n[questions.available]\ntype='noul'\ninstructions='Is checkout unavailable?'\n[questions.topic]\ntype='choice'\ninstructions='Choose the topic.'\n[questions.topic.criteria]\ncommerce='Payments and checkout'\nweather='Weather and clouds'\n"
        }
        "choice" => {
            "version=1\ntype='choice'\ninstructions='Choose the topic.'\n[criteria]\ncommerce='Payments and checkout'\nweather='Weather and clouds'\n"
        }
        "score" => {
            "version=1\ntype='score'\ninstructions='Rate impact.'\ncriteria=['No impact','Full outage']\n"
        }
        _ => "version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n",
    };
    std::fs::write(directory.join("rubric.toml"), document).unwrap();
    let mut bytes = Vec::new();
    for index in 0..9 {
        let label = match kind {
            "mixed" => {
                json!({"available":index%2==0,"topic":if index%2==0{"commerce"}else{"weather"}})
            }
            "choice" => json!(if index % 2 == 0 {
                "commerce"
            } else {
                "weather"
            }),
            "score" => json!(index % 2),
            _ => json!(index % 2 == 0),
        };
        let mut case = json!({"id":format!("case-{index}"),"unit":if index%2==0{"Checkout is down and customers cannot pay."}else{"Blue skies and sunshine all day."},"label":label,"notes":"LABEL_SECRET_DO_NOT_SEND"});
        if index == 8 {
            case["source"] = json!("review");
            case["reviewed"] = json!(true);
            case["sampling"] = json!("uniform-audit");
        }
        serde_json::to_writer(&mut bytes, &case).unwrap();
        bytes.push(b'\n');
    }
    std::fs::write(root.join("cases.jsonl"), bytes).unwrap();
}

fn run(root: &Path, extra: &[&str]) -> Output {
    let local = root.join("config.toml");
    let config = if local.exists() {
        local
    } else {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/config.toml"
        ))
        .to_owned()
    };
    Command::new(env!("CARGO_BIN_EXE_loupe"))
        .current_dir(root)
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "server",
            "calibrate",
            "cases.jsonl",
            "--preset",
            "rubric",
            "--dataset-revision",
            "functional-v1",
            "--json",
        ])
        .args(extra)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("LOUPE_PRESET_PATH")
        .env_remove("LOUPE_CONFIG")
        .env_remove("LOUPE_URL")
        .env_remove("LOUPE_MODEL")
        .env_remove("LOUPE_PROFILE")
        .env_remove("LOUPE_CONCURRENCY")
        .env_remove("LOUPE_API_KEY")
        .output()
        .unwrap()
}
fn decode(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn offline_calibration_selects_only_calibrate_or_captured_and_never_reads_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "noul");
    let marker = root.join("credential-executed");
    std::fs::write(root.join("config.toml"),format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n",marker.display())).unwrap();
    let plan = decode(
        &run(
            root,
            &[
                "--target",
                "precision=0.9",
                "--dry-run",
                "--init-splits",
                "--budget",
                "0",
            ],
        ),
        0,
    );
    assert_eq!(plan["status"], "plan");
    assert_eq!(plan["evaluation"]["token_usage"]["attempts"], 0);
    let suite = &plan["evaluation"]["suites"][0];
    assert_eq!(suite["partition"], "calibrate");
    assert_eq!(suite["selected_case_ids"].as_array().unwrap().len(), 4);
    assert!(
        suite["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["case_id"] != "case-8")
    );
    let captured = decode(
        &run(
            root,
            &[
                "--max-uncertain",
                "10%",
                "--partition",
                "captured",
                "--dry-run",
            ],
        ),
        0,
    );
    assert_eq!(
        captured["evaluation"]["suites"][0]["selected_case_ids"],
        json!(["case-8"])
    );
    assert!(!marker.exists());
    for args in [
        vec![
            "--target",
            "precision=0.9",
            "--partition",
            "test",
            "--dry-run",
        ],
        vec!["--target", "precision=0.9", "--dry-run", "--write"],
        vec![
            "--target",
            "precision=0.9",
            "--sweep-pack",
            "auto",
            "--dry-run",
        ],
    ] {
        assert_eq!(run(root, &args).status.code(), Some(64));
    }
}

#[test]
fn offline_calibration_views_expand_the_full_grid_without_inferred_winners_or_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "noul");
    let marker = root.join("credential-executed");
    std::fs::write(root.join("config.toml"),format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n",marker.display())).unwrap();
    let plan = decode(
        &run(
            root,
            &[
                "--sweep-pack",
                "unit,window:4",
                "--sweep-combine",
                "any,all",
                "--sweep-focus",
                "0,1",
                "--overlap",
                "0",
                "--max-chunks",
                "4",
                "--question-id-source",
                "lines",
                "--target",
                "precision=0.5",
                "--dry-run",
                "--init-splits",
            ],
        ),
        0,
    );
    assert_eq!(plan["kind"], "calibration-sweep");
    assert_eq!(plan["status"], "plan");
    assert_eq!(plan["token_usage"]["attempts"], 0);
    assert!(plan["selected"].as_array().unwrap().is_empty());
    let variants = plan["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 6);
    for variant in variants {
        assert!(variant["fitted"].as_array().unwrap().is_empty());
        let suite = &variant["evaluation"]["suites"][0];
        assert_eq!(suite["partition"], "calibrate");
        assert_eq!(suite["selected_case_ids"].as_array().unwrap().len(), 4);
        for record in suite["records"].as_array().unwrap() {
            if record["packing"]["declared"] == "Window(4)" {
                assert_eq!(record["packing"]["question_id_source"], "Lines");
            }
        }
        assert!(
            !suite["selected_case_ids"]
                .as_array()
                .unwrap()
                .contains(&json!("case-8"))
        );
    }
    assert!(!marker.exists());
    for partition in ["captured", "test"] {
        assert_eq!(
            run(
                root,
                &[
                    "--sweep-pack",
                    "unit,window:4",
                    "--target",
                    "precision=0.5",
                    "--partition",
                    partition,
                    "--dry-run"
                ]
            )
            .status
            .code(),
            Some(64)
        );
    }
}

#[test]
fn offline_published_view_restores_focus_chunks_and_source_family_in_cli_and_eval() {
    // Real filesystem writer and CLI planning, using a pure identity/policy codec fixture.
    // There is no simulated discovery or inference response.
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "noul");
    let store = loupe_core::preset_store::Store::new(
        root,
        &std::collections::HashMap::from([(
            "XDG_CONFIG_HOME".to_owned(),
            root.join("xdg").to_str().unwrap().to_owned(),
        )]),
    )
    .unwrap();
    let path = root.join("xdg/loupe/presets/rubric.toml");
    std::fs::write(&path,"version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n[policy]\nsplittable=true\ncombine='any'\n").unwrap();
    let original = store.lookup("rubric").unwrap();
    let limits = loupe_core::config::Limits::default();
    let pin = loupe_core::calibration_pin::Pin {
        schema_version: 1,
        model: loupe_core::identity::Model::parse(&format!(
            "fixture/model@{};compute=bf16;quantization=nf4;server={}",
            "1".repeat(40),
            "2".repeat(64)
        )).unwrap(),
        local_encoder:"clef-v1;codec-fixture".into(),
        server_encoder:"clef-v1;codec-fixture".into(),
        preset_version:1,preset_blake3:"0".repeat(64),field:"rubric".into(),ensemble:None,
        view:loupe_core::calibration_pin::view(&json!({"declared":"Window(4)","split":false,"combine":{},"focus":1,"overlap":0,"max_chunks":4,"path":"","question_id_source":"Lines"}), &serde_json::to_value(&limits).unwrap()).unwrap(),
        source:json!({"partition":"calibrate","suite":"codec-fixture","view_selection":{"winner":"fixture"}}),
    };
    let proposed = loupe_core::calibration_pin::document(
        &original.document,
        &loupe_core::calibration::Parameters {
            threshold: 0.5,
            margin: 0.0,
            min_confidence: 0.0,
            min_gap: 0.0,
        },
        loupe_core::policy::QuestionKind::Noul,
        pin,
    )
    .unwrap();
    assert!(proposed["policy"].get("splittable").is_none());
    assert!(proposed["policy"].get("combine").is_none());
    loupe_core::calibration_pin::write(&store, &original, &proposed, &limits, "clef-flash")
        .unwrap();
    let marker = root.join("credential-executed");
    std::fs::write(root.join("config.toml"),format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n",marker.display())).unwrap();
    for explicit in [false, true] {
        let mut extra = vec!["--dry-run", "--init-splits"];
        if explicit {
            extra.extend(["--focus", "2", "--question-id-source", "records"]);
        }
        let output = Command::new(env!("CARGO_BIN_EXE_loupe"))
            .current_dir(root)
            .args([
                "--config",
                root.join("config.toml").to_str().unwrap(),
                "--profile",
                "server",
                "eval",
                "cases.jsonl",
                "-p",
                "rubric",
                "--dataset-revision",
                "functional-v1",
                "--partition",
                "calibrate",
                "--json",
            ])
            .args(&extra)
            .env("XDG_CONFIG_HOME", root.join("xdg"))
            .env_remove("LOUPE_CONFIG")
            .env_remove("LOUPE_URL")
            .env_remove("LOUPE_PRESET_PATH")
            .env_remove("LOUPE_PROFILE")
            .env_remove("LOUPE_MODEL")
            .output()
            .unwrap();
        let plan = decode(&output, 0);
        for record in plan["suites"][0]["records"].as_array().unwrap() {
            assert_eq!(record["packing"]["focus"], if explicit { 2 } else { 1 });
            assert_eq!(record["packing"]["max_chunks"], 4);
            assert_eq!(record["packing"]["overlap"], 0);
            assert_eq!(
                record["packing"]["question_id_source"],
                if explicit { "Records" } else { "Lines" }
            );
        }
        let input = root.join("input.txt");
        std::fs::write(&input, "Checkout is down.\nThe sky is blue.\n").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
        command
            .current_dir(root)
            .args([
                "--config",
                root.join("config.toml").to_str().unwrap(),
                "--profile",
                "server",
                "keep",
                input.to_str().unwrap(),
                "-p",
                "rubric",
                "--dry-run",
            ])
            .env("XDG_CONFIG_HOME", root.join("xdg"))
            .env_remove("LOUPE_CONFIG")
            .env_remove("LOUPE_URL")
            .env_remove("LOUPE_PRESET_PATH")
            .env_remove("LOUPE_PROFILE")
            .env_remove("LOUPE_MODEL");
        if explicit {
            command.args(["--focus", "2"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let plans = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(!plans.is_empty());
        for plan in plans {
            assert_eq!(plan["focus"]["k"], if explicit { 2 } else { 1 });
        }
    }
    assert!(!marker.exists());
}

#[test]
fn schema_and_target_errors_precede_split_writes_or_backend_credentials() {
    for (kind, extra) in [
        ("score", vec!["--target", "precision=0.9"]),
        ("mixed", vec!["--target", "precision=0.9"]),
        ("choice", vec!["--target", "precision:missing=0.9"]),
        (
            "noul",
            vec![
                "--target",
                "precision=0.9",
                "--question-id-source",
                "unknown",
            ],
        ),
        (
            "noul",
            vec!["--target", "precision=0.9", "--min-confidence", "0.8"],
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fixture(root, kind);
        let marker = root.join("credential-executed");
        std::fs::write(root.join("config.toml"),format!("[profile.server]\nbase_url='http://127.0.0.1:1'\nencoder='clef-v1'\napi_key_cmd='touch {}'\n",marker.display())).unwrap();
        let mut args = extra;
        args.push("--init-splits");
        assert_eq!(run(root, &args).status.code(), Some(64));
        assert!(!marker.exists());
        assert!(!root.join("cases.jsonl.splits.json").exists());
    }
}

#[test]
fn ensemble_calibration_plans_exclude_variant_fields_and_keep_exact_expanded_schema() {
    for kind in ["noul", "choice", "mixed"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fixture(root, kind);
        let preset = root.join("xdg/loupe/presets/rubric.toml");
        let variant = if kind == "mixed" {
            "\n[[variants]]\nid='direct'\nfield='topic'\ninstructions='Which topic does this describe?'\n"
        } else {
            "\n[[variants]]\nid='direct'\ninstructions='Answer the original classification question from the text.'\n"
        };
        std::fs::write(
            &preset,
            format!("{}{variant}", std::fs::read_to_string(&preset).unwrap()),
        )
        .unwrap();
        let mut args = vec![
            "--ensemble",
            "--min-agreement",
            "0.75",
            "--target",
            "precision=0.5",
            "--dry-run",
            "--init-splits",
        ];
        if kind == "mixed" {
            args.extend(["--field", "topic"]);
        }
        let plan = decode(&run(root, &args), 0);
        let record = &plan["evaluation"]["suites"][0]["records"][0];
        assert_eq!(record["ensemble"]["settings"]["min_agreement"], 0.75);
        assert_eq!(
            record["questions"].as_object().unwrap().len(),
            if kind == "mixed" { 3 } else { 2 }
        );
        assert_eq!(record["counts"]["exact"], true);
        let field = if kind == "mixed" {
            "ensemble:topic/direct"
        } else {
            "ensemble:rubric/direct"
        };
        assert_eq!(
            run(
                root,
                &[
                    "--ensemble",
                    "--field",
                    field,
                    "--target",
                    "precision=0.5",
                    "--dry-run"
                ]
            )
            .status
            .code(),
            Some(64)
        );
    }
}

#[test]
fn ensemble_calibration_pins_guard_cli_mode_and_gate_while_explicit_refitting_can_change_them() {
    // Mathematical identity/codec fixture, with real CLI preflight and no backend.
    for ensemble in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fixture(root, "noul");
        let path = root.join("xdg/loupe/presets/rubric.toml");
        let original = serde_json::json!({"version":1,"type":"noul","instructions":"{{arg.query}}","variants":[{"id":"direct","instructions":"Is checkout currently down?"}]});
        let group = ensemble.then_some(loupe_core::calibration_pin::Ensemble {
            min_agreement: loupe_core::policy::Probability::new(1.0).unwrap(),
        });
        let mut source = json!({"partition":"calibrate","suite":"math-fixture"});
        if let Some(group) = &group {
            source["ensemble"] = serde_json::to_value(group).unwrap();
        }
        let pin = loupe_core::calibration_pin::Pin {
            schema_version: 1,
            model: loupe_core::identity::Model::parse(&format!(
                "fixture/model@{};compute=bf16;quantization=nf4;server={}",
                "1".repeat(40),
                "2".repeat(64)
            ))
            .unwrap(),
            local_encoder: "clef-v1;fixture".into(),
            server_encoder: "clef-v1;fixture".into(),
            preset_version: 1,
            preset_blake3: "0".repeat(64),
            field: "rubric".into(),
            view: loupe_core::calibration_pin::view(
                &json!({"declared":"Unit","combine":{},"split":false,"path":""}),
                &serde_json::to_value(loupe_core::config::Limits::default()).unwrap(),
            )
            .unwrap(),
            source,
            ensemble: group,
        };
        let document = loupe_core::calibration_pin::document(
            &original,
            &loupe_core::calibration::Parameters {
                threshold: 0.5,
                margin: 0.0,
                min_confidence: 0.0,
                min_gap: 0.0,
            },
            loupe_core::policy::QuestionKind::Noul,
            pin,
        )
        .unwrap();
        std::fs::write(&path, loupe_core::presets::to_toml(&document).unwrap()).unwrap();
        let plan = decode(
            &run(
                root,
                &[
                    "--ensemble",
                    "--min-agreement",
                    "0.5",
                    "--arg",
                    "query=Is checkout unavailable?",
                    "--target",
                    "precision=0.5",
                    "--dry-run",
                    "--init-splits",
                ],
            ),
            0,
        );
        assert_eq!(
            plan["evaluation"]["suites"][0]["records"][0]["ensemble"]["settings"]["min_agreement"],
            0.5
        );
        let text = root.join("input.txt");
        std::fs::write(&text, "Checkout is down.").unwrap();
        for (enabled, minimum, expected) in [
            (false, "1", if ensemble { 64 } else { 0 }),
            (true, "1", if ensemble { 0 } else { 64 }),
            (true, "0.5", 64),
        ] {
            for operation in ["keep", "eval"] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_loupe"));
                command
                    .current_dir(root)
                    .args([
                        "--config",
                        concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml"),
                        "--profile",
                        "server",
                    ])
                    .args(if operation == "keep" {
                        vec!["keep", text.to_str().unwrap()]
                    } else {
                        vec![
                            "eval",
                            "cases.jsonl",
                            "--dataset-revision",
                            "functional-v1",
                            "--partition",
                            "calibrate",
                        ]
                    })
                    .args([
                        "-p",
                        "rubric",
                        "--dry-run",
                        "--arg",
                        "query=Is checkout unavailable?",
                    ])
                    .env("XDG_CONFIG_HOME", root.join("xdg"));
                for key in [
                    "LOUPE_CONFIG",
                    "LOUPE_PRESET_PATH",
                    "LOUPE_URL",
                    "LOUPE_MODEL",
                    "LOUPE_PROFILE",
                    "LOUPE_API_KEY",
                ] {
                    command.env_remove(key);
                }
                if enabled {
                    command.args(["--ensemble", "--min-agreement", minimum]);
                }
                let result = command.output().unwrap();
                assert_eq!(
                    result.status.code(),
                    Some(expected),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
    }
}

#[test]
fn mixed_field_plan_and_coverage_bounds_are_validated_without_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "mixed");
    let plan = decode(
        &run(
            root,
            &[
                "--field",
                "topic",
                "--target",
                "precision:commerce=0.97",
                "--max-uncertain",
                "20%",
                "--dry-run",
                "--init-splits",
            ],
        ),
        0,
    );
    assert_eq!(plan["targets"][0]["label"], "commerce");
    assert_eq!(plan["max_uncertain"], 0.2);
    for value in ["NaN", "101%", "-0.1"] {
        assert!(
            run(
                root,
                &["--max-uncertain", value, "--field", "topic", "--dry-run"]
            )
            .status
            .code()
            .is_some_and(|code| code != 0)
        );
    }
}

#[test]
#[ignore = "requires real server model; no fake HTTP/model backend"]
fn real_server_calibration_views_fit_and_select_using_quality_then_uncached_throughput() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "noul");
    let preset = root.join("xdg/loupe/presets/rubric.toml");
    std::fs::write(&preset,format!("{}\n[[variants]]\nid='direct'\ninstructions='Is checkout currently unavailable to customers?'\n",std::fs::read_to_string(&preset).unwrap())).unwrap();
    let original = std::fs::read(&preset).unwrap();
    let report = decode(
        &run(
            root,
            &[
                "--sweep-pack",
                "unit,window:4",
                "--question-id-source",
                "lines",
                "--ensemble",
                "--min-agreement",
                "0",
                "--target",
                "precision=0",
                "--max-uncertain",
                "100%",
                "--allow-unverified",
                "--strict",
                "--no-cache",
                "--init-splits",
                "--warmup",
                "0",
                "--deadline",
                "60s",
            ],
        ),
        0,
    );
    assert_eq!(report["kind"], "calibration-sweep");
    assert_eq!(report["status"], "provisional");
    let variants = report["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    let winner = &report["selected"][0];
    assert!(winner["proposed_preset"].is_null());
    assert!(winner["written"].is_null());
    let mut expected: Option<(&str, u64, u64, f64)> = None;
    for variant in variants {
        let suite = &variant["evaluation"]["suites"][0];
        assert_eq!(suite["partition"], "calibrate");
        assert_eq!(
            suite["selected_case_ids"],
            variants[0]["evaluation"]["suites"][0]["selected_case_ids"]
        );
        for record in suite["records"].as_array().unwrap() {
            if record["packing"]["declared"] == "Window(4)" {
                assert_eq!(record["packing"]["question_id_source"], "Lines");
                assert!(
                    record["binding"]
                        .as_object()
                        .unwrap()
                        .values()
                        .all(|value| value.as_str().unwrap().starts_with('L'))
                );
            }
            assert_eq!(
                record["counts"]["input_tokens"],
                record["evaluation"]["response"]["usage"]["input_tokens"]
            );
        }
        let selected = &variant["fitted"][0]["fit"]["selected"];
        let accepted = selected["accepted"].as_u64().unwrap();
        let correct = selected["correct"].as_u64().unwrap();
        let phase = suite["phases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|phase| phase["name"] == "latency")
            .unwrap();
        assert_eq!(phase["cache_requested"], false);
        assert_eq!(phase["cache_hits"], 0);
        let rate = phase["timing"]["completed_units"].as_f64().unwrap()
            / phase["timing"]["elapsed_seconds"].as_f64().unwrap();
        if expected.is_none_or(|(_, old_accepted, old_correct, old_rate)| {
            (accepted, correct) > (old_accepted, old_correct)
                || (accepted, correct) == (old_accepted, old_correct) && rate > old_rate
        }) {
            expected = Some((variant["name"].as_str().unwrap(), accepted, correct, rate));
        }
    }
    let (name, accepted, correct, rate) = expected.unwrap();
    assert_eq!(winner["variant"], name);
    assert_eq!(winner["accepted"], accepted);
    assert_eq!(winner["correct"], correct);
    assert!((winner["uncached_units_per_second"].as_f64().unwrap() - rate).abs() < 1e-12);
    assert_eq!(std::fs::read(&preset).unwrap(), original);
    let exhausted = decode(
        &run(
            root,
            &[
                "--sweep-pack",
                "unit,window:4",
                "--target",
                "precision=0",
                "--allow-unverified",
                "--budget",
                "0",
                "--warmup",
                "0",
                "--deadline",
                "60s",
            ],
        ),
        4,
    );
    assert_eq!(exhausted["status"], "incomplete");
    assert_eq!(exhausted["variants"].as_array().unwrap().len(), 1);
    assert!(exhausted["selected"].as_array().unwrap().is_empty());
    assert_eq!(exhausted["token_usage"]["attempts"], 0);
    let refused = run(
        root,
        &[
            "--sweep-pack",
            "unit,window:4",
            "--target",
            "precision=0",
            "--allow-unverified",
            "--write",
            "--warmup",
            "0",
            "--deadline",
            "60s",
        ],
    );
    assert_eq!(refused.status.code(), Some(69));
    assert_eq!(std::fs::read(&preset).unwrap(), original);
}

#[test]
#[ignore = "requires real server model; no fake HTTP/model backend"]
fn real_server_ensemble_calibration_fits_means_and_keeps_agreement_out_of_every_candidate() {
    for kind in ["noul", "choice"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fixture(root, kind);
        let preset = root.join("xdg/loupe/presets/rubric.toml");
        std::fs::write(&preset,format!("{}\n[[variants]]\nid='direct'\ninstructions='Answer the original classification question from the text.'\n",std::fs::read_to_string(&preset).unwrap())).unwrap();
        let original = std::fs::read(&preset).unwrap();
        let actual = run(
            root,
            &[
                "--ensemble",
                "--target",
                "precision=0",
                "--max-uncertain",
                "100%",
                "--allow-unverified",
                "--strict",
                "--no-cache",
                "--init-splits",
                "--warmup",
                "0",
                "--deadline",
                "60s",
            ],
        );
        assert!(
            matches!(actual.status.code(), Some(0 | 1)),
            "{}",
            String::from_utf8_lossy(&actual.stderr)
        );
        let report: Value = serde_json::from_slice(&actual.stdout).unwrap();
        assert_eq!(report["evaluation"]["identity"]["verified"], false);
        let fitted = &report["fitted"][0];
        assert_eq!(fitted["ensemble"]["min_agreement"], 1.0);
        assert!(fitted["proposed_preset"].is_null());
        assert!(fitted["written"].is_null());
        let records = report["evaluation"]["suites"][0]["records"]
            .as_array()
            .unwrap();
        let excluded = records
            .iter()
            .filter(|record| record["ensemble"]["reports"]["rubric"]["agreement_uncertain"] == true)
            .map(|record| record["case_id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        let retained = fitted
            .get("agreement_uncertain_cases")
            .map(|cases| {
                cases
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|case| case.as_str().unwrap())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        assert_eq!(retained, excluded);
        if let Some(selected) = fitted["fit"]["selected"].as_object() {
            let parameters: loupe_core::calibration::Parameters =
                serde_json::from_value(selected["parameters"].clone()).unwrap();
            let policy = parameters.policy().unwrap();
            let mut accepted = 0;
            let mut correct = 0;
            for record in records {
                let mean = &record["ensemble"]["reports"]["rubric"];
                let response = &record["evaluation"]["response"];
                let members = mean["members"].as_array().unwrap();
                for member in members {
                    assert_eq!(
                        member["answer"],
                        response["answers"][member["field"].as_str().unwrap()]
                    );
                }
                let distribution = mean["mean_probabilities"].as_object().unwrap();
                for (option, p) in distribution {
                    assert!(
                        (p.as_f64().unwrap()
                            - members
                                .iter()
                                .map(|member| member["probabilities"][option].as_f64().unwrap())
                                .sum::<f64>()
                                / members.len() as f64)
                            .abs()
                            < 1e-12
                    );
                }
                if excluded.contains(record["case_id"].as_str().unwrap()) {
                    continue;
                }
                let assessment = if kind == "noul" {
                    policy
                        .noul(
                            loupe_core::policy::Probability::new(
                                mean["mean_noul"].as_f64().unwrap(),
                            )
                            .unwrap(),
                        )
                        .unwrap()
                } else {
                    policy
                        .choice(
                            &distribution
                                .iter()
                                .map(|(key, p)| {
                                    (
                                        key.clone(),
                                        loupe_core::policy::Probability::new(p.as_f64().unwrap())
                                            .unwrap(),
                                    )
                                })
                                .collect(),
                        )
                        .unwrap()
                };
                if !assessment.uncertain {
                    accepted += 1;
                    let actual = match assessment.selected {
                        loupe_core::policy::Selected::Boolean(value) => json!(value),
                        loupe_core::policy::Selected::Choice(value) => json!(value),
                        _ => unreachable!(),
                    };
                    correct += usize::from(actual == record["label"]);
                }
            }
            assert_eq!(selected["accepted"], accepted);
            assert_eq!(selected["correct"], correct);
            assert_eq!(selected["coverage"], accepted as f64 / records.len() as f64);
        } else {
            assert_eq!(excluded.len(), records.len());
        }
        assert_eq!(std::fs::read(&preset).unwrap(), original);
        let refused = run(
            root,
            &[
                "--ensemble",
                "--target",
                "precision=0",
                "--allow-unverified",
                "--write",
                "--warmup",
                "0",
                "--deadline",
                "60s",
            ],
        );
        assert_eq!(refused.status.code(), Some(69));
        assert_eq!(std::fs::read(&preset).unwrap(), original);
    }
}

#[test]
#[ignore = "requires real server model; no fake HTTP/model backend"]
fn real_server_calibration_fits_noul_and_choice_with_empirical_support_only() {
    for (kind, extra) in [
        ("noul", vec!["--target", "precision=0.5"]),
        (
            "choice",
            vec![
                "--target",
                "precision:commerce=0.5",
                "--max-uncertain",
                "50%",
            ],
        ),
        (
            "mixed",
            vec![
                "--field",
                "topic",
                "--target",
                "precision=0.5",
                "--pack",
                "window:4",
            ],
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fixture(root, kind);
        let mut args = extra;
        args.extend([
            "--allow-unverified",
            "--strict",
            "--init-splits",
            "--warmup",
            "0",
            "--report",
            "reports",
        ]);
        let actual = decode(&run(root, &args), 0);
        assert_eq!(actual["kind"], "calibration");
        assert_eq!(actual["status"], "provisional");
        assert_eq!(actual["evaluation"]["identity"]["verified"], false);
        assert!(actual["fitted"][0]["proposed_preset"].is_null());
        assert!(actual["fitted"][0]["written"].is_null());
        let fit = &actual["fitted"][0]["fit"];
        assert_eq!(fit["cases"], 4);
        assert!(fit["selected"]["accepted"].as_u64().unwrap() >= 1);
        assert!(fit["selected"]["targets"][0]["predicted"].as_u64().unwrap() >= 1);
        let records = actual["evaluation"]["suites"][0]["records"]
            .as_array()
            .unwrap();
        assert!(records.iter().all(|record| record["case_id"] != "case-8"));
        assert!(
            !serde_json::to_string(
                &records
                    .iter()
                    .map(|record| &record["requests"])
                    .collect::<Vec<_>>()
            )
            .unwrap()
            .contains("LABEL_SECRET")
        );
        assert!(
            std::fs::read_dir(root.join("reports"))
                .unwrap()
                .any(|entry| entry.unwrap().path().join("manifest.json").exists())
        );
    }
}

#[test]
#[ignore = "requires real server model; no fake HTTP/model backend"]
fn real_server_unmet_targets_and_unverified_write_never_modify_presets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(root, "noul");
    let preset = root.join("xdg/loupe/presets/rubric.toml");
    let original = std::fs::read(&preset).unwrap();
    let actual = decode(
        &run(
            root,
            &[
                "--target",
                "precision=1",
                "--min-support",
                "5",
                "--allow-unverified",
                "--strict",
                "--init-splits",
                "--warmup",
                "0",
            ],
        ),
        1,
    );
    assert_eq!(actual["status"], "unmet");
    assert!(actual["fitted"][0]["fit"]["selected"].is_null());
    let limited = decode(
        &run(
            root,
            &[
                "--target",
                "precision=0.5",
                "--allow-unverified",
                "--budget",
                "0",
                "--warmup",
                "0",
            ],
        ),
        4,
    );
    assert_eq!(limited["status"], "incomplete");
    assert_eq!(limited["evaluation"]["token_usage"]["attempts"], 0);
    assert!(limited["fitted"].as_array().unwrap().is_empty());
    assert_eq!(
        run(
            root,
            &["--target", "precision=0.5", "--allow-unverified", "--write"]
        )
        .status
        .code(),
        Some(69)
    );
    assert_eq!(std::fs::read(preset).unwrap(), original);
}
