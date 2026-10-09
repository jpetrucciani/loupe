"""Capture local blocking and actual server responses for retained entity pairs."""

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
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "captures": captures,
        "complete": False,
        "proof_boundary": "Actual local candidate blocking and built-CLI server ask requests over retained synthetic pairs. Full raw requests/responses/journals and exact per-attempt accounting are retained. Join inference/cardinality execution is still separate work; these functional probes do not establish held-out blocking recall or pair quality. No fresh full identity/cache, native packaging or activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-blocking-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        right = directory / "right.jsonl"
        right_records = [
            {"id": "r-1", "name": "Acme Dental Clinic", "location": "Portland Maine"},
            {"id": "r-2", "name": "Acme Hardware", "location": "Portland Oregon"},
            {"id": "r-3", "name": "Blue Sky Clinic", "location": "London"},
        ]
        left_records = [
            {
                "id": "l-1",
                "name": "ACME, Dental Clinic.",
                "location": "Portland, Maine",
            },
            {"id": "l-2", "name": "Unlisted Workshop", "location": "Berlin Germany"},
            {"id": "l-3", "name": "Blue Sky Clinic", "location": "LONDON"},
        ]
        right.write_text(
            "\n".join(json.dumps(record) for record in right_records) + "\n"
        )
        source = (
            "\n".join(json.dumps(record) for record in left_records) + "\n"
        ).encode()
        questions = directory / "pair.json"
        question_source = {
            "questions": {
                "same_entity": {
                    "type": "noul",
                    "instructions": "Do the LEFT and RIGHT records identify the same real-world business at the same location? Shared words alone are insufficient.",
                },
                "similarity": {
                    "type": "score",
                    "instructions": "How strong is the evidence that LEFT and RIGHT identify the same business at the same location?",
                    "criteria": [
                        "Different business or different location",
                        "Ambiguous possible match",
                        "Clear match with equivalent business name and location",
                    ],
                },
                "name_agrees": {
                    "type": "noul",
                    "instructions": "Do the LEFT and RIGHT business names describe the same business, allowing case and punctuation differences?",
                },
                "location_agrees": {
                    "type": "noul",
                    "instructions": "Do the LEFT and RIGHT locations describe the same city and state or country?",
                },
            }
        }
        questions.write_text(json.dumps(question_source))
        evidence["question_source"] = question_source
        common = [str(binary), "--config", str(config), "--profile", "server"]
        plans: list[dict[str, Any]] = []
        for mode in ["hybrid", "tokens", "lsh"]:
            command = [
                *common,
                "join",
                "-",
                str(right),
                "--on",
                "name,location",
                "--blocking",
                mode,
                "--dry-run",
            ]
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=environment,
                cwd=directory,
                timeout=15,
                check=False,
            )
            item: dict[str, Any] = {
                "name": "blocking-" + mode,
                "command": command,
                "input": source.decode(),
                "right_input": right.read_text(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
            }
            captures.append(item)
            save()
            assert result.returncode == 0, item
            rows = [json.loads(line) for line in result.stdout.splitlines()]
            item["records"] = rows
            assert rows[0]["candidates"][0]["blocking"]["right_index"] == 1
            assert rows[1]["candidates"] == []
            assert rows[2]["candidates"][0]["blocking"]["right_index"] == 3
            if mode == "hybrid":
                plans = rows
        for plan in plans:
            for candidate in plan["candidates"]:
                left_record = plan["left"]["unit"]
                right_record = candidate["right"]["unit"]
                state = (
                    "LEFT record:\n"
                    + json.dumps(left_record, ensure_ascii=False, sort_keys=True)
                    + "\nRIGHT record:\n"
                    + json.dumps(right_record, ensure_ascii=False, sort_keys=True)
                ).encode()
                name = f"pair-{left_record['id']}-{right_record['id']}"
                journal = directory / (name + ".jsonl")
                command = [
                    *common,
                    "ask",
                    "-f",
                    str(questions),
                    "--strict",
                    "--no-cache",
                    "--summary",
                    "--deadline",
                    "60s",
                    "--failed",
                    str(journal),
                ]
                result = subprocess.run(
                    command,
                    input=state,
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
                item = {
                    "name": name,
                    "command": command,
                    "blocking": candidate["blocking"],
                    "input": state.decode(),
                    "exit": result.returncode,
                    "stdout": result.stdout.decode(),
                    "stderr": result.stderr.decode(),
                    "summary": summary,
                    "journal": [
                        json.loads(line) for line in journal.read_text().splitlines()
                    ],
                }
                captures.append(item)
                save()
                assert result.returncode == 0, item
                response = json.loads(result.stdout)
                item["response"] = response
                assert set(response["answers"]) == set(
                    question_source["questions"]
                ), item
                assert (
                    int(summary["input_counted"])
                    == int(summary["input_reported"])
                    == response["usage"]["input_tokens"]
                ), item
                assert int(summary["unknown_input_counted"]) == 0, item
                assert int(summary["cache_hits"]) == 0, item
        actual = [item for item in captures if "summary" in item]
        evidence["totals"] = {
            key: sum(int(item["summary"][key]) for item in actual)
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
        default=Path(".private/qualification/evidence/captured-blocking.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
