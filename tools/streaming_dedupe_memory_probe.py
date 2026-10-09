"""Measure bounded native CLI state/RSS over a real long input stream."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import subprocess
import tempfile
import threading
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


def run(binary: Path, output: Path, journaled: bool) -> None:
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
    observations: dict[str, Any] = {
        "rows": 0,
        "max_representatives": 0,
        "max_serialized_bytes": 0,
        "max_band_postings": 0,
        "max_distinct_bands": 0,
    }
    checkpoints = [500, 2000, 5000, 10000]
    samples: list[dict[str, int]] = []
    acknowledgements: queue.Queue[int | Exception] = queue.Queue()
    stdout_hash = hashlib.sha256()
    with tempfile.TemporaryDirectory(prefix="loupe-stream-memory-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        command = [
            str(binary),
            "--config",
            str(config),
            "--profile",
            "server",
            "dedupe",
            "--window",
            "8",
            "--max-index-bytes",
            "20000",
            "--max-candidates",
            "1",
            "--dry-run",
            "--summary",
        ]
        journal = directory / "run.jsonl"
        if journaled:
            command.remove("--dry-run")
            command.extend(["--json", "--budget", "0", "--failed", str(journal)])
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
        stdout = process.stdout

        def consume() -> None:
            try:
                for line in stdout:
                    stdout_hash.update(line)
                    row = json.loads(line)
                    observations["rows"] += 1
                    assert row["index"] == observations["rows"]
                    state = row["history"]
                    assert (
                        state["representatives"] <= 9
                        and state["band_postings"] <= 72
                        and state["distinct_bands"] <= 72
                        and state["serialized_bytes"] <= 20000
                    ), row
                    for metric, key in [
                        ("max_representatives", "representatives"),
                        ("max_serialized_bytes", "serialized_bytes"),
                        ("max_band_postings", "band_postings"),
                        ("max_distinct_bands", "distinct_bands"),
                    ]:
                        observations[metric] = max(observations[metric], state[key])
                    observations["last"] = {"index": row["index"], "history": state}
                    if row["index"] in checkpoints:
                        acknowledgements.put(row["index"])
            except (AssertionError, KeyError, ValueError, TypeError, OSError) as error:
                acknowledgements.put(error)

        consumer = threading.Thread(target=consume, daemon=True)
        consumer.start()
        written = 0
        try:
            for checkpoint in checkpoints:
                for index in range(written + 1, checkpoint + 1):
                    text = (
                        f"event{index:08x}\n"
                        if journaled
                        else f"Arrival ALPHA number {index}.\n"
                    )
                    process.stdin.write(text.encode())
                process.stdin.flush()
                observed = acknowledgements.get(timeout=45)
                if isinstance(observed, Exception):
                    raise observed
                assert observed == checkpoint, observed
                status = Path(f"/proc/{process.pid}/status").read_text()
                memory = {
                    line.split(":")[0]: int(line.split()[1])
                    for line in status.splitlines()
                    if line.startswith(("VmRSS:", "VmHWM:"))
                }
                samples.append(
                    {
                        "rows": checkpoint,
                        "rss_kib": memory["VmRSS"],
                        "hwm_kib": memory["VmHWM"],
                    }
                )
                written = checkpoint
            process.stdin.close()
            process.wait(timeout=10)
            consumer.join(timeout=5)
            assert not consumer.is_alive()
            stderr = process.stderr.read().decode()
            assert process.returncode == 0, stderr
            assert observations["rows"] == 10000, observations
            assert observations["last"]["history"]["expired"] == 9991, observations
            growth = (
                max(sample["rss_kib"] for sample in samples) - samples[0]["rss_kib"]
            )
            assert growth <= 16 * 1024, samples
            summary: dict[str, str] = {}
            for line in stderr.splitlines():
                if line.startswith("loupe: units="):
                    summary = dict(
                        piece.split("=", 1)
                        for piece in line.removeprefix("loupe: ").split()
                    )
            assert (
                int(summary["http_attempts"]) == 0
                and int(summary["input_counted"]) == 0
            ), summary
            replay: dict[str, Any] | None = None
            if journaled:
                before = hashlib.sha256(journal.read_bytes()).hexdigest()
                replay_command = [
                    "--resume" if word == "--failed" else word for word in command
                ]
                replay_input = "".join(
                    f"event{index:08x}\n" for index in range(1, 10001)
                ).encode()
                result = subprocess.run(
                    replay_command,
                    input=replay_input,
                    capture_output=True,
                    env=environment,
                    cwd=directory,
                    timeout=150,
                    check=False,
                )
                assert (
                    result.returncode == 0 and not result.stdout
                ), result.stderr.decode()
                assert (
                    "http_attempts=0 " in result.stderr.decode()
                ), result.stderr.decode()
                assert hashlib.sha256(journal.read_bytes()).hexdigest() == before
                replay = {
                    "command": replay_command,
                    "exit": result.returncode,
                    "stdout": result.stdout.decode(),
                    "stderr": result.stderr.decode(),
                    "journal_bytes": journal.stat().st_size,
                    "journal_sha256": before,
                }
            evidence = {
                "schema_version": 1,
                "captured_at_utc": datetime.now(UTC).isoformat(),
                "complete": True,
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "command": command,
                "input_pattern": (
                    "event<eight hexadecimal digits 1..10000>\\n"
                    if journaled
                    else "Arrival ALPHA number <1..10000>.\\n"
                ),
                "journaled": journaled,
                "finished_replay": replay,
                "observations": observations,
                "rss_samples": samples,
                "rss_growth_kib_after_500_rows": growth,
                "stdout_sha256": stdout_hash.hexdigest(),
                "exit": process.returncode,
                "stderr": stderr,
                "summary": summary,
                "proof_boundary": "Linux GNU debug CLI with pinned real tokenizer, real producer/consumer pipes and /proc resident memory. Optional journal mode uses distinct one-token LSH words, zero inference budget, actual durable snapshots and a complete budget-zero replay; normal mode assumes all units retained in dry-run. Functional live inference is separate. This does not prove held-out LSH recall, every record-size/RSS scenario or other platforms.",
            }
            output.write_text(json.dumps(evidence, indent=2) + "\n")
            print(
                json.dumps(
                    {
                        "complete": True,
                        "rows": observations["rows"],
                        "rss_samples": samples,
                        "rss_growth_kib": growth,
                    }
                )
            )
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            for stream in [process.stdin, process.stdout, process.stderr]:
                stream.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--journal", action="store_true")
    arguments = parser.parse_args()
    run(arguments.binary.resolve(), arguments.out.resolve(), arguments.journal)


if __name__ == "__main__":
    main()
