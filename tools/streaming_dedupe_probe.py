"""Capture actual server bounded streaming dedupe and finite bridge comparison."""

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

QUESTION = "Do both records report an outage of the same service (ALPHA or BRAVO)? A service described as healthy is not in outage. Ignore the unrelated node. A record with both services down matches a record with either service down."
BRIDGE = (
    b"Incident ALPHA is down. Incident BRAVO is healthy. The unrelated node is healthy.\n"
    b"Incident ALPHA is down. Incident BRAVO is down. The unrelated node is healthy.\n"
    b"Incident ALPHA is healthy. Incident BRAVO is down. The unrelated node is healthy.\n"
)
SIMPLE = (
    "Do the two records mention at least one identical incident label (ALPHA or BRAVO)?"
)


def run(binary: Path, output: Path) -> None:
    endpoint = os.environ.get("LOUPE_TEST_URL", "").rstrip("/")
    if not endpoint:
        raise SystemExit("Set LOUPE_TEST_URL to the actual server base URL")
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
    captures: list[dict[str, Any]] = []
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "complete": False,
        "captures": captures,
        "proof_boundary": "Actual legacy server pair decisions, functional reader/expiry/capacity/uncertainty evidence and finite-versus-streaming bridge semantics. Bounded streaming recovery, held-out LSH/dedupe quality, authoritative live identity/cache and new native release artifacts remain open.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-streaming-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )

        def capture(
            name: str,
            source: bytes,
            options: list[str],
            expected: int = 0,
            *,
            question: str = SIMPLE,
            json_output: bool = True,
        ) -> dict[str, Any]:
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                "-vv",
                "dedupe",
                question,
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
                *options,
            ]
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
            summary: dict[str, str] = {}
            for line in result.stderr.decode().splitlines():
                if line.startswith("loupe: units="):
                    summary = dict(
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
                "summary": summary,
            }
            if json_output:
                item["records"] = [
                    json.loads(line) for line in result.stdout.splitlines()
                ]
            captures.append(item)
            save()
            assert result.returncode == expected, item
            assert int(summary["input_counted"]) == int(
                summary["input_reported"]
            ) + int(summary["unknown_input_counted"]), item
            if json_output:
                for row in item["records"]:
                    for pair in row.get("pairs", []):
                        assert pair["counts"]["exact"], item
                        if "response" in pair:
                            assert (
                                pair["counts"]["input_tokens"]
                                == pair["response"]["usage"]["input_tokens"]
                            ), item
            return item

        item = capture(
            "streaming-bridge", BRIDGE, ["--window", "10"], question=QUESTION
        )
        assert [row["index"] for row in item["records"]] == [1, 3], item
        assert int(item["summary"]["http_attempts"]) == 2, item
        assert not item["records"][1]["pairs"][0]["assessment"]["selected"][
            "boolean"
        ], item
        assert (
            "pair 1:2 assessment=" in item["stderr"]
            and "pair 1:3 assessment=" in item["stderr"]
        ), item
        item = capture("finite-bridge", BRIDGE, [], question=QUESTION)
        assert len(item["records"]) == 1 and [
            member["index"] for member in item["records"][0]["members"]
        ] == [1, 2, 3], item
        repeated = b"Incident ALPHA is delayed.\r\n" * 4
        for window, expected_indices in [(1, [1, 3]), (2, [1, 4])]:
            item = capture(f"unit-window-{window}", repeated, ["--window", str(window)])
            assert [row["index"] for row in item["records"]] == expected_indices, item
        item = capture("physical-blank-expiry", b"ALPHA\n\nALPHA\n", ["--window", "1"])
        assert [row["index"] for row in item["records"]] == [1, 3] and int(
            item["summary"]["http_attempts"]
        ) == 0, item
        item = capture(
            "time-capacity-fatal",
            b"alpha\nbravo\n",
            ["--window-time", "60s", "--index-capacity", "1"],
            65,
        )
        assert (
            len(item["records"]) == 1 and int(item["summary"]["http_attempts"]) == 0
        ), item
        item = capture(
            "byte-capacity-fatal",
            b"ALPHA\n",
            ["--window", "10", "--max-index-bytes", "1"],
            65,
        )
        assert not item["records"] and int(item["summary"]["http_attempts"]) == 0, item
        for action, code, count in [
            ("keep", 0, 1),
            ("drop", 0, 2),
            ("stderr", 0, 2),
            ("fail", 3, 1),
        ]:
            item = capture(
                "uncertain-" + action,
                b"ALPHA\nALPHA\n",
                ["--window", "2", "--margin", "1", "--on-uncertain", action],
                code,
            )
            assert (
                len(item["records"]) == count
                and int(item["summary"]["http_attempts"]) == 1
            ), item
        item = capture(
            "budget-prefix", b"ALPHA\nALPHA\n", ["--window", "2", "--budget", "0"], 4
        )
        assert (
            len(item["records"]) == 1 and int(item["summary"]["http_attempts"]) == 0
        ), item
        item = capture(
            "plain-crlf",
            repeated[: len(repeated) // 2],
            ["--window", "2"],
            json_output=False,
        )
        assert item["stdout"].encode() == repeated.splitlines(keepends=True)[0], item
        item = capture(
            "plain-nul",
            b"ALPHA\0ALPHA\0",
            ["--window", "2", "-I", "null"],
            json_output=False,
        )
        assert item["stdout"].encode() == b"ALPHA\0", item
        structured = b'{"id":1,"incident":"ALPHA"}\n' * 2
        item = capture(
            "jsonl-full-state-projection",
            structured,
            [
                "--window",
                "2",
                "-I",
                "jsonl",
                "--field",
                "id",
                "--margin",
                "1",
                "--on-uncertain",
                "drop",
            ],
        )
        assert len(item["records"]) == 2, item
        assert item["records"][1]["pairs"][0]["request"]["state"]["left"]["record"] == {
            "id": 1,
            "incident": "ALPHA",
        }, item
        item = capture(
            "csv-selected-state",
            b'id,text\r\na,"ALPHA\r\ndelayed"\r\nb,"ALPHA\r\ndelayed"\r\n',
            ["--window", "2", "-I", "csv", "--select", "text"],
            json_output=False,
        )
        assert item["stdout"].encode() == b'id,text\r\na,"ALPHA\r\ndelayed"\r\n', item
        item = capture(
            "prospective-plan",
            b"ALPHA\n" * 4,
            ["--window", "2", "--max-candidates", "1", "--dry-run"],
        )
        assert (
            len(item["records"]) == 4
            and item["records"][-1]["history"]["band_postings"] == 24
        ), item
        assert item["records"][-1]["expired"] == [1], item
        evidence["complete"] = True
        save()
    print(json.dumps({"complete": True, "captures": len(captures)}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument("--out", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.binary.resolve(), arguments.out.resolve())


if __name__ == "__main__":
    main()
