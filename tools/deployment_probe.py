"""Verify a real deployed server with isolated cache and calibrated presets."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--tokenizer", required=True, type=Path)
    parser.add_argument("--url", required=True)
    parser.add_argument("--out", required=True, type=Path)
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    tokenizer = options.tokenizer.resolve(strict=True)
    captures: list[dict[str, object]] = []
    environment = {k: v for k, v in os.environ.items() if not k.startswith("LOUPE_")}
    environment["LOUPE_TOKENIZER"] = str(tokenizer)
    with tempfile.TemporaryDirectory(prefix="loupe-deployment-") as temporary:
        root = Path(temporary)
        environment["XDG_CONFIG_HOME"] = str(root / "xdg")
        environment["LOUPE_CACHE_DIR"] = str(root / "cache")
        presets = root / "xdg/loupe/presets"
        presets.mkdir(parents=True)
        (root / "config.toml").write_text(
            "[profile.server]\nbase_url="
            + json.dumps(options.url)
            + "\nconcurrency=4\n"
        )
        (presets / "outage.toml").write_text(
            "version=1\ntype='noul'\ninstructions='Is checkout unavailable?'\n"
        )
        (root / "context.txt").write_text("Production incident")
        rows = [
            {
                "id": f"case-{i}",
                "unit": state,
                "label": label,
                "source": "synthetic-functional",
                "reviewed": False,
            }
            for i, (state, label) in enumerate(
                [
                    ("Checkout is down and customers cannot pay.", True),
                    ("Checkout is healthy and payments succeed.", False),
                    ("Checkout is unavailable to every customer.", True),
                    ("Checkout works normally, with no errors.", False),
                    ("Nobody can complete checkout during this outage.", True),
                    ("All checkout requests succeeded today.", False),
                    ("Checkout has stopped processing orders.", True),
                    ("Checkout is operating normally.", False),
                ]
            )
        ]
        (root / "cases.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in rows)
        )

        def invoke(arguments: list[str], state: str = "", expected: int = 0) -> dict:
            run = subprocess.run(
                [str(binary), "--config", str(root / "config.toml"), *arguments],
                cwd=root,
                env=environment,
                input=state,
                text=True,
                capture_output=True,
                timeout=120,
                check=False,
            )
            capture = {
                "arguments": arguments,
                "input": state,
                "exit": run.returncode,
                "stdout": run.stdout,
                "stderr": run.stderr,
            }
            captures.append(capture)
            if run.returncode != expected:
                raise RuntimeError(json.dumps(capture))
            return json.loads(run.stdout) if run.stdout.strip() else {}

        doctor = invoke(["doctor", "--json"])
        assert doctor["cache_eligible"]
        args = ["keep", "-p", "outage", "--strict", "--json", "--summary"]
        state = "Checkout is down.\n"
        assert not invoke(args, state)["cached"]
        assert invoke(args, state)["cached"]
        assert "attempts=0" in captures[-1]["stderr"]
        assert not invoke(args, "Checkout remains unavailable.\n")["cached"]
        assert not invoke(args + ["--context", "context.txt"], state)["cached"]
        assert not invoke(
            [
                "keep",
                "Does this report a checkout outage?",
                "--strict",
                "--json",
                "--summary",
            ],
            state,
        )["cached"]
        assert not invoke(args + ["--refresh"], state)["cached"]
        common = [
            "--labels",
            "cases.jsonl",
            "-p",
            "outage",
            "--dataset-revision",
            "deployment-functional-v1",
            "--json",
            "--strict",
            "--warmup",
            "0",
            "--report",
            "reports",
        ]
        evaluation = invoke(["eval", *common, "--init-splits"])
        assert evaluation["identity"]["verified"]
        assert (
            evaluation["token_usage"]["spent_input"]
            == evaluation["token_usage"]["reported_input"]
        )
        sweep = invoke(["eval", *common, "--sweep-pack", "unit,window:4"])
        assert sweep["status"] == "complete"
        calibration = invoke(
            ["calibrate", *common, "--target", "precision=1", "--write"]
        )
        assert calibration["evaluation"]["identity"]["verified"]
        assert calibration["fitted"][0]["written"]
        written = (presets / "outage.toml").read_text()
        assert "calibration" in written
        applied = invoke(args, state)
        assert applied["model"] == doctor["discovery"]["full_model_identity"]
        invoke(args + ["--threshold", "0.01"], state, expected=64)
        held_out = invoke(["eval", *common])
        assert held_out["identity"]["verified"]
        result = {
            "captured_at_utc": datetime.datetime.now(datetime.UTC).isoformat(),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "endpoint": options.url,
            "captures": captures,
            "published_preset": written,
            "scope": "Functional synthetic cases only; no human-ratified accuracy gate or changed-weight/quantization qualification.",
        }
    options.out.write_text(json.dumps(result, indent=2) + "\n")
    print(
        f"Verified discovery, cache isolation, eval sweep, calibrated write/reuse: {options.out}"
    )


if __name__ == "__main__":
    main()
