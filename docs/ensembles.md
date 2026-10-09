# Joint question variants

`--ensemble` adds a preset's named variants to the same encoded request as its original questions. The original and variants have equal weight. The jointly deciding head lets them affect one another; the report explicitly marks these measurements as correlated and uncalibrated.

For example, save a trusted XDG preset as `outage.toml`:

```toml
type = "noul"
instructions = "Is checkout currently unavailable?"

[[variants]]
id = "direct"
instructions = "Are customers currently unable to check out?"

[[variants]]
id = "countercheck"
instructions = "Does this describe an unresolved checkout outage?"

[policy]
min_agreement = 1.0
```

```sh
printf '%s\n' 'Checkout is down.' \
  | loupe keep -p outage --ensemble --json --on-uncertain keep
printf '%s\n' 'Checkout is down.' \
  | loupe tokens -p outage --ensemble
```

Variant IDs use `[a-z][a-z0-9_-]*`, at most 64 bytes. Their question IDs are `ensemble:outage/direct` and `ensemble:outage/countercheck`; window mode binds these as `L1/ensemble:outage/direct`, for example. The original field remains `outage`, or `L1/outage` in a window. Each field counts toward schema limits and exact encoder budgets. Windows shrink with the full joint schema. Oversized schemas fail before inference. File-routing labels come only from the primary Choice criteria.

With a multi-question source, each variant declares `field = "<primary ID>"`. A variant inherits the primary type/instructions/criteria and may override instructions or descriptions. Choice keys and Score level indices must stay aligned; changing the type or option set is rejected. An inherited absent/null/empty instruction uses the original ID fallback. Presets validate variant declarations even when ensemble execution is disabled. The existing template trust and permissions rules apply.

Normalize each rounded Choice/Score distribution to unit mass, then average each option's probability. Noul adds the implicit false probability. Noul decisions use mean `p`; Score decisions use the expected level index of the mean distribution. Choice uses its highest mean probability and the existing sorted-key tie rule.

Agreement counts members whose unique winning option matches the mean distribution's winner, including the original in the denominator. Noul votes use `p >= 0.5`, irrespective of the policy's decision threshold. An exact Choice/Score tie casts no vote; a tied mean stays uncertain. The default requires all votes to agree. Set `policy.min_agreement` or pass `--min-agreement P` to choose a finite value in `[0,1]`. The CLI overrides the preset. Confidence, gap and Noul margin gates still apply, so agreement alone does not establish certainty. For explicitly split input, each member inherits its primary reducer; those reduced distributions are averaged afterward, and their existing chunk provenance remains attached.

Per-unit JSON preserves `answer` as the raw original field. It adds `ensemble` containing:

| Field | Meaning |
| --- | --- |
| `method` | `equal_mean_probabilities` |
| `independent`, `calibrated` | Both false |
| `model` | Identity reported with the joint response |
| `members` | Field IDs, original answer objects, normalized probabilities and votes |
| `mean_probabilities` | Equal-weight option distribution |
| `mean_noul`, `mean_score` | Applicable mean/expectation, otherwise null |
| `winning_option` | Boolean, Choice key or Score level index; null for an ambiguous option tie |
| `agreement`, `min_agreement` | Observed fraction and required gate |
| `agreement_uncertain` | Additional uncertainty from disagreement or an ambiguous winning option |
| `assessment` | Mean selection/confidence/uncertainty under the ordinary policy plus agreement gate |

The additional `decision` object uses `source = "ensemble"`. JSON always keeps the member answers; TSV and label/score annotations use the mean assessment. Boolean annotations use `ensemble_p=…`. Default keep/drop handling still partitions certain yes/no and uncertainty. Count records uncertain ensembles in its uncertain bucket. Sort orders mean Score decisions. On actions use the ensemble assessment and identify `LOUPE_DECISION_SOURCE=ensemble`; action state identity includes the agreement gate.

An uncertain ensemble can use the existing external fallback. The child receives canonical `LOUPE_ENSEMBLE_JSON`, bounded to 65,536 bytes, alongside unchanged raw primary question/answer metadata and original input bytes. Oversized metadata prevents launch and follows fallback failure policy. A resolved external decision overrides the emitted decision, retains the ensemble report, and supplies no external confidence. Recovery identity pins ensemble enablement and the agreement gate; raw saved responses are re-aggregated before any saved fallback is replayed. The model cache key continues to cover the entire expanded request.

Selection translates each named query variant into a presence field and a Choice over exactly the current bracket's candidates plus `none`. Independent `pick` also gets a relevance Score for every variant. Their IDs are `ensemble:selection-present/<id>`, `ensemble:selection/<id>` and `ensemble:selection-relevance/<id>`. Query instructions affect all generated heads; Noul criteria affect presence, and Score criteria affect independent relevance levels. Candidate IDs/evidence stay aligned across each bracket's Choices. Companion signals remain separate. Every head counts toward schema, option and token limits; repeated Choices reduce candidate capacity rather than bypassing the aggregate option cap.

Presence and Choice use their own mean distributions and agreement gates before survivor reduction. Independent ranking uses the mean Score distribution. Normal presence/none precedence still applies; keeping uncertainty cannot manufacture a span for a winning `none`. Fallback resolves eligible uncertain mean fields in gate order, including uncertainty caused only by disagreement. Each external Choice still contributes only one winner. A failed independent Choice fallback remains uncertain even when the mean `none` option wins.

```sh
loupe --profile server pick -f examples/selection-ensemble.toml \
  --ensemble --min-agreement 0.5 -n 2 --json < incidents.txt
```

`eval --ensemble` scores the derived mean for the original labeled fields; generated variants require no duplicate labels. Each record retains the complete original request/response, binding, policy, ensemble settings and recomputable member/mean reports. Saved comparisons verify those reports against the raw answers and refuse modified summaries, gates or cross-window bindings. Accuracy/Brier/ECE/ordinal error use the derived distribution, while agreement uncertainty also restricts coverage and every confidence-threshold curve. Changing a confidence gate cannot remove disagreement.

Pair rubrics name `same_entity`, `similarity` or a declared companion in each variant's `field`. Join applies the mean gate and mean Score distribution to eligibility/ranking. `decision.gate`, `decision.score`, `decision.probabilities` and companions remain raw diagnostics; `decision.derived` contains effective mean assessments and `decision.ensembles` contains member/mean reports. Gate-first fallback receives the mean report alongside unchanged raw metadata, and records external overrides separately. Dedupe uses its mean Noul decision for finite graph edges or streaming suppression. Accepted links and streaming journal pairs keep raw answers/assessments, an `ensemble` report and a separate derived/external `decision`. All heads include context and count toward exact schema/input limits. Pair plans and journals pin the agreement gate.

Explain variants may explicitly target `factor:<id>` or the primary. An omitted `field` still names the sole primary even when factors are present. Each ensembled factor derives its value from mean Noul/Score probabilities, then applies the existing complement and normalized weight. The contribution preserves raw `answer`, adds `raw_assessment` and `ensemble`, and uses the mean/agreement `assessment`. Uncertainty annotates the contribution without removing it. Unvaried factors retain their existing shape and calculation. JSON adds a primary `ensemble` report when that primary has variants; window reports bind all members to their own source. No calibration, independence or causal interpretation is added.

```sh
loupe --profile server join left.jsonl right.jsonl --on name,location \
  -f examples/join-ensemble.toml --ensemble
loupe --profile server dedupe -f examples/dedupe-ensemble.toml --ensemble < incidents.txt
loupe --profile server explain -f examples/ensemble-factors.toml --ensemble --json < incident.txt
```

Compare original questions alone with the joint configuration over identical fixed cases:

```sh
loupe --config examples/config.toml --profile server eval cases.jsonl \
  -p outage --dataset-revision reviewed-v1 --init-splits \
  --compare standalone ensemble --json --no-cache
```

Both targets retain separate raw requests, actual views, token usage and metrics; one budget/deadline covers the entire experiment. Default partition is test. Calibration sweeps retain their separate calibrate-only rule. Fresh model/local/server encoder identities must remain constant across this same-backend experiment. Default execution requires full authoritative identity; explicit `--allow-unverified` remains provisional. Warmup and measured latency bypass cache. In addition to aggregate decision/distribution differences, each comparison field reports `raw_primary_shift` (case/option counts and maximum/mean absolute probability differences) calculated before averaging, separating observed primary-answer movement from aggregation. Changed packing/views still affect the experiment; this is an observed interaction measurement, without an independence or causal claim. `report:standalone` or `report:ensemble` disambiguates a saved bundle with one of those reserved names. Exact server parity refuses ensemble expansion.

`calibrate --ensemble` fits Noul/Choice thresholds on the mean distribution with the agreement gate fixed. Agreement-failed cases stay uncertain in every candidate and in the coverage denominator. Proposals preserve all variants and pin ensemble mode plus minimum agreement; ordinary execution refuses switching mode or changing that gate. Explicit recalibration can choose a new configuration. The underlying raw/member probabilities remain uncalibrated and correlated. server's missing authoritative identity still limits fitting to provisional diagnostics and prevents live writes. See [calibration](calibration.md).
