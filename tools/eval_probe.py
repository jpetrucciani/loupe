"""Capture actual evaluator reports from an explicitly selected installed binary."""

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
    report: object | None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(".private/qualification/evidence/captured-eval.json"),
    )
    options = parser.parse_args()
    binary = options.binary.resolve()
    checkout = Path(__file__).resolve().parents[1]
    captures: list[Capture] = []
    rows = [
        {
            "id": f"case-{index}",
            "unit": (
                "Checkout is down and customers cannot pay."
                if index % 2 == 0
                else "Blue skies and sunshine all day."
            ),
            "label": {
                "available": index % 2 == 0,
                "topic": "commerce" if index % 2 == 0 else "weather",
                "severity": 2 if index % 2 == 0 else 0,
            },
            "source": "synthetic-functional",
            "reviewed": False,
        }
        for index in range(8)
    ]
    rubric = (
        "version=1\n"
        "[questions.available]\ntype='noul'\ninstructions='Is checkout unavailable?'\n"
        "[questions.topic]\ntype='choice'\ninstructions='Choose the main topic.'\n"
        "[questions.topic.criteria]\ncommerce='Checkout and payments'\nweather='Weather and clouds'\n"
        "[questions.severity]\ntype='score'\ninstructions='Rate checkout impact from zero to two.'\n"
        "criteria=['No impact','Minor impact','Checkout is entirely unavailable']\n"
    )
    with tempfile.TemporaryDirectory(prefix="loupe-eval-proof-") as temporary:
        root = Path(temporary)
        (root / ".git").mkdir()
        presets = root / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        (presets / "rubric.toml").write_text(rubric)
        (root / "cases.jsonl").write_text(
            "\n".join(json.dumps(row, ensure_ascii=False) for row in rows) + "\n"
        )
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("LOUPE_")
            or key in {"LOUPE_TOKENIZER", "LOUPE_TEST_TOKENIZER"}
        }
        environment["XDG_CONFIG_HOME"] = str(root / "xdg")
        common = [
            str(binary),
            "--config",
            str(checkout / "examples/config.toml"),
            "--profile",
            "server",
            "--url",
            options.url,
            "eval",
            "--labels",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "synthetic-functional-v1",
            "--json",
        ]

        def invoke(arguments: list[str], expected: int = 0) -> object | None:
            result = subprocess.run(
                common + arguments,
                cwd=root,
                env=environment,
                capture_output=True,
                timeout=60,
                check=False,
            )
            report = json.loads(result.stdout) if result.stdout else None
            captures.append(
                Capture(arguments, result.returncode, result.stderr.decode(), report)
            )
            if result.returncode != expected:
                raise ValueError(
                    f"unexpected exit {result.returncode}: {result.stderr.decode()}"
                )
            return report

        invoke(["--init-splits", "--dry-run", "--pack", "window:4"])
        invoke([], 69)
        unit = invoke(
            ["--allow-unverified", "--strict", "--pack", "unit", "--report", "reports"]
        )
        invoke(
            [
                "--allow-unverified",
                "--strict",
                "--pack",
                "window:4",
                "--report",
                "reports",
            ]
        )
        invoke(["--allow-unverified", "--strict", "--budget", "0"], 4)
        if not isinstance(unit, dict):
            raise ValueError("unit evaluation did not emit a report object")
        accuracy_cost = unit["suites"][0]["phases"][0]["usage"]["spent_input"]
        invoke(
            [
                "--allow-unverified",
                "--strict",
                "--pack",
                "unit",
                "--budget",
                str(accuracy_cost),
            ],
            4,
        )
        bundles = []
        for bundle in sorted((root / "reports").iterdir()):
            manifest = json.loads((bundle / "manifest.json").read_text())
            report_bytes = (bundle / "report.json").read_bytes()
            markdown_bytes = (bundle / "report.md").read_bytes()
            if len(report_bytes) != manifest["files"]["report.json"]["bytes"]:
                raise ValueError("report publication length mismatch")
            bundles.append(
                {
                    "manifest": manifest,
                    "json_sha256": hashlib.sha256(report_bytes).hexdigest(),
                    "markdown": markdown_bytes.decode(),
                    "directory_mode": oct(bundle.stat().st_mode & 0o777),
                    "file_modes": {
                        name: oct((bundle / name).stat().st_mode & 0o777)
                        for name in ["report.json", "report.md", "manifest.json"]
                    },
                }
            )
    sources = [
        "crates/loupe-cli/src/args.rs",
        "crates/loupe-cli/src/run.rs",
        "crates/loupe-cli/src/run/eval.rs",
        "crates/loupe-cli/src/run/eval/output.rs",
        "crates/loupe-cli/tests/eval_contracts.rs",
        "crates/loupe-core/src/suites.rs",
        "crates/loupe-core/src/report_store.rs",
        "crates/loupe-core/src/task_metrics.rs",
        "crates/loupe-core/src/budget.rs",
        "tools/eval_probe.py",
    ]
    evidence = {
        "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "endpoint": options.url + "/v1/systemone",
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "lock_sha256": hashlib.sha256(
            (checkout / "Cargo.lock").read_bytes()
        ).hexdigest(),
        "source_sha256": {
            name: hashlib.sha256((checkout / name).read_bytes()).hexdigest()
            for name in sources
        },
        "cases": rows,
        "preset_toml": rubric,
        "case_provenance": "authored functional cases, MIT; labels are not human-ratified held-out acceptance",
        "checks": [asdict(capture) for capture in captures],
        "published_bundles": bundles,
        "server_full_identity_available": False,
        "positive_live_cache_qualified": False,
        "python_reference_inference_qualified": False,
        "acceptance_gates_qualified": False,
    }
    options.out.parent.mkdir(parents=True, exist_ok=True)
    options.out.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    print(f"Saved {len(captures)} actual eval checks to {options.out}")


if __name__ == "__main__":
    main()
