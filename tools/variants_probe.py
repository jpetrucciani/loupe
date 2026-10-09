"""Capture real comparison and calibration sweeps from an installed Loupe binary."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import subprocess
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass
class Capture:
    arguments: list[str]
    exit_code: int
    stderr: str
    report: object


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--tokenizer", type=Path, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(".private/qualification/evidence/server-eval-variants.json"),
    )
    options = parser.parse_args()
    binary = options.binary.resolve()
    tokenizer = options.tokenizer.resolve(strict=True)
    checkout = Path(__file__).resolve().parents[1]
    captures: list[Capture] = []
    environment = os.environ.copy()
    environment["LOUPE_TOKENIZER"] = str(tokenizer)
    for key in (
        "LOUPE_URL",
        "LOUPE_MODEL",
        "LOUPE_PROFILE",
        "LOUPE_CONCURRENCY",
        "LOUPE_API_KEY",
        "LOUPE_PRESET_PATH",
    ):
        environment.pop(key, None)
    with tempfile.TemporaryDirectory(prefix="loupe-variants-") as temporary:
        root = Path(temporary)
        (root / ".git").mkdir()
        presets = root / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        environment["XDG_CONFIG_HOME"] = str(root / "xdg")
        environment["LOUPE_CACHE_DIR"] = str(root / "cache")
        rubric = (
            "version=1\n[questions.available]\ntype='noul'\ninstructions='Is checkout unavailable?'\n"
            "[questions.topic]\ntype='choice'\ninstructions='Choose the main topic.'\n"
            "[questions.topic.criteria]\ncommerce='Checkout and payments'\nweather='Weather and clouds'\n"
            "[questions.severity]\ntype='score'\ninstructions='Rate checkout impact from zero to two.'\n"
            "criteria=['No impact','Minor impact','Checkout is entirely unavailable']\n"
        )
        alternate = rubric.replace("version=1", "version=2").replace(
            "Is checkout unavailable?", "Does this describe an unavailable checkout?"
        )
        (presets / "rubric.toml").write_text(rubric)
        (presets / "alternate.toml").write_text(alternate)
        (presets / "document.toml").write_text(
            "version=1\ntype='noul'\ninstructions='Does any part explicitly report that checkout is unavailable?'\n"
        )
        rows = [
            {
                "id": f"case-{index}",
                "unit": (
                    "Checkout is down and customers cannot pay."
                    if index % 2 == 0
                    else "Blue skies and sunshine all day."
                ),
                "label": {
                    "available": index % 2 == 0,
                    "topic": "commerce" if index % 2 == 0 else "weather",
                    "severity": 2 if index % 2 == 0 else 0,
                },
                "source": "synthetic-functional",
                "reviewed": False,
            }
            for index in range(8)
        ]
        long_rows = [
            {
                "id": f"document-{index}",
                "unit": "Neutral archival background. " * 600
                + (
                    "Checkout is unavailable."
                    if index % 2 == 0
                    else "Checkout works normally."
                ),
                "label": index % 2 == 0,
                "source": "synthetic-functional",
                "reviewed": False,
            }
            for index in range(4)
        ]
        for name, data in (("cases.jsonl", rows), ("documents.jsonl", long_rows)):
            (root / name).write_text(
                "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in data)
            )
        config = ""
        for name, model, limit in (
            ("server", "clef-flash", 16384),
            ("alias", "Cloudflare/clef-flash", 16384),
            ("short", "clef-flash", 1400),
        ):
            config += f"[profile.{name}]\nbase_url={json.dumps(options.url)}\npath='/v1/systemone'\nmodel='{model}'\nencoder='clef-v1'\ntokenizer='hf:Cloudflare/clef-flash'\ntokenizer_revision='17f0b0ad64efb65d273590632833508766b2aae6'\nconcurrency=2\nmax_length={limit}\n"
        (root / "config.toml").write_text(config)

        def capture(
            arguments: list[str], expected: int = 0, profile: str = "server"
        ) -> dict[str, object]:
            complete = [
                "--config",
                str(root / "config.toml"),
                "--profile",
                profile,
                "eval",
                *arguments,
                "--json",
            ]
            result = subprocess.run(
                [str(binary), *complete],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
                timeout=240,
            )
            if result.returncode != expected:
                raise RuntimeError(
                    f"command returned {result.returncode}, expected {expected}: {result.stderr}"
                )
            report = json.loads(result.stdout)
            captures.append(Capture(complete, result.returncode, result.stderr, report))
            return report

        common = [
            "--labels",
            "cases.jsonl",
            "-p",
            "rubric",
            "--dataset-revision",
            "functional-v1",
            "--init-splits",
            "--warmup",
            "0",
        ]
        diagnostic = ["--allow-unverified", "--strict", "--no-cache"]
        plan = capture([*common, "--sweep-pack", "unit,window:4", "--dry-run"])
        assert plan["status"] == "plan"
        assert plan["token_usage"]["attempts"] == 0
        unit = capture(
            [
                *common,
                *diagnostic,
                "--partition",
                "calibrate",
                "--pack",
                "unit",
                "--report",
                "unit-reports",
            ]
        )
        capture(
            [
                *common,
                *diagnostic,
                "--partition",
                "calibrate",
                "--pack",
                "window:4",
                "--report",
                "window-reports",
            ]
        )
        left = next((root / "unit-reports").iterdir())
        right = next((root / "window-reports").iterdir())
        saved = capture(
            [
                "--compare",
                str(left),
                str(right),
                "--allow-unverified",
                "--budget",
                "0",
                "--report",
                "saved-comparisons",
            ]
        )
        assert saved["token_usage"]["attempts"] == 0
        assert saved["comparisons"][0]["paired_fields"] == 12
        rubric_comparison = capture(
            [
                *common,
                *diagnostic,
                "--compare",
                "preset:rubric",
                "preset:alternate",
                "--report",
                "rubric-comparisons",
            ]
        )
        alias_comparison = capture(
            [*common, *diagnostic, "--compare", "profile:server", "profile:alias"]
        )
        assert (
            alias_comparison["variants"][1]["report"]["identity"]["requested_model"]
            == "Cloudflare/clef-flash"
        )
        sweep = capture(
            [
                *common,
                *diagnostic,
                "--sweep-pack",
                "unit,window:4",
                "--report",
                "packing-sweeps",
            ]
        )
        partial = capture(
            [
                *common,
                *diagnostic,
                "--compare",
                str(left),
                "profile:server",
                "--partition",
                "calibrate",
                "--budget",
                "0",
            ],
            expected=4,
        )
        assert partial["status"] == "incomplete"
        assert partial["variants"][0]["report"] == unit
        assert partial["variants"][1]["report"]["token_usage"]["attempts"] == 0
        long_sweep = capture(
            [
                "--labels",
                "documents.jsonl",
                "-p",
                "document",
                "--dataset-revision",
                "functional-long-v1",
                "--init-splits",
                "--warmup",
                "0",
                *diagnostic,
                "--sweep-combine",
                "any,mean",
                "--sweep-focus",
                "0,1",
                "--overlap",
                "0",
                "--report",
                "long-sweeps",
            ],
            profile="short",
        )
        assert len(long_sweep["variants"]) == 3
        for aggregate in (rubric_comparison, alias_comparison, sweep, long_sweep):
            usage = aggregate["token_usage"]
            assert (
                usage["spent_input"] == usage["reported_input"] + usage["unknown_input"]
            )
            assert usage["spent_input"] == sum(
                variant["report"]["token_usage"]["spent_input"]
                for variant in aggregate["variants"]
                if variant["source"] == "live"
            )
            for variant in aggregate["variants"]:
                for suite in variant["report"]["suites"]:
                    if aggregate["kind"] == "sweep":
                        assert suite["partition"] == "calibrate"
                        assert (
                            suite["selected_case_ids"] == suite["splits"]["calibrate"]
                        )
                    for phase in suite["phases"]:
                        assert phase["usage"]["cache_hits"] == 0
        bundles = []
        for parent in (
            "unit-reports",
            "window-reports",
            "saved-comparisons",
            "rubric-comparisons",
            "packing-sweeps",
            "long-sweeps",
        ):
            for bundle in (root / parent).iterdir():
                files = [
                    bundle / name
                    for name in ("report.json", "report.md", "manifest.json")
                ]
                assert bundle.stat().st_mode & 0o777 == 0o700
                assert all(path.stat().st_mode & 0o777 == 0o600 for path in files)
                bundles.append(
                    {
                        "manifest": json.loads(files[2].read_text()),
                        "json_sha256": hashlib.sha256(
                            files[0].read_bytes()
                        ).hexdigest(),
                        "markdown": files[1].read_text(),
                        "directory_mode": "0700",
                        "file_modes": ["0600"] * 3,
                    }
                )
        source_paths = [
            "crates/loupe-cli/src/args.rs",
            "crates/loupe-cli/src/run.rs",
            "crates/loupe-cli/src/run/eval.rs",
            "crates/loupe-cli/src/run/eval/output.rs",
            "crates/loupe-cli/src/run/eval/variants.rs",
            "crates/loupe-cli/src/run/eval/variants/compare.rs",
            "crates/loupe-core/src/report_store.rs",
            "default.nix",
            "tools/variants_probe.py",
        ]
        evidence = {
            "observed_at_utc": datetime.datetime.now(datetime.UTC).isoformat(),
            "endpoint": options.url,
            "binary": str(binary),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "tokenizer": {
                "path": str(tokenizer),
                "sha256": hashlib.sha256(tokenizer.read_bytes()).hexdigest(),
            },
            "source_sha256": {
                path: hashlib.sha256((checkout / path).read_bytes()).hexdigest()
                for path in source_paths
            },
            "cases": rows,
            "document_cases": long_rows,
            "presets": {"rubric": rubric, "alternate": alternate},
            "case_provenance": "Synthetic functional fixtures, not human-reviewed acceptance labels or ratified quality floors.",
            "checks": [asdict(capture) for capture in captures],
            "published_bundles": bundles,
            "server_full_identity_available": bool(unit["identity"]["verified"]),
            "acceptance_gates_qualified": False,
            "positive_live_cache_qualified": False,
        }
        options.out.parent.mkdir(parents=True, exist_ok=True)
        options.out.write_text(
            json.dumps(evidence, indent=2, ensure_ascii=False) + "\n"
        )
        usage = [capture.report["token_usage"] for capture in captures]
        print(
            json.dumps(
                {
                    "output": str(options.out),
                    "captures": len(captures),
                    "published_bundles": len(bundles),
                    "spent_input": sum(value["spent_input"] for value in usage),
                    "reported_input": sum(value["reported_input"] for value in usage),
                    "unknown_input": sum(value["unknown_input"] for value in usage),
                    "attempts": sum(value["attempts"] for value in usage),
                    "http_attempts": sum(value["http_attempts"] for value in usage),
                }
            )
        )


if __name__ == "__main__":
    main()
