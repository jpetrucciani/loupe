"""Capture installed find behavior against the actual server endpoint."""

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

QUERY = "Which line describes a current checkout outage?"
POSITIVE = (
    "東京🙂\r\n\r\nCheckout works normally.\r\n"
    "Checkout is unavailable and customers cannot pay.\r\n"
    "Birds sing outside.\r\nThe garden needs water.\r\n"
    "Blue skies and sunshine.\r\nA football team won its match.\r\n"
    "The database backup completed.\r\nClouds will clear tonight.\r\n"
    "Traffic is moving smoothly."
).encode()


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
    with tempfile.TemporaryDirectory(prefix="loupe-find-probe-") as temporary:
        directory = Path(temporary)
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("max_options = 512", "max_options = 4")
        )

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
                "find",
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
            return item

        positive = capture("unicode-tournament", [QUERY], POSITIVE, 0)
        selected = positive["records"][0]
        assert selected["id"] == "L4", selected
        start, end = (
            selected["location"]["byte_start"],
            selected["location"]["byte_end"],
        )
        assert POSITIVE[start:end] == selected["unit"].encode()
        assert selected["answer"]["probabilities"]["L4"] == selected["probability"]
        assert selected["probability_scope"] == "final-surviving-bracket"
        capture(
            "license-source",
            ["Which line says the software is provided AS IS?", str(root / "LICENSE")],
            b"",
            0,
        )
        capture("absent", [QUERY], b"Blue skies and sunshine.\nBirds sing outside.", 1)
        simple = b"Checkout works normally.\r\nCheckout is unavailable and customers cannot pay.\r\nBirds sing outside."
        capture("plain", [QUERY], simple, 0, json_output=False)
        capture(
            "preset-arguments", ["-p", "answers", "--arg", f"query={QUERY}"], simple, 0
        )
        uncertain = directory / "uncertain.txt"
        for action, code in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)]:
            capture(
                f"uncertain-{action}",
                [
                    QUERY,
                    "--min-confidence",
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
        capture("zero-budget", [QUERY, "--budget", "0"], simple, 4)
        journal = directory / "finished.jsonl"
        capture("journal-complete", [QUERY, "--failed", str(journal)], simple, 0)
        completed_frames = [
            json.loads(line) for line in journal.read_text().splitlines()
        ]
        replay = capture(
            "finished-resume",
            [QUERY, "--resume", str(journal), "--budget", "0"],
            simple,
            0,
        )
        assert not replay["stdout"] and int(replay["summary"]["http_attempts"]) == 0
        policy_journal = directory / "uncertain.jsonl"
        policy_flags = [QUERY, "--min-confidence", "1", "--on-uncertain", "fail"]
        capture(
            "policy-failure",
            [*policy_flags, "--failed", str(policy_journal)],
            simple,
            3,
        )
        policy_frames = [
            json.loads(line) for line in policy_journal.read_text().splitlines()
        ]
        replay = capture(
            "policy-resume",
            [*policy_flags, "--resume", str(policy_journal), "--budget", "0"],
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
            "completed_journal": completed_frames,
            "uncertain_journal": policy_frames,
            "totals": totals,
            "proof_boundary": "Installed CLI with real server requests and real source/journal files. Synthetic functional cases plus repository MIT license; not held-out quality, authoritative cache identity, target release or activation proof.",
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
