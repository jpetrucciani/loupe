"""Retain actual server streaming journal partial/fatal/finished replay evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import subprocess
import tempfile
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

QUESTION = (
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
        "proof_boundary": "Actual legacy server responses, real journals/process SIGTERM, partial/fatal/finished replay and downtime expiry. Fresh full identity/cache, held-out quality, stdout flush/ack transactional delivery and current native release artifacts are not qualified.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-stream-recovery-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        base = [
            str(binary),
            "--config",
            str(config),
            "--profile",
            "server",
            "dedupe",
            QUESTION,
            "--strict",
            "--no-cache",
            "--summary",
            "--deadline",
            "60s",
        ]

        def record(
            name: str,
            command: list[str],
            source: bytes,
            result: subprocess.CompletedProcess[bytes],
            journal: Path | None,
            expected: int,
            json_output: bool,
        ) -> dict[str, Any]:
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
                    if journal is not None and journal.exists()
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
            assert int(summary["input_counted"]) == int(
                summary["input_reported"]
            ) + int(summary["unknown_input_counted"]), item
            for frame in item["journal"]:
                record_ = frame["record"]
                if record_["record"] == "plan" and "streaming" in record_["provenance"]:
                    for pair in record_["provenance"]["streaming"]["pairs"]:
                        assert (
                            pair["counts"]["exact"]
                            and pair["counts"]["input_tokens"]
                            == pair["evaluation"]["response"]["usage"]["input_tokens"]
                        ), item
            return item

        def capture(
            name: str,
            source: bytes,
            options: list[str],
            expected: int = 0,
            *,
            action: str = "drop",
            json_output: bool = True,
        ) -> dict[str, Any]:
            command = [*base, "--margin", "1", "--on-uncertain", action, *options]
            if "--window" not in options and "--window-time" not in options:
                command.extend(["--window", "10"])
            if json_output:
                command.append("--json")
            journal = directory / (name + ".jsonl")
            if "--resume" in options:
                journal = Path(options[options.index("--resume") + 1])
            elif "--dry-run" in options:
                journal = None
            else:
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
            return record(name, command, source, result, journal, expected, json_output)

        source = b"ALPHA\nALPHA\nALPHA\n"
        plan = capture("pair-plan", source, ["--dry-run"])
        count = plan["records"][1]["pairs"][0]["counts"]["input_tokens"]
        first = capture("partial", source, ["--budget", str(count * 2)], 4)
        assert (
            len(first["records"]) == 2 and int(first["summary"]["http_attempts"]) == 2
        ), first
        journal = directory / "partial.jsonl"
        replay = capture(
            "partial-zero", source, ["--budget", "0", "--resume", str(journal)], 4
        )
        assert (
            not replay["records"] and int(replay["summary"]["http_attempts"]) == 0
        ), replay
        before = journal.read_bytes()
        changed = capture(
            "changed-input", b"BRAVO\nALPHA\nALPHA\n", ["--resume", str(journal)], 65
        )
        assert not changed["records"] and journal.read_bytes() == before, changed
        changed = capture(
            "changed-policy",
            source,
            ["--threshold", "0.6", "--resume", str(journal)],
            65,
        )
        assert not changed["records"] and journal.read_bytes() == before, changed
        resumed = capture(
            "partial-finish", source, ["--budget", str(count), "--resume", str(journal)]
        )
        assert (
            [row["index"] for row in resumed["records"]] == [3]
            and len(resumed["records"][0]["pairs"]) == 2
            and int(resumed["summary"]["http_attempts"]) == 1
        ), resumed
        before = journal.read_bytes()
        done = capture(
            "completed-zero", source, ["--budget", "0", "--resume", str(journal)]
        )
        assert (
            not done["records"]
            and int(done["summary"]["http_attempts"]) == 0
            and journal.read_bytes() == before
        ), done
        failed = capture("uncertain", b"ALPHA\nALPHA\n", [], 3, action="fail")
        assert len(failed["records"]) == 1, failed
        replay = capture(
            "uncertain-zero",
            b"ALPHA\nALPHA\n",
            ["--budget", "0", "--resume", str(directory / "uncertain.jsonl")],
            3,
            action="fail",
        )
        assert (
            not replay["records"] and int(replay["summary"]["http_attempts"]) == 0
        ), replay
        csv = b"id,text\r\na,ALPHA\r\nb,ALPHA\r\n"
        first = capture(
            "csv",
            csv,
            ["-I", "csv", "--select", "text", "--budget", "0"],
            4,
            json_output=False,
        )
        assert first["stdout"].encode() == b"id,text\r\na,ALPHA\r\n", first
        resumed = capture(
            "csv-resume",
            csv,
            ["-I", "csv", "--select", "text", "--resume", str(directory / "csv.jsonl")],
            json_output=False,
        )
        assert resumed["stdout"].encode() == b"b,ALPHA\r\n", resumed
        journal = directory / "time.jsonl"
        command = [
            *base,
            "--margin",
            "1",
            "--on-uncertain",
            "drop",
            "--window-time",
            "100ms",
            "--failed",
            str(journal),
        ]
        process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            cwd=directory,
        )
        assert (
            process.stdin is not None
            and process.stdout is not None
            and process.stderr is not None
        )
        try:
            process.stdin.write(b"ALPHA\n")
            process.stdin.flush()
            first_line = process.stdout.readline()
            assert first_line == b"ALPHA\n"
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                frames = [
                    json.loads(line)
                    for line in journal.read_text().splitlines()
                    if line.endswith("}")
                ]
                if any(frame["record"].get("record") == "handled" for frame in frames):
                    break
                time.sleep(0.01)
            else:
                raise AssertionError("first representative was not acknowledged")
            process.send_signal(signal.SIGTERM)
            process.wait(timeout=5)
            result = subprocess.CompletedProcess(
                command,
                process.returncode,
                first_line + process.stdout.read(),
                process.stderr.read(),
            )
            record("time-sigterm", command, b"ALPHA\n", result, journal, 143, False)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            for stream in [process.stdin, process.stdout, process.stderr]:
                stream.close()
        time.sleep(0.2)
        resumed = capture(
            "time-resume-expired",
            b"ALPHA\nALPHA\n",
            ["--window-time", "100ms", "--budget", "0", "--resume", str(journal)],
            json_output=False,
        )
        assert (
            resumed["stdout"].encode() == b"ALPHA\n"
            and int(resumed["summary"]["http_attempts"]) == 0
        ), resumed
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
