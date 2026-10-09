"""Check finite dedupe idle deadlines and SIGTERM using real processes."""

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
        "complete": False,
        "captures": captures,
        "proof_boundary": "Real native processes, idle stdin held open, actual SIGTERM and complete catalog source protection. No inference attempts or mock backend.",
    }
    with tempfile.TemporaryDirectory(prefix="loupe-dedupe-controls-") as temporary:
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
            "--no-cache",
            "--summary",
        ]

        def record(
            name: str,
            command: list[str],
            code: int,
            stdout: bytes,
            stderr: bytes,
            expected: int,
            elapsed: float,
        ) -> None:
            summary: dict[str, str] = {}
            for line in stderr.decode().splitlines():
                if line.startswith("loupe: units="):
                    summary = dict(
                        piece.split("=", 1)
                        for piece in line.removeprefix("loupe: ").split()
                    )
            item = {
                "name": name,
                "command": command,
                "exit": code,
                "stdout": stdout.decode(),
                "stderr": stderr.decode(),
                "summary": summary,
                "elapsed_seconds": elapsed,
            }
            captures.append(item)
            output.write_text(json.dumps(evidence, indent=2) + "\n")
            assert code == expected and not stdout, item
            assert int(summary["http_attempts"]) == 0, item

        for name, extra, expected in [
            ("idle-deadline", ["--deadline", "200ms"], 69),
            ("idle-sigterm", [], 143),
        ]:
            command = base + extra
            started = time.monotonic()
            process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
                cwd=directory,
            )
            try:
                if name == "idle-sigterm":
                    time.sleep(0.3)
                    process.send_signal(signal.SIGTERM)
                process.wait(timeout=5)
                stdout, stderr = process.communicate(timeout=1)
                record(
                    name,
                    command,
                    process.returncode,
                    stdout,
                    stderr,
                    expected,
                    time.monotonic() - started,
                )
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate()
        evidence["complete"] = True
        output.write_text(json.dumps(evidence, indent=2) + "\n")
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
