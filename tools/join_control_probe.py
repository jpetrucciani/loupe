"""Verify join cancellation and journal alias refusal with real processes."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import subprocess
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


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
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "captures": [],
        "complete": False,
        "proof_boundary": "Real native processes with stdin held open, actual SIGTERM, deadline and input-file alias refusal. Metadata discovery may contact server; every captured summary proves zero inference attempts. No mock backend or service mutation.",
    }
    with tempfile.TemporaryDirectory(prefix="loupe-join-controls-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        right = directory / "right.jsonl"
        right.write_text('{"name":"Acme Dental Clinic","location":"Portland Maine"}\n')
        original = right.read_bytes()
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
            }
            evidence["captures"].append(item)
            output.write_text(json.dumps(evidence, indent=2) + "\n")
            assert code == expected and stdout == b"", item
            assert (
                int(summary["attempts"])
                == int(summary["http_attempts"])
                == int(summary["input_counted"])
                == 0
            ), item

        for name, options, expected in [
            ("idle-deadline", ["--deadline", "250ms"], 69),
            ("idle-sigterm", [], 143),
        ]:
            command = [*base, *options]
            process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
                cwd=directory,
            )
            prefix = b""
            try:
                assert process.stderr is not None and process.stdout is not None
                if name == "idle-sigterm":
                    prefix = process.stderr.readline()
                    assert b"right records=" in prefix, prefix
                    process.send_signal(signal.SIGTERM)
                code = process.wait(timeout=5)
                record(
                    name,
                    command,
                    code,
                    process.stdout.read(),
                    prefix + process.stderr.read(),
                    expected,
                )
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                for stream in [process.stdin, process.stdout, process.stderr]:
                    if stream is not None:
                        stream.close()
        command = [*base, "--failed", str(right), "--deadline", "5s"]
        result = subprocess.run(
            command,
            input=b"",
            capture_output=True,
            env=environment,
            cwd=directory,
            timeout=10,
            check=False,
        )
        record(
            "journal-input-alias",
            command,
            result.returncode,
            result.stdout,
            result.stderr,
            74,
        )
        assert right.read_bytes() == original
        evidence["input_sha256_before_after"] = hashlib.sha256(original).hexdigest()
        evidence["complete"] = True
        output.write_text(json.dumps(evidence, indent=2) + "\n")
        print(
            json.dumps({"captures": len(evidence["captures"]), "inference_attempts": 0})
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".private/qualification/evidence/join-controls.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
