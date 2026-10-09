# Run summary contract

`--summary` writes one `loupe: units=...` line to stderr when a classification/planning run ends, including normal EOF, fatal budget/input/API failures, signals and broken pipes. Local configuration/grammar failures before a run is prepared do not create a summary. `mode=plan` identifies offline plans; these do not invent submitted usage or inference latency. `-vv` adds a per-submission trace with attempt number, record count and reserved input tokens, without printing request content or authentication.

| Field | Meaning |
| --- | --- |
| `units` | Original units whose completed result/plan reached the ordered output loop, including uncertain decisions and non-emitting `is`/count results. |
| `logical_records` | System One record evaluations started, counted once before cache lookup. Batch members are separate records; a joint window is one record. Split/focus phases contribute their actual underlying requests. Discovery and input/preflight work are excluded. |
| `succeeded_records`, `failed_records`, `cancelled_records` | API/cache outcomes of logical records. Policy uncertainty is a successful API result. Cancellation counts started work abandoned without a final outcome. A preflight failure before evaluation does not invent a failed API record. |
| `attempts` | Submitted record attempts, including retries and every submitted HTTP batch member. |
| `http_attempts` | Actual submitted HTTP requests; an HTTP batch counts once. |
| `input_counted` | Encoded/estimated input charged for every submitted attempt. Cache hits and unsent reservations contribute none. |
| `input_reported`, `output_reported` | Reconciled known usage from newly submitted work, excluding original usage embedded in cached responses. |
| `unknown_attempts`, `unknown_input_counted` | Submitted records without usable returned usage, and their retained counted input. A timeout/cancellation does not refund these. |
| `input_reserved` | Input reserved for work not yet submitted. Normal shutdown drops cancelled reservations before rendering the summary. |
| `cache_hits`, `cache_hit_rate` | Cached logical records and hits divided by started logical records. The rate is unavailable when there are no logical records. Work completed ahead of an emitted prefix remains part of the run's accounting. |
| `uncertain` | Uncertain original-unit decisions reached by the ordered loop; separate from transport errors. |
| `errors` | Original-unit failures reached by the ordered loop, including preflight overflow and unsent budget failures. Separate from completed `units`, uncertainty, and API-level `failed_records`. Reader failures without a complete unit are excluded. |
| `latency_samples` | Completed uncached logical records that actually submitted at least one attempt. Unsent budget failures, cache hits and cancelled unfinished evaluations have no completed latency sample. |
| `p50_ms`, `p95_ms` | Nearest-rank percentiles over those records, as the upper endpoints of histogram buckets. Shared HTTP batch duration contributes one weighted sample per member. |
| `latency_bounds`, `latency_kind` | `upper` and `uncached_submitted_logical`. Duration runs from evaluation start through the validated result/cache insertion, including queue/admission, retries and backoff; it excludes tokenizer/planner and discovery work. |
| `gpu_passes` | Unavailable until the server can attribute actual GPU passes to this run. HTTP counts, record counts, and concurrent global health-counter deltas are insufficient. |
| `cost_usd_estimate`, `cost_unknown_output` | Best-effort token cost under explicitly configured rates; unavailable without rates or on arithmetic overflow. Unknown output usage makes the numeric estimate incomplete. |

Latency uses a fixed 7,424-counter logarithmic histogram, retaining all completed samples across arbitrarily long streams without storing individual timings. Values below 128 ns are exact; later buckets have at most 1/128 relative width. The reported endpoints are upper bounds, rather than exact interpolated quantiles. Tests cover all bit boundaries, `u64::MAX`, weighted nearest ranks, empty samples, overflow and a long stream. Eval's uncached warmup/measurement requirements remain separate from this operational summary.

Optional prices live in trusted profile configuration:

```toml
[profile.example.pricing]
input_usd_per_million = 2.0
output_usd_per_million = 4.0
```

These example rates are mathematical fixtures, not provider prices. Both rates must be finite and nonnegative. Explicit settings override the profile, which overrides defaults. Cost equals `(reported_input + unknown_input_counted) × input_rate / 1e6 + output_reported × output_rate / 1e6`, rounded to six decimal places for display. Unknown input is counted conservatively; estimated encoders remain estimates, unknown output is unpriced, and external fallback costs are excluded. This is neither an invoice nor a dollar-budget guarantee. Cache hits create no new priced usage. Hosted pricing discovery and external costs remain later profile/cascade work.

Evidence uses real server batches/windows/focus, a real uncertainty failure, an ordered prefix before preflight failure, and a genuine refused TCP connection interrupted by SIGTERM after a submission trace. Mathematical cache/price fixtures exercise ledger transformation without introducing an HTTP replacement.

Fallback summaries add new logical `fallback_attempts`, `fallback_fraction` of consumed units with new fallback work, `fallback_resolved`, `fallback_failed`, unfinished `fallback_cancelled`, saved `fallback_replayed` and completed `fallback_elapsed_ms`. Model attempts/tokens/pricing remain separate. The label-only external protocol cannot report provider usage, so its cost is `fallback_cost_usd=unavailable` with `fallback_cost_scope=external`. A join unit is an arriving left record, finite dedupe counts each assessed pair, streaming dedupe counts each input decision, and selection counts each document. Multiple candidate/head attempts count separately while the fraction counts a consumed unit once. Replayed outcomes do not increase new attempts or the fraction. These fields appear only when an invocation attempts or replays fallback work. See [fallback.md](fallback.md).
