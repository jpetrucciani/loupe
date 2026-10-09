"""Retain real server composite decisions and independently recompute every term."""

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


def verify(row: dict[str, Any]) -> None:
    expected = 0.0
    for factor in row["composite"]["factors"].values():
        raw = factor["answer"]
        assert raw == row["response"]["answers"][factor["field"]], row
        if raw["type"] == "noul":
            value = raw["noul"]
        else:
            probabilities = raw["probabilities"]
            value = (
                sum(int(key) * p for key, p in probabilities.items())
                / sum(probabilities.values())
                / (len(probabilities) - 1)
            )
        if factor["invert"]:
            value = 1 - value
        assert abs(value - factor["value"]) < 1e-12, row
        contribution = value * factor["normalized_weight"]
        assert abs(contribution - factor["contribution"]) < 1e-12, row
        expected += contribution
    assert abs(expected - row["composite"]["score"]) < 1e-12, row
    assert row["counts"]["exact"], row
    assert (
        row["counts"]["input_tokens"] == row["response"]["usage"]["input_tokens"]
    ), row


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
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "complete": False,
        "captures": [],
        "proof_boundary": "Functional jointly evaluated factors and independent arithmetic verification, real text/window/context/signal output and journal replay. The weighted index is not a calibrated probability or a causal explanation. Held-out composite usefulness, full live identity/cache and refreshed release artifacts remain open.",
    }
    with tempfile.TemporaryDirectory(prefix="loupe-explain-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        presets = directory / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        preset = presets / "incident-factors.toml"
        preset.write_text((root / "examples/incident-factors.toml").read_text())
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )

        def capture(
            name: str, source: bytes, args: list[str], expected: int = 0
        ) -> dict[str, Any]:
            command = [
                str(binary),
                "--config",
                str(config),
                "--profile",
                "server",
                "explain",
                "-p",
                "incident-factors",
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
                *args,
            ]
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                cwd=directory,
                env=environment,
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
            rows = (
                [json.loads(line) for line in result.stdout.splitlines()]
                if "--json" in args or "--dry-run" in args
                else []
            )
            item = {
                "name": name,
                "command": command,
                "input": source.decode(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
                "summary": summary,
                "records": rows,
            }
            evidence["captures"].append(item)
            output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")
            assert result.returncode == expected, item
            if summary:
                assert int(summary["input_counted"]) == int(
                    summary["input_reported"]
                ) + int(summary["unknown_input_counted"]), item
            for row in rows:
                if "composite" in row:
                    verify(row)
            return item

        source = b"Checkout is down for all customers and no retry has recovered it.\n"
        capture("whole-json", source, ["--json"])
        window = capture(
            "joint-window",
            source + b"The retry succeeded; service is fully recovered.\n",
            ["-I", "lines", "--pack", "window:2", "--json"],
        )
        assert len(window["records"]) == 2, window
        capture("plain-contributions", source, [])
        tsv = capture("tsv-contributions", source, ["-O", "tsv"])
        assert len(tsv["stdout"].strip().split("\t")) == 3, tsv
        context = directory / "context.txt"
        context.write_text("Customer impact is reported in the incident text.")
        preset.write_text(
            preset.read_text()
            + '\n[[signals]]\nid = "urgent"\ninstructions = "Does this need immediate action?"\n'
        )
        contextual = capture(
            "context-signals", source, ["--context", str(context), "--why", "--json"]
        )
        assert "urgent" in contextual["records"][0]["signals"], contextual
        capture("dry-full-schema", source, ["--dry-run"])
        journal = directory / "run.jsonl"
        saved = capture(
            "journal-original", source, ["--json", "--failed", str(journal)]
        )
        saved["journal"] = journal.read_text()
        replay = capture(
            "finished-replay",
            source,
            ["--json", "--resume", str(journal), "--budget", "0"],
        )
        assert (
            not replay["stdout"] and int(replay["summary"]["http_attempts"]) == 0
        ), replay
        preset.write_text(preset.read_text().replace("weight = 3", "weight = 4"))
        changed = capture(
            "weight-change-refused",
            source,
            ["--json", "--resume", str(journal), "--budget", "0"],
            65,
        )
        assert not changed["stdout"], changed
    evidence["complete"] = True
    output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({"complete": True, "captures": len(evidence["captures"])}))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    run(args.binary.resolve(), args.out.resolve())


if __name__ == "__main__":
    main()
