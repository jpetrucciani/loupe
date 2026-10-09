"""Measure a real System One HTTP endpoint with mixed lengths and question types."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from http.client import HTTPException
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen


@dataclass
class Sample:
    workload: str
    status: int
    latency_ms: float
    input_tokens: int
    response_model: str
    error: str | None


def percentile(values: list[float], fraction: float) -> float | None:
    return sorted(values)[math.ceil(len(values) * fraction) - 1] if values else None


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--concurrency", nargs="+", type=int, default=[1, 2, 4])
    parser.add_argument("--cycles", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=20)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if min(args.concurrency) < 1 or args.cycles < 1 or args.timeout <= 0:
        parser.error("concurrency, cycles and timeout must be positive")
    base = args.url.rstrip("/")
    questions = {
        "outage": {
            "type": "noul",
            "instructions": "Is checkout currently unavailable?",
        },
        "topic": {
            "type": "choice",
            "instructions": "Choose the subject.",
            "criteria": {
                "commerce": "Checkout and payments",
                "weather": "Weather and clouds",
                "other": "Other topics",
            },
        },
        "impact": {
            "type": "score",
            "instructions": "Rate current customer impact.",
            "criteria": ["None", "Degraded service", "Checkout unavailable"],
        },
    }
    payloads: list[tuple[str, bytes]] = []
    for length, repeats in [("short", 8), ("medium", 128), ("long", 512)]:
        state = "A routine status check completed successfully. " * repeats
        state += "\nCheckout is now down. Customers cannot place orders."
        for kind, fields in [
            ("noul", ["outage"]),
            ("choice", ["topic"]),
            ("score", ["impact"]),
            ("joint", list(questions)),
        ]:
            payload = json.dumps(
                {
                    "model": args.model,
                    "state": state,
                    "questions": {key: questions[key] for key in fields},
                }
            ).encode()
            payloads.append((f"{length}/{kind}", payload))

    def send(work: tuple[str, bytes]) -> Sample:
        name, payload = work
        start = time.perf_counter()
        status = 0
        try:
            request = Request(
                base + "/v1/systemone",
                data=payload,
                headers={"Content-Type": "application/json"},
            )
            with urlopen(request, timeout=args.timeout) as response:
                status = response.status
                result = json.load(response)
            expected = json.loads(payload)["questions"]
            answers = result["answers"]
            if set(answers) != set(expected):
                raise ValueError("response question IDs differ from request")
            for key, question in expected.items():
                answer = answers[key]
                if answer["type"] != question["type"]:
                    raise ValueError("response question type differs from request")
                probabilities = (
                    [answer["noul"]]
                    if question["type"] == "noul"
                    else list(answer["probabilities"].values())
                )
                if not probabilities or any(
                    not math.isfinite(p) or not 0 <= p <= 1 for p in probabilities
                ):
                    raise ValueError("invalid probabilities")
                if question["type"] != "noul" and abs(sum(probabilities) - 1) > 0.01:
                    raise ValueError("probability mass differs from one")
            return Sample(
                name,
                status,
                (time.perf_counter() - start) * 1000,
                int(result["usage"]["input_tokens"]),
                result["model"],
                None,
            )
        except HTTPError as error:
            status = error.code
            message = error.read().decode(errors="replace")[:2000]
        except (OSError, HTTPException, ValueError, KeyError, TypeError) as error:
            message = f"{type(error).__name__}: {error}"
        return Sample(
            name, status, (time.perf_counter() - start) * 1000, 0, "", message
        )

    with urlopen(base + "/health", timeout=args.timeout) as response:
        health = json.load(response)
    # Warm every shape once, sequentially, and retain errors separately.
    warmup = [send(payload) for payload in payloads]
    measurements = []
    for concurrency in args.concurrency:
        start = time.perf_counter()
        with ThreadPoolExecutor(max_workers=concurrency) as executor:
            samples = list(executor.map(send, payloads * args.cycles))
        wall = time.perf_counter() - start
        successful = [sample for sample in samples if sample.error is None]
        latencies = [sample.latency_ms for sample in successful]
        groups = {}
        for length in ["short", "medium", "long"]:
            group = [
                sample
                for sample in successful
                if sample.workload.startswith(length + "/")
            ]
            times = [sample.latency_ms for sample in group]
            groups[length] = {
                "successful": len(group),
                "p50_ms": percentile(times, 0.5),
                "p95_ms": percentile(times, 0.95),
                "reported_input_tokens": sorted(
                    {sample.input_tokens for sample in group}
                ),
            }
        measurements.append(
            {
                "concurrency": concurrency,
                "successful": len(successful),
                "failed": len(samples) - len(successful),
                "wall_seconds": wall,
                "requests_per_second": len(successful) / wall,
                "reported_input_tokens_per_second": sum(
                    sample.input_tokens for sample in successful
                )
                / wall,
                "p50_ms": percentile(latencies, 0.5),
                "p95_ms": percentile(latencies, 0.95),
                "by_length": groups,
                "samples": [asdict(sample) for sample in samples],
            }
        )
        print(
            json.dumps(
                {
                    key: value
                    for key, value in measurements[-1].items()
                    if key != "samples"
                }
            ),
            flush=True,
        )
    report = {
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "url": base,
        "model": args.model,
        "health": health,
        "cycles": args.cycles,
        "workload_sha256": hashlib.sha256(
            b"\n".join(payload for _, payload in payloads)
        ).hexdigest(),
        "workloads": [
            {"name": name, "payload_bytes": len(payload)} for name, payload in payloads
        ],
        "warmup": [asdict(sample) for sample in warmup],
        "measurements": measurements,
        "scope": "HTTP round trips, new urllib connections, no client retries/cache; repeated synthetic payloads may benefit from server prefix caching. Token lengths are server-reported, not encoder-verified. No quality or GPU attribution claim.",
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
