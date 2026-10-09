# Companion signals and `--why`

Presets and question files can declare additional Noul questions with `[[signals]]`. These questions are always sent alongside the primary question. `--why` changes display only, so adding or removing that flag leaves the encoded request unchanged. The model decides all fields jointly; declaring signals can change primary answers and must be included in evaluation/calibration identity.

```toml
type = "choice"
instructions = "Classify the current service condition."

[criteria]
page = "An unresolved outage prevents customers checking out."
noise = "No current outage, or the service recovered."

[[signals]]
id = "impact"
instructions = "Does the state explicitly say customers cannot use checkout or pay?"
[signals.policy]
threshold = 0.7
margin = 0.05

[[signals]]
id = "retried"
instructions = "Does the state explicitly say a retry or recovery succeeded?"
```

```sh
loupe tag -f incident.toml --why < events.log
loupe tag -f incident.toml --why --json --pack window:2 < events.log
```

Each signal needs a unique ID matching `[a-z][a-z0-9_-]*`, at most 64 bytes. `type = "noul"` is optional; other types are invalid. `instructions` accepts the ordinary question value shapes and has `question` as an alias. Missing instructions fall back to the encoded field ID. Optional true/false `criteria` merge with normal Noul descriptions. Signal policy accepts only threshold and margin, defaulting to 0.5 and 0. Unknown fields, malformed criteria, invalid policies, duplicate IDs and collisions with primary questions are exit 64. Argument/context/env/file templates use the existing preset trust and permissions rules.

Signal fields use the reserved names `signal:<id>`. Primary fields retain their order; signals follow declaration order. Static/CLI reference context applies to every field. Window fields bind to original source IDs, such as `L2/signal:impact` or `R3/signal:retried`, independently of other units in that window. All field IDs, instructions, criteria, context and order contribute to prompt counts and request cache identity. Signals share the backbone pass but consume schema tokens, question slots and options. Limits and hard attempt budgets include them even without `--why`.

Signal decisions are separate local assessments of their returned true probability. Confidence is `max(p, 1-p)`. Equality with threshold is yes; strict distance below margin is uncertain. CLI primary threshold/confidence flags do not override signal policy. These assessments describe signals and do not veto, route or otherwise override the primary policy. The model's joint attention can still change primary probabilities.

With `--why`, stderr includes signal probabilities and yes/no/uncertain states for each evaluated primary unit, including omitted decisions. Sort diagnostics follow evaluation/input order while stdout follows score order. Selection diagnostics identify document, round and request, including before explicit uncertainty failure. `-q` suppresses these diagnostics. Plain, annotated and TSV stdout keep their established bytes/columns.

JSON per-unit output adds a `signals` object. Each logical signal ID maps to:

| Field | Meaning |
| --- | --- |
| `field` | Actual API question ID, including a window source prefix when present. |
| `answer` | Original Noul API answer, or the existing derived answer with provenance for explicit combine/focus views. |
| `probability` | Returned true probability. |
| `assessment` | Local selected boolean, confidence, uncertainty and nullable gap. |
| `threshold`, `margin` | Effective signal policy. |

The primary `answer` remains unchanged. Without `--why`, normal per-unit JSON retains its previous shape even though the complete request contains signals. Selection JSON uses signals from the response supplying that candidate's final answer. Independent ranking uses the candidate's own request; earlier brackets are retained in full history rather than being merged into a global signal probability. Quiet JSON still includes requested signal metadata.

`--why` is valid for `is`, `keep`, `drop`, `tag`, `rate`, `sort`, `route`, `find`, `pick`, `extract` and `featurize`, and requires at least one declared signal. It is exit 64 for `ask`, `tokens` and aggregate `count`. Those commands still encode declared signals: `ask` returns them in its exact API response object; `tokens` exposes all questions and their counts. Eval/calibrate use their own grammar and retain signals automatically without this display flag.

For explicit splitting, combine modes must cover the complete question set and be valid for each field. A single Choice vote reducer cannot silently reduce Noul signals. Use the existing per-field mapping, for example `{"severity":"vote","signal:impact":"any","signal:retried":"any"}` with the actual primary field name. Each signal reducer has its documented part/document meaning. Focus final requests re-ask signals over selected evidence and retain view provenance.

Evaluation and packing sweeps encode signals and record all raw answers, actual bindings, policies and complete question order. Scalar truth labels apply to a single primary field. Mixed label maps must cover every primary field and may additionally label declared signals. Unlabeled signals stay outside accuracy metrics and paired-field counts; labeled signals receive their own metrics. Primary field compatibility/provenance checks and complete request/answer/policy/binding checks remain mandatory. Different signal sets may be explicitly compared over the same primary labels to measure their effect. Calibration currently fits primary rubric fields while retaining all signal definitions/order in preset/view identity; signal policies remain explicit configuration.

Fresh identity is required for cache access. Raw full responses can be reused across display/policy changes only when their complete encoded request and model/encoder identity match. General and route journals bind `--why` when enabled because it changes delivered JSON. Source/input/plan checks precede replay. Saved complete responses can reproduce explanations without new inference, including explicit uncertainty failure. Generic stdout/stderr/capture acknowledgement gaps remain documented in the [failure contract](failure-policy.md).

Qualification uses exact encoder/cache-key tests, real files/processes and actual server classification, window binding, retrieval, independent ranking, extraction, uncertainty failure, zero budgets, journal replay and packing comparisons. Synthetic cases establish these contracts; they do not establish held-out signal calibration or retrieval accuracy. Complete live identity/cache, newly packaged release artifacts and target-platform qualification remain separate.
