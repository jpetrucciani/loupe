"""Capture installed Choice and independent Score selection against server."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import subprocess
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

QUERY = "Which line describes a current checkout outage?"
POSITIVE = (
    b"Checkout is unavailable and customers cannot pay.\r\n"
    b"Checkout requests fail with errors for every customer.\r\n"
    b"Checkout works normally.\r\nBirds sing outside."
)


def run(binary: Path, output: Path, endpoint: str) -> None:
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
    with tempfile.TemporaryDirectory(prefix="loupe-pick-probe-") as temporary:
        directory = Path(temporary)
        config = directory / "config.toml"
        config.write_text((root / "examples/config.toml").read_text())

        def capture(
            name: str,
            flags: list[str],
            source: bytes,
            expected: int,
            *,
            json_output: bool = True,
        ) -> dict[str, Any]:
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                "--url",
                endpoint,
                "pick",
                *flags,
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
            ]
            if json_output:
                command.append("--json")
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=environment,
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
            if json_output:
                item["records"] = [
                    json.loads(line) for line in result.stdout.splitlines()
                ]
            captures.append(item)
            assert result.returncode == expected, item
            assert int(metrics["input_counted"]) == int(
                metrics["input_reported"]
            ) + int(metrics["unknown_input_counted"]), item
            for record in item.get("records", []):
                start = record["location"]["byte_start"]
                end = record["location"]["byte_end"]
                assert source[start:end] == record["unit"].encode(), record
            return item

        choice = capture("choice-top-two", [QUERY, "-n", "2"], POSITIVE, 0)
        assert len(choice["records"]) == 2
        for record in choice["records"]:
            assert record["probability_scope"] == "all-candidates"
            assert (
                record["probability"] == record["answer"]["probabilities"][record["id"]]
            )
        plain = capture("plain", [QUERY], POSITIVE, 0, json_output=False)
        assert plain["stdout"].encode() in POSITIVE.splitlines(keepends=True)[:2]

        independent = capture(
            "independent-top-two", [QUERY, "--independent", "-n", "2"], POSITIVE, 0
        )
        assert {record["id"] for record in independent["records"]} == {"L1", "L2"}
        previous = math.inf
        for record in independent["records"]:
            assert record["answer"]["type"] == "score"
            assert "probability" not in record
            assert record["probability_scope"] == "independent-candidate-score"
            probabilities = record["answer"]["probabilities"]
            mean = sum(int(key) * p for key, p in probabilities.items()) / sum(
                probabilities.values()
            )
            assert math.isclose(record["ranking_score"], mean, abs_tol=1e-12)
            assert record["ranking_score"] <= previous
            previous = record["ranking_score"]
            history = record["provenance"]["history"]
            items = history["plans"][0]["items"]
            assert len(items) == 4
            assert all(len(item["bracket"]["candidate_ids"]) == 1 for item in items)
            index = next(
                i for i, item in enumerate(items) if item["id"] == record["id"]
            )
            assert (
                record["answer"]
                == history["evaluations"][0][index]["response"]["answers"][
                    "selection-relevance"
                ]
            )

        capture(
            "score-preset",
            ["-p", "relevant", "--arg", f"query={QUERY}", "--independent"],
            POSITIVE,
            0,
        )
        absent = b"Birds sing outside.\nBlue skies."
        capture("choice-absent", [QUERY], absent, 1)
        capture("independent-absent", [QUERY, "--independent"], absent, 1)
        simple = (
            b"Checkout is unavailable and customers cannot pay.\nBirds sing outside."
        )
        uncertain = directory / "uncertain.txt"
        for action, code in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)]:
            capture(
                f"independent-uncertain-{action}",
                [
                    QUERY,
                    "--independent",
                    "--margin",
                    "1",
                    "--on-uncertain",
                    action,
                    "--uncertain",
                    str(uncertain),
                ],
                simple,
                code,
            )
        assert uncertain.read_bytes() == simple * 4
        zero = capture(
            "zero-budget", [QUERY, "--independent", "--budget", "0"], simple, 4
        )
        assert int(zero["summary"]["http_attempts"]) == 0
        journal = directory / "uncertain.jsonl"
        flags = [QUERY, "--independent", "--margin", "1", "--on-uncertain", "fail"]
        capture("policy-failure", [*flags, "--failed", str(journal)], simple, 3)
        frames = [json.loads(line) for line in journal.read_text().splitlines()]
        replay = capture(
            "policy-resume",
            [*flags, "--resume", str(journal), "--budget", "0"],
            simple,
            3,
        )
        assert int(replay["summary"]["http_attempts"]) == 0

        totals = {
            key: sum(int(item["summary"][key]) for item in captures)
            for key in [
                "attempts",
                "http_attempts",
                "input_counted",
                "input_reported",
                "unknown_input_counted",
                "cache_hits",
            ]
        }
        evidence = {
            "schema_version": 1,
            "captured_at_utc": datetime.now(UTC).isoformat(),
            "binary": str(binary),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "endpoint": endpoint + "/v1/systemone",
            "captures": captures,
            "uncertain_journal": frames,
            "totals": totals,
            "proof_boundary": "Installed CLI with actual server inference and real source/journal files. Synthetic functional cases; no held-out retrieval quality, authoritative cache identity, target release or activation claim.",
        }
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")
        print(json.dumps({"captures": len(captures), "totals": totals}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--endpoint", required=True)
    options = parser.parse_args()
    run(options.binary.resolve(), options.out.resolve(), options.endpoint.rstrip("/"))


if __name__ == "__main__":
    main()
