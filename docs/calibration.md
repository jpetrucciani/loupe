# Calibration

`calibrate` fits empirical decision thresholds on pinned labeled data. It uses the evaluator's execution, raw answers, exact retry accounting, fixed split assignments and report publication. It defaults to `calibrate`; `--partition captured` explicitly fits review/development data. `test` is always rejected. Results do not ratify a baseline, satisfy a held-out gate or promise population precision.

```sh
loupe calibrate --labels alerts.jsonl -p severity --dataset-revision reviewed-v1 \
  --init-splits --target precision:page=0.97 --max-uncertain 10% --report reports
loupe calibrate --suite presets/severity --target precision:page=0.97 --write
```

`precision=RATE` targets the positive class for Noul and overall selective accuracy for Choice. `precision:LABEL=RATE` targets one declared label, with `true`/`false` or `yes`/`no` for Noul. Repeat `--target` for several labels. Every target needs positive predicted support, controlled by `--min-support` (default 1). The accepted population also needs that much support. A zero-support target never passes. `--max-uncertain` accepts a fraction or percentage and measures the actual uncertain fraction. If no policy satisfies the constraints, the command reports `unmet`, exits 1 and writes no presets.

The search considers finite probability boundaries, adjacent representable values and centers between observed Noul probabilities. Noul candidates vary threshold and strict uncertainty margin. Choice candidates vary minimum confidence and minimum top-two gap; exact option ties remain uncertain. Precision targets maximize accepted support, then correctness, then minimize parameter movement from the existing policy. A coverage-only search maximizes selective accuracy within the uncertainty cap, then accepted support. Explicit threshold/gate flags hold their applicable parameter fixed. Scores are unsupported. A mixed rubric requires `--field NAME`; fitted parameters go into `[policy.fields.NAME]` and preserve sibling policies.

This is an empirical event search, with tests against independent dense grids. It is not a proof of optimality over every floating-point parameter pair. The current Noul search considers quadratically many centers and processes all observations for each center; large calibration sets can be expensive. Reports retain the selected parameters, accepted/correct counts, each target's predicted/correct support, candidate counts and the entire underlying evaluation. The fitted accuracy pass alone trains the policy; uncached warmup and latency still use the same partition and count toward the shared budget.

`calibrate --ensemble` fits the same Noul/Choice decision parameters on equal-weight mean distributions from the original and its named variants. The agreement gate is fixed at `policy.min_agreement` or the explicit `--min-agreement`; fitting cannot relax it. Agreement-failed cases remain uncertain under every candidate and remain in the coverage denominator. Fitted records add `ensemble:{min_agreement}` and, when nonempty, `agreement_uncertain_cases`. The raw original/member answers and normalized means remain in the underlying evaluation. Choose an original field with `--field` for mixed rubrics; generated variants and companion signals cannot be fitted independently.

```sh
loupe --profile server calibrate alerts.jsonl -p outage --dataset-revision reviewed-v1 \
  --ensemble --min-agreement 1 --target precision=0.98 --max-uncertain 10% \
  --init-splits --allow-unverified --json
```

Ensemble proposals bind mode, the fixed agreement gate and all variant definitions/order in addition to the existing model/preset/view identities. Their annotations include `policy.calibration.ensemble` and matching fitting provenance; `policy.min_agreement` is pinned. Applying an ensemble fit requires `--ensemble` with that gate. Applying a scalar fit to an expanded ensemble, or disabling/changing a pinned ensemble, exits 64. Explicit `calibrate` may refit either mode or a different agreement gate on calibration/development cases; ordinary execution cannot make that switch. Preset live lint currently evaluates scalar examples, so it refuses ensemble-calibrated policies. Trusted template arguments are expanded after validating the original pin, while annotation metadata stays opaque.

`--dry-run` plans without credentials or inference and cannot be combined with `--write`. Default live runs require fresh authoritative identity. Explicit `--allow-unverified` fits diagnostic parameters with provisional reports and no writable preset proposal. `--write` forces verified identity and strict count parity even when that diagnostic flag is supplied.

## Execution views

`--sweep-pack unit,window:4,window:16`, `--sweep-combine any,all` and `--sweep-focus 0,3` fit the effective view grid on the same fixed `calibrate` cases. Combine candidates imply splitting; focus zero requests splitting without focus, while positive focus counts clear the combine operation. Duplicate effective views run once. The entire grid is preflighted before credentials, discovery or split publication. Test and captured partitions cannot enter a grid. One budget/deadline covers every accuracy, warmup and uncached latency phase; discovered model/encoder and observed response identities, fixed cases and provenance must stay constant.

Each suite selects among its feasible fits by accepted support, then correct decisions, then uncached original-unit throughput. Exact ties retain declaration order. Quality constraints are enforced before throughput breaks a tie. The command does not optimize token billing. Missing feasible fits yield `unmet`, exit 1 and no writes. A failed or incomplete grid retains completed evaluations/fits, reports the failure and selects no winner. Dry runs contain plans with no inferred fits or winners. Unverified diagnostics can report a provisional winner, but cannot produce or publish a preset proposal.

```sh
loupe calibrate alerts.jsonl -p outage --dataset-revision reviewed-v1 \
  --sweep-pack unit,window:4,window:16 --question-id-source lines \
  --target precision=0.98 --max-uncertain 10% --init-splits --write
```

The `calibration-sweep` JSON object includes status, target/support/uncertainty constraints, a shared `token_usage`, raw per-view `variants` with evaluations and fits, per-suite `selected` winners, `error` and notes. Winners contain their view name, suite/field, accepted/correct/case counts, uncached units per second, proposed preset and written path. Proposed annotations retain `source.view_selection` with the method, winner, throughput and candidate fits; each underlying evaluation retains the raw answers and phase timings. Reports use the ordinary private JSON/Markdown bundle publication.

The proposed preset increments its version and binds the full model tuple, local/server encoder identities, canonical preset content and question order, concrete pack/combine/split/focus/truncate/path settings, effective limits, exact calibration case IDs and split/fixture provenance. Calibration annotations are opaque metadata and never enter file/environment template expansion. The raw preset is checked before templates expand. Application requires fresh discovery with matching identity/view and matching fitted decision parameters. Explicit recalibration can retune an old policy for a newly discovered model, while keeping its source/trust validation. Additional CLI context changes the calibrated question set and is rejected.

Window views also bind their source-derived question ID family. `--question-id-source lines|records|files|units` selects `L<n>`, `R<n>`, `F<n>` or `U<n>` for evaluation windows, using original case source indices. It defaults to records, or the recorded family when evaluating an existing calibrated policy. Choose lines for ordinary line filters, records for paragraphs/record formats, and files for `--files`. Unit/whole requests retain their base question IDs. Application still requires the matching actual reader family; changing it changes the prompt and fails the live view check.

A published winner records `[policy] pack` for automatic packing. CLI classification and ordinary evaluation restore its focus, split/truncation and chunk settings when no explicit view control is supplied; evaluation also restores the recorded question ID family. Explicit controls take precedence and must pass the live pin check. Structured field truncation restores its field path, even when that path is named `unit` or `content`. Input formats, contextual wrapping and file handling are never inferred from an annotation. Explicit recalibration can measure a different view. A newly selected nonsplit view removes the old preset's split/combine defaults.

Before writing, Loupe rechecks fresh server identity, all proposals and original preset hashes. Each preset uses a cooperating-writer directory lock, a private staged file, file synchronization, atomic rename and directory synchronization. Built-ins are copied to the XDG preset directory without replacing a concurrently created user preset. Project writes walk held root/directory descriptors without following symlinks. Changed sources, redirected files and busy locks fail. Project/external trust is refreshed only for the generated content hash; a crash before trust publication leaves it untrusted. An external editor that ignores the lock is checked optimistically before commit; this is not a filesystem compare-and-swap against concurrent editors.

Atomicity is per preset. Multiple suites cannot overwrite the same preset in one invocation. If a later filesystem commit fails, the report records earlier committed paths and exits with the failure. Reports contain units, labels and server metadata, so the same private bundle rules as [eval](eval.md) apply. server's identity-pinned writing and application now pass real functional checks. Synthetic functional cases establish mechanics, not production model quality.
