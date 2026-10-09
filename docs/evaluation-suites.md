# Evaluation suite manifests and fixed splits

`loupe-core::suites` loads version-1 `suite.toml` and pinned JSONL cases, resolves registered document fixtures and creates or validates fixed split assignments. The [evaluation runner](eval.md) consumes these manifests and pure [metric calculations](evaluation-metrics.md). Calibration, baseline acceptance and reviewed public seed sets remain separate TODO items.

The manifest records:

| Field | Meaning |
| --- | --- |
| `schema_version` | Exactly `1`; unknown fields and versions are rejected. |
| `id`, `revision` | Nonempty suite identity and revision. |
| `preset` | Trusted preset name; its effective content/version is part of run/report identity. Parity uses the exact saved reference questions instead. |
| `task` | `noul`, `choice`, `score`, `retrieval`, `pair`, `parity`, `mixed` or `join`. |
| `join` | Required only for task `join`: scalar `fields`, `blocking`, positive `max_candidates`/`max_pairs`, and distinct nonempty `cardinalities`. See [catalog join evaluation](join-evaluation.md). |
| `selection` | Optional retrieval settings: find/pick/independent/extract mode, positive take/round bounds, context/blank-line controls and extraction normalization options. See [document selection evaluation](selection-evaluation.md). |
| `cases` | `{path, revision, blake3}` pin for the exact JSONL bytes. |
| `fixtures` | Optional list of `{path, revision, blake3}` pins for large documents. |
| `splits` | Relative path of the persisted JSON split artifact. Its parent must already exist. |
| `split` | `{seed, calibrate_percent}`; percentage is an integer from 1 through 99. |
| `provenance` | Nonempty `source` and `license`; this records provenance without asserting legal approval. |
| `metrics` | Optional metric identifiers; empty leaves selection to task defaults in the later runner. |
| `gates` | Optional bounds `{metric, minimum?, maximum?, max_drop?, max_increase?}`. Bounds must be finite, ordered and have at least one value; regression tolerances are nonnegative. [`eval --baseline FILE`](baselines.md) requires every declaration to be covered by a human-ratified check with the same bounds and all applicable fields. There is no automatic ratification. |

Metric identifiers are `accuracy`, `macro-f1`, `auroc`, `f1`, `brier`, `ece`, `coverage`, `selective-accuracy`, `mae`, `within-one-accuracy`, `probability-max-error`, `probability-mean-error`, `argmax-agreement`, `latency-p50-ms`, `latency-p95-ms`, `units-per-second`, `blocking-recall` and `assignment-accuracy`. Parsing a declaration does not establish that its metric is implemented or appropriate for a task; the runner must enforce applicability.

All paths are relative to the manifest directory and contain ordinary components only. Absolute paths, parent traversal and symlinks are rejected. A held directory descriptor and NOFOLLOW opens confine case/fixture reads and split publication. Files must be regular; FIFO opens cannot block. Every pin has a revision and a lowercase, 64-digit BLAKE3 digest. All fixture pins are checked at load, and selected document bytes are checked again when materialized. Changing bytes without updating the pin fails before evaluation.

Each JSONL case requires `id`, exactly one input shape, and `label`. The input is `unit` (any JSON value, including null), `doc: "@relative/path"` referencing a registered fixture, or both `left` and `right`. Optional fields are `query`, `candidates`, `context`, `tags`, `source`, `reviewed`, `sampling` and `notes`. Unknown fields and duplicate IDs fail. `source` defaults to `handwritten`; `reviewed` defaults to false, preserving the distinction between drafted labels and human acceptance. Document fixtures become UTF-8 strings; pair inputs become `{left, right}`.

Parity cases additionally require `reference: {request, response, encoder_identity}` containing the exact saved request and validated raw response. Other tasks cannot carry a reference. Encoder identity must be nonempty and matches the pinned local encoder at execution. Reference state, question order and model alias are retained exactly; known different full model identities require explicit comparison support. An alias-only reference remains provisional. Case locations record original physical JSONL line numbers and UTF-8 byte ranges, including gaps from blank lines.

| Task | Label |
| --- | --- |
| `noul`, `pair` | Boolean; pair tasks require left/right input. |
| `choice` | Nonempty option key. |
| `score` | Nonnegative integer level, or `[low, high]` with low <= high. Level/criterion agreement belongs to runner validation. |
| `retrieval` | Unique, nonempty candidate IDs; an empty list represents absence. |
| `retrieval` span expectations | Optional `expected_spans` with original verbatim/UTF-8 byte/physical line coordinates and normalized values for gold IDs; never encoded into inference. |
| `parity`, `mixed` | Nonempty question-ID map with the preceding scalar/interval/ID labels. Nested label maps are rejected. |
| `join` | `unit: {left: [records], right: [records]}` and a complete map from each `L<n>` to true `R<n>` IDs. The whole catalog is one split unit; empty ID lists represent absence. |

`source = "review"` always enters the separate `captured` split. Its optional `sampling` is `uncertainty` or `uniform-audit`; other sources cannot declare review sampling. This preserves the audit origin for separate reporting. Captures do not enter either fixed calibrate or held-out test assignments. Calibration/development can consume captured data through an explicit separate selection, while acceptance gates must select test only.

For non-review cases, stratification uses the complete serialized label, including the full label map for mixed cases. Within each stratum, rank by BLAKE3 using the derive-key context `loupe evaluation split v1`, the little-endian u64 seed, little-endian u64 label-byte length, serialized JSON label bytes and case-ID bytes. Hash ties use lexical case-ID order. Select floor(n * percent / 100) calibration cases, clamped to 1 through n-1 for strata with at least two cases; the rest enter test. Singletons enter calibrate only. Small or combinatorial strata can therefore yield little or no test coverage, which must remain visible in later reports and gates.

The artifact records schema version, suite ID/revision, dataset revision/content digest, seed/percentage and sorted `calibrate`, `test`, `captured` ID lists. Assignment is independent of JSONL record order; the content pin still detects reordered dataset bytes. Loading an artifact recomputes and compares all assignments, rejecting changed identities, missing/duplicate/reassigned IDs or captures moved into test. A missing split must be created explicitly; existing invalid splits are never repaired or replaced automatically.

Publication holds an exclusive directory lock, writes a private mode-0600 temporary file, syncs it, links it into the final name without replacement, syncs the directory, removes the temporary link and syncs cleanup. Readers see either no artifact or a complete artifact. An interrupted publication may leave a private temporary link, which is outside evaluation; the final artifact, if present, is validated on reopen. Inference-free validation does not execute preset templates, credentials or external processes.

Seven real unit/filesystem tests cover label/input serialization (including null and score intervals), fixed seed/stratification and review exclusion, singleton handling, idempotent split loading, changed revisions/assignments, pinned and changed fixtures, symlinks, invalid cases/settings/pins, real directory ownership and held-root path replacement. No inference backend or HTTP mock is involved. These tests do not qualify the later runner, metrics, held-out model quality or power-loss behavior.
