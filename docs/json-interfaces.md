# JSON interfaces and provenance

This map identifies the JSON entry points and anonymous objects emitted by the CLI and MCP interfaces. The [generated type reference](json-types.md) supplies the exact fields, value types, enum layouts, null/omission rules and Serde attributes for all 210 locally serialized declarations, including private reports and recovery frames. The [CLI reference](cli-reference.md) defines format applicability and exit precedence. These remain v0.1 development contracts until the comprehensive review and 1.0 freeze are complete.

JSONL means one complete JSON object plus LF per record. A single-object interface also ends with LF. Object insertion order is retained by current serializers, but consumers should address named keys. Question insertion order is semantic and is recorded separately in request plans and recovery data; Choice option order is sorted by key. Numeric precision and raw answer extension fields are retained. An absent optional key differs from a present null; the type reference lists the actual skip predicates. Error messages are diagnostics, not identifiers to parse.

## Common API and decision rows

| Interface | Root and conditions |
| --- | --- |
| Raw `ask`, MCP `ask` | [Response](json-types.md#loupe-core-wire-response): exactly `model`, `answers`, `usage`. Each answer retains the original API field object, including backend extensions. Unknown top-level response/usage extensions are not re-emitted by these typed serializers. |
| Sent request | [Request](json-types.md#loupe-core-wire-request): `state`, `model`, ordered nonempty `questions`. `state` is arbitrary JSON for Clef; Jev imposes its backend-specific schema. [Question](json-types.md#loupe-core-wire-question) gives all three question variants and optional instruction/criteria layouts. |
| `is`, `keep`, `drop`, `tag`, `rate`, `sort`, route output files in JSON mode | JSONL decision rows defined below. Filtering controls whether a row is emitted; it does not replace the raw answer with a policy decision. |
| `count --json` | `{counts: object<string, unsigned integer>}`; adds top-level `errors: unsigned integer` only when retained unit failures occurred. Noul keys are `yes`, `no`, `uncertain`; Choice uses declared labels and uncertainty. It emits one aggregate rather than JSONL unit rows. |
| Kept per-unit failure | `index`, `unit`, `answer:null`, `model:null`, `cached:false`, `error:{code:integer,message:string}`. This is a complete-unit recovery policy; reader/auth/budget/deadline/signal failures do not become kept decision rows. |

Shared decision rows are emitted by [the production writer](../crates/loupe-cli/src/run.rs#L2938):

| Key | Value / presence |
| --- | --- |
| `index` | Positive global physical unit index, preserving skipped-unit gaps. |
| `unit` | Original input subject, as JSON. It can differ from request state when IDs/projections/context are configured. |
| `answer` | Raw API head for ordinary input. Explicit splitting emits the marked part aggregate and retains original responses in provenance; focus/truncation annotate the actual final head with view metadata. |
| `model` | Backend response model string. A legacy alias is usable for inference but is not an authoritative cache identity. |
| `cached` | Boolean describing this result's response cache status. |
| `ensemble` | Present for a primary ensemble; [ensemble Report](json-types.md#loupe-core-ensemble-report) retains actual members, mean distribution, votes, agreement and assessment. |
| `decision` | Present for an ensemble/external/human override. Object `{selected,source,confidence,uncertain}`; `selected` follows [Selected](json-types.md#loupe-core-policy-selected). `source` is `ensemble`, `fallback` or `review`; external/human confidence is null. The precedence follows actual policy processing, without overwriting `answer`. |
| `fallback` | Present when an external fallback ran; [Execution](json-types.md#loupe-core-fallback-execution) includes success/failure, process outcome, timing, actual typed decision and diagnostics. |
| `review` | Present when a review task exists. The task/decision/sampling payload follows [CLI review Decision](json-types.md#loupe-run-review-decision) and the review queue definitions below. Human corrections can replace the separate `decision`. |
| `signals` | Present with `--why`; object mapping declared logical signal IDs to [signals Report](json-types.md#loupe-core-signals-report). Each has its own actual binding, raw answer, probability and policy assessment. |

Raw answers use `type:"noul"` with numeric `noul`; `type:"choice"` with `choice`, numeric `confidence` and a string-keyed probability map; or `type:"score"` with numeric `score`, `confidence`, `legend` and probabilities. The validator checks head type, requested option keys, probabilities in `[0,1]`, a rounding-aware sum, valid Choice labels, Score range and matching legend keys. All requested question IDs must be answered exactly once. Confidence used for policy is recomputed from probabilities. [Source](../crates/loupe-core/src/wire.rs#L155), [uncertainty policy](selection-policy.md).

## Planning and explicit input views

Ordinary `tokens` and `--dry-run` rows contain `index`, [request](json-types.md#loupe-core-wire-request), [counts](json-types.md#loupe-core-encoder-counts), and `encoder` (identity string or null). Window plans additionally include `indices`; feature plans include ordered string array `feature_columns`. A requested ensemble adds `{method:"equal_mean_probabilities",independent:false,min_agreement:number}`. Route plans add `{out:path|null,labels:object,process_labels:string[],resume:boolean,reset:boolean}`. Active views add `split`, `truncate` or `focus`. Plans contain actual schema/state inputs; no model result is fabricated. [Source](../crates/loupe-cli/src/run.rs#L1280).

| View/provenance | Shape and meaning |
| --- | --- |
| Split plan | `{operation:"split",path:string,coordinate_space:string,combine:object,weight:string,source:Location,chunks:Chunk[]}`. [Chunk](json-types.md#loupe-core-split-chunk) includes original byte/token ranges and actual request/counts. `combine` maps fields to the declared combine mode. |
| Combined raw answer | Adds `derived:true` and `provenance:{operation:"split",path,coordinate_space,combine,weight,source,ranges,responses}`. Here `combine` is this head's selected mode, `ranges` contains original chunk ranges and `responses` contains actual complete API responses. Aggregated probabilities answer the declared part-combination question. |
| Truncation | [truncate View](json-types.md#loupe-core-truncate-view), with `source:Location` added by the CLI. Each raw answer adds `lossy:true` and this view provenance. A field path scopes the operation to that string, rather than all structured input. |
| Focus plan | [focus Plan](json-types.md#loupe-core-focus-plan). Relevance requests/counts address original chunks; later evidence selection is conditional on actual relevance responses. |
| Focused raw answer | Adds `focused:true`, `lossy:boolean`, `provenance:{operation:"focus",path,coordinate_space,k,selected_chunks,ranges,separator,source,original_counts,relevance,final}`. `relevance` is an array of `{range,counts,response}` and `final` contains the actual `{request,counts,response}`. |
| Window admission | `{operation:"window",bindings:[{index:integer,source_id:string,questions:object<string,string>}...]}`. Bindings map logical fields to actual source-prefixed prompt IDs. Raw per-unit rows still use the shared layout. |

[Location](json-types.md#loupe-core-input-location) always names a nullable source path, zero-based exclusive byte bounds and 1-based inclusive physical line bounds. Delimiters and view/candidate spans have their documented scopes; they are not interchangeable. Core [Unit](json-types.md#loupe-core-input-unit) also contains raw bytes, parsed/context state and optional header for durable recovery. See [input views](usage-guide.md#read-paragraphs-records-or-entire-files), [features](features.md), [recovery](recovery.md).

## Selection and extraction

Find/pick/extract JSONL rows contain `index`, `unit`, raw `answer`, `model`, `cached`, then `id`, `location`, `probability_scope`, `uncertain`, `restored`, `provenance`. Here `index` is the original physical start line within its document; it restarts for each document and can repeat for several extracted spans on one line. `provenance` is `{document_index:integer,outcome:object,history:History}`. The [actual History](json-types.md#loupe-run-selection-history) preserves every ordered plan, actual evaluation and completed external decision; [tournament Outcome](json-types.md#loupe-core-selection-outcome) and [independent Outcome](json-types.md#loupe-core-independent-outcome) describe their different populations. [Source](../crates/loupe-cli/src/run/selection.rs#L1147).

| Conditional key | Value / condition |
| --- | --- |
| `probability` | Choice marginal, present in tournament modes; null for an external winner. It is conditional on the final/bracket population, including `none`. |
| `model_probability` | Present for an external Choice winner; preserves its original model marginal. |
| `ensemble_probability` | Present for an external Choice winner with ensemble mode; preserves the derived mean marginal. |
| `probability_source` | Present in ensemble tournament mode: `ensemble` or `fallback`. |
| `ranking_score`, `score_calculation` | Present instead of `probability` for independent selection. Calculation is `normalized_weighted_mean_of_returned_level_probabilities`, `equal_mean_level_probabilities` or `external_score`. This is a Score expectation, not a marginal probability. |
| `ensembles` | Present with aligned query variants; object mapping actual presence/Choice/relevance heads to ensemble reports. |
| `selection_decisions` | Present for ensemble/fallback decisions; head-to-override map with the shared decision layout. External results take precedence for heads they resolved. |
| `fallback` | Present if external work ran for the selected request; actual field-to-Execution map. Each external Choice bracket retains one winner. |
| `candidate_type` | Present for extraction: [extract Kind](json-types.md#loupe-core-extract-kind). |
| `normalized` | Present when extraction normalization is requested: [Normalized](json-types.md#loupe-core-normalize-normalized). Its original `text` is retained; `value` follows the date/money/semver domain described in [extraction](extract.md). |
| `signals` | Present with `--why`, using actual companion-head reports. |

Tournament dry runs are `{kind:"selection-plan",document_index,source,catalog,round,future_rounds}`; independent dry runs use `kind:"independent-selection-plan"` and `plan` instead of `round`/`future_rounds`. Catalog entries are [Candidate](json-types.md#loupe-core-candidates-candidate), and request/count/policy/bracket fields are defined by [Round](json-types.md#loupe-core-tournament-round) or [independent Plan](json-types.md#loupe-core-independent-plan). `future_rounds` explicitly says `none` or describes conditional future inference. [Find](find.md), [pick](pick.md), [extraction](extract.md) define source IDs, boundaries and absence behavior.

## Features, factor explanations, matching and actions

| Interface | Required JSON fields and conditional additions |
| --- | --- |
| `featurize` | `{index,row_id,unit,features,answers,bindings,model,cached,location}`. `features` maps ordered generated columns to numbers, `answers` retains logical raw heads, and `bindings` maps logical to actual prompt IDs. Adds `ensembles` when nonempty, `signals` with `--why`, `indices` in windows and active `split`/`truncate`/`focus` views. Failure rows have null features per column, null answers/bindings/model, `cached:false`, and `error:{code,message}`. [Source](../crates/loupe-cli/src/run/features.rs#L124), [features](features.md). |
| `explain` | `{index,unit,answer,model,cached,composite,request,counts,response,location}`. `composite` is [factors Report](json-types.md#loupe-core-factors-report), with every factor's raw head, assessment, weight and contribution. Adds primary `ensemble`, `signals`, `indices` and active views when present. [Source](../crates/loupe-cli/src/run/explain.rs#L45), [factor contract](explain.md). |
| Join row | `{index,kind,left,matches,evaluated_pairs,cardinality,blocking}`. `kind` is `matched` or `unmatched_left`; `left` is `{unit,location}`. `blocking` has integer `eligible_records`/`unavailable_records`. Each match is `{right:{index,unit,location},ranking_score,decision,model,cached}`. Each evaluated pair is `{id,blocking,request,question_order,counts,response,cached,decision}`. Candidate/decision fields use [blocking Candidate](json-types.md#loupe-core-blocking-candidate) and [matching Decision](json-types.md#loupe-core-matching-decision). Pairs/matches add nonempty `fallback` maps of `{execution,decision}` by head. [Source](../crates/loupe-cli/src/run/matching.rs#L866), [join](join.md). |
| Join unmatched right / error | Unmatched right is `{kind:"unmatched_right",left:null,right:{index,unit,location}}`. Kept error is `{index,kind:"error",left:{unit,location},matches:[],evaluated_pairs:[],error:{code,message}}`. It does not pretend a failed candidate evaluation was a nonmatch. |
| Join dry run | `{index,left:{unit,location},fields:string[],blocking_mode,eligible_records,unavailable_records,candidates,right_index}`. Candidates are `{blocking,right:{index,unit,location},pair_plan,question_order}` with [matching Plan](json-types.md#loupe-core-matching-plan). Right-index statistics use [blocking Stats](json-types.md#loupe-core-blocking-stats). |
| Finite dedupe | `{index,unit,cluster,keep,members,links,location}`. Members are `{index,unit,location}`; links use [Link](json-types.md#loupe-run-dedupe-link) with actual raw answers, assessments, cache status, tokens, signals and optional ensemble/external decision provenance. [Source](../crates/loupe-cli/src/run/dedupe.rs#L793), [dedupe](dedupe.md). Pair dry runs contain `pair`, `ordinal`, `left`, `right`, `request`, `counts`, `question_order`. |
| Streaming dedupe | `{index,unit,mode:"streaming",keep:"first",candidate_scope:"retained_lsh",candidates,expired,pairs,history,location}`. Pair evidence has `representative`, `request`, `counts`, `response`, `cached`, raw `assessment`, `signals`, and optional `decision`, `fallback`, `ensemble`. History uses [streaming Stats](json-types.md#loupe-core-dedupe-streaming-stats). Dry run instead uses `mode:"streaming-dry-run"`, `population:"all_prior_units_assumed_representatives"`, and actual planned pair/head/count fields without responses. [Source](../crates/loupe-cli/src/run/dedupe/streaming.rs#L427), [streaming semantics](streaming-dedupe.md). |
| `on` | `{index,unit,answer,model,cached,label,assessment,actions,signals,request,counts,response,location}`. `assessment` remains the raw model assessment. Actions are `{id,suppressed:true}`, `{id,kind:"append",success:true}`, `{id,kind:"exec",result:trigger::Outcome}`, or `{id,kind,error:string}`. Adds separate `ensemble`/`decision`, `fallback` and `review` when used. [Source](../crates/loupe-cli/src/run/trigger.rs#L431), [actions](on.md), [durable claims](on-state.md). |
| `on --dry-run` | `{index,unit,request,counts,question_order,actions}`, adding the ordinary ensemble plan annotation when requested. Actions use [trigger Action](json-types.md#loupe-core-trigger-action). No processes or append files are created. |
| `review FILE` corrections | `{index,unit,answer,model,cached:false,decision:{selected,source:"review",confidence:null,uncertain:false},review:{task,run,sampling},location}`. Raw head remains unchanged; selected is the actual human label. [Source](../crates/loupe-cli/src/run/review.rs#L370), [review](review.md). |
| Hook install | `{directory:path,installed:string[],unchanged:string[]}`. Hook dry run is `{hook,request,counts,budget,deadline_seconds,fail_open,presets:[{id,name,content_hash}...]}`. Normal hook runs use exit codes/diagnostics rather than JSON decision rows. [Source](../crates/loupe-cli/src/run/hooks.rs#L63), [hooks](hooks.md). |

## Evaluation, calibration and private publication bundles

These interfaces serialize concrete report types, whose complete field graph is in the generated reference:

| JSON entry point | Definition and arbitrary-JSON semantics |
| --- | --- |
| Normal `eval --json` / report.json | [RunReport](json-types.md#loupe-run-eval-runreport), [SuiteReport](json-types.md#loupe-run-eval-suitereport), raw [Record](json-types.md#loupe-run-eval-record), [Phase](json-types.md#loupe-run-eval-phase). `environment` records declared execution/timing context; `server` is actual discovery; phase `groups` retain each operation's real plan/request/evaluation, not invented independent observations. Packing/provenance fields use the view/pair/selection layouts above. |
| Join / selection suite stages | Optional [join Report](json-types.md#loupe-run-eval-join-report) or [selection Report](json-types.md#loupe-run-eval-selection-report). Their pair/request/offset/normalization metadata is fully typed, and nested raw API/evidence objects use the contracts above. Undefined metrics remain null and cannot satisfy a gate. |
| Acceptance extension | Optional [gates Report](json-types.md#loupe-run-eval-gates-report): planned/passed/failed/incomplete status, human-ratified bounds, measured values, positive support and reasons. It is omitted when no baseline is requested. |
| View sweeps | [variants Report](json-types.md#loupe-run-eval-variants-report) and [Variant](json-types.md#loupe-run-eval-variants-variant), each retaining an independent concrete view report under a shared admitted budget/deadline. |
| Comparisons | Saved/live [compare Report](json-types.md#loupe-run-eval-variants-compare-report) entries in the [variants Report](json-types.md#loupe-run-eval-variants-report) `comparisons` array. Identities, policy/request bindings, paired cases and raw shifts stay separate. |
| Scalar / view calibration | [calibrate Report](json-types.md#loupe-run-eval-calibrate-report) or [calibrate views Report](json-types.md#loupe-run-eval-calibrate-views-report), actual [Fit](json-types.md#loupe-core-calibration-fit) and [calibration Pin](json-types.md#loupe-core-calibration-pin-pin). Written annotation/source objects carry fixed suite revision/split/case IDs, targets, minimum support and fitted values; optional view selection records its method, winner and candidate throughput/fit support. |
| Publication manifest | `{schema_version:1,generated_at:string,files:{"report.json":{bytes:integer,blake3:string},"report.md":{bytes:integer,blake3:string}}}`. Both files are staged, synced and published together; the reader verifies these hashes before trusting saved reports. [Source](../crates/loupe-core/src/report_store.rs#L191). |

[Evaluation](eval.md), [join evaluation](join-evaluation.md), [selection evaluation](selection-evaluation.md), [calibration](calibration.md) and [baselines](baselines.md) specify split leakage restrictions, identity requirements, metric formulas, support, uncertainty and failure precedence. An unverified identity is explicit in the report, never an implicit cache/calibration qualification. Pricing/cost and raw usage types distinguish estimates, actual reported values and unknown attempts.

## Administrative outputs and MCP

| Interface | JSON layout |
| --- | --- |
| `doctor --json` | `{profile:string,endpoint:string,discovery:Discovery,tokenizer:object,cache_eligible:boolean}`. Tokenizer is `{ready:true,identity:string}` for loaded exact encoding, `{ready:true,exact:false}` for estimates, or `{ready:false,error:string}`. [Discovery](json-types.md#loupe-core-backend-discovery) retains optional raw health/models, negotiated limits, unavailable/unsupported endpoint lists and nullable authoritative identity. [Source](../crates/loupe-cli/src/run.rs#L379). |
| `models` | Raw actual `/v1/models` JSON. It has the provider's extension schema; Loupe does not invent a supported model list when the endpoint is absent. Unsupported is exit 69. |
| `presets list --json`, `presets show NAME` | Array of [Preset](json-types.md#loupe-core-preset-store-preset), or one Preset, respectively. Origin, trust/content hash and raw effective document remain explicit. |
| `presets edit`, `presets trust` | Edit emits `{path,trusted,content_hash}`; trust emits JSONL `{name,trusted:true,content_hash}` for granted presets. `presets new` emits a path line rather than a JSON object. |
| `presets lint --json` | `{name,origin,trusted,valid,policy_identity}`, with `error:string` on failure. When live examples run, adds `examples_passed:boolean` and `examples:[{unit,expect,passed,uncertain,response}...]`. `policy_identity` starts as `unverified`; successful effective-policy validation changes it to `verified` or `not_required`. [Source](../crates/loupe-cli/src/preset_commands.rs#L199). |
| `cache stats --json` | [cache Stats](json-types.md#loupe-core-cache-stats). Prune/clear produce their documented text diagnostics rather than a JSON stats object. |
| MCP `tools/list` | Exact versioned [tool schemas/annotations](../crates/loupe-cli/tests/compatibility/v0.1/mcp-tools.json). The SDK provides the negotiated MCP/JSON-RPC envelopes, IDs, cancellation and errors; results contain matching text JSON and structured content. [Protocol](mcp.md). |
| MCP `classify` | `{response,decisions,ensembles,cached,counts,usage,warnings}`. Decisions are primary-field [Assessment](json-types.md#loupe-core-policy-assessment) values; ensembles retain named variants. `usage` is the per-call [budget Usage](json-types.md#loupe-core-budget-usage), distinct from raw API usage. |
| MCP `tokens`, `presets` | Tokens emits `{request,counts}` without inference. Presets emits `{presets:[{name,content_hash,document}...]}`, restricted to explicitly allowlisted trusted snapshots. |
| MCP `find`, `pick`, `extract` | `{selected,outcome,provenance,usage,warning}`. Selected entries have `{id,unit,location,probability}` and optional `normalized`; history entries have `{round,bracket,request,counts,evaluation}`. This is a finite tool-call object rather than CLI JSONL. [Source](../crates/loupe-core/src/mcp.rs#L438). |
| MCP tool error | SDK structured tool error content with `{error:string,exit_code:integer}`. Operation cancellation also follows this tool-error path; malformed protocol messages/methods use SDK JSON-RPC errors. No route, exec, queue capture or arbitrary file-write tools are exposed. |

## Durable provenance artifacts

These files are private execution data, not stdout transports. Their version/checksum/identity validation and fsync/recovery rules are part of the interface; editing nested payloads by hand does not create a valid resumable run.

| Artifact | Definition / nested payload |
| --- | --- |
| Generic run journal | [Frame](json-types.md#loupe-core-recovery-frame) wraps version, sequence, previous checksum, tagged [Record](json-types.md#loupe-core-recovery-record) and checksum. `ready`/`ready_many` payloads use the concrete operation snapshots below. Admission stores actual Unit bytes/state/header/location. |
| Ordinary decision completion | [CLI recovery Snapshot](json-types.md#loupe-run-recovery-snapshot): original request/head order/counts/evaluation/answer binding and optional split/truncate/focus/window data. External and human completion records are separately synced. |
| Selection / pair completion | [History](json-types.md#loupe-run-selection-history), [matching journal Snapshot](json-types.md#loupe-run-matching-journal-snapshot), or [streaming Completion](json-types.md#loupe-run-dedupe-streaming-recovery-completion). These bind actual requests, raw responses, partial completed work and external head decisions before replay. |
| Durable streaming index | [streaming Snapshot](json-types.md#loupe-core-dedupe-streaming-snapshot) and [Progress](json-types.md#loupe-run-dedupe-streaming-recovery-progress) retain the exact bounded representative population, expiry and candidate history. |
| Route manifest | [route_log Envelope](json-types.md#loupe-core-route-log-envelope) wraps the tagged [route_log Record](json-types.md#loupe-core-route-log-record). Output transactions store pre/post offsets, checksums and input/run identity; recoverable resets use [route_reset Envelope](json-types.md#loupe-core-route-reset-envelope). Route process destinations have a different recovery boundary. [Protocol](route-recovery.md). |
| `on --state` | A private Redb database, not a JSON frame file. It stores canonical JSON run/action identity alongside version/window/capacity/clock metadata and durable hashed claims/expiry indices. A content/action claim is synced before execution; it expires only under an explicitly configured dedupe window. Unknown action effects remain at-most-once claims. Action outcomes use the `on` stdout layout above. [Protocol](on-state.md), [source](../crates/loupe-core/src/trigger_state.rs#L29). |
| Review queue / captured labels | [review Frame](json-types.md#loupe-core-review-frame), tagged [review Record](json-types.md#loupe-core-review-record), [Task](json-types.md#loupe-core-review-task), and [suite Case](json-types.md#loupe-core-suites-case). Sampling metadata, original request/response, source bounds, run/task IDs and human label are retained. Review captures belong to captured/dev/calibration, never held-out test. |
| Cache entries / trust grants | [cache Entry](json-types.md#loupe-core-cache-entry), discovered [identity Discovery](json-types.md#loupe-core-identity-discovery), [Grants](json-types.md#loupe-core-preset-store-grants). Complete rendered-request/encoder/current model identities and content hashes remain authoritative; serialized aliases do not authorize cache reuse. |

Generic stdout has a write/acknowledgement crash gap; transactional route files and durable action claims have their own stronger/different boundaries. See [recovery](recovery.md), [route recovery](route-recovery.md), [on state](on-state.md), [review](review.md) and [server identity](server-identity.md). Those guarantees cannot be inferred from JSON parsing alone.

## Regeneration and review

The type reference comes from [Rustdoc's JSON AST](https://doc.rust-lang.org/nightly/rustdoc/unstable-features.html#-w--output-format-output-format), using the repository's existing pinned nightly toolchain. No schema dependency is added:

```sh
cargo rustdoc --locked --offline -p loupe-core --all-features --lib -- \
  --document-private-items -Z unstable-options --output-format json
cargo rustdoc --locked --offline -p loupe-cli --all-features --bin loupe -- \
  --document-private-items -Z unstable-options --output-format json
uv run python tools/schema_catalog.py --check
# After intentional declaration changes, regenerate and review before the check.
uv run python tools/schema_catalog.py
```

The catalog fails on unknown Rustdoc formats, unresolved/stripped fields, unsupported type nodes and custom serializers needing wire review. The authored interface map covers anonymous objects and the semantics of arbitrary JSON fields. Compatibility snapshots and actual process/inference tests remain required; generating a field table does not validate all optional branches, backend quality, authoritative cache identity or release artifacts. Comprehensive source/fixture coverage and freeze review remain pending.
