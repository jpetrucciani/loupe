"""Generate CPU-only fixtures from the pinned upstream encoder, without torch."""

from __future__ import annotations

import argparse
import ast
import dataclasses
import hashlib
import json
import random
import struct
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from tokenizers import Tokenizer

REVISION = "17f0b0ad64efb65d273590632833508766b2aae6"
SOURCE_SHA = "0e304cf7c6500e8bb59bef7e2afd2c6373f82596dfb3b57d1aa93c175e2dc3a3"
TOKENIZER_SHA = "06b9509352d2af50381ab2247e083b80d32d5c0aba91c272ca9ff729b6a0e523"
FUNCTIONS = {"render", "question_options", "_tokens", "_encode_media", "encode_record"}
CLASSES = {"EncodedQuestion", "EncodedRecord"}
CONSTANTS = {
    "SYSTEM_PROMPT",
    "IMAGE_PLACEHOLDER",
    "VIDEO_PLACEHOLDER",
    "MEDIA_BATCH_KEYS",
    "MEDIA_TOKEN_KEYS",
    "QUESTION_TYPES",
}


def reference(source: Path) -> dict[str, Any]:
    data = source.read_bytes()
    if hashlib.sha256(data).hexdigest() != SOURCE_SHA:
        raise ValueError("reference source does not match pinned content")
    tree = ast.parse(data)
    nodes = [
        node
        for node in tree.body
        if (isinstance(node, ast.FunctionDef) and node.name in FUNCTIONS)
        or (isinstance(node, ast.ClassDef) and node.name in CLASSES)
        or (
            isinstance(node, ast.Assign)
            and any(
                isinstance(target, ast.Name) and target.id in CONSTANTS
                for target in node.targets
            )
        )
    ]
    prelude = ast.parse(
        "from __future__ import annotations\nimport json\nfrom dataclasses import dataclass\nfrom dataclasses import field as dataclass_field\nfrom typing import Any\n"
    )
    namespace: dict[str, Any] = {"__name__": "loupe_reference_encoder"}
    # Dataclass annotation resolution needs a registered module.
    import sys
    import types

    module = types.ModuleType(namespace["__name__"])
    sys.modules[module.__name__] = module
    namespace = module.__dict__
    exec(
        compile(
            ast.Module(body=prelude.body + nodes, type_ignores=[]), str(source), "exec"
        ),
        namespace,
    )
    return namespace


class Adapter:
    def __init__(self, path: Path) -> None:
        self.tokenizer = Tokenizer.from_file(str(path))

    def __call__(self, text: str, add_special_tokens: bool = False) -> SimpleNamespace:
        return SimpleNamespace(
            input_ids=self.tokenizer.encode(
                text, add_special_tokens=add_special_tokens
            ).ids
        )


def requests() -> list[dict[str, Any]]:
    examples: list[dict[str, Any]] = []
    states: list[Any] = [
        "",
        "hello",
        "é café 東京 😀\nline\tend",
        {"z": [1e16, -0.0, 1e-5], "a": "α"},
        None,
        True,
        [0, 1, 2],
        {"nul": "\u0000", "quote": '"\\', "control": "\b\f\r"},
        {"big_integer": 2**100, "negative_integer": -(2**100)},
    ]
    rng = random.Random(731)
    for index in range(100):
        questions: dict[str, Any] = {
            "outage": {
                "type": "noul",
                "instructions": [
                    None,
                    "",
                    {"question": "Is this an outage?", "context": "checkout"},
                ][index % 3],
            },
            "severity": {
                "type": "choice",
                "instructions": "Choose severity",
                "criteria": {"ticket": "Can wait", "page": "Act now", "noise": None},
            },
            "urgency": {
                "type": "score",
                "criteria": [
                    "Can wait",
                    {"description": "Soon", "weight": 1e16},
                    "Now",
                ],
            },
        }
        if index % 5 == 0:
            questions["outage"]["criteria"] = {"true": None, "false": "Nothing failed"}
        if index % 7 == 0:
            questions["severity"]["criteria"] = {"only": None}
        if index % 11 == 0:
            questions["urgency"]["criteria"] = ["Only level"]
        items = list(questions.items())
        rng.shuffle(items)
        state = (
            states[index % len(states)]
            if index < 40
            else {
                "index": index,
                "text": ("record café\n" * rng.randrange(0, 80)),
                "nested": {"a": rng.random(), "b": rng.uniform(-1e20, 1e20)},
            }
        )
        examples.append(
            {"model": "clef-flash", "state": state, "questions": dict(items)}
        )
    return examples


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--assets",
        type=Path,
        default=Path.home()
        / ".cache/loupe/tokenizers/Cloudflare--clef-flash"
        / REVISION,
    )
    parser.add_argument("--source", type=Path)
    parser.add_argument("--tokenizer", type=Path)
    parser.add_argument("--out", type=Path, default=Path("evals/golden/clef-v1"))
    args = parser.parse_args()
    tokenizer_path = args.tokenizer or args.assets / "tokenizer.json"
    if hashlib.sha256(tokenizer_path.read_bytes()).hexdigest() != TOKENIZER_SHA:
        raise ValueError("tokenizer does not match pinned content")
    namespace = reference(args.source or args.assets / "joint_schema_model.py")
    tokenizer = Adapter(tokenizer_path)
    args.out.mkdir(parents=True, exist_ok=True)
    records = []
    for request in requests():
        # Exercise the exact values after wire decoding, rather than Python-only objects.
        wire_request = json.loads(
            json.dumps(request, ensure_ascii=False, allow_nan=False)
        )
        encoded = namespace["encode_record"](
            tokenizer, wire_request, max_length=1_000_000
        )
        record = dataclasses.asdict(encoded)
        record.pop("media")
        record.pop("record_id")
        state_tokens = len(
            tokenizer(namespace["render"](wire_request["state"])).input_ids
        )
        records.append(
            {
                "request": wire_request,
                "encoded": record,
                "state_tokens": state_tokens,
                "input_tokens": len(encoded.input_ids),
            }
        )
    (args.out / "records.json").write_text(
        json.dumps(records, ensure_ascii=False, separators=(",", ":")) + "\n"
    )
    rng = random.Random(1081)
    numbers = [1e16, -0.0, 1e-5, 1e-4, 1e15, 1e20, 1e-300, 1e300]
    while len(numbers) < 1000:
        number = struct.unpack("!d", rng.getrandbits(64).to_bytes(8, "big"))[0]
        import math

        if math.isfinite(number):
            numbers.append(number)
    (args.out / "numbers.json").write_text(
        json.dumps(
            [
                {"value": number, "rendered": namespace["render"](number)}
                for number in numbers
            ],
            separators=(",", ":"),
            allow_nan=False,
        )
        + "\n"
    )
    (args.out / "provenance.json").write_text(
        json.dumps(
            {
                "revision": REVISION,
                "reference_sha256": SOURCE_SHA,
                "tokenizer_sha256": TOKENIZER_SHA,
                "tokenizers": "0.23.2",
                "license": "Apache-2.0",
                "source": f"https://huggingface.co/Cloudflare/clef-flash/blob/{REVISION}/joint_schema_model.py",
                "records": len(records),
                "numeric_cases": len(numbers),
                "model_weights_loaded": False,
            },
            indent=2,
        )
        + "\n"
    )
    print(
        f"Generated {len(records)} full-ID/head records and {len(numbers)} numeric fixtures without model weights"
    )


if __name__ == "__main__":
    main()
