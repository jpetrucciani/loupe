# Unit failure policy

`--keep-going` handles failures of complete, independently identified input units. Its default is `--on-error drop`. `--on-error keep` emits the original unit, and `--on-error fail` stops with the original failure code after flushing the completed ordered prefix. `--on-error` requires `--keep-going`.

These flags apply to `keep`, `drop`, `tag`, `rate`, `sort`, `count` and `route`. `is`, `ask`, `tokens` and `--dry-run` reject them with exit 64 because their boolean, API-response and planning contracts do not have a kept-unit error result.

Token overflow and inference failures are eligible when the original unit is available. A failed HTTP batch or joint window produces a separate failure for every affected original unit, preserving their indices and order. Subsequent units are evaluated under the normal packing rules. Split and focus failures apply to the original unit, without producing a partial combined answer or partial evidence answer.

Malformed input without a complete parsed unit, unreadable input, invalid configuration/flags, authentication, budget exhaustion, the overall deadline, cancellation and output/file/process failures remain fatal. In particular, an exhausted budget does not authorize more submissions. Reader failures cannot invent a parsed state for an invalid JSON or CSV record. Per-request retry exhaustion can be handled; the overall deadline ends the run.

Kept failures have no answer, model identity or calibrated confidence:

| Output | Kept failure |
| --- | --- |
| Plain `keep`, `drop`, `sort`, `route` | Original bytes, including any CSV/TSV header required by that destination |
| Plain `tag`/`rate`, or annotate | `error` and a tab, followed by original bytes |
| TSV | Four columns: original index, `error`, empty confidence, escaped unit text |
| JSONL | `{index, unit, answer: null, model: null, cached: false, error: {code, message}}` |

`sort` places kept failures after all scored units, in source order, including with `--reverse`. Their position does not imply a score. `count` excludes failures from its answer histogram and reports their number separately: JSON adds an `errors` field when nonzero; plain/TSV adds `N\t@errors`. A choice label called `error` remains an ordinary histogram label. Both drop and keep retain this diagnostic count.

`route` records omitted failures in its journal and manifest. Kept failures use the reserved `uncertain` destination, including an explicitly configured process for that label; their payload follows the selected output format. They bypass the answer's uncertainty policy and the raw uncertainty-capture transformation. A route file can consequently contain formatted kept failures alongside raw captured uncertain answers when both features are selected. Select plain output when a homogeneous raw stream is required. Route resume validates the error policy as part of the run identity and does not infer recorded failures again. Process resume remains unavailable because external actions cannot be replayed safely.

Errors and uncertainty have independent policies and counters. `--on-error keep` does not override `--on-uncertain fail`, which still exits 3. An error does not increment `uncertain` or enter a non-route `--uncertain FILE`. `--summary` adds `errors`, the number of original-unit failures reached by the ordered loop. Existing `units` counts completed results; `failed_records` counts actual logical API failures, excluding input preflight errors. An overflow contributes no submitted tokens or API latency. Refused submitted requests retain their charged unknown usage.

After handled errors, the normal command exit rule applies: emitted units or a nonempty count report return 0, and an entirely omitted stream returns 1. A fatal error keeps its original exit code. Kept errors are also diagnosed on stderr unless `-q` is given.

Add `--failed FILE` to persist admitted units, request plans, failures and actual completed responses. Repeat the command and original input with `--resume FILE`; acknowledged kept/dropped errors are skipped, while unhandled failures can be attempted again. Sort/count retain completed answers until their aggregate output is acknowledged. A budget applies separately to each invocation, and restored responses consume no new tokens. See [recovery.md](recovery.md) for source/run validation and the stdout acknowledgement boundary. Route's transactional file resume remains available through its reserved manifest filename.
