"""Compare a real HTTP endpoint with explicitly selected local reference weights."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import sys
import urllib.request
from pathlib import Path
from typing import Any

REVISION = "17f0b0ad64efb65d273590632833508766b2aae6"
SOURCE_SHA = "0e304cf7c6500e8bb59bef7e2afd2c6373f82596dfb3b57d1aa93c175e2dc3a3"


def probabilities(response: dict[str, Any]) -> dict[str, dict[str, float]]:
    result: dict[str, dict[str, float]] = {}
    for key, answer in response["answers"].items():
        if answer["type"] == "noul":
            p = float(answer["noul"])
            result[key] = {"true": p, "false": 1 - p}
        else:
            result[key] = {key: float(p) for key, p in answer["probabilities"].items()}
        if not result[key] or any(not math.isfinite(p) or not 0 <= p <= 1 for p in result[key].values()):
            raise ValueError("response has empty, nonfinite or out-of-range probabilities")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--device", required=True, help="Explicit reference device, e.g. cuda:1; never selected automatically")
    parser.add_argument("--server-precision", required=True)
    parser.add_argument("--max-diff", type=float, required=True, help="Explicit accepted bound for this precision comparison")
    parser.add_argument("--min-argmax", type=float, required=True)
    parser.add_argument("--cases", type=Path, default=Path("evals/golden/clef-v1/records.json"))
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if not (math.isfinite(args.max_diff) and args.max_diff >= 0 and math.isfinite(args.min_argmax) and 0 <= args.min_argmax <= 1):
        parser.error("bounds must be finite, with max-diff >= 0 and min-argmax in [0,1]")
    if hashlib.sha256(args.source.read_bytes()).hexdigest() != SOURCE_SHA:
        parser.error("reference source does not match the pinned release")
    if not args.weights.is_dir() or not (args.weights / "joint_head.safetensors").is_file():
        parser.error("weights must be an existing local release directory; implicit model downloads are disabled")
    import torch
    spec = importlib.util.spec_from_file_location("loupe_clef_reference", args.source)
    if spec is None or spec.loader is None:
        raise ValueError("reference source cannot be imported")
    reference = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = reference
    spec.loader.exec_module(reference)
    model, processor = reference.load_release_model(args.weights, device=args.device, dtype=torch.bfloat16, local_files_only=True)
    differences: list[float] = []
    agreements = 0
    questions = 0
    records = []
    for fixture in json.loads(args.cases.read_text()):
        request = fixture.get("request", fixture)
        expected = reference.systemone(model, processor, request)
        http_request = urllib.request.Request(args.url, data=json.dumps(request, ensure_ascii=False, allow_nan=False).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(http_request, timeout=30) as response:
            actual = json.load(response)
        left, right = probabilities(expected), probabilities(actual)
        if left.keys() != right.keys():
            raise ValueError("question IDs differ")
        for question, distribution in left.items():
            if distribution.keys() != right[question].keys():
                raise ValueError("option IDs differ")
            differences.extend(abs(p - right[question][key]) for key, p in distribution.items())
            best = lambda values: min(values, key=lambda key: (-values[key], key))
            agreements += best(distribution) == best(right[question])
            questions += 1
        records.append({"request": request, "reference": expected, "actual": actual})
    maximum = max(differences, default=0.0)
    agreement = agreements / questions if questions else 0.0
    report = {"reference_revision": REVISION, "source_sha256": SOURCE_SHA, "reference_precision": "bf16", "reference_device": args.device, "weights_directory": str(args.weights.resolve()), "weights_identity_verified": False, "server_precision": args.server_precision, "server_identity_verified": False, "bounds": {"max_diff": args.max_diff, "min_argmax": args.min_argmax}, "max_diff": maximum, "mean_diff": sum(differences) / len(differences) if differences else None, "argmax_agreement": agreement, "questions": questions, "requests": records}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    if not questions or maximum > args.max_diff or agreement < args.min_argmax:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
