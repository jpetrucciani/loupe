"""Capture companion signal requests, explanations and packing comparisons."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


def run(binary: Path, output: Path) -> None:
    endpoint = os.environ.get("LOUPE_TEST_URL", "").rstrip("/")
    if not endpoint:
        raise SystemExit("Set LOUPE_TEST_URL to the actual server base URL")
    root = Path(__file__).resolve().parent.parent
    captures: list[dict[str, Any]] = []
    environment = dict(os.environ)
    for key in [
        "LOUPE_API_KEY",
        "LOUPE_URL",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
    ]:
        environment.pop(key, None)
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "complete": False,
        "captures": captures,
        "proof_boundary": "Built GNU x86_64 CLI, actual server inference and real private files/journals/report bundles. Synthetic functional fixtures; no held-out signal calibration, full live identity/cache, native release packaging or activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-signals-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        presets = directory / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        signals = [
            {
                "id": "impact",
                "instructions": "Does the state explicitly say customers cannot use checkout or pay?",
                "policy": {"threshold": 0.7, "margin": 0.05},
            },
            {
                "id": "retried",
                "instructions": "Does the state explicitly say a retry or recovery succeeded?",
            },
        ]
        documents = {
            "severity": {
                "type": "choice",
                "instructions": "Classify the current service condition.",
                "criteria": {
                    "page": "An unresolved outage prevents customers checking out.",
                    "noise": "No current outage, or the service recovered.",
                },
            },
            "outage": {
                "type": "noul",
                "instructions": "Is checkout currently unavailable to customers?",
            },
            "retrieval": {
                "type": "noul",
                "instructions": "Which line describes a current checkout outage?",
            },
            "rank": {
                "type": "score",
                "instructions": "How directly does this answer: Which line describes a current checkout outage?",
                "criteria": ["Unrelated", "Direct evidence"],
            },
            "billing": {
                "type": "noul",
                "instructions": "Which email address is for billing?",
            },
        }
        for name, document in documents.items():
            document["signals"] = signals
            (presets / (name + ".json")).write_text(json.dumps(document))

        def capture(
            name: str,
            args: list[str],
            source: bytes,
            expected: int,
            *,
            json_output: bool = True,
        ) -> dict[str, Any]:
            evaluation = args[0] == "eval"
            journal = directory / (name + ".jsonl")
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                *args,
                "--strict",
                "--no-cache",
                "--deadline",
                "60s",
            ]
            if not evaluation:
                command.append("--summary")
                if "--resume" not in args:
                    command.extend(["--failed", str(journal)])
                else:
                    journal = Path(args[args.index("--resume") + 1])
            if json_output:
                command.append("--json")
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=environment,
                cwd=directory,
                timeout=75,
                check=False,
            )
            metrics: dict[str, str] = {}
            for line in result.stderr.decode().splitlines():
                if line.startswith("loupe: units="):
                    metrics = dict(
                        piece.split("=", 1)
                        for piece in line.removeprefix("loupe: ").split()
                    )
            item: dict[str, Any] = {
                "name": name,
                "command": command,
                "input": source.decode(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
                "summary": metrics,
            }
            if not evaluation:
                item["journal"] = [
                    json.loads(line) for line in journal.read_text().splitlines()
                ]
            if json_output:
                item["records"] = (
                    [json.loads(result.stdout)]
                    if evaluation
                    else [json.loads(line) for line in result.stdout.splitlines()]
                )
            captures.append(item)
            save()
            assert result.returncode == expected, item
            if metrics:
                assert int(metrics["input_counted"]) == int(
                    metrics["input_reported"]
                ) + int(metrics["unknown_input_counted"]), item
            return item

        source = b"Checkout is unavailable and customers cannot pay.\nCheckout recovered after a successful retry.\n"
        without = capture("tag-without-why", ["tag", "-p", "severity"], source, 0)
        assert all("signals" not in record for record in without["records"])
        window = capture(
            "tag-window-why",
            ["tag", "-p", "severity", "--why", "--pack", "window:2"],
            source,
            0,
        )
        for index, record in enumerate(window["records"], 1):
            signal = record["signals"]["impact"]
            assert signal["field"] == f"L{index}/signal:impact"
            assert signal["probability"] == signal["answer"]["noul"]
        keep = capture(
            "keep-plain-why",
            ["keep", "-p", "outage", "--why"],
            source,
            0,
            json_output=False,
        )
        assert keep["stdout"] == source.splitlines(keepends=True)[0].decode()
        capture("is-why", ["is", "-p", "outage", "--why"], source.splitlines()[0], 0)
        capture("quiet-json-why", ["-q", "tag", "-p", "severity", "--why"], source, 0)
        document = b"Checkout works normally.\nCheckout is unavailable and customers cannot pay.\nBirds sing outside."
        for verb in ["find", "pick"]:
            item = capture(
                verb + "-why", [verb, "-p", "retrieval", "--why"], document, 0
            )
            assert item["records"][0]["id"] == "L2"
        independent = capture(
            "pick-independent-why",
            ["pick", "-p", "rank", "--independent", "--why"],
            document,
            0,
        )
        assert independent["records"][0]["id"] == "L2"
        capture(
            "extract-why",
            ["extract", "-p", "billing", "--candidates", "emails", "--why"],
            b"Support: help@example.com; billing: bills@example.org.",
            0,
        )
        policy = [
            "find",
            "-p",
            "retrieval",
            "--why",
            "--margin",
            "1",
            "--on-uncertain",
            "fail",
        ]
        failed = capture("uncertainty-failure", policy, document, 3)
        assert "[impact " in failed["stderr"]
        replay = capture(
            "policy-resume",
            [
                *policy,
                "--resume",
                str(directory / "uncertainty-failure.jsonl"),
                "--budget",
                "0",
            ],
            document,
            3,
        )
        assert int(replay["summary"]["http_attempts"]) == 0
        zero = capture(
            "zero-budget",
            ["tag", "-p", "severity", "--why", "--budget", "0"],
            source,
            4,
        )
        assert int(zero["summary"]["http_attempts"]) == 0
        dataset = directory / "cases.jsonl"
        dataset.write_text(
            "\n".join(
                json.dumps(
                    {
                        "id": f"case-{index}",
                        "unit": source.splitlines()[index % 2].decode(),
                        "label": index % 2 == 0,
                        "source": "synthetic functional signal fixture",
                    }
                )
                for index in range(8)
            )
            + "\n"
        )
        report_directory = directory / "reports"
        comparison = capture(
            "packing-sweep",
            [
                "eval",
                str(dataset),
                "-p",
                "outage",
                "--dataset-revision",
                "signals-fixture-v1",
                "--init-splits",
                "--sweep-pack",
                "unit,window:2",
                "--warmup",
                "0",
                "--allow-unverified",
                "--report",
                str(report_directory),
            ],
            b"",
            0,
        )
        report = comparison["records"][0]
        for variant in report["variants"]:
            suite = variant["report"]["suites"][0]
            assert len(suite["questions"]) == 1
            assert suite["questions"][0]["field"] == "outage"
            for record in suite["records"]:
                assert len(record["questions"]) == 3
                for field in ["signal:impact", "signal:retried"]:
                    assert (
                        record["binding"][field]
                        in record["evaluation"]["response"]["answers"]
                    )
        evidence["reports"] = {
            str(path.relative_to(report_directory)): path.read_text()
            for path in report_directory.rglob("*")
            if path.is_file()
        }
        evidence["presets"] = documents
        evidence["complete"] = True
        totals = {
            key: sum(int(item["summary"][key]) for item in captures if item["summary"])
            for key in [
                "attempts",
                "http_attempts",
                "input_counted",
                "input_reported",
                "unknown_input_counted",
                "cache_hits",
            ]
        }
        evidence["classification_totals"] = totals
        evidence["evaluation_usage"] = report["token_usage"]
        save()
        print(
            json.dumps(
                {
                    "captures": len(captures),
                    "classification_totals": totals,
                    "evaluation_usage": report["token_usage"],
                }
            )
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    options = parser.parse_args()
    run(options.binary.resolve(), options.out.resolve())


if __name__ == "__main__":
    main()
