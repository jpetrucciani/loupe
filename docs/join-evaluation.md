# Native join evaluation

`loupe eval --suite join` measures candidate blocking, pair decisions and greedy assignments separately. The seed lives in `evals/suites/join/` and uses eight fictional catalogs, 48 pairs and four fixed test scenarios. These are drafted labels, not human-reviewed production baselines. A current model identity is required by default; explicit `--allow-unverified` emits provisional diagnostics with cache disabled.

```bash
loupe --config examples/config.toml --profile server eval \
  --suite join --strict --no-cache --warmup 0 \
  --allow-unverified --json --report /tmp/loupe-join-report
```

The suite task is `join`. Each case uses `unit: {left: [record, ...], right: [record, ...]}` and a label map from every `L<n>` to its true `R<n>` IDs. Indices are 1-based catalog positions. Empty lists mean no true match. Both catalogs must contain nonempty object records; labels cannot name missing records or omit left records. The entire catalog is one stratified split unit, so its pairs never cross calibration and test. Per-case `context` supplies generic identity/normalization rules through the pair rubric; labels, case IDs, review status and notes are never sent to inference.

```toml
task = "join"
preset = "join-default"

[join]
fields = ["name", "city"]
blocking = "hybrid"
max_candidates = 20
max_pairs = 10000
cardinalities = ["1:1", "1:n", "n:m"]
```

`join-default` compiles the same Score, same-entity Noul and field Nouls as the join command. Trusted custom pair presets and ordinary threshold/margin/confidence/gap overrides use the shared compiler. Invalid policies, catalog labels, pins and the total selected Cartesian pair count are checked before credential lookup, split publication or inference. `max_pairs` bounds that total across selected scenarios. Fresh discovered limits revalidate every pair before cache admission. Pair records remain whole; classification packing/splitting/focus/truncation, ensemble fitting, classification baselines, view sweeps and comparisons are rejected with exit 64. Run join suites separately from classification/parity suites. Fallback commands cannot execute through evaluation. Preset keep/drop/fail/stderr uncertainty policy controls assignment; stderr has the same eligibility as drop.

The accuracy phase evaluates every Cartesian pair once. It preserves raw requests, exact counts, API responses, local gate/score/companion decisions, policies and ordered questions. Blocking recall is computed with all right records available, before assignment claims, and after the configured candidate cap. Its denominator is all true edges, including positives that share no blocking tokens. Gate accuracy, F1, AUROC, Brier, ECE, coverage and selective accuracy are reported for both the full pair universe and the independently retained subset. Ranking AUROC uses the native probability-weighted Score mean against binary match truth; the ranking mean is not presented as a calibrated binary probability.

Accuracy assignments reuse each exact pair's measured decision, then execute the native blocker and per-left greedy assignment for every configured cardinality. Claimed rights are excluded before the candidate cap. Each row retains candidates and selected rights. The metrics are:

| Metric | Denominator and meaning |
| --- | --- |
| Blocking recall | True edges retained by blocking / all true edges, with no prior claims. |
| Assignment accuracy | Correct left rows / completed left rows. A correct row contains only true edges and as many available true matches as its cardinality permits. Availability excludes actual prior right claims for 1:1 and 1:n. This is conditional on prior output, so read it alongside edge precision and feasible recall. |
| Edge precision | Correct output edges / all output edges. No output has an explicit undefined precision. |
| Feasible edge recall | Correct output edges / maximum true-edge capacity without blocking. Capacity is maximum bipartite matching for 1:1, the number of right records with any true left partner for 1:n, and all true edges for n:m. This avoids penalizing valid cardinality limits while retaining losses from greedy choices, wrong claims, blocking and model decisions. |

Zero denominators remain explicit undefined values with support and reason. They are never silently replaced by zero or a passing score. Negative-only cases still contribute meaningful correct-unmatched rows. Assignment summaries include only completely measured catalogs; pair summaries retain the answered prefix when accuracy stops early.

Every warmup pass and the measured latency pass run complete native join pipelines independently for each scenario/cardinality, with uncached inference. They do not replay the accuracy answers or infer speed from all-pair sampling. Timing samples cover a complete scenario/cardinality, and throughput counts completed left records. Accuracy timing separately counts catalog pairs. Current execution is sequential at the client; shared server load is uncontrolled and timings are diagnostics, not isolated GPU throughput. Raw phase groups include submitted pair plans, responses, rows, completion flags and usage. One budget/deadline covers all phases; each retry remains separately charged. Fatal errors, budget exhaustion and signals produce an incomplete report retaining completed responses and unknown charged attempts.

The JSON report preserves the ordinary run identity, split, usage and phase envelope and adds optional `suites[].join` with `cases`, `blocking_recall`, `all_pair_quality`, `blocked_pair_quality`, `ranking_auroc` and per-cardinality `assignments`. Each case contains source/review/location metadata, complete catalogs, effective rubric/policies/order, truth, independent blocking candidates, raw `pairs` and assignment rows/metrics. Ordinary classification reports omit this field. Private atomic JSON/Markdown publication uses the existing report store.

`tools/join_eval_probe.py` checks an explicitly configured live endpoint, including default identity refusal, one-attempt budget exhaustion, and all three direct CLI cardinalities. It verifies that native join requests/counts match the evaluator's pair universe and independently replays each direct CLI ranking/claim result. Reports default to the ignored `.private/qualification/evidence/` directory. Native tests cover fixed graph-stratified splits, offline full plans, incompatible modes, malformed catalogs, pair caps, all 512 three-by-three capacity graphs and opt-in real inference/MCP. The 400-pair target, human review/ratification, authoritative live identity and stage-specific baseline/comparison acceptance remain separate work.
