# Native document selection evaluation

Retrieval suites run through the ordinary evaluator with its fresh discovery, fixed partitions, shared budget/deadline and private reports. They use the existing CLI tournament, independent scoring, parsers and normalizer. CLI, MCP and evaluation share containing-line evidence construction.

```sh
loupe --config examples/config.toml --profile server eval \
  --suite find-pick/find --suite find-pick/pick --suite find-pick/independent \
  --suite out-of-scope/selection --suite extract/dates --suite extract/money \
  --suite extract/semver --suite extract/emails --suite extract/hostnames \
  --suite extract/tickers --allow-unverified --strict --no-cache --json
```

The ten seed suites contain 44 fictional examples and 22 fixed test cases. Labels are agent drafts with `reviewed=false`, MIT provenance and explicit source metadata. Calibration/test variants use different facts and wording. These are small diagnostic seeds, not target-size or human-ratified production suites. [The label review sheet](selection-seed-review.md) records each proposed answer. Actual results include an email false positive: the model selected an address explicitly marked Sales for a billing query. The report retains this error.

```toml
task = "retrieval"
preset = "selection-query"

[selection]
mode = "extract" # find, pick, independent or extract
take = 1
max_rounds = 64

[selection.normalize]
tz = "UTC"
now = "2026-10-06T00:00:00Z"
```

Missing `[selection]` means find, take one and at most 64 rounds. Independent mode rejects explicit `max_rounds`; it uses one gate/Score request per candidate. Context-line and keep-blank settings honor the whole-document input defaults, with explicit selection context overriding the default. Extraction requires each case's `candidates` kind and rejects keep-blank. Only extraction accepts normalization settings; an empty normalization table enables verbatim-preserving kinds. Unsupported kinds/settings fail before credentials or split writes.

`selection-query` uses a nonempty case `query` literally. Named trusted Noul query presets can interpolate case query/candidates arguments; pick and independent also accept Score relevance rubrics. Template/permissions rules apply. Context and companions affect encoded requests. Classification calibration pins, weighted factor formulas, structured/projection input and classification packing/split/combine/focus/truncation policies are inapplicable. Classification fitting, baseline checks, comparisons/sweeps and ensembles are explicit exit 64 for these suites. Fallback commands cannot execute during evaluation.

Labels are unique candidate ID lists; empty means absent. A case must provide a string unit or pinned document fixture. Physical line IDs remain `L<n>` and parsed spans `C<n>`. Missing gold IDs, malformed span labels and normalization expectations without enabled normalization are input errors before network calls. Case IDs, labels, expected spans, source/review flags and notes never enter requests.

An optional `expected_spans` array records gold ID, verbatim `text`, UTF-8 `byte_start`/`byte_end`, physical `line_start`/`line_end`, and optional expected normalized **value**. It applies only to retrieval tasks. Selected results preserve original text/location and the full shared normalization object, including display text and optional precision. Byte ranges address the original document, not the JSONL dataset's escaped string. Expected strings must match valid original UTF-8 ranges. Offset/normalization metrics compare labeled selected spans; missing/wrong selections remain visible in retrieval metrics, and zero labeled selected support is explicitly undefined.

The report adds optional `suites[].selection`, preserving the ordinary run identity, manifests, splits, usage and phase envelope. Classification reports omit this field. Selection reports include:

| Field | Meaning |
| --- | --- |
| `cases` | Original documents, corpus/review/source position, effective rubric/template/order/policy, complete candidate catalog, parser/normalization identity, expected IDs/spans, first plan, actual raw ordered requests/responses/decisions, final outcome and selected spans. |
| `retrieval` | Completed-case set exact match, micro precision, recall and F1, with explicit denominators/undefined reasons. Empty output can match an absent label even when uncertain; consult certainty and absence metrics separately. |
| `presence_accuracy`, `absence_accuracy` | Confirmed nonempty/empty output on present/absent cases. Uncertainty cannot pass either confirmation. |
| `coverage` | Fraction of completed documents with a certain final outcome; independent mode retains uncertainty in candidate decisions. |
| `offset_accuracy`, `normalization_accuracy` | Exact original text/coordinates and normalized value on selected, explicitly labeled gold spans. These are conditional transformation checks, not retrieval recall. |
| `bracket_presence_quality` | Noul accuracy/F1/AUROC/Brier/ECE/coverage/selective accuracy over actual bracket requests, grouped by threshold/margin policy. Gold is whether any labeled ID appears in that request's listed options, including later rounds. This does not invent a global document probability or a probability distribution over multiple correct Choice answers. Support counts bracket/candidate requests, which are correlated within each document. |

Choice selections retain final-bracket marginal probability; independent selections retain `ranking_score`. Score means are not presented as calibrated probabilities. Raw API answers remain unchanged. Different policies have separate bracket calibration summaries.

Every uncached warmup pass and latency pass executes complete document pipelines, including new candidate planning, all conditional tournament rounds or independent requests, and normalization. Timing samples/throughput count completed documents; current selection execution is sequential at the client. Shared server load is uncontrolled. One budget covers accuracy, all warmups and latency; retries remain separately charged and unknown admitted attempts remain spent. Fatal errors, budget exhaustion and signals preserve incomplete reports with raw completed responses. Normalization failures retain the selected source span/error and make the run incomplete.

`tools/selection_eval_probe.py` checks ten suites against an explicitly configured live endpoint, including uncached warmup/latency, fresh-identity refusal, one-attempt budget failure, forced uncertainty, admitted SIGTERM and direct CLI request/offset/normalization equality for every mode/kind. It also checks MCP classify/ask/find/pick/extract. Reports default to the ignored `.private/qualification/evidence/` directory. Pure transformations replay a captured email false positive. Authoritative identity/cache, human label verification, target-size corpora and selection-specific acceptance/comparison/calibration remain open.
