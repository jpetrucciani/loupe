# Held-out baselines and acceptance

`eval --baseline FILE` evaluates absolute floors/ceilings and regression bounds against a pinned, human-ratified held-out report. Loupe never creates or ratifies baseline records automatically. First run a verified evaluation, inspect its numbers and raw answers, then have a person write the floors, support requirements and justification. Current server reports are provisional and cannot supply an accepted baseline.

```sh
loupe eval alerts.jsonl -p severity --dataset-revision reviewed-v1 \
  --init-splits --strict --report reports --json
loupe eval alerts.jsonl -p severity --dataset-revision reviewed-v1 \
  --baseline severity-baseline.toml --strict --report reports --json
```

Both commands need an endpoint that reports the full model/encoder identity. For provisional server measurements, omit `--baseline` and add `--allow-unverified`. That diagnostic flag cannot relax acceptance eligibility.

The baseline file can be TOML or JSON. This example is a template; copy the actual bundle path, `report.json` digest from its `manifest.json`, suite/field names and reviewed bounds:

```toml
schema_version = 1
report = "reports/eval-TIMESTAMP-PID"
report_blake3 = "COPY_THE_64_CHARACTER_REPORT_JSON_DIGEST"

[ratification]
by = "REVIEWER"
at = "2026-10-06T22:00:00Z"
justification = "Explain why these inspected held-out values justify the chosen absolute floors and regression bounds."

[[checks]]
suite = "adhoc/severity/alerts.jsonl"
field = "severity"
minimum_support = 50
[checks.gate]
metric = "macro-f1"
minimum = 0.90
max_drop = 0.02

[[checks]]
suite = "adhoc/severity/alerts.jsonl"
minimum_support = 10
[checks.gate]
metric = "latency-p95-ms"
maximum = 500.0
max_increase = 50.0
```

Each check requires `suite`, a `gate` with `metric` and at least one explicit `minimum` or `maximum`, and positive `minimum_support` (default 1). Regression bounds alone cannot ratify the first run. Mixed fields need `field`; otherwise one applicable field is inferred. Multiple incompatible policy/option domains for that field must be separated into suites. Timing and parity checks omit `field`. Duplicate checks, unknown/inapplicable fields or metrics, nonfinite/reversed bounds, negative regression tolerances, zero support and blank ratification/justification are rejected. `at` requires RFC 3339.

Classification metrics are `accuracy`, `macro-f1`, `f1` (positive Noul), `auroc` (Noul), `brier`, `ece`, `coverage`, `selective-accuracy`, `mae` and `within-one-accuracy` (Score). Timing metrics are `latency-p50-ms`, `latency-p95-ms` and `units-per-second`; they use the complete uncached latency phase. Parity suites additionally support `probability-max-error`, `probability-mean-error` and `argmax-agreement`. Blocking recall and assignment quality need their separate command-specific evaluation pipeline.

All bounds are inclusive and use the metric's native units. `max_drop` means `reference - current <= bound`; `max_increase` means `current - reference <= bound`. These are absolute differences, not relative percentages. Improvements produce negative differences. Both current and reference values need defined finite numbers and the declared minimum support; undefined values stay undefined and never become zero or pass. Latency percentiles count completed packing-group samples, while throughput support counts original units. Classification support follows the [metric definitions](evaluation-metrics.md).

Only the complete fixed `test` partition can supply or receive acceptance. Calibration/captured partitions, comparisons and sweep flags are rejected; `calibrate` also rejects `--baseline`. The reference must be a complete verified evaluator bundle, with every held-out case and complete uncached latency. Its classification/parity summaries are recomputed from saved raw answers and must agree. Plans, partial reports, aliases and provisional identities cannot be ratified. Every evaluated suite needs an explicit check, and any suite-level gate declarations must be covered with the same bounds for every applicable field.

The reference report supplies the model identity tuple, local/server encoders, preset content/version/question order, decision policy, ensemble mode/agreement, packing/combine/focus/truncate settings and effective limits. Preparation compares manifests, immutable dataset/fixture/split pins, held-out IDs and actual prepared questions before credentials, split publication or inference. Fresh discovery must match model revision, dtype/quantization, server build, both encoder identities and limits before cache lookup or admission. Changed models, quantization, policies or views require an explicit comparison and a separately ratified baseline. A filename or profile alias cannot substitute for this identity.

`report` stays relative to the baseline file's held directory, with no traversal or linked directory components. The bounded baseline record and sealed bundle are read through regular-file descriptors without following final symlinks or blocking on FIFOs. Both published report files must match their manifest, and JSON must additionally match the ratification's BLAKE3. Reading a baseline creates no files. Baseline records inherit the privacy of their referenced reports.

Reports add optional `acceptance` only when this flag is used. It contains schema version, `plan|passed|failed|incomplete`, baseline/reference digests, full reference model/encoder identity, ratification and per-check suite/field/metric/bounds/support. Completed assessments retain actual/reference metric values, support, each signed bound calculation, failure reasons and pass state. Plans infer no acceptance; failed requests retain the original error and incomplete assessments. Exit 0 means all qualified checks passed (or an offline plan completed), exit 1 means a defined/support or bound check failed, and fatal input/usage/identity/request failures retain their normal exit codes. Report generation remains `complete` when measurement succeeded but acceptance failed; the separate `acceptance.status` carries that verdict.

Numeric, identity codec, real filesystem and real saved-answer projection tests qualify the client mechanics. Fresh actual server reports verify strict counts and refusal of provisional baselines. Positive live acceptance and production human-ratified records still require authoritative server metadata and review; the client does not certify those prerequisites on its own.
