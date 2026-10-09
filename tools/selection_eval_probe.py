"""Retain real retrieval/extraction suite reports and matching CLI contracts."""

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

from join_eval_probe import cancellation

SUITES = [
    "find-pick/find",
    "find-pick/pick",
    "find-pick/independent",
    "out-of-scope/selection",
    "extract/dates",
    "extract/money",
    "extract/semver",
    "extract/emails",
    "extract/hostnames",
    "extract/tickers",
]


def run(binary: Path, output: Path) -> None:
    endpoint = os.environ.get("LOUPE_TEST_URL", "").rstrip("/")
    if not endpoint:
        raise SystemExit("Set LOUPE_TEST_URL to the actual server base URL")
    output.parent.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parent.parent
    environment = dict(os.environ)
    for key in [
        "LOUPE_CONFIG",
        "LOUPE_URL",
        "LOUPE_PATH",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_API_KEY",
        "LOUPE_PRESET_PATH",
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
        "proof_boundary": "Actual GNU native CLI/eval/MCP inference, pinned fictional drafted labels, original UTF-8 spans and shared normalization. No human review, production ratification, authoritative model identity/cache, isolated timing, release or activation proof.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-selection-eval-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        common = [
            str(binary),
            "--config",
            str(root / "examples/config.toml"),
            "--profile",
            "server",
            "--url",
            endpoint,
        ]
        suite_args = [
            value
            for suite in SUITES
            for value in ["--suite", str(root / "evals/suites" / suite)]
        ]
        base = ["eval", *suite_args, "--strict", "--no-cache", "--json"]

        def capture(
            name: str, args: list[str], expected: int, source: bytes | None = None
        ) -> dict[str, Any]:
            command = [*common, *args]
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=environment,
                cwd=directory,
                timeout=210,
                check=False,
            )
            item: dict[str, Any] = {
                "name": name,
                "command": command,
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
            }
            if source is not None:
                item["input"] = source.decode()
            evidence["captures"].append(item)
            save()
            assert result.returncode == expected, item
            if result.stdout:
                item["json"] = (
                    json.loads(result.stdout)
                    if args[0] == "eval"
                    else [json.loads(line) for line in result.stdout.splitlines()]
                )
            save()
            return item

        capture("offline-selection-plan", [*base, "--dry-run"], 0)
        capture("requires-authoritative-identity", base, 69)
        live = capture(
            "ten-selection-suites",
            [
                *base,
                "--allow-unverified",
                "--warmup",
                "1",
                "--deadline",
                "180s",
                "--report",
                str(directory / "reports"),
            ],
            0,
        )["json"]
        assert live["status"] == "provisional"
        assert (
            live["token_usage"]["spent_input"] == live["token_usage"]["reported_input"]
        )
        assert (
            live["token_usage"]["unknown_attempts"]
            == live["token_usage"]["cache_hits"]
            == 0
        )
        completed = 0
        correct = 0
        for suite in live["suites"]:
            stage = suite["selection"]
            completed += stage["retrieval"]["cases"]
            for case in stage["cases"]:
                assert case["complete"]
                actual = {selected["id"] for selected in case["selected"]}
                correct += actual == set(case["expected"])
                for request in case["requests"]:
                    assert (
                        request["counts"]["input_tokens"]
                        == request["evaluation"]["response"]["usage"]["input_tokens"]
                    )
                source = case["text"].encode()
                for selected in case["selected"]:
                    location = selected["location"]
                    assert (
                        source[location["byte_start"] : location["byte_end"]].decode()
                        == selected["unit"]
                    )
                    if selected["normalization_correct"] is not None:
                        assert selected["normalization_correct"]
            assert [phase["name"] for phase in suite["phases"]] == [
                "accuracy",
                "warmup",
                "latency",
            ]
            for phase in suite["phases"][1:]:
                assert not phase["cache_requested"] and phase["cache_hits"] == 0
                assert phase["timing"]["completed_units"] == stage["retrieval"]["cases"]
                for group in phase["groups"]:
                    assert group["complete"]
                    for request in group["requests"]:
                        assert (
                            request["counts"]["input_tokens"]
                            == request["evaluation"]["response"]["usage"][
                                "input_tokens"
                            ]
                        )
        assert completed == 22
        evidence["measured"] = {
            "completed_cases": completed,
            "exact_matches": correct,
            "mismatches": [
                {
                    "suite": suite["manifest"]["id"],
                    "case_id": case["case_id"],
                    "expected": case["expected"],
                    "actual": [selected["id"] for selected in case["selected"]],
                }
                for suite in live["suites"]
                for case in suite["selection"]["cases"]
                if {selected["id"] for selected in case["selected"]}
                != set(case["expected"])
            ],
        }
        first = live["suites"][0]["selection"]["cases"][0]["requests"][0]["counts"][
            "input_tokens"
        ]
        partial = capture(
            "one-attempt-budget",
            [*base, "--allow-unverified", "--budget", str(first), "--warmup", "0"],
            4,
        )["json"]
        assert (
            partial["status"] == "incomplete"
            and partial["token_usage"]["attempts"] == 1
        )
        assert (
            partial["token_usage"]["spent_input"]
            == partial["token_usage"]["reported_input"]
            == first
        )
        short = [
            "eval",
            "--suite",
            str(root / "evals/suites/extract/semver"),
            "--strict",
            "--no-cache",
            "--json",
            "--allow-unverified",
            "--warmup",
            "0",
        ]
        uncertain = capture(
            "uncertain-is-not-confirmed-absence", [*short, "--margin", "1"], 0
        )["json"]["suites"][0]["selection"]
        assert (
            uncertain["coverage"]["value"] == 0
            and uncertain["absence_accuracy"]["value"] == 0
        )
        evidence["captures"].append(
            cancellation([*common, "-vv", *short], environment, directory)
        )
        save()

        for suite in live["suites"]:
            if suite["manifest"]["id"].startswith("out-of-scope"):
                continue
            case = next(
                case for case in suite["selection"]["cases"] if case["expected"]
            )
            mode = case["mode"]
            args = [
                "pick" if mode == "independent" else mode,
                case["template"]["value"]["question"],
                "-n",
                str(suite["manifest"]["selection"]["take"]),
            ]
            if mode == "independent":
                args.append("--independent")
            if mode == "extract":
                args += [
                    "--candidates",
                    case["candidate_parser"]["kind"],
                    "--normalize",
                ]
                for key, value in case["normalization"]["options"].items():
                    if value is not None:
                        args += ["--" + key.replace("_", "-"), value]
            args += [
                "--strict",
                "--no-cache",
                "--summary",
                "--json",
                "--deadline",
                "60s",
            ]
            item = capture(
                "direct-" + suite["manifest"]["id"], args, 0, case["text"].encode()
            )
            direct = item["json"]
            assert {selected["id"] for selected in direct} == {
                selected["id"] for selected in case["selected"]
            }
            for selected in direct:
                original = next(
                    value for value in case["selected"] if value["id"] == selected["id"]
                )
                assert (
                    selected["unit"] == original["unit"]
                    and selected["location"] == original["location"]
                )
                assert selected.get("normalized") == original["normalized"]
                history = selected["provenance"]["history"]
                if mode == "independent":
                    requests = [
                        item["bracket"]["request"]
                        for plan in history["plans"]
                        for item in plan["items"]
                    ]
                else:
                    requests = [
                        bracket["request"]
                        for plan in history["plans"]
                        for bracket in plan["brackets"]
                    ]
                assert requests == [request["request"] for request in case["requests"]]
        evidence["published_reports"] = [
            json.loads(path.read_text())
            for path in sorted((directory / "reports").rglob("report.json"))
        ]
        evidence["complete"] = True
        save()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".private/qualification/evidence/selection-evaluation.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())
