"""Retain real native join-stage reports and direct CLI request/assignment proof."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import subprocess
import tempfile
import threading
from datetime import UTC, datetime
from pathlib import Path
from queue import Queue
from typing import Any


def cancellation(command: list[str], env: dict[str, str], cwd: Path) -> dict[str, Any]:
    """Interrupt a genuine admitted request and retain its unknown charged usage."""
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        cwd=cwd,
    )
    assert process.stderr is not None
    queue: Queue[bytes | None] = Queue()

    def submitted() -> None:
        assert process.stderr is not None
        for line in process.stderr:
            queue.put(line)
            if b"loupe: submitted" in line:
                return
        queue.put(None)

    reader = threading.Thread(target=submitted, daemon=True)
    reader.start()
    prefix = b""
    try:
        while True:
            line = queue.get(timeout=30)
            assert line is not None, "join process ended before admission"
            prefix += line
            if b"loupe: submitted" in line:
                break
        reader.join(timeout=1)
        assert not reader.is_alive()
        process.terminate()
        stdout, stderr = process.communicate(timeout=30)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
    report = json.loads(stdout)
    assert process.returncode == 143 and report["status"] == "incomplete"
    assert report["error"]["code"] == 143
    assert report["token_usage"]["attempts"] == 1
    assert report["token_usage"]["unknown_attempts"] == 1
    assert report["token_usage"]["spent_input"] > 0
    assert (
        report["token_usage"]["spent_input"] == report["token_usage"]["unknown_input"]
    )
    return {
        "name": "sigterm-after-admission",
        "command": command,
        "exit": process.returncode,
        "stdout": stdout.decode(),
        "stderr": (prefix + stderr).decode(),
        "json": report,
    }


def run(binary: Path, output: Path) -> None:
    endpoint = os.environ.get("LOUPE_TEST_URL", "").rstrip("/")
    if not endpoint:
        raise SystemExit("Set LOUPE_TEST_URL to the actual server base URL")
    output.parent.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parent.parent
    env = dict(os.environ)
    for key in [
        "LOUPE_CONFIG",
        "LOUPE_URL",
        "LOUPE_PATH",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
        "LOUPE_API_KEY",
        "LOUPE_PRESET_PATH",
    ]:
        env.pop(key, None)
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "endpoint": f"{endpoint}/v1/systemone",
        "complete": False,
        "captures": [],
        "proof_boundary": "Real native evaluation and join requests on handwritten fictional catalogs. Provisional model identity, no cache, no human review or ratification, no production floor or release/activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-join-eval-") as temporary:
        directory = Path(temporary)
        env["XDG_CONFIG_HOME"] = str(directory / "xdg")
        report_dir = directory / "reports"
        common = [
            str(binary),
            "--config",
            str(root / "examples/config.toml"),
            "--profile",
            "server",
            "--url",
            endpoint,
        ]
        suite = root / "evals/suites/join/suite.toml"

        def capture(
            name: str, args: list[str], expected: int, source: bytes | None = None
        ) -> dict[str, Any]:
            command = [*common, *args]
            result = subprocess.run(
                command,
                input=source,
                capture_output=True,
                env=env,
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

        base = [
            "eval",
            "--suite",
            str(suite),
            "--json",
            "--strict",
            "--no-cache",
            "--warmup",
            "0",
        ]
        plan = capture("offline-plan", [*base, "--dry-run"], 0)["json"]
        assert plan["token_usage"]["attempts"] == 0
        capture("requires-authoritative-identity", base, 69)
        live = capture(
            "held-out-join-stages",
            [
                *base,
                "--allow-unverified",
                "--deadline",
                "180s",
                "--report",
                str(report_dir),
            ],
            0,
        )["json"]
        assert live["status"] == "provisional"
        usage = live["token_usage"]
        assert (
            usage["spent_input"] == usage["reported_input"]
            and usage["cache_hits"] == usage["unknown_attempts"] == 0
        )
        stage = live["suites"][0]["join"]
        assert stage["all_pair_quality"]["cases"] == 24
        assert stage["blocking_recall"]["value"] == 5 / 7
        for phase in live["suites"][0]["phases"]:
            if phase["name"] == "latency":
                assert not phase["cache_requested"] and len(phase["groups"]) == 12
                for group in phase["groups"]:
                    assert group["complete"]
                    for pair in group["pairs"]:
                        assert (
                            pair["evaluation"]["response"]["usage"]["input_tokens"]
                            == pair["counts"]["input_tokens"]
                        )
        for case in stage["cases"]:
            assert len(case["pairs"]) == 6
            for pair in case["pairs"]:
                assert (
                    pair["evaluation"]["response"]["usage"]["input_tokens"]
                    == pair["counts"]["input_tokens"]
                )
        answers = [
            (
                pair["evaluation"]["response"]["answers"]["same_entity"]["noul"],
                pair["label"],
            )
            for case in stage["cases"]
            for pair in case["pairs"]
        ]
        correct = sum((p >= 0.5) == truth for p, truth in answers)
        brier = sum((p - truth) ** 2 for p, truth in answers) / len(answers)
        assert math.isclose(
            stage["all_pair_quality"]["accuracy"]["value"], correct / len(answers)
        )
        assert math.isclose(
            stage["all_pair_quality"]["brier"]["value"],
            brier,
            rel_tol=1e-12,
            abs_tol=1e-12,
        )
        budget = stage["cases"][0]["pairs"][0]["counts"]["input_tokens"]
        partial = capture(
            "one-attempt-budget",
            [*base, "--allow-unverified", "--budget", str(budget)],
            4,
        )["json"]
        assert (
            partial["status"] == "incomplete"
            and partial["token_usage"]["attempts"] == 1
        )
        assert (
            partial["token_usage"]["spent_input"]
            == partial["token_usage"]["reported_input"]
            == budget
        )
        assert partial["suites"][0]["join"]["all_pair_quality"]["cases"] == 1
        evidence["captures"].append(
            cancellation([*common, "-vv", *base, "--allow-unverified"], env, directory)
        )
        save()

        duplicate = next(
            case for case in stage["cases"] if case["case_id"].startswith("duplicate-")
        )
        context = directory / "context.txt"
        drafted = [
            json.loads(line)
            for line in (suite.parent / "cases.jsonl").read_text().splitlines()
        ]
        context.write_text(
            next(
                case["context"]
                for case in drafted
                if case["id"] == duplicate["case_id"]
            )
        )
        right = directory / "right.jsonl"
        right.write_bytes(b"".join(bytes(unit["raw"]) for unit in duplicate["right"]))
        left = b"".join(bytes(unit["raw"]) for unit in duplicate["left"])
        universe = {(pair["left"], pair["right"]): pair for pair in duplicate["pairs"]}
        for cardinality in ["1:1", "1:n", "n:m"]:
            item = capture(
                "direct-cli-" + cardinality,
                [
                    "join",
                    "-",
                    str(right),
                    "--on",
                    "name,city",
                    "--context",
                    str(context),
                    "--max-candidates",
                    "2",
                    "--cardinality",
                    cardinality,
                    "--strict",
                    "--no-cache",
                    "--summary",
                    "--json",
                    "--deadline",
                    "60s",
                ],
                0,
                left,
            )
            claimed: set[int] = set()
            for row in item["json"]:
                eligible = []
                for pair in row["evaluated_pairs"]:
                    l = pair["decision"]["left_index"]
                    r = pair["decision"]["right_index"]
                    assert pair["request"] == universe[l, r]["request"]
                    assert pair["counts"] == universe[l, r]["counts"]
                    assert cardinality == "n:m" or r not in claimed
                    if pair["decision"]["eligible"]:
                        eligible.append((pair["decision"]["ranking_score"], r))
                winners = [
                    r for _, r in sorted(eligible, key=lambda pair: (-pair[0], pair[1]))
                ]
                if cardinality == "1:1":
                    winners = winners[:1]
                selected = [match["right"]["index"] for match in row["matches"]]
                assert selected == winners
                claimed.update(selected)
        evidence["published_reports"] = [
            json.loads(path.read_text())
            for path in sorted(report_dir.rglob("report.json"))
        ]
        evidence["measured"] = {
            "blocking_recall": stage["blocking_recall"],
            "all_pair_accuracy": stage["all_pair_quality"]["accuracy"],
            "assignments": stage["assignments"],
        }
        evidence["complete"] = True
        save()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".private/qualification/evidence/join-evaluation.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())
