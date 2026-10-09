"""Save CLI/endpoint evidence from actual requests, with no simulated responses."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("target/debug/loupe"))
    parser.add_argument("--url", required=True)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(".private/qualification/evidence/server-cli.json"),
    )
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    common = [
        binary,
        "--config",
        "examples/config.toml",
        "--profile",
        "server",
        "--url",
        args.url,
    ]
    input_bytes = b"Checkout is down. Customers cannot place orders.\r\nScheduled heartbeat completed successfully.\n"
    checks = [
        (["doctor", "--json"], b"", 0),
        (
            [
                "keep",
                "-p",
                "real-failure",
                "--json",
                "--strict",
                "--no-cache",
                "--summary",
            ],
            input_bytes,
            0,
        ),
        (["tag", "-p", "severity", "--json", "--strict", "--no-cache"], input_bytes, 0),
        (
            ["rate", "-p", "sentiment", "--json", "--strict", "--no-cache"],
            input_bytes,
            0,
        ),
        (["tokens", "-p", "real-failure", "-I", "lines"], input_bytes, 0),
        (["models"], b"", 69),
    ]
    evidence = []
    for arguments, input_bytes, expected in checks:
        result = subprocess.run(
            common + arguments,
            input=input_bytes,
            capture_output=True,
            timeout=45,
            check=False,
        )
        evidence.append(
            {
                "arguments": arguments,
                "input": input_bytes.decode(),
                "exit": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
            }
        )
        if result.returncode != expected:
            raise ValueError(
                f"{arguments[0]} failed: {result.returncode}, {result.stderr.decode()}"
            )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(
            {
                "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "endpoint": args.url + "/v1/systemone",
                "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                "lock_sha256": hashlib.sha256(
                    Path("Cargo.lock").read_bytes()
                ).hexdigest(),
                "checks": evidence,
                "server_full_identity_available": False,
                "reference_inference_qualified": False,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n"
    )
    print(f"Saved {len(evidence)} actual CLI checks to {args.out}")


if __name__ == "__main__":
    main()
