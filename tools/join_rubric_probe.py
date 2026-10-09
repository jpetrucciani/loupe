"""Validate trusted pair rubric templates, context/signals and saved replay."""

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

from join_probe import entity, lines


def run(binary: Path, output: Path) -> None:
    endpoint = os.environ.get("LOUPE_TEST_URL", "").rstrip("/")
    if not endpoint:
        raise SystemExit("Set LOUPE_TEST_URL to the actual server base URL")
    output.parent.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parent.parent
    environment = dict(os.environ)
    for key in [
        "LOUPE_API_KEY",
        "LOUPE_URL",
        "LOUPE_PATH",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
    ]:
        environment.pop(key, None)
    rubric = {
        "questions": {
            "similarity": {
                "type": "score",
                "instructions": "How strongly do left.record and right.record refer to the same {{arg.kind}} by name and location? Ignore unrelated source record IDs.",
                "criteria": [
                    "Different companies",
                    "Ambiguous possible match",
                    "Same company name and location",
                ],
            },
            "same_entity": {
                "type": "noul",
                "instructions": "Do left.record and right.record describe the same {{arg.kind}} according to their name and location? Ignore source record IDs.",
            },
        },
        "signals": [
            {
                "id": "agreement",
                "instructions": "Do the company names in left.record and right.record agree?",
                "policy": {"threshold": 0.8, "margin": 0.05},
            }
        ],
        "context": "The records come from two directories. Source row IDs do not identify the business.",
    }
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "question_source": rubric,
        "captures": [],
        "complete": False,
        "proof_boundary": "Actual server inference with an explicit trusted pair question file, argument template, static/CLI context and declared companion policy. Full raw pair/journal provenance and inference-free replay. Synthetic functional case; no pair calibration, held-out quality, full identity/cache or release packaging claim.",
    }
    with tempfile.TemporaryDirectory(prefix="loupe-join-rubric-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        questions = directory / "pair.json"
        questions.write_text(json.dumps(rubric))
        context = directory / "context.txt"
        context.write_text(
            "Use company name and city/state for identity; formatting differences are allowed."
        )
        evidence["cli_context"] = context.read_text()
        right = directory / "right.jsonl"
        right.write_bytes(lines([entity("right-row")]))
        source = lines([entity("left-row")])
        journal = directory / "run.jsonl"
        base = [
            str(binary),
            "--config",
            str(config),
            "--profile",
            "server",
            "join",
            "-",
            str(right),
            "--on",
            "name,location",
            "-f",
            str(questions),
            "--arg",
            "kind=company",
            "--context",
            str(context),
            "--why",
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ]
        for name, extra in [
            ("rubric-context-signals", ["--failed", str(journal)]),
            ("rubric-completed-resume", ["--resume", str(journal), "--budget", "0"]),
        ]:
            command = [*base, *extra]
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=environment,
                cwd=directory,
                timeout=75,
                check=False,
            )
            summary: dict[str, str] = {}
            for line in result.stderr.decode().splitlines():
                if line.startswith("loupe: units="):
                    summary = dict(
                        piece.split("=", 1)
                        for piece in line.removeprefix("loupe: ").split()
                    )
            item = {
                "name": name,
                "command": command,
                "input": source.decode(),
                "right_input": right.read_text(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
                "summary": summary,
                "journal": [
                    json.loads(line) for line in journal.read_text().splitlines()
                ],
            }
            evidence["captures"].append(item)
            output.write_text(json.dumps(evidence, indent=2) + "\n")
            assert result.returncode == 0, item
            assert int(summary["input_counted"]) == int(summary["input_reported"]), item
            if name == "rubric-completed-resume":
                assert result.stdout == b"" and int(summary["http_attempts"]) == 0, item
            else:
                row = json.loads(result.stdout)
                assert row["kind"] == "matched", item
                pair = row["evaluated_pairs"][0]
                assert pair["question_order"] == [
                    "similarity",
                    "same_entity",
                    "signal:agreement",
                ], item
                assert (
                    pair["counts"]["input_tokens"]
                    == pair["response"]["usage"]["input_tokens"]
                ), item
                assert "same company" in json.dumps(
                    pair["request"]["questions"]["same_entity"]
                ), item
                assert "Source row IDs" in json.dumps(
                    pair["request"]["questions"]
                ), item
                assert "city/state" in json.dumps(pair["request"]["questions"]), item
                raw = pair["response"]["answers"]["signal:agreement"]["noul"]
                assessment = pair["decision"]["companions"]["signal:agreement"]
                assert assessment["selected"] == {"boolean": raw >= 0.8}, item
                assert assessment["uncertain"] == (abs(raw - 0.8) < 0.05), item
                item["row"] = row
        evidence["complete"] = True
        evidence["totals"] = {
            key: sum(int(item["summary"][key]) for item in evidence["captures"])
            for key in [
                "attempts",
                "http_attempts",
                "input_counted",
                "input_reported",
                "unknown_input_counted",
                "cache_hits",
            ]
        }
        output.write_text(json.dumps(evidence, indent=2) + "\n")
        print(json.dumps(evidence["totals"]))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".private/qualification/evidence/server-join-rubric.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
