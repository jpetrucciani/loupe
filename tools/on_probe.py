"""Retain actual server trigger decisions and real one-shot action artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

QUESTION = "Does this text explicitly mention ALPHA?"


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
        "proof_boundary": "Actual legacy server decisions and real append/exec child processes. Functional trigger ordering, thresholds, uncertainty, bounded local windows and action cleanup. Durable claims, cursor integration, held-out trigger quality, full live identity/cache and new native release artifacts remain open.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-on-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        effects: list[Path] = []

        def capture(
            name: str,
            source: bytes,
            options: list[str],
            expected: int = 0,
            *,
            preset: str | None = None,
        ) -> dict[str, Any]:
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                "on",
            ]
            command.extend(["-p", preset] if preset else [QUESTION])
            command.extend(
                [
                    "--json",
                    "--strict",
                    "--no-cache",
                    "--summary",
                    "--deadline",
                    "60s",
                    *options,
                ]
            )
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
                "records": [json.loads(line) for line in result.stdout.splitlines()],
                "effects": {
                    str(path.name): (
                        path.read_bytes().decode() if path.exists() else None
                    )
                    for path in effects
                },
            }
            captures.append(item)
            save()
            assert result.returncode == expected, item
            assert int(summary["input_counted"]) == int(
                summary["input_reported"]
            ) + int(summary["unknown_input_counted"]), item
            for record in item["records"]:
                if "response" in record:
                    assert record["counts"]["exact"], record
                    assert (
                        record["counts"]["input_tokens"]
                        == record["response"]["usage"]["input_tokens"]
                    ), record
                    assert record["counts"]["input_tokens"] == (
                        record["counts"]["state_tokens"]
                        + record["counts"]["question_tokens"]
                    ), record
            return item

        append = directory / "events"
        exec_file = directory / "executed"
        effects.extend([append, exec_file])
        command = f"printf '%s:%s\\n' \"$LOUPE_INDEX\" \"$LOUPE_UNIT\" >> '{exec_file}'"
        item = capture(
            "ordered-exec-append",
            b"ALPHA\r\nBRAVO\r\n",
            ["--when", "yes", "--exec", command, "--append", str(append)],
        )
        assert (
            len(item["records"][0]["actions"]) == 2
            and not item["records"][1]["actions"]
        ), item
        assert (
            append.read_bytes() == b"ALPHA\r\n"
            and exec_file.read_bytes() == b"1:ALPHA\n"
        ), item
        item = capture(
            "stateless-repeat", b"ALPHA\r\n", ["--when", "yes", "--append", str(append)]
        )
        assert append.read_bytes() == b"ALPHA\r\nALPHA\r\n", item
        window_file = directory / "window"
        effects.append(window_file)
        item = capture(
            "local-window",
            b"ALPHA\nALPHA\n",
            ["--when", "yes", "--append", str(window_file), "--dedupe-window", "60s"],
        )
        assert (
            window_file.read_bytes() == b"ALPHA\n"
            and item["records"][1]["actions"][0]["suppressed"]
        ), item
        nonzero = directory / "nonzero"
        effects.append(nonzero)
        item = capture(
            "nonzero-once",
            b"ALPHA\n",
            ["--when", "yes", "--exec", f"printf once >> '{nonzero}'; exit 7"],
            69,
        )
        assert (
            nonzero.read_bytes() == b"once"
            and item["records"][0]["actions"][0]["result"]["code"] == 7
        ), item
        leaked = directory / "leaked"
        effects.append(leaked)
        item = capture(
            "timeout-process-group",
            b"ALPHA\n",
            [
                "--when",
                "yes",
                "--exec",
                f"(sleep 0.3; printf leaked > '{leaked}') & sleep 30",
                "--exec-timeout",
                "50ms",
            ],
            69,
        )
        assert item["records"][0]["actions"][0]["result"]["timed_out"], item
        time.sleep(0.4)
        assert not leaked.exists(), item
        uncertain = directory / "uncertain"
        effects.append(uncertain)
        item = capture(
            "uncertain-explicit",
            b"ALPHA\n",
            [
                "--when",
                "yes",
                "--exec",
                "true",
                "--when",
                "uncertain",
                "--append",
                str(uncertain),
                "--margin",
                "1",
            ],
        )
        assert (
            len(item["records"][0]["actions"]) == 1
            and uncertain.read_bytes() == b"ALPHA\n"
        ), item
        item = capture(
            "uncertain-fail",
            b"ALPHA again\n",
            [
                "--when",
                "yes",
                "--append",
                str(uncertain),
                "--margin",
                "1",
                "--on-uncertain",
                "fail",
            ],
            3,
        )
        assert not item["records"] and uncertain.read_bytes() == b"ALPHA\n", item
        item = capture(
            "threshold-no",
            b"BRAVO\n",
            ["--when", "no", "--exec", "printf no", "--threshold", "0.9"],
        )
        assert (
            item["records"][0]["label"] == "no"
            and item["records"][0]["actions"][0]["result"]["code"] == 0
        ), item
        dry = directory / "dry"
        effects.append(dry)
        item = capture(
            "dry-no-effects",
            b"ALPHA\n",
            [
                "--when",
                "yes",
                "--exec",
                f"printf bad > '{dry}'",
                "--append",
                str(dry),
                "--dry-run",
            ],
        )
        assert not dry.exists() and int(item["summary"]["http_attempts"]) == 0, item
        page = directory / "page"
        ticket = directory / "ticket"
        effects.extend([page, ticket])
        item = capture(
            "choice-labels",
            b"The checkout service is completely down for all customers, returning HTTP 503.\nThe retry succeeded, service recovered, and all health checks are green.\n",
            [
                "--when",
                "page",
                "--append",
                str(page),
                "--when",
                "ticket",
                "--append",
                str(ticket),
            ],
            preset="severity",
        )
        assert [record["label"] for record in item["records"]] == [
            "page",
            "noise",
        ], item
        assert (
            page.read_bytes().startswith(b"The checkout") and not ticket.read_bytes()
        ), item
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
