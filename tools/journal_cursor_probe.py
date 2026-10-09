"""Check a real read-only journald cursor without retaining private log contents."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


def invoke(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, capture_output=True, text=True, timeout=15, check=False
    )


def rows(result: subprocess.CompletedProcess[str]) -> list[dict[str, Any]]:
    return [
        json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")
    ]


def run(output: Path) -> None:
    version = invoke(["journalctl", "--version"])
    seed = invoke(["journalctl", "-n", "1", "--no-pager", "-o", "json"])
    records = rows(seed)
    assert seed.returncode == 0 and len(records) == 1, seed.stderr
    record = records[0]
    stamp = record["__REALTIME_TIMESTAMP"]
    until = "@" + stamp[:-6] + "." + stamp[-6:]
    with tempfile.TemporaryDirectory(prefix="loupe-journal-cursor-") as temporary:
        cursor = Path(temporary) / "cursor"
        command = [
            "journalctl",
            "--cursor-file",
            str(cursor),
            "--until",
            until,
            "-n",
            "1",
            "--no-pager",
            "-o",
            "json",
        ]
        first = invoke(command)
        first_rows = rows(first)
        assert first.returncode == 0 and len(first_rows) == 1, first.stderr
        assert first_rows[0]["__CURSOR"] == record["__CURSOR"]
        saved = cursor.read_bytes()
        assert saved
        second = invoke(command)
        assert second.returncode == 0 and not rows(second), second.stderr
        assert cursor.read_bytes() == saved
        cursor.unlink()
        replay = invoke(command)
        replay_rows = rows(replay)
        assert replay.returncode == 0 and len(replay_rows) == 1, replay.stderr
        assert replay_rows[0]["__CURSOR"] == record["__CURSOR"]
        evidence = {
            "schema_version": 1,
            "captured_at_utc": datetime.now(UTC).isoformat(),
            "systemd_version": version.stdout.splitlines()[0],
            "command": command,
            "first": {"exit": first.returncode, "records": len(first_rows)},
            "resumed": {
                "exit": second.returncode,
                "records": len(rows(second)),
                "cursor_unchanged": True,
            },
            "reset": {
                "exit": replay.returncode,
                "records": len(replay_rows),
                "same_source_cursor": True,
            },
            "source_cursor_sha256": hashlib.sha256(
                record["__CURSOR"].encode()
            ).hexdigest(),
            "cursor_file_sha256": hashlib.sha256(saved).hexdigest(),
            "source_record_sha256": hashlib.sha256(
                json.dumps(record, sort_keys=True).encode()
            ).hexdigest(),
            "proof_boundary": "Real local systemd journal read/advance/restart/reset. Private message/metadata contents are not retained or submitted to a model. Durable action claims have separate real filesystem/process tests. Cursor advancement and action claims are not one transaction; no loss-free or exactly-once pipeline delivery is promised.",
        }
    output.write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps({"complete": True, "source_rows": [1, 0, 1]}))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    run(args.out)


if __name__ == "__main__":
    main()
