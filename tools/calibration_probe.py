"""Capture calibration contracts against a real endpoint and installed binary."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import subprocess
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass
class Capture:
    arguments: list[str]
    exit_code: int
    stderr: str
    report: object


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--tokenizer", type=Path, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(".private/qualification/evidence/server-calibration.json"),
    )
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    tokenizer = options.tokenizer.resolve(strict=True)
    checkout = Path(__file__).resolve().parents[1]
    environment = os.environ.copy()
    environment["LOUPE_TOKENIZER"] = str(tokenizer)
    for name in (
        "LOUPE_URL",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
        "LOUPE_API_KEY",
        "LOUPE_PRESET_PATH",
        "LOUPE_CONFIG",
    ):
        environment.pop(name, None)
    captures: list[Capture] = []
    with tempfile.TemporaryDirectory(prefix="loupe-calibration-") as temporary:
        root = Path(temporary)
        (root / ".git").mkdir()
        presets = root / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        environment["XDG_CONFIG_HOME"] = str(root / "xdg")
        environment["LOUPE_CACHE_DIR"] = str(root / "cache")
        definitions = {
            "boolean": "version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n",
            "topic": "version=1\ntype='choice'\ninstructions='Choose the topic.'\n[criteria]\ncommerce='Checkout and payments'\nweather='Weather and clouds'\n",
            "mixed": "version=1\n[questions.available]\ntype='noul'\ninstructions='Is checkout unavailable?'\n[questions.topic]\ntype='choice'\ninstructions='Choose the topic.'\n[questions.topic.criteria]\ncommerce='Checkout and payments'\nweather='Weather and clouds'\n",
        }
        for name, text in definitions.items():
            (presets / f"{name}.toml").write_text(text)
            rows = []
            for index in range(9):
                boolean = index % 2 == 0
                topic = "commerce" if boolean else "weather"
                label = boolean if name == "boolean" else topic
                if name == "mixed":
                    label = {"available": boolean, "topic": topic}
                case = {
                    "id": f"case-{index}",
                    "unit": (
                        "Checkout is down and customers cannot pay."
                        if boolean
                        else "Blue skies and sunshine all day."
                    ),
                    "label": label,
                    "source": "synthetic-functional" if index < 8 else "review",
                    "reviewed": index == 8,
                    "notes": "LABEL_SECRET_DO_NOT_SEND",
                }
                if index == 8:
                    case["sampling"] = "uniform-audit"
                rows.append(case)
            (root / f"{name}.jsonl").write_text(
                "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows)
            )
        config = (
            f"[profile.server]\nbase_url={json.dumps(options.url)}\npath='/v1/systemone'\n"
            "model='clef-flash'\nencoder='clef-v1'\ntokenizer='hf:Cloudflare/clef-flash'\n"
            "tokenizer_revision='17f0b0ad64efb65d273590632833508766b2aae6'\nconcurrency=2\n"
        )
        (root / "config.toml").write_text(config)

        def capture(name: str, arguments: list[str], expected: int = 0) -> object:
            complete = [
                "--config",
                str(root / "config.toml"),
                "--profile",
                "server",
                "calibrate",
                "--labels",
                f"{name}.jsonl",
                "-p",
                name,
                "--dataset-revision",
                "functional-v1",
                "--init-splits",
                "--warmup",
                "0",
                "--json",
                *arguments,
            ]
            result = subprocess.run(
                [str(binary), *complete],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
                timeout=240,
            )
            if result.returncode != expected:
                raise RuntimeError(
                    f"exit {result.returncode}, expected {expected}: {result.stderr} {result.stdout}"
                )
            report = json.loads(result.stdout) if result.stdout.strip() else None
            captures.append(Capture(complete, result.returncode, result.stderr, report))
            return report

        plan = capture(
            "boolean", ["--target", "precision=0.9", "--dry-run", "--budget", "0"]
        )
        assert plan["status"] == "plan"
        assert plan["evaluation"]["token_usage"]["attempts"] == 0
        diagnostics = ["--allow-unverified", "--strict", "--no-cache"]
        fitted = [
            capture(
                "boolean",
                [
                    *diagnostics,
                    "--target",
                    "precision=0.5",
                    "--report",
                    "boolean-reports",
                ],
            ),
            capture(
                "topic",
                [
                    *diagnostics,
                    "--target",
                    "precision:commerce=0.5",
                    "--max-uncertain",
                    "50%",
                    "--report",
                    "topic-reports",
                ],
            ),
            capture(
                "mixed",
                [
                    *diagnostics,
                    "--field",
                    "topic",
                    "--target",
                    "precision=0.5",
                    "--pack",
                    "window:4",
                    "--report",
                    "mixed-reports",
                ],
            ),
            capture(
                "boolean",
                [
                    *diagnostics,
                    "--target",
                    "precision=0",
                    "--partition",
                    "captured",
                    "--report",
                    "captured-reports",
                ],
            ),
        ]
        for report in fitted:
            assert report["status"] == "provisional"
            assert report["evaluation"]["identity"]["verified"] is False
            assert report["fitted"][0]["proposed_preset"] is None
            assert report["fitted"][0]["written"] is None
            assert report["fitted"][0]["fit"]["selected"]["accepted"] >= 1
            usage = report["evaluation"]["token_usage"]
            assert (
                usage["spent_input"] == usage["reported_input"] + usage["unknown_input"]
            )
        assert fitted[3]["evaluation"]["suites"][0]["selected_case_ids"] == ["case-8"]
        unmet = capture(
            "boolean",
            [
                *diagnostics,
                "--target",
                "precision=1",
                "--min-support",
                "5",
                "--report",
                "unmet-reports",
            ],
            1,
        )
        assert unmet["status"] == "unmet"
        assert unmet["fitted"][0]["fit"]["selected"] is None
        limited = capture(
            "boolean",
            [
                *diagnostics,
                "--target",
                "precision=0.5",
                "--budget",
                "0",
                "--report",
                "limited-reports",
            ],
            4,
        )
        assert limited["status"] == "incomplete"
        assert limited["evaluation"]["token_usage"]["attempts"] == 0
        original = (presets / "boolean.toml").read_bytes()
        capture(
            "boolean",
            ["--target", "precision=0.5", "--allow-unverified", "--write"],
            69,
        )
        assert (presets / "boolean.toml").read_bytes() == original
        published = []
        for manifest in sorted(root.glob("*-reports/*/manifest.json")):
            published.append(
                {
                    "bundle": str(manifest.parent.relative_to(root)),
                    "manifest": json.loads(manifest.read_text()),
                }
            )
        assert len(published) == 6
    sources = [
        "crates/loupe-core/src/calibration.rs",
        "crates/loupe-core/src/calibration_pin.rs",
        "crates/loupe-core/src/policy.rs",
        "crates/loupe-core/src/preset_store.rs",
        "crates/loupe-core/src/templates.rs",
        "crates/loupe-cli/src/args.rs",
        "crates/loupe-cli/src/run.rs",
        "crates/loupe-cli/src/run/eval.rs",
        "crates/loupe-cli/src/run/eval/calibrate.rs",
        "crates/loupe-cli/src/preset_commands.rs",
        "tools/calibration_probe.py",
    ]
    evidence = {
        "captured_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "endpoint": f"{options.url}/v1/systemone",
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "sources_sha256": {
            name: hashlib.sha256((checkout / name).read_bytes()).hexdigest()
            for name in sources
        },
        "captures": [asdict(capture) for capture in captures],
        "published_bundles": published,
        "proof_boundary": "Synthetic functional cases, not ratified labels. Real model requests and provisional fitting; no authoritative identity, positive live calibrated writes/application, held-out acceptance, activation or GPU reassignment.",
    }
    options.out.parent.mkdir(parents=True, exist_ok=True)
    options.out.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    print(options.out)


if __name__ == "__main__":
    main()
