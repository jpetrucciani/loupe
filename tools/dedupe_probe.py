"""Capture finite transitive dedupe against server and real recovery journals."""

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

QUESTION = (
    "Do the two records mention at least one identical incident label (ALPHA or BRAVO)?"
)
SOURCE = (
    b"Incident ALPHA: checkout is unavailable.\n"
    b"Incident ALPHA: checkout is unavailable. Incident BRAVO: email delivery is delayed.\n"
    b"Incident BRAVO: email delivery is delayed.\n"
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
        "proof_boundary": "Actual server inference and real finite catalogs/journals. Synthetic functional transitive, representative, uncertainty and recovery fixtures, not held-out dedupe quality. Cache is disabled because live full identity discovery is unavailable. No host or git mutation, native release rebuild or streaming dedupe claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-dedupe-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
            .replace("concurrency = 4", "concurrency = 1")
        )

        def capture(
            name: str,
            source: bytes,
            options: list[str],
            expected: int = 0,
            *,
            question: str | None = QUESTION,
            json_output: bool = True,
        ) -> dict[str, Any]:
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                "dedupe",
            ]
            if question is not None:
                command.append(question)
            command.extend(["--strict", "--no-cache", "--summary", "--deadline", "60s"])
            if json_output:
                command.append("--json")
            command.extend(options)
            journal = directory / (name + ".jsonl")
            if "--resume" in options:
                journal = Path(options[options.index("--resume") + 1])
            elif "--dry-run" not in options:
                command.extend(["--failed", str(journal)])
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
                "journal": (
                    [json.loads(line) for line in journal.read_text().splitlines()]
                    if journal.exists()
                    else []
                ),
            }
            if json_output:
                item["records"] = [
                    json.loads(line) for line in result.stdout.splitlines()
                ]
            captures.append(item)
            save()
            assert result.returncode == expected, item
            if summary:
                assert int(summary["input_counted"]) == int(
                    summary["input_reported"]
                ) + int(summary["unknown_input_counted"]), item
            return item

        for keep, index in [("first", 1), ("last", 3), ("longest", 2)]:
            item = capture("bridge-" + keep, SOURCE, ["--keep", keep])
            assert len(item["records"]) == 1, item
            row = item["records"][0]
            assert row["index"] == index and row["cluster"] == 1, item
            assert [member["index"] for member in row["members"]] == [1, 2, 3], item
            assert [(link["left"], link["right"]) for link in row["links"]] == [
                (1, 2),
                (2, 3),
            ], item
            assert int(item["summary"]["http_attempts"]) == 3, item

        plan = capture("plan", SOURCE, ["--dry-run"])
        assert [row["pair"] for row in plan["records"]] == [
            [1, 2],
            [1, 3],
            [2, 3],
        ], plan
        allowance = str(plan["records"][0]["counts"]["input_tokens"])
        partial = capture("partial", SOURCE, ["--budget", allowance], 4)
        assert (
            not partial["records"] and int(partial["summary"]["http_attempts"]) == 1
        ), partial
        journal = str(directory / "partial.jsonl")
        repeated = capture(
            "partial-zero", SOURCE, ["--budget", "0", "--resume", journal], 4
        )
        assert (
            not repeated["records"] and int(repeated["summary"]["http_attempts"]) == 0
        ), repeated
        resumed = capture("partial-resume", SOURCE, ["--resume", journal])
        assert (
            len(resumed["records"]) == 1
            and int(resumed["summary"]["http_attempts"]) == 2
        ), resumed
        finished = capture(
            "finished-resume", SOURCE, ["--budget", "0", "--resume", journal]
        )
        assert (
            not finished["records"] and int(finished["summary"]["http_attempts"]) == 0
        ), finished
        changed = capture(
            "changed-source",
            SOURCE.replace(b"checkout", b"payments"),
            ["--resume", journal],
            65,
        )
        assert (
            not changed["records"] and int(changed["summary"]["http_attempts"]) == 0
        ), changed

        pair = b"Incident ALPHA is delayed.\nIncident ALPHA is delayed.\n"
        for action, count in [("drop", 2), ("keep", 1), ("stderr", 2)]:
            item = capture(
                "uncertain-" + action, pair, ["--margin", "1", "--on-uncertain", action]
            )
            assert len(item["records"]) == count, item
            if action == "keep":
                assert item["records"][0]["links"][0]["assessment"]["uncertain"], item
        failed = capture(
            "uncertain-fail", pair, ["--margin", "1", "--on-uncertain", "fail"], 3
        )
        assert not failed["records"], failed
        item = capture(
            "uncertain-replay",
            pair,
            [
                "--margin",
                "1",
                "--on-uncertain",
                "fail",
                "--budget",
                "0",
                "--resume",
                str(directory / "uncertain-fail.jsonl"),
            ],
            3,
        )
        assert not item["records"] and int(item["summary"]["http_attempts"]) == 0, item

        raw = b"Incident ALPHA is delayed.\r\nIncident ALPHA is delayed.\r\n"
        item = capture("plain-crlf", raw, [], json_output=False)
        assert item["stdout"].encode() == raw.splitlines(keepends=True)[0], item
        item = capture(
            "plain-nul", b"ALPHA\0ALPHA\0", ["-I", "null"], json_output=False
        )
        assert item["stdout"].encode() == b"ALPHA\0", item
        structured = b'{"id":"a","incident":"ALPHA"}\n{"id":"b","incident":"ALPHA","extra":"longer"}\n'
        item = capture(
            "jsonl-projection",
            structured,
            ["-I", "jsonl", "--field", "id", "--keep", "longest"],
        )
        assert (
            item["records"][0]["index"] == 2 and item["records"][0]["unit"] == "b"
        ), item
        assert [member["unit"] for member in item["records"][0]["members"]] == [
            "a",
            "b",
        ], item
        csv = b'id,incident\r\na,"ALPHA\r\ndelayed"\r\nb,"ALPHA\r\ndelayed"\r\n'
        item = capture("csv-header", csv, ["-I", "csv"], json_output=False)
        assert (
            item["stdout"].encode() == b'id,incident\r\na,"ALPHA\r\ndelayed"\r\n'
        ), item
        item = capture("singleton", b"only\r\n", [], json_output=False)
        assert (
            item["stdout"].encode() == b"only\r\n"
            and int(item["summary"]["http_attempts"]) == 0
        ), item
        item = capture("empty", b"", [], 1)
        assert not item["records"], item
        item = capture("pair-cap", SOURCE, ["--max-pairs", "2"], 65)
        assert not item["records"] and int(item["summary"]["http_attempts"]) == 0, item

        item = capture("default-question", pair, [], question=None)
        assert len(item["records"]) == 1, item
        evidence["complete"] = True
        save()
    print(
        json.dumps(
            {"complete": True, "captures": len(captures), "evidence": str(output)}
        )
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument("--out", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.binary.resolve(), arguments.out.resolve())


if __name__ == "__main__":
    main()
