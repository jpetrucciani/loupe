"""Capture actual joint features and validate CSV/JSON against saved answers."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
import subprocess
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

COLUMNS = [
    "p_config_change",
    "p_customer_impact",
    "s_urgency",
    "p_urgency_0",
    "p_urgency_1",
    "p_urgency_2",
]


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
    captures: list[dict[str, Any]] = []
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "complete": False,
        "captures": captures,
        "proof_boundary": "Built GNU x86_64 CLI and actual server model. Synthetic functional fixtures, complete raw run journals, CSV/JSON projections checked against their corresponding real responses. No held-out feature usefulness, full live identity/cache, Parquet, native release packaging or activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-features-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        questions = directory / "features.toml"
        questions.write_text((root / "examples/postmortem-features.toml").read_text())
        evidence["question_source"] = questions.read_text()

        def capture(
            name: str,
            options: list[str],
            source: bytes,
            expected: int = 0,
            *,
            selected: bool = False,
            limited: bool = False,
        ) -> dict[str, Any]:
            active_config = config
            if limited:
                active_config = directory / "limited.toml"
                active_config.write_text(
                    config.read_text().replace(
                        "max_length = 16384",
                        "max_length = 16384\nmax_state_tokens = 128",
                    )
                )
            journal = directory / (name + ".jsonl")
            command = [
                str(binary),
                "--config",
                str(active_config),
                "--profile",
                "server",
                "featurize",
                "-f",
                str(questions),
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
                *options,
            ]
            if selected:
                command.extend(["--field", "id"])
            if "--resume" in options:
                journal = Path(options[options.index("--resume") + 1])
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
            metrics: dict[str, str] = {}
            for line in result.stderr.decode().splitlines():
                if line.startswith("loupe: units="):
                    metrics = dict(
                        piece.split("=", 1)
                        for piece in line.removeprefix("loupe: ").split()
                    )
            records = [json.loads(line) for line in journal.read_text().splitlines()]
            item: dict[str, Any] = {
                "name": name,
                "command": command,
                "input": source.decode(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
                "summary": metrics,
                "journal": records,
            }
            captures.append(item)
            save()
            assert result.returncode == expected, item
            assert int(metrics["input_counted"]) == int(
                metrics["input_reported"]
            ) + int(metrics["unknown_input_counted"]), item
            snapshots: dict[int, dict[str, Any]] = {}
            units: dict[int, dict[str, Any]] = {}
            for envelope in records:
                record = envelope["record"]
                if record["record"] == "admit":
                    units[record["unit"]["index"]] = record["unit"]
                if record["record"] == "ready":
                    snapshots[record["index"]] = record["completed"]
                if record["record"] == "ready_many":
                    snapshots.update(
                        (value["index"], value["completed"])
                        for value in record["completions"]
                    )
            json_output = "--json" in options
            rows = (
                [json.loads(line) for line in result.stdout.splitlines()]
                if json_output
                else (
                    list(csv.DictReader(io.StringIO(result.stdout.decode())))
                    if "--resume" not in options
                    else []
                )
            )
            item["parsed_rows"] = rows
            for row in rows:
                index = int(row["index"])
                row_id = units[index]["subject"] if selected else f"R{index}"
                assert row["row_id"] == (row_id if json_output else str(row_id)), item
                if json_output and row.get("error"):
                    assert all(
                        value is None for value in row["features"].values()
                    ), item
                    continue
                if not json_output and row["error"]:
                    assert all(row[column] == "" for column in COLUMNS), item
                    continue
                snapshot = snapshots[index]
                anchor = snapshot["answer_id"]
                prefix = (
                    anchor.removesuffix("/config_change")
                    if anchor != "config_change"
                    else None
                )
                bindings = {
                    id_: f"{prefix}/{id_}" if prefix else id_
                    for id_ in ["config_change", "customer_impact", "urgency"]
                }
                answers = {
                    id_: snapshot["evaluation"]["response"]["answers"][field]
                    for id_, field in bindings.items()
                }
                expected_features = {
                    "p_config_change": answers["config_change"]["noul"],
                    "p_customer_impact": answers["customer_impact"]["noul"],
                    "s_urgency": answers["urgency"]["score"],
                    **{
                        f"p_urgency_{level}": answers["urgency"]["probabilities"][
                            str(level)
                        ]
                        for level in range(3)
                    },
                }
                if json_output:
                    assert row["features"] == expected_features, item
                    assert list(row["features"]) == COLUMNS, item
                    assert (
                        row["answers"] == answers and row["bindings"] == bindings
                    ), item
                    if "--why" in options:
                        field = (
                            f"{prefix}/signal:recovered"
                            if prefix
                            else "signal:recovered"
                        )
                        assert (
                            row["signals"]["recovered"]["answer"]
                            == snapshot["evaluation"]["response"]["answers"][field]
                        ), item
                else:
                    assert list(row) == ["index", "row_id", *COLUMNS, "error"], item
                    assert {
                        column: float(row[column]) for column in COLUMNS
                    } == expected_features, item
            item["raw_projection_verified"] = True
            save()
            return item

        source = (
            "\n".join(
                json.dumps(row, ensure_ascii=False)
                for row in [
                    {
                        "id": 'é,"incident"\nsecond',
                        "text": "A configuration change broke checkout. Customers cannot pay.",
                    },
                    {
                        "id": 42,
                        "text": "Checkout recovered after rollback; customers can pay normally.",
                    },
                ]
            )
            + "\n"
        ).encode()
        for mode in ["unit", "window:2"]:
            for format_ in ["csv", "json"]:
                options = ["--pack", mode, "--why"]
                if format_ == "json":
                    options.append("--json")
                capture(
                    mode.replace(":", "-") + "-" + format_,
                    options,
                    source,
                    selected=True,
                )
        resume = capture(
            "csv-completed-resume",
            [
                "--pack",
                "unit",
                "--why",
                "--resume",
                str(directory / "unit-csv.jsonl"),
                "--budget",
                "0",
            ],
            source,
            selected=True,
        )
        assert resume["stdout"] == "" and int(resume["summary"]["http_attempts"]) == 0
        large = (
            "\n".join(
                json.dumps(row)
                for row in [
                    {"text": "Checkout is down."},
                    {"text": "large " * 400},
                    {"text": "Checkout recovered."},
                ]
            )
            + "\n"
        ).encode()
        for format_ in ["csv", "json"]:
            capture(
                "keep-error-" + format_,
                [
                    "--keep-going",
                    "--on-error",
                    "keep",
                    *(["--json"] if format_ == "json" else []),
                ],
                large,
                limited=True,
            )
        prefix = capture("partial-csv", [], large, 65, limited=True)
        assert len(prefix["parsed_rows"]) == 1
        repeated = capture(
            "partial-csv-resume",
            ["--resume", str(directory / "partial-csv.jsonl"), "--budget", "0"],
            large,
            65,
            limited=True,
        )
        assert (
            repeated["stdout"] == "" and int(repeated["summary"]["http_attempts"]) == 0
        )
        capture("zero-budget", ["--json", "--budget", "0"], source, 4, selected=True)
        invalid = capture(
            "invalid-row-id",
            [],
            b'{"id":null,"text":"Checkout is down."}\n',
            65,
            selected=True,
        )
        assert int(invalid["summary"]["http_attempts"]) == 0
        empty = capture("empty-csv", [], b"", 1)
        assert len(empty["stdout"].splitlines()) == 1
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
        evidence["complete"] = True
        save()
        print(json.dumps(evidence["totals"]))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".private/qualification/evidence/server-features.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
