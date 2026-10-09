"""Retain real CLI extraction requests, source spans and server responses."""

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

CASES = [
    (
        "dates",
        "The meeting is 2026-10-06; the settlement date is 2026-10-09.",
        "Which date is the settlement date?",
        "2026-10-09",
    ),
    (
        "money",
        "The offer was USD 12.50; the settlement paid was USD 40.00.",
        "Which amount was paid as the settlement?",
        "USD 40.00",
    ),
    (
        "semver",
        "The installed version is v1.2.3; the regression first appeared in v2.0.0-rc.1+build.7.",
        "Which version first introduced the regression?",
        "v2.0.0-rc.1+build.7",
    ),
    (
        "emails",
        "Support: help@example.com; billing: bills@example.org.",
        "Which email address is for billing?",
        "bills@example.org",
    ),
    (
        "hostnames",
        "Development: dev.example.com; production: api.example.net.",
        "Which hostname serves production?",
        "api.example.net",
    ),
    (
        "tickers",
        "The watchlist includes $MSFT; the purchased symbol was NASDAQ:AAPL.",
        "Which ticker was purchased?",
        "NASDAQ:AAPL",
    ),
]


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
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": endpoint + "/v1/systemone",
        "complete": False,
        "captures": captures,
        "proof_boundary": "Built CLI, real source/journal files and actual server inference. Synthetic functional fixtures; no normalization, held-out quality, native release packaging, authoritative cache identity or activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-extract-probe-") as temporary:
        directory = Path(temporary)

        def capture(
            name: str,
            flags: list[str],
            source: bytes,
            expected: int,
            *,
            json_output: bool = True,
        ) -> dict[str, Any]:
            journal = directory / (name + ".jsonl")
            command = [
                str(binary),
                "--config",
                str(root / "examples/config.toml"),
                "--profile",
                "server",
                "--url",
                endpoint,
                "extract",
                *flags,
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
            ]
            if "--resume" not in flags:
                command.extend(["--failed", str(journal)])
            else:
                journal = Path(flags[flags.index("--resume") + 1])
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
                "journal": [
                    json.loads(line) for line in journal.read_text().splitlines()
                ],
            }
            if json_output:
                item["records"] = [
                    json.loads(line) for line in result.stdout.splitlines()
                ]
            captures.append(item)
            save()
            assert result.returncode == expected, item
            assert int(metrics["input_counted"]) == int(
                metrics["input_reported"]
            ) + int(metrics["unknown_input_counted"]), item
            return item

        for kind, line, query, expected in CASES:
            source = f"東京🙂\r\n{line}\r\n".encode()
            item = capture(kind, [query, "--candidates", kind], source, 0)
            record = item["records"][0]
            assert record["unit"] == expected and record["id"] == "C2", record
            start = record["location"]["byte_start"]
            end = record["location"]["byte_end"]
            assert source[start:end] == expected.encode(), record
            assert record["probability"] == record["answer"]["probabilities"]["C2"]

        query = "Which email address is for billing?"
        flags = [query, "--candidates", "emails"]
        source = b"Support: help@example.com; billing: bills@example.org."
        plain = capture("plain-email", flags, source, 0, json_output=False)
        assert plain["stdout"] == "bills@example.org\n"
        capture("empty-catalog", flags, b"Birds sing outside.", 1)
        capture(
            "unrelated-candidate",
            flags,
            b"Birdwatching newsletter: bird@example.org. No billing contact is provided.",
            1,
        )
        zero = capture("zero-budget", [*flags, "--budget", "0"], source, 4)
        assert int(zero["summary"]["http_attempts"]) == 0
        uncertain = directory / "uncertain.txt"
        for action, code in [("keep", 0), ("drop", 1), ("stderr", 1), ("fail", 3)]:
            capture(
                "uncertain-" + action,
                [
                    *flags,
                    "--margin",
                    "1",
                    "--on-uncertain",
                    action,
                    "--uncertain",
                    str(uncertain),
                ],
                source,
                code,
            )
        assert uncertain.read_bytes() == source * 4
        policy_flags = [*flags, "--margin", "1", "--on-uncertain", "fail"]
        capture("policy-failure", policy_flags, source, 3)
        replay = capture(
            "policy-resume",
            [
                *policy_flags,
                "--resume",
                str(directory / "policy-failure.jsonl"),
                "--budget",
                "0",
            ],
            source,
            3,
        )
        assert int(replay["summary"]["http_attempts"]) == 0

    evidence["complete"] = True
    evidence["totals"] = {
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
    save()
    print(json.dumps({"captures": len(captures), "totals": evidence["totals"]}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--endpoint", required=True)
    options = parser.parse_args()
    run(options.binary.resolve(), options.out.resolve(), options.endpoint.rstrip("/"))


if __name__ == "__main__":
    main()
