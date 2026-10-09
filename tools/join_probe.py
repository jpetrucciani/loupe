"""Capture actual semantic join cardinality, provenance and journal replay."""

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


def lines(records: list[dict[str, Any]]) -> bytes:
    return (
        "\n".join(json.dumps(record, ensure_ascii=False) for record in records) + "\n"
    ).encode()


def entity(id_: str) -> dict[str, str]:
    return {"id": id_, "name": "Acme Dental Clinic", "location": "Portland Maine"}


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
        "complete": False,
        "captures": captures,
        "proof_boundary": "Built GNU x86_64 CLI, actual server pair-local joint inference and real run journals. Synthetic functional cardinality/uncertainty/recovery fixtures. No held-out blocking recall or pair-quality gate, fresh full identity/cache, new dependencies, native release packaging or host activation claim.",
    }

    def save() -> None:
        output.write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n")

    with tempfile.TemporaryDirectory(prefix="loupe-join-probe-") as temporary:
        directory = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(directory / "xdg")
        config = directory / "config.toml"
        config.write_text(
            (root / "examples/config.toml")
            .read_text()
            .replace("http://127.0.0.1:8015", endpoint)
        )
        right = directory / "right.jsonl"
        right.write_bytes(
            lines(
                [
                    entity("r-a"),
                    entity("r-b"),
                    {
                        "id": "r-noise",
                        "name": "Acme Hardware",
                        "location": "Portland Oregon",
                    },
                ]
            )
        )
        common = [str(binary), "--config", str(config), "--profile", "server"]

        def capture(
            name: str,
            options: list[str],
            source: bytes,
            expected: int = 0,
            *,
            right_path: Path = right,
            active_config: Path = config,
            json_output: bool = True,
        ) -> dict[str, Any]:
            journal = directory / (name + ".jsonl")
            command = [
                str(binary),
                "--config",
                str(active_config),
                "--profile",
                "server",
                "join",
                "-",
                str(right_path),
                "--on",
                "name,location",
                "--strict",
                "--no-cache",
                "--summary",
                "--deadline",
                "60s",
                *options,
            ]
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
                "right_input": right_path.read_text(),
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
            assert result.returncode == expected, item
            assert int(summary["input_counted"]) == int(
                summary["input_reported"]
            ) + int(summary["unknown_input_counted"]), item
            if json_output:
                rows = [json.loads(line) for line in result.stdout.splitlines()]
                item["records"] = rows
                for row in rows:
                    for pair in row.get("evaluated_pairs", []):
                        assert pair["counts"]["exact"]
                        assert (
                            pair["counts"]["input_tokens"]
                            == pair["response"]["usage"]["input_tokens"]
                        ), item
                        assert (
                            pair["request"]["state"]["left"]["id"] == f"L{row['index']}"
                        ), item
                        probabilities = pair["response"]["answers"]["similarity"][
                            "probabilities"
                        ]
                        ranking = (
                            sum(
                                int(level) * probability
                                for level, probability in probabilities.items()
                            )
                            / sum(probabilities.values())
                            / (len(probabilities) - 1)
                        )
                        assert (
                            abs(pair["decision"]["ranking_score"] - ranking) < 1e-12
                        ), item
                        assert pair["decision"]["probabilities"] == probabilities, item
                    if row.get("matches"):
                        matches = row["matches"]
                        assert all(
                            match["decision"]["eligible"] for match in matches
                        ), item
                        assert [match["ranking_score"] for match in matches] == sorted(
                            [match["ranking_score"] for match in matches], reverse=True
                        ), item
            save()
            return item

        source = lines(
            [
                entity("left-a"),
                entity("left-b"),
                {
                    "id": "left-none",
                    "name": "Unlisted Workshop",
                    "location": "Berlin Germany",
                },
            ]
        )
        for cardinality in ["1:1", "1:n", "n:m"]:
            name = "cardinality-" + cardinality.replace(":", "-")
            item = capture(
                name,
                ["--cardinality", cardinality, "--unmatched", "both", "--why"],
                source,
            )
            expected_counts = {"1:1": [1, 1, 0], "1:n": [2, 0, 0], "n:m": [2, 2, 0]}
            assert [
                len(row["matches"]) for row in item["records"][:3]
            ] == expected_counts[cardinality], item
            assert item["records"][3]["right"]["unit"]["id"] == "r-noise", item
            resumed = capture(
                name + "-resume",
                [
                    "--cardinality",
                    cardinality,
                    "--unmatched",
                    "both",
                    "--why",
                    "--resume",
                    str(directory / (name + ".jsonl")),
                    "--budget",
                    "0",
                ],
                source,
            )
            assert (
                resumed["stdout"] == ""
                and int(resumed["summary"]["http_attempts"]) == 0
            ), resumed
        one = directory / "one.jsonl"
        one.write_bytes(lines([entity("right-only")]))
        for ids in [["alpha", "beta"], ["beta", "alpha"]]:
            item = capture(
                "order-" + ids[0],
                ["--unmatched", "both"],
                lines([entity(ids[0]), entity(ids[1])]),
                right_path=one,
            )
            assert (
                item["records"][0]["kind"] == "matched"
                and item["records"][0]["left"]["unit"]["id"] == ids[0]
            ), item
            assert (
                item["records"][1]["kind"] == "unmatched_left"
                and item["records"][1]["evaluated_pairs"] == []
            ), item
        for action in ["drop", "keep", "stderr"]:
            item = capture(
                "uncertain-" + action,
                ["--margin", "1", "--on-uncertain", action, "--unmatched", "both"],
                lines([entity("left")]),
                right_path=one,
            )
            assert item["records"][0]["kind"] == (
                "matched" if action == "keep" else "unmatched_left"
            ), item
        failed = capture(
            "uncertain-fail",
            ["--margin", "1", "--on-uncertain", "fail", "--why"],
            lines([entity("left")]),
            3,
            right_path=one,
        )
        assert "uncertain answers=" in failed["stderr"], failed
        replayed = capture(
            "uncertain-fail-resume",
            [
                "--margin",
                "1",
                "--on-uncertain",
                "fail",
                "--why",
                "--resume",
                str(directory / "uncertain-fail.jsonl"),
                "--budget",
                "0",
            ],
            lines([entity("left")]),
            3,
            right_path=one,
        )
        assert int(replayed["summary"]["http_attempts"]) == 0
        three = lines([entity("alpha"), entity("beta"), entity("gamma")])
        plan = subprocess.run(
            [
                *common,
                "join",
                "-",
                str(one),
                "--on",
                "name,location",
                "--cardinality",
                "n:m",
                "--dry-run",
            ],
            input=three,
            capture_output=True,
            env=environment,
            cwd=directory,
            timeout=15,
            check=False,
        )
        assert plan.returncode == 0, plan.stderr
        plans = [json.loads(line) for line in plan.stdout.splitlines()]
        allowance = str(
            plans[0]["candidates"][0]["pair_plan"]["counts"]["input_tokens"]
        )
        item = capture(
            "budget-prefix",
            ["--cardinality", "n:m", "--budget", allowance],
            three,
            4,
            right_path=one,
        )
        assert len(item["records"]) == 1, item
        item = capture(
            "budget-prefix-repeat",
            [
                "--cardinality",
                "n:m",
                "--resume",
                str(directory / "budget-prefix.jsonl"),
                "--budget",
                "0",
            ],
            three,
            4,
            right_path=one,
        )
        assert item["stdout"] == "" and int(item["summary"]["http_attempts"]) == 0, item
        item = capture(
            "budget-prefix-complete",
            [
                "--cardinality",
                "n:m",
                "--resume",
                str(directory / "budget-prefix.jsonl"),
            ],
            three,
            right_path=one,
        )
        assert [row["left"]["unit"]["id"] for row in item["records"]] == [
            "beta",
            "gamma",
        ], item
        capture(
            "tsv",
            ["-O", "tsv"],
            lines([entity("left")]),
            right_path=one,
            json_output=False,
        )
        capture("zero-budget", ["--budget", "0"], source, 4)
        limited = directory / "limited.toml"
        limited.write_text(
            config.read_text().replace(
                "max_length = 16384", "max_length = 16384\nmax_state_tokens = 128"
            )
        )
        oversized = lines(
            [{**entity("large"), "note": "large " * 400}, entity("small")]
        )
        item = capture(
            "keep-error",
            ["--keep-going", "--on-error", "keep"],
            oversized,
            active_config=limited,
            right_path=one,
        )
        assert (
            item["records"][0]["kind"] == "error"
            and item["records"][0]["error"]["code"] == 65
        ), item
        assert item["records"][1]["kind"] == "matched", item
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
        default=Path(".private/qualification/evidence/server-join.json"),
    )
    args = parser.parse_args()
    run(args.binary.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
