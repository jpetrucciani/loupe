# JSON type field reference

Generated from both crates' actual all-feature Rustdoc JSON ASTs, including private items. This catalog covers every local `Serialize` implementation, with real field types and Serde attributes. It is a declaration reference, not a JSON Schema validator or a stable Rust library API. [JSON interfaces](json-interfaces.md) maps command outputs and anonymous provenance objects onto these definitions.

The current catalog contains 210 serialized declarations. Rustdoc format 61 is explicitly checked; unsupported AST/type/serialization forms fail generation rather than silently disappearing.

An `Option` without `skip_serializing_if` emits JSON null when absent. A skip predicate omits its key when true. `default` governs deserialization and does not by itself omit output. `alias` accepts an input name; it does not change the emitted name. `flatten` merges the referenced fields into the parent object. Container `tag`, `content`, `rename_all`, `transparent`, `into` and field attributes determine the declared wire layout. Arbitrary JSON fields require the command-specific semantics linked from the interface map; their content is not inferred from a sample.

Rust integer bounds remain applicable; `usize`/`isize` use the native platform width. JSON object map keys are strings, including rendered numeric indices. Filesystem paths require UTF-8 for JSON serialization. Probability validation, supported normalization domains, request/answer consistency and source bounds are enforced in the referenced implementations and documented interface contracts.

## Definitions

- [loupe::args::ReviewMode](#loupe-args-reviewmode)
- [loupe::run::dedupe::Link](#loupe-run-dedupe-link)
- [loupe::run::dedupe::streaming::recovery::Completion](#loupe-run-dedupe-streaming-recovery-completion)
- [loupe::run::dedupe::streaming::recovery::Pair](#loupe-run-dedupe-streaming-recovery-pair)
- [loupe::run::dedupe::streaming::recovery::Progress](#loupe-run-dedupe-streaming-recovery-progress)
- [loupe::run::eval::Failure](#loupe-run-eval-failure)
- [loupe::run::eval::Identity](#loupe-run-eval-identity)
- [loupe::run::eval::Phase](#loupe-run-eval-phase)
- [loupe::run::eval::PolicyData](#loupe-run-eval-policydata)
- [loupe::run::eval::QuestionReport](#loupe-run-eval-questionreport)
- [loupe::run::eval::Record](#loupe-run-eval-record)
- [loupe::run::eval::RequestRole](#loupe-run-eval-requestrole)
- [loupe::run::eval::RunReport](#loupe-run-eval-runreport)
- [loupe::run::eval::SuiteReport](#loupe-run-eval-suitereport)
- [loupe::run::eval::calibrate::Fitted](#loupe-run-eval-calibrate-fitted)
- [loupe::run::eval::calibrate::Report](#loupe-run-eval-calibrate-report)
- [loupe::run::eval::calibrate::views::Report](#loupe-run-eval-calibrate-views-report)
- [loupe::run::eval::calibrate::views::Variant](#loupe-run-eval-calibrate-views-variant)
- [loupe::run::eval::calibrate::views::Winner](#loupe-run-eval-calibrate-views-winner)
- [loupe::run::eval::ensemble::Saved](#loupe-run-eval-ensemble-saved)
- [loupe::run::eval::ensemble::Settings](#loupe-run-eval-ensemble-settings)
- [loupe::run::eval::gates::Checked](#loupe-run-eval-gates-checked)
- [loupe::run::eval::gates::Report](#loupe-run-eval-gates-report)
- [loupe::run::eval::gates::Status](#loupe-run-eval-gates-status)
- [loupe::run::eval::join::Assigned](#loupe-run-eval-join-assigned)
- [loupe::run::eval::join::AssignmentMetrics](#loupe-run-eval-join-assignmentmetrics)
- [loupe::run::eval::join::Catalog](#loupe-run-eval-join-catalog)
- [loupe::run::eval::join::Pair](#loupe-run-eval-join-pair)
- [loupe::run::eval::join::Report](#loupe-run-eval-join-report)
- [loupe::run::eval::join::Row](#loupe-run-eval-join-row)
- [loupe::run::eval::selection::CandidateData](#loupe-run-eval-selection-candidatedata)
- [loupe::run::eval::selection::CaseReport](#loupe-run-eval-selection-casereport)
- [loupe::run::eval::selection::PresenceQuality](#loupe-run-eval-selection-presencequality)
- [loupe::run::eval::selection::Report](#loupe-run-eval-selection-report)
- [loupe::run::eval::selection::Saved](#loupe-run-eval-selection-saved)
- [loupe::run::eval::selection::Selected](#loupe-run-eval-selection-selected)
- [loupe::run::eval::selection::TemplateData](#loupe-run-eval-selection-templatedata)
- [loupe::run::eval::variants::Report](#loupe-run-eval-variants-report)
- [loupe::run::eval::variants::Variant](#loupe-run-eval-variants-variant)
- [loupe::run::eval::variants::compare::Cell](#loupe-run-eval-variants-compare-cell)
- [loupe::run::eval::variants::compare::Decision](#loupe-run-eval-variants-compare-decision)
- [loupe::run::eval::variants::compare::Disagreement](#loupe-run-eval-variants-compare-disagreement)
- [loupe::run::eval::variants::compare::Field](#loupe-run-eval-variants-compare-field)
- [loupe::run::eval::variants::compare::RawPrimaryShift](#loupe-run-eval-variants-compare-rawprimaryshift)
- [loupe::run::eval::variants::compare::Report](#loupe-run-eval-variants-compare-report)
- [loupe::run::fallback::Plan](#loupe-run-fallback-plan)
- [loupe::run::matching::journal::Pair](#loupe-run-matching-journal-pair)
- [loupe::run::matching::journal::Snapshot](#loupe-run-matching-journal-snapshot)
- [loupe::run::recovery::Snapshot](#loupe-run-recovery-snapshot)
- [loupe::run::review::Decision](#loupe-run-review-decision)
- [loupe::run::selection::History](#loupe-run-selection-history)
- [loupe_core::backend::BatchCapability](#loupe-core-backend-batchcapability)
- [loupe_core::backend::Discovery](#loupe-core-backend-discovery)
- [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation)
- [loupe_core::baseline::Assessment](#loupe-core-baseline-assessment)
- [loupe_core::baseline::Bound](#loupe-core-baseline-bound)
- [loupe_core::baseline::Check](#loupe-core-baseline-check)
- [loupe_core::baseline::Constraint](#loupe-core-baseline-constraint)
- [loupe_core::baseline::Ratification](#loupe-core-baseline-ratification)
- [loupe_core::baseline::Reason](#loupe-core-baseline-reason)
- [loupe_core::baseline::Record](#loupe-core-baseline-record)
- [loupe_core::blocking::Candidate](#loupe-core-blocking-candidate)
- [loupe_core::blocking::Mode](#loupe-core-blocking-mode)
- [loupe_core::blocking::Selection](#loupe-core-blocking-selection)
- [loupe_core::blocking::Stats](#loupe-core-blocking-stats)
- [loupe_core::budget::Usage](#loupe-core-budget-usage)
- [loupe_core::cache::Entry](#loupe-core-cache-entry)
- [loupe_core::cache::Stats](#loupe-core-cache-stats)
- [loupe_core::calibration::Candidate](#loupe-core-calibration-candidate)
- [loupe_core::calibration::Fit](#loupe-core-calibration-fit)
- [loupe_core::calibration::Parameters](#loupe-core-calibration-parameters)
- [loupe_core::calibration::Support](#loupe-core-calibration-support)
- [loupe_core::calibration::Target](#loupe-core-calibration-target)
- [loupe_core::calibration_pin::Ensemble](#loupe-core-calibration-pin-ensemble)
- [loupe_core::calibration_pin::Pin](#loupe-core-calibration-pin-pin)
- [loupe_core::candidates::Candidate](#loupe-core-candidates-candidate)
- [loupe_core::candidates::Id](#loupe-core-candidates-id)
- [loupe_core::combine::Mode](#loupe-core-combine-mode)
- [loupe_core::config::BackendKind](#loupe-core-config-backendkind)
- [loupe_core::config::Limits](#loupe-core-config-limits)
- [loupe_core::cost::Pricing](#loupe-core-cost-pricing)
- [loupe_core::dedupe::Cluster](#loupe-core-dedupe-cluster)
- [loupe_core::dedupe::Keep](#loupe-core-dedupe-keep)
- [loupe_core::dedupe::streaming::Candidates](#loupe-core-dedupe-streaming-candidates)
- [loupe_core::dedupe::streaming::Horizon](#loupe-core-dedupe-streaming-horizon)
- [loupe_core::dedupe::streaming::SavedEntry](#loupe-core-dedupe-streaming-savedentry)
- [loupe_core::dedupe::streaming::Snapshot](#loupe-core-dedupe-streaming-snapshot)
- [loupe_core::dedupe::streaming::Stats](#loupe-core-dedupe-streaming-stats)
- [loupe_core::encoder::Counts](#loupe-core-encoder-counts)
- [loupe_core::encoder::EncodedQuestion](#loupe-core-encoder-encodedquestion)
- [loupe_core::encoder::EncodedRecord](#loupe-core-encoder-encodedrecord)
- [loupe_core::ensemble::Member](#loupe-core-ensemble-member)
- [loupe_core::ensemble::Report](#loupe-core-ensemble-report)
- [loupe_core::ensemble::Vote](#loupe-core-ensemble-vote)
- [loupe_core::extract::Kind](#loupe-core-extract-kind)
- [loupe_core::factors::Contribution](#loupe-core-factors-contribution)
- [loupe_core::factors::Report](#loupe-core-factors-report)
- [loupe_core::fallback::Execution](#loupe-core-fallback-execution)
- [loupe_core::fallback::FailureKind](#loupe-core-fallback-failurekind)
- [loupe_core::fallback::FailurePolicy](#loupe-core-fallback-failurepolicy)
- [loupe_core::features::Values](#loupe-core-features-values)
- [loupe_core::focus::Plan](#loupe-core-focus-plan)
- [loupe_core::focus::Range](#loupe-core-focus-range)
- [loupe_core::focus::Selection](#loupe-core-focus-selection)
- [loupe_core::identity::Discovery](#loupe-core-identity-discovery)
- [loupe_core::identity::Model](#loupe-core-identity-model)
- [loupe_core::independent::Decision](#loupe-core-independent-decision)
- [loupe_core::independent::Item](#loupe-core-independent-item)
- [loupe_core::independent::Outcome](#loupe-core-independent-outcome)
- [loupe_core::independent::Plan](#loupe-core-independent-plan)
- [loupe_core::independent::Selected](#loupe-core-independent-selected)
- [loupe_core::input::Location](#loupe-core-input-location)
- [loupe_core::input::Unit](#loupe-core-input-unit)
- [loupe_core::matching::Cardinality](#loupe-core-matching-cardinality)
- [loupe_core::matching::Decision](#loupe-core-matching-decision)
- [loupe_core::matching::Plan](#loupe-core-matching-plan)
- [loupe_core::normalize::DateOrder](#loupe-core-normalize-dateorder)
- [loupe_core::normalize::Normalized](#loupe-core-normalize-normalized)
- [loupe_core::normalize::Options](#loupe-core-normalize-options)
- [loupe_core::normalize::Separator](#loupe-core-normalize-separator)
- [loupe_core::policy::Assessment](#loupe-core-policy-assessment)
- [loupe_core::policy::Destination](#loupe-core-policy-destination)
- [loupe_core::policy::Probability](#loupe-core-policy-probability)
- [loupe_core::policy::Selected](#loupe-core-policy-selected)
- [loupe_core::preset_store::Grants](#loupe-core-preset-store-grants)
- [loupe_core::preset_store::Origin](#loupe-core-preset-store-origin)
- [loupe_core::preset_store::Preset](#loupe-core-preset-store-preset)
- [loupe_core::process::Capture](#loupe-core-process-capture)
- [loupe_core::records::Format](#loupe-core-records-format)
- [loupe_core::records::Options](#loupe-core-records-options)
- [loupe_core::recovery::Completion](#loupe-core-recovery-completion)
- [loupe_core::recovery::Disposition](#loupe-core-recovery-disposition)
- [loupe_core::recovery::Entry](#loupe-core-recovery-entry)
- [loupe_core::recovery::Frame](#loupe-core-recovery-frame)
- [loupe_core::recovery::Record](#loupe-core-recovery-record)
- [loupe_core::review::Frame](#loupe-core-review-frame)
- [loupe_core::review::Record](#loupe-core-review-record)
- [loupe_core::review::Task](#loupe-core-review-task)
- [loupe_core::route_dir::Artifact](#loupe-core-route-dir-artifact)
- [loupe_core::route_labels::Labels](#loupe-core-route-labels-labels)
- [loupe_core::route_labels::Name](#loupe-core-route-labels-name)
- [loupe_core::route_log::Append](#loupe-core-route-log-append)
- [loupe_core::route_log::Envelope](#loupe-core-route-log-envelope)
- [loupe_core::route_log::Record](#loupe-core-route-log-record)
- [loupe_core::route_log::Target](#loupe-core-route-log-target)
- [loupe_core::route_reset::Clear](#loupe-core-route-reset-clear)
- [loupe_core::route_reset::Entry](#loupe-core-route-reset-entry)
- [loupe_core::route_reset::Envelope](#loupe-core-route-reset-envelope)
- [loupe_core::route_reset::Intent](#loupe-core-route-reset-intent)
- [loupe_core::route_store::Manifest](#loupe-core-route-store-manifest)
- [loupe_core::selection::Decision](#loupe-core-selection-decision)
- [loupe_core::selection::Outcome](#loupe-core-selection-outcome)
- [loupe_core::selection::Reason](#loupe-core-selection-reason)
- [loupe_core::selection::Status](#loupe-core-selection-status)
- [loupe_core::selection::Step](#loupe-core-selection-step)
- [loupe_core::selection_ensemble::Ensemble](#loupe-core-selection-ensemble-ensemble)
- [loupe_core::selection_ensemble::Variant](#loupe-core-selection-ensemble-variant)
- [loupe_core::signals::Id](#loupe-core-signals-id)
- [loupe_core::signals::Report](#loupe-core-signals-report)
- [loupe_core::split::Chunk](#loupe-core-split-chunk)
- [loupe_core::split::Range](#loupe-core-split-range)
- [loupe_core::suites::Case](#loupe-core-suites-case)
- [loupe_core::suites::Gate](#loupe-core-suites-gate)
- [loupe_core::suites::Input](#loupe-core-suites-input)
- [loupe_core::suites::Join](#loupe-core-suites-join)
- [loupe_core::suites::Label](#loupe-core-suites-label)
- [loupe_core::suites::Manifest](#loupe-core-suites-manifest)
- [loupe_core::suites::Metric](#loupe-core-suites-metric)
- [loupe_core::suites::Pin](#loupe-core-suites-pin)
- [loupe_core::suites::Provenance](#loupe-core-suites-provenance)
- [loupe_core::suites::Reference](#loupe-core-suites-reference)
- [loupe_core::suites::Sampling](#loupe-core-suites-sampling)
- [loupe_core::suites::Selection](#loupe-core-suites-selection)
- [loupe_core::suites::SelectionMode](#loupe-core-suites-selectionmode)
- [loupe_core::suites::Span](#loupe-core-suites-span)
- [loupe_core::suites::SplitConfig](#loupe-core-suites-splitconfig)
- [loupe_core::suites::Splits](#loupe-core-suites-splits)
- [loupe_core::suites::Task](#loupe-core-suites-task)
- [loupe_core::task_metrics::Bin](#loupe-core-task-metrics-bin)
- [loupe_core::task_metrics::Bounds](#loupe-core-task-metrics-bounds)
- [loupe_core::task_metrics::BrierScale](#loupe-core-task-metrics-brierscale)
- [loupe_core::task_metrics::CalibrationKind](#loupe-core-task-metrics-calibrationkind)
- [loupe_core::task_metrics::Class](#loupe-core-task-metrics-class)
- [loupe_core::task_metrics::Parity](#loupe-core-task-metrics-parity)
- [loupe_core::task_metrics::Report](#loupe-core-task-metrics-report)
- [loupe_core::task_metrics::Retrieval](#loupe-core-task-metrics-retrieval)
- [loupe_core::task_metrics::Selective](#loupe-core-task-metrics-selective)
- [loupe_core::task_metrics::Timing](#loupe-core-task-metrics-timing)
- [loupe_core::task_metrics::Undefined](#loupe-core-task-metrics-undefined)
- [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value)
- [loupe_core::tournament::Bracket](#loupe-core-tournament-bracket)
- [loupe_core::tournament::Final](#loupe-core-tournament-final)
- [loupe_core::tournament::Options](#loupe-core-tournament-options)
- [loupe_core::tournament::Ranking](#loupe-core-tournament-ranking)
- [loupe_core::tournament::Reduction](#loupe-core-tournament-reduction)
- [loupe_core::tournament::Round](#loupe-core-tournament-round)
- [loupe_core::tournament::Scored](#loupe-core-tournament-scored)
- [loupe_core::tournament::Template](#loupe-core-tournament-template)
- [loupe_core::trigger::Action](#loupe-core-trigger-action)
- [loupe_core::trigger::Declaration](#loupe-core-trigger-declaration)
- [loupe_core::trigger::Matcher](#loupe-core-trigger-matcher)
- [loupe_core::trigger::Outcome](#loupe-core-trigger-outcome)
- [loupe_core::trigger::Target](#loupe-core-trigger-target)
- [loupe_core::truncate::Mode](#loupe-core-truncate-mode)
- [loupe_core::truncate::Range](#loupe-core-truncate-range)
- [loupe_core::truncate::View](#loupe-core-truncate-view)
- [loupe_core::wire::Question](#loupe-core-wire-question)
- [loupe_core::wire::Request](#loupe-core-wire-request)
- [loupe_core::wire::Response](#loupe-core-wire-response)
- [loupe_core::wire::Usage](#loupe-core-wire-usage)

<a id="loupe-args-reviewmode"></a>
## `loupe::args::ReviewMode`

Source: [crates/loupe-cli/src/args.rs:753](../crates/loupe-cli/src/args.rs#L753).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `interactive`

No payload.

### `defer`

No payload.


<a id="loupe-run-dedupe-link"></a>
## `loupe::run::dedupe::Link`

Source: [crates/loupe-cli/src/run/dedupe.rs:43](../crates/loupe-cli/src/run/dedupe.rs#L43).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left` | integer (u64) | Always emitted; no field-level overrides. |
| `right` | integer (u64) | Always emitted; no field-level overrides. |
| `answer` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `model` | string | Always emitted; no field-level overrides. |
| `cached` | boolean | Always emitted; no field-level overrides. |
| `input_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `signals` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `decision` | arbitrary JSON value or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `fallback` | [loupe_core::fallback::Execution](#loupe-core-fallback-execution) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `ensemble` | arbitrary JSON value or null | `#[serde(skip_serializing_if = "Option::is_none")]` |

<a id="loupe-run-dedupe-streaming-recovery-completion"></a>
## `loupe::run::dedupe::streaming::recovery::Completion`

Source: [crates/loupe-cli/src/run/dedupe/streaming/recovery.rs:53](../crates/loupe-cli/src/run/dedupe/streaming/recovery.rs#L53).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `history` | [loupe_core::dedupe::streaming::Snapshot](#loupe-core-dedupe-streaming-snapshot) | Always emitted; no field-level overrides. |
| `wall` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `kept` | boolean | Always emitted; no field-level overrides. |
| `row` | arbitrary JSON value | Always emitted; no field-level overrides. |

<a id="loupe-run-dedupe-streaming-recovery-pair"></a>
## `loupe::run::dedupe::streaming::recovery::Pair`

Source: [crates/loupe-cli/src/run/dedupe/streaming/recovery.rs:14](../crates/loupe-cli/src/run/dedupe/streaming/recovery.rs#L14).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `representative` | integer (u64) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) | Always emitted; no field-level overrides. |
| `fallback` | [loupe_core::fallback::Execution](#loupe-core-fallback-execution) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-run-dedupe-streaming-recovery-progress"></a>
## `loupe::run::dedupe::streaming::recovery::Progress`

Source: [crates/loupe-cli/src/run/dedupe/streaming/recovery.rs:46](../crates/loupe-cli/src/run/dedupe/streaming/recovery.rs#L46).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `admitted` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `wall` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `pairs` | array of [loupe::run::dedupe::streaming::recovery::Pair](#loupe-run-dedupe-streaming-recovery-pair) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-failure"></a>
## `loupe::run::eval::Failure`

Source: [crates/loupe-cli/src/run/eval.rs:108](../crates/loupe-cli/src/run/eval.rs#L108).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `code` | integer (u8) | Always emitted; no field-level overrides. |
| `message` | string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-identity"></a>
## `loupe::run::eval::Identity`

Source: [crates/loupe-cli/src/run/eval.rs:78](../crates/loupe-cli/src/run/eval.rs#L78).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `verified` | boolean | Always emitted; no field-level overrides. |
| `model` | [loupe_core::identity::Model](#loupe-core-identity-model) or null | Always emitted; no field-level overrides. |
| `requested_model` | string | Always emitted; no field-level overrides. |
| `local_encoder` | string | Always emitted; no field-level overrides. |
| `server_encoder` | string or null | Always emitted; no field-level overrides. |
| `reason` | string or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-phase"></a>
## `loupe::run::eval::Phase`

Source: [crates/loupe-cli/src/run/eval.rs:159](../crates/loupe-cli/src/run/eval.rs#L159).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | string | Always emitted; no field-level overrides. |
| `pass` | integer (usize) | Always emitted; no field-level overrides. |
| `cache_requested` | boolean | Always emitted; no field-level overrides. |
| `cache_hits` | integer (u64) | Always emitted; no field-level overrides. |
| `logical_requests` | integer (u64) | Always emitted; no field-level overrides. |
| `cache_hit_rate` | number (f64) or null | Always emitted; no field-level overrides. |
| `usage` | [loupe_core::budget::Usage](#loupe-core-budget-usage) | Always emitted; no field-level overrides. |
| `timing` | [loupe_core::task_metrics::Timing](#loupe-core-task-metrics-timing) | Always emitted; no field-level overrides. |
| `groups` | array of arbitrary JSON value | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-policydata"></a>
## `loupe::run::eval::PolicyData`

Source: [crates/loupe-cli/src/run/eval.rs:48](../crates/loupe-cli/src/run/eval.rs#L48).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `threshold` | number (f64) | Always emitted; no field-level overrides. |
| `margin` | number (f64) | Always emitted; no field-level overrides. |
| `min_confidence` | number (f64) | Always emitted; no field-level overrides. |
| `min_gap` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-questionreport"></a>
## `loupe::run::eval::QuestionReport`

Source: [crates/loupe-cli/src/run/eval.rs:114](../crates/loupe-cli/src/run/eval.rs#L114).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `field` | string | Always emitted; no field-level overrides. |
| `rubric_signature` | string | Always emitted; no field-level overrides. |
| `question` | [loupe_core::wire::Question](#loupe-core-wire-question) | Always emitted; no field-level overrides. |
| `policy` | [loupe::run::eval::PolicyData](#loupe-run-eval-policydata) | Always emitted; no field-level overrides. |
| `case_ids` | array of string | Always emitted; no field-level overrides. |
| `metrics` | [loupe_core::task_metrics::Report](#loupe-core-task-metrics-report) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-record"></a>
## `loupe::run::eval::Record`

Source: [crates/loupe-cli/src/run/eval.rs:133](../crates/loupe-cli/src/run/eval.rs#L133).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `ensemble` | [loupe::run::eval::ensemble::Saved](#loupe-run-eval-ensemble-saved) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `case_id` | string | Always emitted; no field-level overrides. |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `label` | [loupe_core::suites::Label](#loupe-core-suites-label) | Always emitted; no field-level overrides. |
| `source` | string | Always emitted; no field-level overrides. |
| `reviewed` | boolean | Always emitted; no field-level overrides. |
| `sampling` | [loupe_core::suites::Sampling](#loupe-core-suites-sampling) or null | Always emitted; no field-level overrides. |
| `preset` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `effective_document` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `policies` | object mapping string keys to [loupe::run::eval::PolicyData](#loupe-run-eval-policydata) | Always emitted; no field-level overrides. |
| `questions` | object mapping string keys to [loupe_core::wire::Question](#loupe-core-wire-question) | Always emitted; no field-level overrides. |
| `base_question_order` | array of string | Always emitted; no field-level overrides. |
| `binding` | object mapping string keys to string | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `request_role` | [loupe::run::eval::RequestRole](#loupe-run-eval-requestrole) | Always emitted; no field-level overrides. |
| `request_question_order` | array of string | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) or null | Always emitted; no field-level overrides. |
| `packing` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `provenance` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `reference` | [loupe_core::suites::Reference](#loupe-core-suites-reference) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-requestrole"></a>
## `loupe::run::eval::RequestRole`

Source: [crates/loupe-cli/src/run/eval.rs:125](../crates/loupe-cli/src/run/eval.rs#L125).

Container attributes: `#[serde(deny_unknown_fields)]`; `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `original-split-input`

No payload.

### `original-focus-input`

No payload.

### `focus-final`

No payload.

### `evaluated`

No payload.


<a id="loupe-run-eval-runreport"></a>
## `loupe::run::eval::RunReport`

Source: [crates/loupe-cli/src/run/eval.rs:88](../crates/loupe-cli/src/run/eval.rs#L88).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `generated_at` | string | Always emitted; no field-level overrides. |
| `cli_version` | string | Always emitted; no field-level overrides. |
| `status` | string | Always emitted; no field-level overrides. |
| `identity` | [loupe::run::eval::Identity](#loupe-run-eval-identity) | Always emitted; no field-level overrides. |
| `endpoint` | string | Always emitted; no field-level overrides. |
| `environment` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `server` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `token_usage` | [loupe_core::budget::Usage](#loupe-core-budget-usage) | Always emitted; no field-level overrides. |
| `pricing` | [loupe_core::cost::Pricing](#loupe-core-cost-pricing) or null | Always emitted; no field-level overrides. |
| `cost_usd_estimate` | number (f64) or null | Always emitted; no field-level overrides. |
| `cost_unknown_output` | boolean | Always emitted; no field-level overrides. |
| `suites` | array of [loupe::run::eval::SuiteReport](#loupe-run-eval-suitereport) | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |
| `acceptance` | [loupe::run::eval::gates::Report](#loupe-run-eval-gates-report) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-run-eval-suitereport"></a>
## `loupe::run::eval::SuiteReport`

Source: [crates/loupe-cli/src/run/eval.rs:173](../crates/loupe-cli/src/run/eval.rs#L173).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `manifest` | [loupe_core::suites::Manifest](#loupe-core-suites-manifest) | Always emitted; no field-level overrides. |
| `splits` | [loupe_core::suites::Splits](#loupe-core-suites-splits) | Always emitted; no field-level overrides. |
| `partition` | string | Always emitted; no field-level overrides. |
| `selected_case_ids` | array of string | Always emitted; no field-level overrides. |
| `records` | array of [loupe::run::eval::Record](#loupe-run-eval-record) | Always emitted; no field-level overrides. |
| `questions` | array of [loupe::run::eval::QuestionReport](#loupe-run-eval-questionreport) | Always emitted; no field-level overrides. |
| `parity` | [loupe_core::task_metrics::Parity](#loupe-core-task-metrics-parity) or null | Always emitted; no field-level overrides. |
| `phases` | array of [loupe::run::eval::Phase](#loupe-run-eval-phase) | Always emitted; no field-level overrides. |
| `warnings` | array of string | Always emitted; no field-level overrides. |
| `join` | [loupe::run::eval::join::Report](#loupe-run-eval-join-report) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `selection` | [loupe::run::eval::selection::Report](#loupe-run-eval-selection-report) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-run-eval-calibrate-fitted"></a>
## `loupe::run::eval::calibrate::Fitted`

Source: [crates/loupe-cli/src/run/eval/calibrate.rs:26](../crates/loupe-cli/src/run/eval/calibrate.rs#L26).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `suite` | string | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `partition` | string | Always emitted; no field-level overrides. |
| `fit` | [loupe_core::calibration::Fit](#loupe-core-calibration-fit) | Always emitted; no field-level overrides. |
| `proposed_preset` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `written` | string (UTF-8 filesystem path) or null | Always emitted; no field-level overrides. |
| `ensemble` | [loupe_core::calibration_pin::Ensemble](#loupe-core-calibration-pin-ensemble) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `agreement_uncertain_cases` | array of string | `#[serde(skip_serializing_if = "Vec::is_empty")]` |
| `original (storage only, skipped)` | [loupe_core::preset_store::Preset](#loupe-core-preset-store-preset) or null | `#[serde(skip)]` |

<a id="loupe-run-eval-calibrate-report"></a>
## `loupe::run::eval::calibrate::Report`

Source: [crates/loupe-cli/src/run/eval/calibrate.rs:41](../crates/loupe-cli/src/run/eval/calibrate.rs#L41).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `kind` | string | Always emitted; no field-level overrides. |
| `status` | string | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |
| `targets` | array of [loupe_core::calibration::Target](#loupe-core-calibration-target) | Always emitted; no field-level overrides. |
| `max_uncertain` | number (f64) or null | Always emitted; no field-level overrides. |
| `minimum_support` | integer (usize) | Always emitted; no field-level overrides. |
| `fitted` | array of [loupe::run::eval::calibrate::Fitted](#loupe-run-eval-calibrate-fitted) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe::run::eval::RunReport](#loupe-run-eval-runreport) | Always emitted; no field-level overrides. |
| `notes` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-calibrate-views-report"></a>
## `loupe::run::eval::calibrate::views::Report`

Source: [crates/loupe-cli/src/run/eval/calibrate/views.rs:57](../crates/loupe-cli/src/run/eval/calibrate/views.rs#L57).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `kind` | string | Always emitted; no field-level overrides. |
| `status` | string | Always emitted; no field-level overrides. |
| `targets` | array of [loupe_core::calibration::Target](#loupe-core-calibration-target) | Always emitted; no field-level overrides. |
| `max_uncertain` | number (f64) or null | Always emitted; no field-level overrides. |
| `minimum_support` | integer (usize) | Always emitted; no field-level overrides. |
| `token_usage` | [loupe_core::budget::Usage](#loupe-core-budget-usage) | Always emitted; no field-level overrides. |
| `variants` | array of [loupe::run::eval::calibrate::views::Variant](#loupe-run-eval-calibrate-views-variant) | Always emitted; no field-level overrides. |
| `selected` | array of [loupe::run::eval::calibrate::views::Winner](#loupe-run-eval-calibrate-views-winner) | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |
| `notes` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-calibrate-views-variant"></a>
## `loupe::run::eval::calibrate::views::Variant`

Source: [crates/loupe-cli/src/run/eval/calibrate/views.rs:19](../crates/loupe-cli/src/run/eval/calibrate/views.rs#L19).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | string | Always emitted; no field-level overrides. |
| `evaluation` | [loupe::run::eval::RunReport](#loupe-run-eval-runreport) | Always emitted; no field-level overrides. |
| `fitted` | array of [loupe::run::eval::calibrate::Fitted](#loupe-run-eval-calibrate-fitted) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-calibrate-views-winner"></a>
## `loupe::run::eval::calibrate::views::Winner`

Source: [crates/loupe-cli/src/run/eval/calibrate/views.rs:25](../crates/loupe-cli/src/run/eval/calibrate/views.rs#L25).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `variant` | string | Always emitted; no field-level overrides. |
| `suite` | string | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `accepted` | integer (usize) | Always emitted; no field-level overrides. |
| `correct` | integer (usize) | Always emitted; no field-level overrides. |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `uncached_units_per_second` | number (f64) | Always emitted; no field-level overrides. |
| `proposed_preset` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `written` | string (UTF-8 filesystem path) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-ensemble-saved"></a>
## `loupe::run::eval::ensemble::Saved`

Source: [crates/loupe-cli/src/run/eval/ensemble.rs:22](../crates/loupe-cli/src/run/eval/ensemble.rs#L22).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `settings` | [loupe::run::eval::ensemble::Settings](#loupe-run-eval-ensemble-settings) | Always emitted; no field-level overrides. |
| `reports` | object mapping string keys to arbitrary JSON value | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-ensemble-settings"></a>
## `loupe::run::eval::ensemble::Settings`

Source: [crates/loupe-cli/src/run/eval/ensemble.rs:15](../crates/loupe-cli/src/run/eval/ensemble.rs#L15).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `min_agreement` | number (f64) | Always emitted; no field-level overrides. |
| `primaries` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-gates-checked"></a>
## `loupe::run::eval::gates::Checked`

Source: [crates/loupe-cli/src/run/eval/gates.rs:32](../crates/loupe-cli/src/run/eval/gates.rs#L32).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `suite` | string | Always emitted; no field-level overrides. |
| `field` | string or null | Always emitted; no field-level overrides. |
| `metric` | [loupe_core::suites::Metric](#loupe-core-suites-metric) | Always emitted; no field-level overrides. |
| `gate` | [loupe_core::suites::Gate](#loupe-core-suites-gate) | Always emitted; no field-level overrides. |
| `minimum_support` | integer (usize) | Always emitted; no field-level overrides. |
| `baseline` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::baseline::Assessment](#loupe-core-baseline-assessment) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-gates-report"></a>
## `loupe::run::eval::gates::Report`

Source: [crates/loupe-cli/src/run/eval/gates.rs:44](../crates/loupe-cli/src/run/eval/gates.rs#L44).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `status` | [loupe::run::eval::gates::Status](#loupe-run-eval-gates-status) | Always emitted; no field-level overrides. |
| `baseline_blake3` | string | Always emitted; no field-level overrides. |
| `reference_report_blake3` | string | Always emitted; no field-level overrides. |
| `reference_model` | string | Always emitted; no field-level overrides. |
| `reference_local_encoder` | string | Always emitted; no field-level overrides. |
| `reference_server_encoder` | string | Always emitted; no field-level overrides. |
| `ratification` | [loupe_core::baseline::Ratification](#loupe-core-baseline-ratification) | Always emitted; no field-level overrides. |
| `checks` | array of [loupe::run::eval::gates::Checked](#loupe-run-eval-gates-checked) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-gates-status"></a>
## `loupe::run::eval::gates::Status`

Source: [crates/loupe-cli/src/run/eval/gates.rs:23](../crates/loupe-cli/src/run/eval/gates.rs#L23).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `plan`

No payload.

### `passed`

No payload.

### `failed`

No payload.

### `incomplete`

No payload.


<a id="loupe-run-eval-join-assigned"></a>
## `loupe::run::eval::join::Assigned`

Source: [crates/loupe-cli/src/run/eval/join.rs:72](../crates/loupe-cli/src/run/eval/join.rs#L72).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cardinality` | [loupe_core::matching::Cardinality](#loupe-core-matching-cardinality) | Always emitted; no field-level overrides. |
| `rows` | array of [loupe::run::eval::join::Row](#loupe-run-eval-join-row) | Always emitted; no field-level overrides. |
| `metrics` | [loupe::run::eval::join::AssignmentMetrics](#loupe-run-eval-join-assignmentmetrics) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-join-assignmentmetrics"></a>
## `loupe::run::eval::join::AssignmentMetrics`

Source: [crates/loupe-cli/src/run/eval/join.rs:86](../crates/loupe-cli/src/run/eval/join.rs#L86).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cardinality` | [loupe_core::matching::Cardinality](#loupe-core-matching-cardinality) | Always emitted; no field-level overrides. |
| `correct_rows` | integer (usize) | Always emitted; no field-level overrides. |
| `rows` | integer (usize) | Always emitted; no field-level overrides. |
| `true_edges` | integer (usize) | Always emitted; no field-level overrides. |
| `output_edges` | integer (usize) | Always emitted; no field-level overrides. |
| `feasible_edges` | integer (usize) | Always emitted; no field-level overrides. |
| `assignment_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `edge_precision` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `feasible_edge_recall` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-join-catalog"></a>
## `loupe::run::eval::join::Catalog`

Source: [crates/loupe-cli/src/run/eval/join.rs:43](../crates/loupe-cli/src/run/eval/join.rs#L43).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `case_id` | string | Always emitted; no field-level overrides. |
| `source` | string | Always emitted; no field-level overrides. |
| `reviewed` | boolean | Always emitted; no field-level overrides. |
| `source_location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |
| `left` | array of [loupe_core::input::Unit](#loupe-core-input-unit) | Always emitted; no field-level overrides. |
| `right` | array of [loupe_core::input::Unit](#loupe-core-input-unit) | Always emitted; no field-level overrides. |
| `effective_document` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `policies` | object mapping string keys to [loupe::run::eval::PolicyData](#loupe-run-eval-policydata) | Always emitted; no field-level overrides. |
| `question_order` | array of string | Always emitted; no field-level overrides. |
| `truth` | object mapping integer (u64) keys to array of integer (u64) | Always emitted; no field-level overrides. |
| `blocked` | object mapping integer (u64) keys to array of integer (u64) | Always emitted; no field-level overrides. |
| `pairs` | array of [loupe::run::eval::join::Pair](#loupe-run-eval-join-pair) | Always emitted; no field-level overrides. |
| `assignments` | array of [loupe::run::eval::join::Assigned](#loupe-run-eval-join-assigned) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-join-pair"></a>
## `loupe::run::eval::join::Pair`

Source: [crates/loupe-cli/src/run/eval/join.rs:60](../crates/loupe-cli/src/run/eval/join.rs#L60).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |
| `left` | integer (u64) | Always emitted; no field-level overrides. |
| `right` | integer (u64) | Always emitted; no field-level overrides. |
| `label` | [loupe_core::suites::Label](#loupe-core-suites-label) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) or null | Always emitted; no field-level overrides. |
| `decision` | arbitrary JSON value or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-join-report"></a>
## `loupe::run::eval::join::Report`

Source: [crates/loupe-cli/src/run/eval/join.rs:33](../crates/loupe-cli/src/run/eval/join.rs#L33).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | array of [loupe::run::eval::join::Catalog](#loupe-run-eval-join-catalog) | Always emitted; no field-level overrides. |
| `blocking_recall` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `all_pair_quality` | [loupe_core::task_metrics::Report](#loupe-core-task-metrics-report) or null | Always emitted; no field-level overrides. |
| `blocked_pair_quality` | [loupe_core::task_metrics::Report](#loupe-core-task-metrics-report) or null | Always emitted; no field-level overrides. |
| `ranking_auroc` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `assignments` | array of [loupe::run::eval::join::AssignmentMetrics](#loupe-run-eval-join-assignmentmetrics) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-join-row"></a>
## `loupe::run::eval::join::Row`

Source: [crates/loupe-cli/src/run/eval/join.rs:79](../crates/loupe-cli/src/run/eval/join.rs#L79).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left` | integer (u64) | Always emitted; no field-level overrides. |
| `candidates` | array of integer (u64) | Always emitted; no field-level overrides. |
| `selected` | array of integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-candidatedata"></a>
## `loupe::run::eval::selection::CandidateData`

Source: [crates/loupe-cli/src/run/eval/selection.rs:97](../crates/loupe-cli/src/run/eval/selection.rs#L97).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `verbatim` | string | Always emitted; no field-level overrides. |
| `location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-casereport"></a>
## `loupe::run::eval::selection::CaseReport`

Source: [crates/loupe-cli/src/run/eval/selection.rs:63](../crates/loupe-cli/src/run/eval/selection.rs#L63).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `case_id` | string | Always emitted; no field-level overrides. |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `source` | string | Always emitted; no field-level overrides. |
| `reviewed` | boolean | Always emitted; no field-level overrides. |
| `sampling` | [loupe_core::suites::Sampling](#loupe-core-suites-sampling) or null | Always emitted; no field-level overrides. |
| `source_location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |
| `text` | string | Always emitted; no field-level overrides. |
| `mode` | [loupe_core::suites::SelectionMode](#loupe-core-suites-selectionmode) | Always emitted; no field-level overrides. |
| `preset` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `effective_document` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `template` | [loupe::run::eval::selection::TemplateData](#loupe-run-eval-selection-templatedata) | Always emitted; no field-level overrides. |
| `policy` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `candidate_parser` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `normalization` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `expected` | array of string | Always emitted; no field-level overrides. |
| `expected_spans` | array of [loupe_core::suites::Span](#loupe-core-suites-span) | Always emitted; no field-level overrides. |
| `catalog` | array of [loupe::run::eval::selection::CandidateData](#loupe-run-eval-selection-candidatedata) | Always emitted; no field-level overrides. |
| `plan` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `requests` | array of [loupe::run::eval::selection::Saved](#loupe-run-eval-selection-saved) | Always emitted; no field-level overrides. |
| `outcome` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `selected` | array of [loupe::run::eval::selection::Selected](#loupe-run-eval-selection-selected) | Always emitted; no field-level overrides. |
| `complete` | boolean | Always emitted; no field-level overrides. |
| `uncertain` | boolean | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-presencequality"></a>
## `loupe::run::eval::selection::PresenceQuality`

Source: [crates/loupe-cli/src/run/eval/selection.rs:57](../crates/loupe-cli/src/run/eval/selection.rs#L57).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `policy` | [loupe::run::eval::PolicyData](#loupe-run-eval-policydata) | Always emitted; no field-level overrides. |
| `metrics` | [loupe_core::task_metrics::Report](#loupe-core-task-metrics-report) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-report"></a>
## `loupe::run::eval::selection::Report`

Source: [crates/loupe-cli/src/run/eval/selection.rs:45](../crates/loupe-cli/src/run/eval/selection.rs#L45).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | array of [loupe::run::eval::selection::CaseReport](#loupe-run-eval-selection-casereport) | Always emitted; no field-level overrides. |
| `retrieval` | [loupe_core::task_metrics::Retrieval](#loupe-core-task-metrics-retrieval) | Always emitted; no field-level overrides. |
| `presence_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `absence_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `coverage` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `offset_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `normalization_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `bracket_presence_quality` | array of [loupe::run::eval::selection::PresenceQuality](#loupe-run-eval-selection-presencequality) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-saved"></a>
## `loupe::run::eval::selection::Saved`

Source: [crates/loupe-cli/src/run/eval/selection.rs:104](../crates/loupe-cli/src/run/eval/selection.rs#L104).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `round` | integer (usize) | Always emitted; no field-level overrides. |
| `bracket` | integer (usize) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `question_order` | array of string | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) or null | Always emitted; no field-level overrides. |
| `decision` | arbitrary JSON value or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-selected"></a>
## `loupe::run::eval::selection::Selected`

Source: [crates/loupe-cli/src/run/eval/selection.rs:115](../crates/loupe-cli/src/run/eval/selection.rs#L115).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `unit` | string | Always emitted; no field-level overrides. |
| `location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |
| `probability` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `ranking_score` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `normalized` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `normalization_error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |
| `offset_correct` | boolean or null | Always emitted; no field-level overrides. |
| `normalization_correct` | boolean or null | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-selection-templatedata"></a>
## `loupe::run::eval::selection::TemplateData`

Source: [crates/loupe-cli/src/run/eval/selection.rs:91](../crates/loupe-cli/src/run/eval/selection.rs#L91).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `value` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `companion_order` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-report"></a>
## `loupe::run::eval::variants::Report`

Source: [crates/loupe-cli/src/run/eval/variants.rs:63](../crates/loupe-cli/src/run/eval/variants.rs#L63).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `generated_at` | string | Always emitted; no field-level overrides. |
| `kind` | string | Always emitted; no field-level overrides. |
| `status` | string | Always emitted; no field-level overrides. |
| `token_usage` | [loupe_core::budget::Usage](#loupe-core-budget-usage) | Always emitted; no field-level overrides. |
| `variants` | array of [loupe::run::eval::variants::Variant](#loupe-run-eval-variants-variant) | Always emitted; no field-level overrides. |
| `comparisons` | array of [loupe::run::eval::variants::compare::Report](#loupe-run-eval-variants-compare-report) | Always emitted; no field-level overrides. |
| `error` | [loupe::run::eval::Failure](#loupe-run-eval-failure) or null | Always emitted; no field-level overrides. |
| `notes` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-variant"></a>
## `loupe::run::eval::variants::Variant`

Source: [crates/loupe-cli/src/run/eval/variants.rs:57](../crates/loupe-cli/src/run/eval/variants.rs#L57).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | string | Always emitted; no field-level overrides. |
| `source` | string | Always emitted; no field-level overrides. |
| `report` | [loupe::run::eval::RunReport](#loupe-run-eval-runreport) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-compare-cell"></a>
## `loupe::run::eval::variants::compare::Cell`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:22](../crates/loupe-cli/src/run/eval/variants/compare.rs#L22).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left` | [loupe::run::eval::variants::compare::Decision](#loupe-run-eval-variants-compare-decision) | Always emitted; no field-level overrides. |
| `right` | [loupe::run::eval::variants::compare::Decision](#loupe-run-eval-variants-compare-decision) | Always emitted; no field-level overrides. |
| `count` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-compare-decision"></a>
## `loupe::run::eval::variants::compare::Decision`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:15](../crates/loupe-cli/src/run/eval/variants/compare.rs#L15).

Container attributes: `#[serde(tag = "type", content = "value", rename_all = "kebab-case")]`.

Discriminator key: `type`. Payload key: `value`.

### `uncertain`

No payload.

### `boolean`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | boolean | Always emitted; no field-level overrides. |

### `choice`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

### `score`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | number (f64) | Always emitted; no field-level overrides. |


<a id="loupe-run-eval-variants-compare-disagreement"></a>
## `loupe::run::eval::variants::compare::Disagreement`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:58](../crates/loupe-cli/src/run/eval/variants/compare.rs#L58).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `suite_left` | string | Always emitted; no field-level overrides. |
| `suite_right` | string | Always emitted; no field-level overrides. |
| `case_id` | string | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `field_left` | string | Always emitted; no field-level overrides. |
| `field_right` | string | Always emitted; no field-level overrides. |
| `left` | [loupe::run::eval::variants::compare::Decision](#loupe-run-eval-variants-compare-decision) | Always emitted; no field-level overrides. |
| `right` | [loupe::run::eval::variants::compare::Decision](#loupe-run-eval-variants-compare-decision) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-compare-field"></a>
## `loupe::run::eval::variants::compare::Field`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:28](../crates/loupe-cli/src/run/eval/variants/compare.rs#L28).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `suite_left` | string | Always emitted; no field-level overrides. |
| `suite_right` | string | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `changed_decisions` | integer (usize) | Always emitted; no field-level overrides. |
| `matrix` | array of [loupe::run::eval::variants::compare::Cell](#loupe-run-eval-variants-compare-cell) | Always emitted; no field-level overrides. |
| `max_probability_difference` | number (f64) | Always emitted; no field-level overrides. |
| `mean_probability_difference` | number (f64) | Always emitted; no field-level overrides. |
| `probabilities` | integer (usize) | Always emitted; no field-level overrides. |
| `raw_primary_shift` | [loupe::run::eval::variants::compare::RawPrimaryShift](#loupe-run-eval-variants-compare-rawprimaryshift) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |

<a id="loupe-run-eval-variants-compare-rawprimaryshift"></a>
## `loupe::run::eval::variants::compare::RawPrimaryShift`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:43](../crates/loupe-cli/src/run/eval/variants/compare.rs#L43).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `max_probability_difference` | number (f64) | Always emitted; no field-level overrides. |
| `mean_probability_difference` | number (f64) | Always emitted; no field-level overrides. |
| `probabilities` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-run-eval-variants-compare-report"></a>
## `loupe::run::eval::variants::compare::Report`

Source: [crates/loupe-cli/src/run/eval/variants/compare.rs:69](../crates/loupe-cli/src/run/eval/variants/compare.rs#L69).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left` | string | Always emitted; no field-level overrides. |
| `right` | string | Always emitted; no field-level overrides. |
| `paired_fields` | integer (usize) | Always emitted; no field-level overrides. |
| `fields` | array of [loupe::run::eval::variants::compare::Field](#loupe-run-eval-variants-compare-field) | Always emitted; no field-level overrides. |
| `disagreements` | array of [loupe::run::eval::variants::compare::Disagreement](#loupe-run-eval-variants-compare-disagreement) | Always emitted; no field-level overrides. |

<a id="loupe-run-fallback-plan"></a>
## `loupe::run::fallback::Plan`

Source: [crates/loupe-cli/src/run/fallback.rs:16](../crates/loupe-cli/src/run/fallback.rs#L16).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `command` | string | Always emitted; no field-level overrides. |
| `timeout` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `failure` | [loupe_core::fallback::FailurePolicy](#loupe-core-fallback-failurepolicy) | Always emitted; no field-level overrides. |

<a id="loupe-run-matching-journal-pair"></a>
## `loupe::run::matching::journal::Pair`

Source: [crates/loupe-cli/src/run/matching/journal.rs:14](../crates/loupe-cli/src/run/matching/journal.rs#L14).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `right_index` | integer (u64) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) | Always emitted; no field-level overrides. |
| `fallbacks` | object mapping string keys to [loupe_core::fallback::Execution](#loupe-core-fallback-execution) | `#[serde(default, skip_serializing_if = "IndexMap::is_empty")]` |

<a id="loupe-run-matching-journal-snapshot"></a>
## `loupe::run::matching::journal::Snapshot`

Source: [crates/loupe-cli/src/run/matching/journal.rs:22](../crates/loupe-cli/src/run/matching/journal.rs#L22).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `pairs` | array of [loupe::run::matching::journal::Pair](#loupe-run-matching-journal-pair) | Always emitted; no field-level overrides. |
| `complete` | boolean | Always emitted; no field-level overrides. |
| `selected_rights` | array of integer (u64) | Always emitted; no field-level overrides. |
| `error` | tuple array (integer (u8), string) or null | Always emitted; no field-level overrides. |

<a id="loupe-run-recovery-snapshot"></a>
## `loupe::run::recovery::Snapshot`

Source: [crates/loupe-cli/src/run/recovery.rs:42](../crates/loupe-cli/src/run/recovery.rs#L42).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `question_order` | array of string | Always emitted; no field-level overrides. |
| `answer_order` | array of string | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `evaluation` | [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) | Always emitted; no field-level overrides. |
| `answer_id` | string | Always emitted; no field-level overrides. |
| `packed_indices` | array of integer (u64) or null | Always emitted; no field-level overrides. |
| `split_plan` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `view` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `focus_plan` | arbitrary JSON value or null | Always emitted; no field-level overrides. |

<a id="loupe-run-review-decision"></a>
## `loupe::run::review::Decision`

Source: [crates/loupe-cli/src/run/review.rs:19](../crates/loupe-cli/src/run/review.rs#L19).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `task` | string or null | Always emitted; no field-level overrides. |
| `sampling` | [loupe_core::suites::Sampling](#loupe-core-suites-sampling) or null | Always emitted; no field-level overrides. |
| `queued` | boolean | Always emitted; no field-level overrides. |
| `label` | [loupe_core::policy::Selected](#loupe-core-policy-selected) or null | Always emitted; no field-level overrides. |
| `skipped` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-run-selection-history"></a>
## `loupe::run::selection::History`

Source: [crates/loupe-cli/src/run/selection.rs:44](../crates/loupe-cli/src/run/selection.rs#L44).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u32) | Always emitted; no field-level overrides. |
| `plans` | array of arbitrary JSON value | Always emitted; no field-level overrides. |
| `evaluations` | array of array of [loupe_core::backend::Evaluation](#loupe-core-backend-evaluation) | Always emitted; no field-level overrides. |
| `fallbacks` | object mapping integer (usize) keys to object mapping integer (usize) keys to object mapping string keys to [loupe_core::fallback::Execution](#loupe-core-fallback-execution) | `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]` |
| `complete (storage only, skipped)` | boolean | `#[serde(skip)]` |

<a id="loupe-core-backend-batchcapability"></a>
## `loupe_core::backend::BatchCapability`

Source: [crates/loupe-core/src/backend.rs:40](../crates/loupe-core/src/backend.rs#L40).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `supported` | boolean | Always emitted; no field-level overrides. |
| `path` | string | Always emitted; no field-level overrides. |
| `max_records` | integer (usize) | Always emitted; no field-level overrides. |
| `queue_records` | integer (usize) or null | Always emitted; no field-level overrides. |
| `gpu_records` | integer (usize) or null | Always emitted; no field-level overrides. |
| `gpu_padded_tokens` | integer (usize) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-backend-discovery"></a>
## `loupe_core::backend::Discovery`

Source: [crates/loupe-core/src/backend.rs:28](../crates/loupe-core/src/backend.rs#L28).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `health` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `models` | arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `limits` | [loupe_core::config::Limits](#loupe-core-config-limits) | Always emitted; no field-level overrides. |
| `unsupported` | array of string | Always emitted; no field-level overrides. |
| `unavailable` | array of string | Always emitted; no field-level overrides. |
| `full_model_identity` | string or null | Always emitted; no field-level overrides. |
| `identity` | [loupe_core::identity::Discovery](#loupe-core-identity-discovery) or null | Always emitted; no field-level overrides. |
| `batch` | [loupe_core::backend::BatchCapability](#loupe-core-backend-batchcapability) | Always emitted; no field-level overrides. |

<a id="loupe-core-backend-evaluation"></a>
## `loupe_core::backend::Evaluation`

Source: [crates/loupe-core/src/backend.rs:89](../crates/loupe-core/src/backend.rs#L89).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `response` | [loupe_core::wire::Response](#loupe-core-wire-response) | Always emitted; no field-level overrides. |
| `warning` | string or null | Always emitted; no field-level overrides. |
| `cached` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-baseline-assessment"></a>
## `loupe_core::baseline::Assessment`

Source: [crates/loupe-core/src/baseline.rs:185](../crates/loupe-core/src/baseline.rs#L185).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `current` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `baseline` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `minimum_support` | integer (usize) | Always emitted; no field-level overrides. |
| `bounds` | array of [loupe_core::baseline::Bound](#loupe-core-baseline-bound) | Always emitted; no field-level overrides. |
| `reasons` | array of [loupe_core::baseline::Reason](#loupe-core-baseline-reason) | Always emitted; no field-level overrides. |
| `passed` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-baseline-bound"></a>
## `loupe_core::baseline::Bound`

Source: [crates/loupe-core/src/baseline.rs:166](../crates/loupe-core/src/baseline.rs#L166).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `constraint` | [loupe_core::baseline::Constraint](#loupe-core-baseline-constraint) | Always emitted; no field-level overrides. |
| `limit` | number (f64) | Always emitted; no field-level overrides. |
| `observed` | number (f64) or null | Always emitted; no field-level overrides. |
| `passed` | boolean or null | Always emitted; no field-level overrides. |

<a id="loupe-core-baseline-check"></a>
## `loupe_core::baseline::Check`

Source: [crates/loupe-core/src/baseline.rs:25](../crates/loupe-core/src/baseline.rs#L25).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `suite` | string | Always emitted; no field-level overrides. |
| `field` | string or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `minimum_support` | nonzero integer (NonZeroUsize) | `#[serde(default = "one")]` |
| `gate` | [loupe_core::suites::Gate](#loupe-core-suites-gate) | Always emitted; no field-level overrides. |

<a id="loupe-core-baseline-constraint"></a>
## `loupe_core::baseline::Constraint`

Source: [crates/loupe-core/src/baseline.rs:157](../crates/loupe-core/src/baseline.rs#L157).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `minimum`

No payload.

### `maximum`

No payload.

### `max-drop`

No payload.

### `max-increase`

No payload.


<a id="loupe-core-baseline-ratification"></a>
## `loupe_core::baseline::Ratification`

Source: [crates/loupe-core/src/baseline.rs:17](../crates/loupe-core/src/baseline.rs#L17).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `by` | string | Always emitted; no field-level overrides. |
| `at` | string | Always emitted; no field-level overrides. |
| `justification` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-baseline-reason"></a>
## `loupe_core::baseline::Reason`

Source: [crates/loupe-core/src/baseline.rs:175](../crates/loupe-core/src/baseline.rs#L175).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `current-undefined`

No payload.

### `baseline-undefined`

No payload.

### `insufficient-current-support`

No payload.

### `insufficient-baseline-support`

No payload.

### `non-finite-difference`

No payload.


<a id="loupe-core-baseline-record"></a>
## `loupe_core::baseline::Record`

Source: [crates/loupe-core/src/baseline.rs:42](../crates/loupe-core/src/baseline.rs#L42).

Container attributes: `#[serde(deny_unknown_fields)]`.

Explicit human-authored floors over a pinned, published evaluator report.
The report, rather than a model alias in this file, supplies execution identity.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `report` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |
| `report_blake3` | string | Always emitted; no field-level overrides. |
| `ratification` | [loupe_core::baseline::Ratification](#loupe-core-baseline-ratification) | Always emitted; no field-level overrides. |
| `checks` | array of [loupe_core::baseline::Check](#loupe-core-baseline-check) | Always emitted; no field-level overrides. |

<a id="loupe-core-blocking-candidate"></a>
## `loupe_core::blocking::Candidate`

Source: [crates/loupe-core/src/blocking.rs:146](../crates/loupe-core/src/blocking.rs#L146).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `right_index` | integer (u64) | Always emitted; no field-level overrides. |
| `exact_fields` | integer (usize) | Always emitted; no field-level overrides. |
| `intersection` | integer (usize) | Always emitted; no field-level overrides. |
| `union` | integer (usize) | Always emitted; no field-level overrides. |
| `token_jaccard` | number (f64) | Always emitted; no field-level overrides. |
| `matching_lsh_bands` | integer (usize) | Always emitted; no field-level overrides. |
| `position (storage only, skipped)` | integer (usize) | `#[serde(skip)]` |
| `owner (storage only, skipped)` | array[32] of integer (u8) | `#[serde(skip)]` |

<a id="loupe-core-blocking-mode"></a>
## `loupe_core::blocking::Mode`

Source: [crates/loupe-core/src/blocking.rs:21](../crates/loupe-core/src/blocking.rs#L21).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `tokens`

No payload.

### `lsh`

No payload.

### `hybrid`

No payload.


<a id="loupe-core-blocking-selection"></a>
## `loupe_core::blocking::Selection`

Source: [crates/loupe-core/src/blocking.rs:171](../crates/loupe-core/src/blocking.rs#L171).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `eligible_records` | integer (usize) | Always emitted; no field-level overrides. |
| `unavailable_records` | integer (usize) | Always emitted; no field-level overrides. |
| `candidates` | array of [loupe_core::blocking::Candidate](#loupe-core-blocking-candidate) | Always emitted; no field-level overrides. |

<a id="loupe-core-blocking-stats"></a>
## `loupe_core::blocking::Stats`

Source: [crates/loupe-core/src/blocking.rs:136](../crates/loupe-core/src/blocking.rs#L136).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `records` | integer (usize) | Always emitted; no field-level overrides. |
| `raw_bytes` | integer (u64) | Always emitted; no field-level overrides. |
| `distinct_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `token_postings` | integer (usize) | Always emitted; no field-level overrides. |
| `distinct_bands` | integer (usize) | Always emitted; no field-level overrides. |
| `band_postings` | integer (usize) | Always emitted; no field-level overrides. |
| `scratch_slots` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-budget-usage"></a>
## `loupe_core::budget::Usage`

Source: [crates/loupe-core/src/budget.rs:7](../crates/loupe-core/src/budget.rs#L7).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `spent_input` | integer (u64) | Always emitted; no field-level overrides. |
| `reserved_input` | integer (u64) | Always emitted; no field-level overrides. |
| `attempts` | integer (u64) | Always emitted; no field-level overrides. |
| `http_attempts` | integer (u64) | Always emitted; no field-level overrides. |
| `unknown_attempts` | integer (u64) | Always emitted; no field-level overrides. |
| `unknown_input` | integer (u64) | Always emitted; no field-level overrides. |
| `reported_input` | integer (u64) | Always emitted; no field-level overrides. |
| `reported_output` | integer (u64) | Always emitted; no field-level overrides. |
| `cache_hits` | integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-core-cache-entry"></a>
## `loupe_core::cache::Entry`

Source: [crates/loupe-core/src/cache.rs:67](../crates/loupe-core/src/cache.rs#L67).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u64) | Always emitted; no field-level overrides. |
| `created_at` | integer (u64) | Always emitted; no field-level overrides. |
| `model` | [loupe_core::identity::Model](#loupe-core-identity-model) | Always emitted; no field-level overrides. |
| `encoder_identity` | string | Always emitted; no field-level overrides. |
| `response` | [loupe_core::wire::Response](#loupe-core-wire-response) | Always emitted; no field-level overrides. |

<a id="loupe-core-cache-stats"></a>
## `loupe_core::cache::Stats`

Source: [crates/loupe-core/src/cache.rs:111](../crates/loupe-core/src/cache.rs#L111).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u64) | Always emitted; no field-level overrides. |
| `present` | boolean | Always emitted; no field-level overrides. |
| `entries` | integer (u64) | Always emitted; no field-level overrides. |
| `payload_bytes` | integer (u64) | Always emitted; no field-level overrides. |
| `database_bytes` | integer (u64) | Always emitted; no field-level overrides. |
| `oldest` | integer (u64) or null | Always emitted; no field-level overrides. |
| `newest` | integer (u64) or null | Always emitted; no field-level overrides. |
| `by_model` | object mapping string keys to integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-candidate"></a>
## `loupe_core::calibration::Candidate`

Source: [crates/loupe-core/src/calibration.rs:103](../crates/loupe-core/src/calibration.rs#L103).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `parameters` | [loupe_core::calibration::Parameters](#loupe-core-calibration-parameters) | Always emitted; no field-level overrides. |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `accepted` | integer (usize) | Always emitted; no field-level overrides. |
| `correct` | integer (usize) | Always emitted; no field-level overrides. |
| `coverage` | number (f64) | Always emitted; no field-level overrides. |
| `selective_accuracy` | number (f64) | Always emitted; no field-level overrides. |
| `targets` | array of [loupe_core::calibration::Support](#loupe-core-calibration-support) | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-fit"></a>
## `loupe_core::calibration::Fit`

Source: [crates/loupe-core/src/calibration.rs:114](../crates/loupe-core/src/calibration.rs#L114).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `candidates` | integer (usize) | Always emitted; no field-level overrides. |
| `feasible_candidates` | integer (usize) | Always emitted; no field-level overrides. |
| `selected` | [loupe_core::calibration::Candidate](#loupe-core-calibration-candidate) or null | Always emitted; no field-level overrides. |
| `objective` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-parameters"></a>
## `loupe_core::calibration::Parameters`

Source: [crates/loupe-core/src/calibration.rs:63](../crates/loupe-core/src/calibration.rs#L63).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `threshold` | number (f64) | Always emitted; no field-level overrides. |
| `margin` | number (f64) | Always emitted; no field-level overrides. |
| `min_confidence` | number (f64) | Always emitted; no field-level overrides. |
| `min_gap` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-support"></a>
## `loupe_core::calibration::Support`

Source: [crates/loupe-core/src/calibration.rs:93](../crates/loupe-core/src/calibration.rs#L93).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `label` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `minimum_precision` | number (f64) | Always emitted; no field-level overrides. |
| `predicted` | integer (usize) | Always emitted; no field-level overrides. |
| `correct` | integer (usize) | Always emitted; no field-level overrides. |
| `observed_precision` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-target"></a>
## `loupe_core::calibration::Target`

Source: [crates/loupe-core/src/calibration.rs:14](../crates/loupe-core/src/calibration.rs#L14).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `label` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `minimum_precision` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-calibration-pin-ensemble"></a>
## `loupe_core::calibration_pin::Ensemble`

Source: [crates/loupe-core/src/calibration_pin.rs:14](../crates/loupe-core/src/calibration_pin.rs#L14).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `min_agreement` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | `#[serde(deserialize_with = "deserialize_probability")]` |

<a id="loupe-core-calibration-pin-pin"></a>
## `loupe_core::calibration_pin::Pin`

Source: [crates/loupe-core/src/calibration_pin.rs:28](../crates/loupe-core/src/calibration_pin.rs#L28).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `model` | [loupe_core::identity::Model](#loupe-core-identity-model) | Always emitted; no field-level overrides. |
| `local_encoder` | string | Always emitted; no field-level overrides. |
| `server_encoder` | string | Always emitted; no field-level overrides. |
| `preset_version` | integer (u64) | Always emitted; no field-level overrides. |
| `preset_blake3` | string | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `view` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `source` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `ensemble` | [loupe_core::calibration_pin::Ensemble](#loupe-core-calibration-pin-ensemble) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-candidates-candidate"></a>
## `loupe_core::candidates::Candidate`

Source: [crates/loupe-core/src/candidates.rs:80](../crates/loupe-core/src/candidates.rs#L80).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `verbatim` | string | Always emitted; no field-level overrides. |
| `location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |
| `raw (storage only, skipped)` | string | `#[serde(skip)]` |
| `range (storage only, skipped)` | object {start, end}, each integer (usize) | `#[serde(skip)]` |

<a id="loupe-core-candidates-id"></a>
## `loupe_core::candidates::Id`

Source: [crates/loupe-core/src/candidates.rs:21](../crates/loupe-core/src/candidates.rs#L21).

Container attributes: `#[serde(try_from = "String", into = "String")]`.

JSON serialization converts the value into `String`. The fields below describe its Rust storage, rather than JSON object keys.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `family` | `loupe_core::candidates::Family` | Always emitted; no field-level overrides. |
| `number` | nonzero integer (NonZeroU64) | Always emitted; no field-level overrides. |

<a id="loupe-core-combine-mode"></a>
## `loupe_core::combine::Mode`

Source: [crates/loupe-core/src/combine.rs:13](../crates/loupe-core/src/combine.rs#L13).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `any`

No payload.

### `all`

No payload.

### `noisy-or`

No payload.

### `mean`

No payload.

### `max-confidence`

No payload.

### `vote`

No payload.

### `max`

No payload.

### `min`

No payload.


<a id="loupe-core-config-backendkind"></a>
## `loupe_core::config::BackendKind`

Source: [crates/loupe-core/src/config.rs:15](../crates/loupe-core/src/config.rs#L15).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `clef`

No payload.

### `jev`

No payload.


<a id="loupe-core-config-limits"></a>
## `loupe_core::config::Limits`

Source: [crates/loupe-core/src/config.rs:59](../crates/loupe-core/src/config.rs#L59).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `backend` | [loupe_core::config::BackendKind](#loupe-core-config-backendkind) | `#[serde(skip_serializing_if = "BackendKind::is_clef")]` |
| `max_length` | integer (usize) | Always emitted; no field-level overrides. |
| `max_state_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `max_questions` | integer (usize) | Always emitted; no field-level overrides. |
| `max_options` | integer (usize) | Always emitted; no field-level overrides. |
| `max_total_options` | integer (usize) | Always emitted; no field-level overrides. |
| `max_batch` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-cost-pricing"></a>
## `loupe_core::cost::Pricing`

Source: [crates/loupe-core/src/cost.rs:8](../crates/loupe-core/src/cost.rs#L8).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `input_usd_per_million` | number (f64) | Always emitted; no field-level overrides. |
| `output_usd_per_million` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-dedupe-cluster"></a>
## `loupe_core::dedupe::Cluster`

Source: [crates/loupe-core/src/dedupe.rs:38](../crates/loupe-core/src/dedupe.rs#L38).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | integer (u64) | Always emitted; no field-level overrides. |
| `representative` | integer (u64) | Always emitted; no field-level overrides. |
| `members` | array of integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-core-dedupe-keep"></a>
## `loupe_core::dedupe::Keep`

Source: [crates/loupe-core/src/dedupe.rs:12](../crates/loupe-core/src/dedupe.rs#L12).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `first`

No payload.

### `last`

No payload.

### `longest`

No payload.


<a id="loupe-core-dedupe-streaming-candidates"></a>
## `loupe_core::dedupe::streaming::Candidates`

Source: [crates/loupe-core/src/dedupe/streaming.rs:63](../crates/loupe-core/src/dedupe/streaming.rs#L63).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `indices` | array of integer (u64) | Always emitted; no field-level overrides. |
| `eligible` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-dedupe-streaming-horizon"></a>
## `loupe_core::dedupe::streaming::Horizon`

Source: [crates/loupe-core/src/dedupe/streaming.rs:14](../crates/loupe-core/src/dedupe/streaming.rs#L14).

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `Units`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |

### `Time`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `span` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `capacity` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |


<a id="loupe-core-dedupe-streaming-savedentry"></a>
## `loupe_core::dedupe::streaming::SavedEntry`

Source: [crates/loupe-core/src/dedupe/streaming.rs:23](../crates/loupe-core/src/dedupe/streaming.rs#L23).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `unit` | [loupe_core::input::Unit](#loupe-core-input-unit) | Always emitted; no field-level overrides. |
| `admitted` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |

<a id="loupe-core-dedupe-streaming-snapshot"></a>
## `loupe_core::dedupe::streaming::Snapshot`

Source: [crates/loupe-core/src/dedupe/streaming.rs:29](../crates/loupe-core/src/dedupe/streaming.rs#L29).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `horizon` | [loupe_core::dedupe::streaming::Horizon](#loupe-core-dedupe-streaming-horizon) | Always emitted; no field-level overrides. |
| `max_bytes` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `current` | integer (u64) | Always emitted; no field-level overrides. |
| `now` | object {secs: unsigned integer, nanos: unsigned integer} | Always emitted; no field-level overrides. |
| `expired` | integer (u64) | Always emitted; no field-level overrides. |
| `peak_count` | integer (usize) | Always emitted; no field-level overrides. |
| `peak_bytes` | integer (usize) | Always emitted; no field-level overrides. |
| `entries` | array of [loupe_core::dedupe::streaming::SavedEntry](#loupe-core-dedupe-streaming-savedentry) | Always emitted; no field-level overrides. |

<a id="loupe-core-dedupe-streaming-stats"></a>
## `loupe_core::dedupe::streaming::Stats`

Source: [crates/loupe-core/src/dedupe/streaming.rs:51](../crates/loupe-core/src/dedupe/streaming.rs#L51).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `representatives` | integer (usize) | Always emitted; no field-level overrides. |
| `serialized_bytes` | integer (usize) | Always emitted; no field-level overrides. |
| `distinct_bands` | integer (usize) | Always emitted; no field-level overrides. |
| `band_postings` | integer (usize) | Always emitted; no field-level overrides. |
| `capacity` | integer (usize) | Always emitted; no field-level overrides. |
| `byte_capacity` | integer (usize) | Always emitted; no field-level overrides. |
| `expired` | integer (u64) | Always emitted; no field-level overrides. |
| `peak_representatives` | integer (usize) | Always emitted; no field-level overrides. |
| `peak_serialized_bytes` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-encoder-counts"></a>
## `loupe_core::encoder::Counts`

Source: [crates/loupe-core/src/encoder.rs:31](../crates/loupe-core/src/encoder.rs#L31).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `state_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `question_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `input_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `exact` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-encoder-encodedquestion"></a>
## `loupe_core::encoder::EncodedQuestion`

Source: [crates/loupe-core/src/encoder.rs:15](../crates/loupe-core/src/encoder.rs#L15).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `question_id` | string | Always emitted; no field-level overrides. |
| `question_type` | integer (u8) | Always emitted; no field-level overrides. |
| `question_span` | tuple array (integer (usize), integer (usize)) | Always emitted; no field-level overrides. |
| `option_spans` | array of tuple array (integer (usize), integer (usize)) | Always emitted; no field-level overrides. |
| `option_ids` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-core-encoder-encodedrecord"></a>
## `loupe_core::encoder::EncodedRecord`

Source: [crates/loupe-core/src/encoder.rs:24](../crates/loupe-core/src/encoder.rs#L24).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `input_ids` | array of integer (u32) | Always emitted; no field-level overrides. |
| `questions` | array of [loupe_core::encoder::EncodedQuestion](#loupe-core-encoder-encodedquestion) | Always emitted; no field-level overrides. |

<a id="loupe-core-ensemble-member"></a>
## `loupe_core::ensemble::Member`

Source: [crates/loupe-core/src/ensemble.rs:59](../crates/loupe-core/src/ensemble.rs#L59).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `field` | string | Always emitted; no field-level overrides. |
| `answer` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `probabilities` | object mapping string keys to [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `vote` | [loupe_core::ensemble::Vote](#loupe-core-ensemble-vote) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-ensemble-report"></a>
## `loupe_core::ensemble::Report`

Source: [crates/loupe-core/src/ensemble.rs:67](../crates/loupe-core/src/ensemble.rs#L67).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `method` | string | Always emitted; no field-level overrides. |
| `independent` | boolean | Always emitted; no field-level overrides. |
| `calibrated` | boolean | Always emitted; no field-level overrides. |
| `model` | string | Always emitted; no field-level overrides. |
| `members` | array of [loupe_core::ensemble::Member](#loupe-core-ensemble-member) | Always emitted; no field-level overrides. |
| `mean_probabilities` | object mapping string keys to [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `mean_noul` | [loupe_core::policy::Probability](#loupe-core-policy-probability) or null | Always emitted; no field-level overrides. |
| `mean_score` | number (f64) or null | Always emitted; no field-level overrides. |
| `winning_option` | [loupe_core::ensemble::Vote](#loupe-core-ensemble-vote) or null | Always emitted; no field-level overrides. |
| `agreement` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `min_agreement` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `agreement_uncertain` | boolean | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |

<a id="loupe-core-ensemble-vote"></a>
## `loupe_core::ensemble::Vote`

Source: [crates/loupe-core/src/ensemble.rs:40](../crates/loupe-core/src/ensemble.rs#L40).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `boolean`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | boolean | Always emitted; no field-level overrides. |

### `choice`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

### `level`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | integer (usize) | Always emitted; no field-level overrides. |


<a id="loupe-core-extract-kind"></a>
## `loupe_core::extract::Kind`

Source: [crates/loupe-core/src/extract.rs:13](../crates/loupe-core/src/extract.rs#L13).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `dates`

No payload.

### `money`

No payload.

### `semver`

No payload.

### `emails`

No payload.

### `hostnames`

No payload.

### `tickers`

No payload.


<a id="loupe-core-factors-contribution"></a>
## `loupe_core::factors::Contribution`

Source: [crates/loupe-core/src/factors.rs:49](../crates/loupe-core/src/factors.rs#L49).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `field` | string | Always emitted; no field-level overrides. |
| `answer` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `weight` | number (f64) | Always emitted; no field-level overrides. |
| `normalized_weight` | number (f64) | Always emitted; no field-level overrides. |
| `invert` | boolean | Always emitted; no field-level overrides. |
| `value` | number (f64) | Always emitted; no field-level overrides. |
| `contribution` | number (f64) | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `raw_assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `ensemble` | arbitrary JSON value or null | `#[serde(skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-factors-report"></a>
## `loupe_core::factors::Report`

Source: [crates/loupe-core/src/factors.rs:65](../crates/loupe-core/src/factors.rs#L65).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `formula` | string | Always emitted; no field-level overrides. |
| `score` | number (f64) | Always emitted; no field-level overrides. |
| `factors` | object mapping string keys to [loupe_core::factors::Contribution](#loupe-core-factors-contribution) | Always emitted; no field-level overrides. |

<a id="loupe-core-fallback-execution"></a>
## `loupe_core::fallback::Execution`

Source: [crates/loupe-core/src/fallback.rs:28](../crates/loupe-core/src/fallback.rs#L28).

Container attributes: `#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]`.

Discriminator key: `status`. Variant fields merge into the same object.

### `resolved`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `selected` | [loupe_core::policy::Selected](#loupe-core-policy-selected) | Always emitted; no field-level overrides. |
| `outcome` | [loupe_core::trigger::Outcome](#loupe-core-trigger-outcome) | Always emitted; no field-level overrides. |

### `failed`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `kind` | [loupe_core::fallback::FailureKind](#loupe-core-fallback-failurekind) | Always emitted; no field-level overrides. |
| `message` | string | Always emitted; no field-level overrides. |
| `outcome` | [loupe_core::trigger::Outcome](#loupe-core-trigger-outcome) or null | Always emitted; no field-level overrides. |


<a id="loupe-core-fallback-failurekind"></a>
## `loupe_core::fallback::FailureKind`

Source: [crates/loupe-core/src/fallback.rs:18](../crates/loupe-core/src/fallback.rs#L18).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `invalid_output`

No payload.

### `exit`

No payload.

### `timeout`

No payload.

### `metadata`

No payload.

### `process`

No payload.


<a id="loupe-core-fallback-failurepolicy"></a>
## `loupe_core::fallback::FailurePolicy`

Source: [crates/loupe-core/src/fallback.rs:42](../crates/loupe-core/src/fallback.rs#L42).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `uncertain`

No payload.

### `fail`

No payload.


<a id="loupe-core-features-values"></a>
## `loupe_core::features::Values`

Source: [crates/loupe-core/src/features.rs:23](../crates/loupe-core/src/features.rs#L23).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `features` | object mapping string keys to number (f64) | Always emitted; no field-level overrides. |
| `answers` | object mapping string keys to arbitrary JSON value | Always emitted; no field-level overrides. |
| `bindings` | object mapping string keys to string | Always emitted; no field-level overrides. |
| `ensembles` | object mapping string keys to [loupe_core::ensemble::Report](#loupe-core-ensemble-report) | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |

<a id="loupe-core-focus-plan"></a>
## `loupe_core::focus::Plan`

Source: [crates/loupe-core/src/focus.rs:17](../crates/loupe-core/src/focus.rs#L17).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `operation` | string | Always emitted; no field-level overrides. |
| `path` | string | Always emitted; no field-level overrides. |
| `coordinate_space` | string | Always emitted; no field-level overrides. |
| `k` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `original_counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `relevance` | array of [loupe_core::split::Chunk](#loupe-core-split-chunk) | Always emitted; no field-level overrides. |
| `final_template` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `final_schema_counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `final_status` | string | Always emitted; no field-level overrides. |
| `text (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-focus-range"></a>
## `loupe_core::focus::Range`

Source: [crates/loupe-core/src/focus.rs:32](../crates/loupe-core/src/focus.rs#L32).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `byte_start` | integer (usize) | Always emitted; no field-level overrides. |
| `byte_end` | integer (usize) | Always emitted; no field-level overrides. |
| `token_start` | integer (usize) | Always emitted; no field-level overrides. |
| `token_end` | integer (usize) | Always emitted; no field-level overrides. |
| `line_start` | integer (u64) | Always emitted; no field-level overrides. |
| `line_end` | integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-core-focus-selection"></a>
## `loupe_core::focus::Selection`

Source: [crates/loupe-core/src/focus.rs:42](../crates/loupe-core/src/focus.rs#L42).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `selected_chunks` | array of integer (usize) | Always emitted; no field-level overrides. |
| `ranges` | array of [loupe_core::focus::Range](#loupe-core-focus-range) | Always emitted; no field-level overrides. |
| `separator` | string | Always emitted; no field-level overrides. |
| `lossy` | boolean | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |

<a id="loupe-core-identity-discovery"></a>
## `loupe_core::identity::Discovery`

Source: [crates/loupe-core/src/identity.rs:86](../crates/loupe-core/src/identity.rs#L86).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `model` | [loupe_core::identity::Model](#loupe-core-identity-model) | Always emitted; no field-level overrides. |
| `encoder_identity` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-identity-model"></a>
## `loupe_core::identity::Model`

Source: [crates/loupe-core/src/identity.rs:7](../crates/loupe-core/src/identity.rs#L7).

Container attributes: `#[serde(try_from = "String", into = "String")]`.

JSON serialization converts the value into `String`. The fields below describe its Rust storage, rather than JSON object keys.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-independent-decision"></a>
## `loupe_core::independent::Decision`

Source: [crates/loupe-core/src/independent.rs:59](../crates/loupe-core/src/independent.rs#L59).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `status` | [loupe_core::selection::Status](#loupe-core-selection-status) | Always emitted; no field-level overrides. |
| `gate` | [loupe_core::selection::Decision](#loupe-core-selection-decision) | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `ranking_score` | number (f64) | Always emitted; no field-level overrides. |
| `probabilities` | object mapping string keys to number (f64) | Always emitted; no field-level overrides. |
| `overrides` | object mapping string keys to [loupe_core::policy::Selected](#loupe-core-policy-selected) | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `ensemble` | arbitrary JSON value or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |
| `policy (storage only, skipped)` | string | `#[serde(skip)]` |
| `action (storage only, skipped)` | `loupe_core::policy::UncertainAction` | `#[serde(skip)]` |

<a id="loupe-core-independent-item"></a>
## `loupe_core::independent::Item`

Source: [crates/loupe-core/src/independent.rs:31](../crates/loupe-core/src/independent.rs#L31).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `bracket` | [loupe_core::tournament::Bracket](#loupe-core-tournament-bracket) | Always emitted; no field-level overrides. |

<a id="loupe-core-independent-outcome"></a>
## `loupe_core::independent::Outcome`

Source: [crates/loupe-core/src/independent.rs:108](../crates/loupe-core/src/independent.rs#L108).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `original_candidates` | integer (usize) | Always emitted; no field-level overrides. |
| `selected` | array of [loupe_core::independent::Selected](#loupe-core-independent-selected) | Always emitted; no field-level overrides. |
| `decisions` | array of [loupe_core::independent::Decision](#loupe-core-independent-decision) | Always emitted; no field-level overrides. |
| `probability_scope` | string | Always emitted; no field-level overrides. |
| `uncertain_candidates` | integer (usize) | Always emitted; no field-level overrides. |
| `exit_code` | integer (u8) | Always emitted; no field-level overrides. |

<a id="loupe-core-independent-plan"></a>
## `loupe_core::independent::Plan`

Source: [crates/loupe-core/src/independent.rs:51](../crates/loupe-core/src/independent.rs#L51).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `items` | array of [loupe_core::independent::Item](#loupe-core-independent-item) | Always emitted; no field-level overrides. |
| `criteria` | array of arbitrary JSON value | Always emitted; no field-level overrides. |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-independent-selected"></a>
## `loupe_core::independent::Selected`

Source: [crates/loupe-core/src/independent.rs:79](../crates/loupe-core/src/independent.rs#L79).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `request_index` | integer (usize) | Always emitted; no field-level overrides. |
| `ranking_score` | number (f64) | Always emitted; no field-level overrides. |
| `uncertain` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-input-location"></a>
## `loupe_core::input::Location`

Source: [crates/loupe-core/src/input.rs:17](../crates/loupe-core/src/input.rs#L17).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `source` | string (UTF-8 filesystem path) or null | Always emitted; no field-level overrides. |
| `byte_start` | integer (u64) | Always emitted; no field-level overrides. |
| `byte_end` | integer (u64) | Always emitted; no field-level overrides. |
| `line_start` | integer (u64) | Always emitted; no field-level overrides. |
| `line_end` | integer (u64) | Always emitted; no field-level overrides. |

<a id="loupe-core-input-unit"></a>
## `loupe_core::input::Unit`

Source: [crates/loupe-core/src/input.rs:27](../crates/loupe-core/src/input.rs#L27).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `text` | string | Always emitted; no field-level overrides. |
| `raw` | array of integer (u8) | Always emitted; no field-level overrides. |
| `subject` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `state` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `header` | array of integer (u8) or null | Always emitted; no field-level overrides. |
| `location` | [loupe_core::input::Location](#loupe-core-input-location) | Always emitted; no field-level overrides. |

<a id="loupe-core-matching-cardinality"></a>
## `loupe_core::matching::Cardinality`

Source: [crates/loupe-core/src/matching.rs:22](../crates/loupe-core/src/matching.rs#L22).

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `1:1`

No payload.

### `1:n`

No payload.

### `n:m`

No payload.


<a id="loupe-core-matching-decision"></a>
## `loupe_core::matching::Decision`

Source: [crates/loupe-core/src/matching.rs:70](../crates/loupe-core/src/matching.rs#L70).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left_index` | integer (u64) | Always emitted; no field-level overrides. |
| `right_index` | integer (u64) | Always emitted; no field-level overrides. |
| `gate` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `score` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `ranking_score` | number (f64) | Always emitted; no field-level overrides. |
| `probabilities` | object mapping string keys to number (f64) | Always emitted; no field-level overrides. |
| `companions` | object mapping string keys to [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `overrides` | object mapping string keys to [loupe_core::policy::Selected](#loupe-core-policy-selected) | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `ensembles` | object mapping string keys to arbitrary JSON value | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `derived` | object mapping string keys to [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `uncertain` | boolean | Always emitted; no field-level overrides. |
| `eligible` | boolean | Always emitted; no field-level overrides. |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-matching-plan"></a>
## `loupe_core::matching::Plan`

Source: [crates/loupe-core/src/matching.rs:50](../crates/loupe-core/src/matching.rs#L50).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left_index` | integer (u64) | Always emitted; no field-level overrides. |
| `right_index` | integer (u64) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-normalize-dateorder"></a>
## `loupe_core::normalize::DateOrder`

Source: [crates/loupe-core/src/normalize.rs:35](../crates/loupe-core/src/normalize.rs#L35).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `mdy`

No payload.

### `dmy`

No payload.


<a id="loupe-core-normalize-normalized"></a>
## `loupe_core::normalize::Normalized`

Source: [crates/loupe-core/src/normalize.rs:68](../crates/loupe-core/src/normalize.rs#L68).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `value` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `text` | string | Always emitted; no field-level overrides. |
| `precision` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-normalize-options"></a>
## `loupe_core::normalize::Options`

Source: [crates/loupe-core/src/normalize.rs:18](../crates/loupe-core/src/normalize.rs#L18).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `now` | string or null | Always emitted; no field-level overrides. |
| `tz` | string or null | Always emitted; no field-level overrides. |
| `currency` | string or null | Always emitted; no field-level overrides. |
| `decimal_separator` | [loupe_core::normalize::Separator](#loupe-core-normalize-separator) or null | Always emitted; no field-level overrides. |
| `date_order` | [loupe_core::normalize::DateOrder](#loupe-core-normalize-dateorder) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-normalize-separator"></a>
## `loupe_core::normalize::Separator`

Source: [crates/loupe-core/src/normalize.rs:28](../crates/loupe-core/src/normalize.rs#L28).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `dot`

No payload.

### `comma`

No payload.


<a id="loupe-core-policy-assessment"></a>
## `loupe_core::policy::Assessment`

Source: [crates/loupe-core/src/policy.rs:55](../crates/loupe-core/src/policy.rs#L55).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `selected` | [loupe_core::policy::Selected](#loupe-core-policy-selected) | Always emitted; no field-level overrides. |
| `confidence` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `gap` | number (f64) or null | Always emitted; no field-level overrides. |
| `uncertain` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-policy-destination"></a>
## `loupe_core::policy::Destination`

Source: [crates/loupe-core/src/policy.rs:79](../crates/loupe-core/src/policy.rs#L79).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `stdout`

No payload.

### `stderr`

No payload.

### `omit`

No payload.


<a id="loupe-core-policy-probability"></a>
## `loupe_core::policy::Probability`

Source: [crates/loupe-core/src/policy.rs:8](../crates/loupe-core/src/policy.rs#L8).

Container attributes: `#[serde(transparent)]`.

A tuple array, unless the container's `transparent` attribute emits its single inner value.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-policy-selected"></a>
## `loupe_core::policy::Selected`

Source: [crates/loupe-core/src/policy.rs:48](../crates/loupe-core/src/policy.rs#L48).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `boolean`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | boolean | Always emitted; no field-level overrides. |

### `choice`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

### `score`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | number (f64) | Always emitted; no field-level overrides. |


<a id="loupe-core-preset-store-grants"></a>
## `loupe_core::preset_store::Grants`

Source: [crates/loupe-core/src/preset_store.rs:247](../crates/loupe-core/src/preset_store.rs#L247).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u32) | Always emitted; no field-level overrides. |
| `grants` | object mapping string (UTF-8 filesystem path) keys to string | Always emitted; no field-level overrides. |

<a id="loupe-core-preset-store-origin"></a>
## `loupe_core::preset_store::Origin`

Source: [crates/loupe-core/src/preset_store.rs:18](../crates/loupe-core/src/preset_store.rs#L18).

Container attributes: `#[serde(tag = "kind", content = "path", rename_all = "lowercase")]`.

Discriminator key: `kind`. Payload key: `path`.

### `builtin`

No payload.

### `project`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |

### `xdg`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |

### `external`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |

### `explicit`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |


<a id="loupe-core-preset-store-preset"></a>
## `loupe_core::preset_store::Preset`

Source: [crates/loupe-core/src/preset_store.rs:41](../crates/loupe-core/src/preset_store.rs#L41).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | string | Always emitted; no field-level overrides. |
| `document` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `origin` | [loupe_core::preset_store::Origin](#loupe-core-preset-store-origin) | Always emitted; no field-level overrides. |
| `trusted` | boolean | Always emitted; no field-level overrides. |
| `content_hash` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-process-capture"></a>
## `loupe_core::process::Capture`

Source: [crates/loupe-core/src/process.rs:15](../crates/loupe-core/src/process.rs#L15).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `bytes` | array of integer (u8) | Always emitted; no field-level overrides. |
| `total_bytes` | integer (u64) | Always emitted; no field-level overrides. |
| `truncated` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-records-format"></a>
## `loupe_core::records::Format`

Source: [crates/loupe-core/src/records.rs:15](../crates/loupe-core/src/records.rs#L15).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `lines`

No payload.

### `whole`

No payload.

### `null`

No payload.

### `paragraphs`

No payload.

### `records`

No payload.

### `jsonl`

No payload.

### `csv`

No payload.

### `tsv`

No payload.

### `files`

No payload.


<a id="loupe-core-records-options"></a>
## `loupe_core::records::Options`

Source: [crates/loupe-core/src/records.rs:28](../crates/loupe-core/src/records.rs#L28).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `format` | [loupe_core::records::Format](#loupe-core-records-format) | Always emitted; no field-level overrides. |
| `keep_blank` | boolean | Always emitted; no field-level overrides. |
| `records` | string or null | Always emitted; no field-level overrides. |
| `field` | string or null | Always emitted; no field-level overrides. |
| `select` | array of string | Always emitted; no field-level overrides. |
| `with_context` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-recovery-completion"></a>
## `loupe_core::recovery::Completion`

Source: [crates/loupe-core/src/recovery.rs:32](../crates/loupe-core/src/recovery.rs#L32).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `completed` | arbitrary JSON value | Always emitted; no field-level overrides. |

<a id="loupe-core-recovery-disposition"></a>
## `loupe_core::recovery::Disposition`

Source: [crates/loupe-core/src/recovery.rs:26](../crates/loupe-core/src/recovery.rs#L26).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `emitted`

No payload.

### `omitted`

No payload.


<a id="loupe-core-recovery-entry"></a>
## `loupe_core::recovery::Entry`

Source: [crates/loupe-core/src/recovery.rs:110](../crates/loupe-core/src/recovery.rs#L110).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `admit` | integer (u64) | Always emitted; no field-level overrides. |
| `plan` | integer (u64) or null | Always emitted; no field-level overrides. |
| `ready` | integer (u64) or null | Always emitted; no field-level overrides. |
| `fallback` | integer (u64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `review` | integer (u64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `failed` | integer (u64) or null | Always emitted; no field-level overrides. |
| `handled` | [loupe_core::recovery::Disposition](#loupe-core-recovery-disposition) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-recovery-frame"></a>
## `loupe_core::recovery::Frame`

Source: [crates/loupe-core/src/recovery.rs:93](../crates/loupe-core/src/recovery.rs#L93).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u64) | Always emitted; no field-level overrides. |
| `sequence` | integer (u64) | Always emitted; no field-level overrides. |
| `previous` | string | Always emitted; no field-level overrides. |
| `record` | [loupe_core::recovery::Record](#loupe-core-recovery-record) | Always emitted; no field-level overrides. |
| `checksum` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-recovery-record"></a>
## `loupe_core::recovery::Record`

Source: [crates/loupe-core/src/recovery.rs:39](../crates/loupe-core/src/recovery.rs#L39).

Container attributes: `#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]`.

Discriminator key: `record`. Variant fields merge into the same object.

### `begin`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `identity` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `admit`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `unit` | [loupe_core::input::Unit](#loupe-core-input-unit) | Always emitted; no field-level overrides. |

### `plan`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `indices` | array of integer (u64) | Always emitted; no field-level overrides. |
| `provenance` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `ready`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `completed` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `ready_many`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `completions` | array of [loupe_core::recovery::Completion](#loupe-core-recovery-completion) | Always emitted; no field-level overrides. |

### `fallback`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `execution` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `review_run`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `seed` | string | Always emitted; no field-level overrides. |

### `review`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `decision` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `failed`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) or null | Always emitted; no field-level overrides. |
| `code` | integer (u8) | Always emitted; no field-level overrides. |
| `message` | string | Always emitted; no field-level overrides. |

### `handled`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `disposition` | [loupe_core::recovery::Disposition](#loupe-core-recovery-disposition) | Always emitted; no field-level overrides. |

### `input_end`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `fingerprint` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `output_checkpoint`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `namespace` | string | Always emitted; no field-level overrides. |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `emitted` | integer (u64) | Always emitted; no field-level overrides. |

### `finish`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `emitted` | integer (u64) | Always emitted; no field-level overrides. |
| `exit_code` | integer (u8) | Always emitted; no field-level overrides. |


<a id="loupe-core-review-frame"></a>
## `loupe_core::review::Frame`

Source: [crates/loupe-core/src/review.rs:113](../crates/loupe-core/src/review.rs#L113).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u32) | Always emitted; no field-level overrides. |
| `record` | [loupe_core::review::Record](#loupe-core-review-record) | Always emitted; no field-level overrides. |
| `checksum` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-review-record"></a>
## `loupe_core::review::Record`

Source: [crates/loupe-core/src/review.rs:106](../crates/loupe-core/src/review.rs#L106).

Container attributes: `#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]`.

Discriminator key: `record`. Variant fields merge into the same object.

### `task`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `task` | [loupe_core::review::Task](#loupe-core-review-task) | Always emitted; no field-level overrides. |

### `decision`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |
| `label` | [loupe_core::policy::Selected](#loupe-core-policy-selected) or null | Always emitted; no field-level overrides. |

### `handled`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |


<a id="loupe-core-review-task"></a>
## `loupe_core::review::Task`

Source: [crates/loupe-core/src/review.rs:23](../crates/loupe-core/src/review.rs#L23).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |
| `run` | string | Always emitted; no field-level overrides. |
| `preset` | string | Always emitted; no field-level overrides. |
| `unit` | [loupe_core::input::Unit](#loupe-core-input-unit) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `response` | [loupe_core::wire::Response](#loupe-core-wire-response) | Always emitted; no field-level overrides. |
| `field` | string | Always emitted; no field-level overrides. |
| `encoder_identity` | string | Always emitted; no field-level overrides. |
| `sampling` | [loupe_core::suites::Sampling](#loupe-core-suites-sampling) | Always emitted; no field-level overrides. |

<a id="loupe-core-route-dir-artifact"></a>
## `loupe_core::route_dir::Artifact`

Source: [crates/loupe-core/src/route_dir.rs:14](../crates/loupe-core/src/route_dir.rs#L14).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `journal`

No payload.

### `manifest`

No payload.

### `manifest_temp`

No payload.

### `reset`

No payload.

### `reset_temp`

No payload.


<a id="loupe-core-route-labels-labels"></a>
## `loupe_core::route_labels::Labels`

Source: [crates/loupe-core/src/route_labels.rs:43](../crates/loupe-core/src/route_labels.rs#L43).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `names` | object mapping string keys to [loupe_core::route_labels::Name](#loupe-core-route-labels-name) | Always emitted; no field-level overrides. |

<a id="loupe-core-route-labels-name"></a>
## `loupe_core::route_labels::Name`

Source: [crates/loupe-core/src/route_labels.rs:9](../crates/loupe-core/src/route_labels.rs#L9).

Container attributes: `#[serde(try_from = "String", into = "String")]`.

JSON serialization converts the value into `String`. The fields below describe its Rust storage, rather than JSON object keys.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-log-append"></a>
## `loupe_core::route_log::Append`

Source: [crates/loupe-core/src/route_log.rs:23](../crates/loupe-core/src/route_log.rs#L23).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `run` | integer (u64) | Always emitted; no field-level overrides. |
| `sequence` | integer (u64) | Always emitted; no field-level overrides. |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `unit_hash` | string | Always emitted; no field-level overrides. |
| `prefix_hash` | string | Always emitted; no field-level overrides. |
| `label` | string | Always emitted; no field-level overrides. |
| `target` | [loupe_core::route_log::Target](#loupe-core-route-log-target) or null | Always emitted; no field-level overrides. |
| `payload` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-log-envelope"></a>
## `loupe_core::route_log::Envelope`

Source: [crates/loupe-core/src/route_log.rs:131](../crates/loupe-core/src/route_log.rs#L131).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u64) | Always emitted; no field-level overrides. |
| `record` | [loupe_core::route_log::Record](#loupe-core-route-log-record) | Always emitted; no field-level overrides. |
| `checksum` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-log-record"></a>
## `loupe_core::route_log::Record`

Source: [crates/loupe-core/src/route_log.rs:36](../crates/loupe-core/src/route_log.rs#L36).

Container attributes: `#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]`.

Discriminator key: `record`. Variant fields merge into the same object.

### `begin`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `run` | integer (u64) | Always emitted; no field-level overrides. |
| `identity` | string | Always emitted; no field-level overrides. |
| `labels` | array of string or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

### `prepare`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `append` | [loupe_core::route_log::Append](#loupe-core-route-log-append) | Always emitted; no field-level overrides. |

### `commit`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `run` | integer (u64) | Always emitted; no field-level overrides. |
| `sequence` | integer (u64) | Always emitted; no field-level overrides. |
| `prepare_hash` | string | Always emitted; no field-level overrides. |

### `finish`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `run` | integer (u64) | Always emitted; no field-level overrides. |
| `sequence` | integer (u64) | Always emitted; no field-level overrides. |
| `prefix_hash` | string | Always emitted; no field-level overrides. |


<a id="loupe-core-route-log-target"></a>
## `loupe_core::route_log::Target`

Source: [crates/loupe-core/src/route_log.rs:13](../crates/loupe-core/src/route_log.rs#L13).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | [loupe_core::route_labels::Name](#loupe-core-route-labels-name) | Always emitted; no field-level overrides. |
| `device` | integer (u64) | Always emitted; no field-level overrides. |
| `inode` | integer (u64) | Always emitted; no field-level overrides. |
| `start` | integer (u64) | Always emitted; no field-level overrides. |
| `before_hash` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-reset-clear"></a>
## `loupe_core::route_reset::Clear`

Source: [crates/loupe-core/src/route_reset.rs:37](../crates/loupe-core/src/route_reset.rs#L37).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `entry` | [loupe_core::route_reset::Entry](#loupe-core-route-reset-entry) | Always emitted; no field-level overrides. |
| `device` | integer (u64) | Always emitted; no field-level overrides. |
| `inode` | integer (u64) | Always emitted; no field-level overrides. |
| `bytes` | integer (u64) | Always emitted; no field-level overrides. |
| `hash` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-reset-entry"></a>
## `loupe_core::route_reset::Entry`

Source: [crates/loupe-core/src/route_reset.rs:14](../crates/loupe-core/src/route_reset.rs#L14).

Container attributes: `#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]`.

Discriminator key: `kind`. Variant fields merge into the same object.

### `label`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `name` | [loupe_core::route_labels::Name](#loupe-core-route-labels-name) | Always emitted; no field-level overrides. |

### `artifact`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `artifact` | [loupe_core::route_dir::Artifact](#loupe-core-route-dir-artifact) | Always emitted; no field-level overrides. |


<a id="loupe-core-route-reset-envelope"></a>
## `loupe_core::route_reset::Envelope`

Source: [crates/loupe-core/src/route_reset.rs:52](../crates/loupe-core/src/route_reset.rs#L52).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `intent` | [loupe_core::route_reset::Intent](#loupe-core-route-reset-intent) | Always emitted; no field-level overrides. |
| `checksum` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-route-reset-intent"></a>
## `loupe_core::route_reset::Intent`

Source: [crates/loupe-core/src/route_reset.rs:46](../crates/loupe-core/src/route_reset.rs#L46).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `version` | integer (u64) | Always emitted; no field-level overrides. |
| `files` | array of [loupe_core::route_reset::Clear](#loupe-core-route-reset-clear) | Always emitted; no field-level overrides. |

<a id="loupe-core-route-store-manifest"></a>
## `loupe_core::route_store::Manifest`

Source: [crates/loupe-core/src/route_store.rs:713](../crates/loupe-core/src/route_store.rs#L713).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `run` | integer (u64) | Always emitted; no field-level overrides. |
| `sequence` | integer (u64) | Always emitted; no field-level overrides. |
| `index` | integer (u64) | Always emitted; no field-level overrides. |
| `label` | string | Always emitted; no field-level overrides. |
| `unit_hash` | string | Always emitted; no field-level overrides. |
| `prefix_hash` | string | Always emitted; no field-level overrides. |
| `target` | [loupe_core::route_log::Target](#loupe-core-route-log-target) or null | Always emitted; no field-level overrides. |
| `payload_bytes` | integer (usize) | Always emitted; no field-level overrides. |
| `payload_hash` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-selection-decision"></a>
## `loupe_core::selection::Decision`

Source: [crates/loupe-core/src/selection.rs:39](../crates/loupe-core/src/selection.rs#L39).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `status` | [loupe_core::selection::Status](#loupe-core-selection-status) | Always emitted; no field-level overrides. |
| `reason` | [loupe_core::selection::Reason](#loupe-core-selection-reason) | Always emitted; no field-level overrides. |
| `presence_probability` | number (f64) | Always emitted; no field-level overrides. |
| `presence` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `choice` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `ranking` | [loupe_core::tournament::Ranking](#loupe-core-tournament-ranking) | Always emitted; no field-level overrides. |
| `overrides` | object mapping string keys to [loupe_core::policy::Selected](#loupe-core-policy-selected) | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `ensembles` | object mapping string keys to arbitrary JSON value | `#[serde(skip_serializing_if = "IndexMap::is_empty")]` |
| `policy (storage only, skipped)` | array[8] of number (f64) | `#[serde(skip)]` |
| `action (storage only, skipped)` | `loupe_core::policy::UncertainAction` | `#[serde(skip)]` |

<a id="loupe-core-selection-outcome"></a>
## `loupe_core::selection::Outcome`

Source: [crates/loupe-core/src/selection.rs:344](../crates/loupe-core/src/selection.rs#L344).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `tournament` | [loupe_core::tournament::Final](#loupe-core-tournament-final) | Always emitted; no field-level overrides. |
| `status` | [loupe_core::selection::Status](#loupe-core-selection-status) | Always emitted; no field-level overrides. |
| `destination` | [loupe_core::policy::Destination](#loupe-core-policy-destination) | Always emitted; no field-level overrides. |
| `exit_code` | integer (u8) | Always emitted; no field-level overrides. |
| `uncertain_brackets` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-selection-reason"></a>
## `loupe_core::selection::Reason`

Source: [crates/loupe-core/src/selection.rs:30](../crates/loupe-core/src/selection.rs#L30).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `candidate`

No payload.

### `presence_no`

No payload.

### `none_wins`

No payload.

### `presence_uncertain`

No payload.

### `choice_uncertain`

No payload.


<a id="loupe-core-selection-status"></a>
## `loupe_core::selection::Status`

Source: [crates/loupe-core/src/selection.rs:22](../crates/loupe-core/src/selection.rs#L22).

Container attributes: `#[serde(rename_all = "snake_case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `selected`

No payload.

### `absent`

No payload.

### `uncertain`

No payload.


<a id="loupe-core-selection-step"></a>
## `loupe_core::selection::Step`

Source: [crates/loupe-core/src/selection.rs:354](../crates/loupe-core/src/selection.rs#L354).

Container attributes: `#[serde(tag = "stage", rename_all = "lowercase")]`.

Discriminator key: `stage`. Variant fields merge into the same object.

### `next`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `round` | [loupe_core::tournament::Round](#loupe-core-tournament-round) | Always emitted; no field-level overrides. |

### `final`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `outcome` | [loupe_core::selection::Outcome](#loupe-core-selection-outcome) | Always emitted; no field-level overrides. |


<a id="loupe-core-selection-ensemble-ensemble"></a>
## `loupe_core::selection_ensemble::Ensemble`

Source: [crates/loupe-core/src/selection_ensemble.rs:21](../crates/loupe-core/src/selection_ensemble.rs#L21).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `variants` | array of [loupe_core::selection_ensemble::Variant](#loupe-core-selection-ensemble-variant) | Always emitted; no field-level overrides. |
| `min_agreement` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |

<a id="loupe-core-selection-ensemble-variant"></a>
## `loupe_core::selection_ensemble::Variant`

Source: [crates/loupe-core/src/selection_ensemble.rs:13](../crates/loupe-core/src/selection_ensemble.rs#L13).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |
| `query` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `presence_criteria` | object mapping string keys to arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `score_criteria` | array of arbitrary JSON value or null | Always emitted; no field-level overrides. |

<a id="loupe-core-signals-id"></a>
## `loupe_core::signals::Id`

Source: [crates/loupe-core/src/signals.rs:14](../crates/loupe-core/src/signals.rs#L14).

Container attributes: `#[serde(try_from = "String", into = "String")]`.

JSON serialization converts the value into `String`. The fields below describe its Rust storage, rather than JSON object keys.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-signals-report"></a>
## `loupe_core::signals::Report`

Source: [crates/loupe-core/src/signals.rs:94](../crates/loupe-core/src/signals.rs#L94).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `field` | string | Always emitted; no field-level overrides. |
| `answer` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `probability` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `assessment` | [loupe_core::policy::Assessment](#loupe-core-policy-assessment) | Always emitted; no field-level overrides. |
| `threshold` | [loupe_core::policy::Probability](#loupe-core-policy-probability) | Always emitted; no field-level overrides. |
| `margin` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-split-chunk"></a>
## `loupe_core::split::Chunk`

Source: [crates/loupe-core/src/split.rs:39](../crates/loupe-core/src/split.rs#L39).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `range` | [loupe_core::split::Range](#loupe-core-split-range) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |

<a id="loupe-core-split-range"></a>
## `loupe_core::split::Range`

Source: [crates/loupe-core/src/split.rs:28](../crates/loupe-core/src/split.rs#L28).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `byte_start` | integer (usize) | Always emitted; no field-level overrides. |
| `byte_end` | integer (usize) | Always emitted; no field-level overrides. |
| `token_start` | integer (usize) | Always emitted; no field-level overrides. |
| `token_end` | integer (usize) | Always emitted; no field-level overrides. |
| `line_start` | integer (u64) | Always emitted; no field-level overrides. |
| `line_end` | integer (u64) | Always emitted; no field-level overrides. |
| `text_tokens` | integer (usize) | Always emitted; no field-level overrides. |
| `unique_tokens` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-case"></a>
## `loupe_core::suites::Case`

Source: [crates/loupe-core/src/suites.rs:102](../crates/loupe-core/src/suites.rs#L102).

Container attributes: `#[serde(try_from = "RawCase")]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | string | Always emitted; no field-level overrides. |
| `input` | [loupe_core::suites::Input](#loupe-core-suites-input) | `#[serde(flatten)]` |
| `label` | [loupe_core::suites::Label](#loupe-core-suites-label) | Always emitted; no field-level overrides. |
| `query` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `candidates` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `context` | arbitrary JSON value or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `tags` | array of string | Always emitted; no field-level overrides. |
| `source` | string | Always emitted; no field-level overrides. |
| `reviewed` | boolean | Always emitted; no field-level overrides. |
| `sampling` | [loupe_core::suites::Sampling](#loupe-core-suites-sampling) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `notes` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `reference` | [loupe_core::suites::Reference](#loupe-core-suites-reference) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `expected_spans` | array of [loupe_core::suites::Span](#loupe-core-suites-span) | `#[serde(default, skip_serializing_if = "Vec::is_empty")]` |

<a id="loupe-core-suites-gate"></a>
## `loupe_core::suites::Gate`

Source: [crates/loupe-core/src/suites.rs:278](../crates/loupe-core/src/suites.rs#L278).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `metric` | [loupe_core::suites::Metric](#loupe-core-suites-metric) | Always emitted; no field-level overrides. |
| `minimum` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `maximum` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `max_drop` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `max_increase` | number (f64) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-suites-input"></a>
## `loupe_core::suites::Input`

Source: [crates/loupe-core/src/suites.rs:79](../crates/loupe-core/src/suites.rs#L79).

Container attributes: `#[serde(untagged)]`.

No discriminator or variant wrapper; each variant emits its payload directly.

### `Unit`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `unit` | arbitrary JSON value | Always emitted; no field-level overrides. |

### `Document`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `doc` | string | Always emitted; no field-level overrides. |

### `Pair`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `left` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `right` | arbitrary JSON value | Always emitted; no field-level overrides. |


<a id="loupe-core-suites-join"></a>
## `loupe_core::suites::Join`

Source: [crates/loupe-core/src/suites.rs:355](../crates/loupe-core/src/suites.rs#L355).

Container attributes: `#[serde(deny_unknown_fields)]`.

A complete labeled join catalog is one independently split scenario.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `fields` | array of string | Always emitted; no field-level overrides. |
| `blocking` | [loupe_core::blocking::Mode](#loupe-core-blocking-mode) | `#[serde(default)]` |
| `max_candidates` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `max_pairs` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `cardinalities` | array of [loupe_core::matching::Cardinality](#loupe-core-matching-cardinality) | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-label"></a>
## `loupe_core::suites::Label`

Source: [crates/loupe-core/src/suites.rs:27](../crates/loupe-core/src/suites.rs#L27).

Container attributes: `#[serde(untagged)]`.

No discriminator or variant wrapper; each variant emits its payload directly.

### `Noul`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | boolean | Always emitted; no field-level overrides. |

### `Level`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | integer (u32) | Always emitted; no field-level overrides. |

### `Interval`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | array[2] of integer (u32) | Always emitted; no field-level overrides. |

### `Choice`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

### `Ids`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | array of string | Always emitted; no field-level overrides. |

### `Fields`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | object mapping string keys to [loupe_core::suites::Label](#loupe-core-suites-label) | Always emitted; no field-level overrides. |


<a id="loupe-core-suites-manifest"></a>
## `loupe_core::suites::Manifest`

Source: [crates/loupe-core/src/suites.rs:305](../crates/loupe-core/src/suites.rs#L305).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `id` | string | Always emitted; no field-level overrides. |
| `revision` | string | Always emitted; no field-level overrides. |
| `preset` | string | Always emitted; no field-level overrides. |
| `task` | [loupe_core::suites::Task](#loupe-core-suites-task) | Always emitted; no field-level overrides. |
| `cases` | [loupe_core::suites::Pin](#loupe-core-suites-pin) | Always emitted; no field-level overrides. |
| `splits` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |
| `split` | [loupe_core::suites::SplitConfig](#loupe-core-suites-splitconfig) | Always emitted; no field-level overrides. |
| `provenance` | [loupe_core::suites::Provenance](#loupe-core-suites-provenance) | Always emitted; no field-level overrides. |
| `metrics` | array of [loupe_core::suites::Metric](#loupe-core-suites-metric) | `#[serde(default)]` |
| `gates` | array of [loupe_core::suites::Gate](#loupe-core-suites-gate) | `#[serde(default)]` |
| `fixtures` | array of [loupe_core::suites::Pin](#loupe-core-suites-pin) | `#[serde(default)]` |
| `join` | [loupe_core::suites::Join](#loupe-core-suites-join) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `selection` | [loupe_core::suites::Selection](#loupe-core-suites-selection) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-suites-metric"></a>
## `loupe_core::suites::Metric`

Source: [crates/loupe-core/src/suites.rs:256](../crates/loupe-core/src/suites.rs#L256).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `accuracy`

No payload.

### `macro-f1`

No payload.

### `auroc`

No payload.

### `f1`

No payload.

### `brier`

No payload.

### `ece`

No payload.

### `coverage`

No payload.

### `selective-accuracy`

No payload.

### `mae`

No payload.

### `within-one-accuracy`

No payload.

### `probability-max-error`

No payload.

### `probability-mean-error`

No payload.

### `argmax-agreement`

No payload.

### `latency-p50-ms`

No payload.

### `latency-p95-ms`

No payload.

### `units-per-second`

No payload.

### `blocking-recall`

No payload.

### `assignment-accuracy`

No payload.


<a id="loupe-core-suites-pin"></a>
## `loupe_core::suites::Pin`

Source: [crates/loupe-core/src/suites.rs:217](../crates/loupe-core/src/suites.rs#L217).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `path` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |
| `revision` | string | Always emitted; no field-level overrides. |
| `blake3` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-provenance"></a>
## `loupe_core::suites::Provenance`

Source: [crates/loupe-core/src/suites.rs:244](../crates/loupe-core/src/suites.rs#L244).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `source` | string | Always emitted; no field-level overrides. |
| `license` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-reference"></a>
## `loupe_core::suites::Reference`

Source: [crates/loupe-core/src/suites.rs:94](../crates/loupe-core/src/suites.rs#L94).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `response` | [loupe_core::wire::Response](#loupe-core-wire-response) | Always emitted; no field-level overrides. |
| `encoder_identity` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-sampling"></a>
## `loupe_core::suites::Sampling`

Source: [crates/loupe-core/src/suites.rs:87](../crates/loupe-core/src/suites.rs#L87).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `uncertainty`

No payload.

### `uniform-audit`

No payload.


<a id="loupe-core-suites-selection"></a>
## `loupe_core::suites::Selection`

Source: [crates/loupe-core/src/suites.rs:339](../crates/loupe-core/src/suites.rs#L339).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `mode` | [loupe_core::suites::SelectionMode](#loupe-core-suites-selectionmode) | Always emitted; no field-level overrides. |
| `take` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `max_rounds` | nonzero integer (NonZeroUsize) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `context_lines` | integer (usize) | `#[serde(default)]` |
| `keep_blank` | boolean | `#[serde(default)]` |
| `normalize` | [loupe_core::normalize::Options](#loupe-core-normalize-options) or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-suites-selectionmode"></a>
## `loupe_core::suites::SelectionMode`

Source: [crates/loupe-core/src/suites.rs:329](../crates/loupe-core/src/suites.rs#L329).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `find`

No payload.

### `pick`

No payload.

### `independent`

No payload.

### `extract`

No payload.


<a id="loupe-core-suites-span"></a>
## `loupe_core::suites::Span`

Source: [crates/loupe-core/src/suites.rs:128](../crates/loupe-core/src/suites.rs#L128).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `text` | string | Always emitted; no field-level overrides. |
| `byte_start` | integer (u64) | Always emitted; no field-level overrides. |
| `byte_end` | integer (u64) | Always emitted; no field-level overrides. |
| `line_start` | integer (u64) | Always emitted; no field-level overrides. |
| `line_end` | integer (u64) | Always emitted; no field-level overrides. |
| `normalized` | arbitrary JSON value or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-suites-splitconfig"></a>
## `loupe_core::suites::SplitConfig`

Source: [crates/loupe-core/src/suites.rs:250](../crates/loupe-core/src/suites.rs#L250).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `seed` | integer (u64) | Always emitted; no field-level overrides. |
| `calibrate_percent` | integer (u8) | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-splits"></a>
## `loupe_core::suites::Splits`

Source: [crates/loupe-core/src/suites.rs:366](../crates/loupe-core/src/suites.rs#L366).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `schema_version` | integer (u32) | Always emitted; no field-level overrides. |
| `suite_id` | string | Always emitted; no field-level overrides. |
| `suite_revision` | string | Always emitted; no field-level overrides. |
| `dataset_revision` | string | Always emitted; no field-level overrides. |
| `cases_blake3` | string | Always emitted; no field-level overrides. |
| `config` | [loupe_core::suites::SplitConfig](#loupe-core-suites-splitconfig) | Always emitted; no field-level overrides. |
| `calibrate` | array of string | Always emitted; no field-level overrides. |
| `test` | array of string | Always emitted; no field-level overrides. |
| `captured` | array of string | Always emitted; no field-level overrides. |

<a id="loupe-core-suites-task"></a>
## `loupe_core::suites::Task`

Source: [crates/loupe-core/src/suites.rs:14](../crates/loupe-core/src/suites.rs#L14).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `noul`

No payload.

### `choice`

No payload.

### `score`

No payload.

### `retrieval`

No payload.

### `pair`

No payload.

### `join`

No payload.

### `parity`

No payload.

### `mixed`

No payload.


<a id="loupe-core-task-metrics-bin"></a>
## `loupe_core::task_metrics::Bin`

Source: [crates/loupe-core/src/task_metrics.rs:73](../crates/loupe-core/src/task_metrics.rs#L73).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `lower` | number (f64) | Always emitted; no field-level overrides. |
| `upper` | number (f64) | Always emitted; no field-level overrides. |
| `lower_inclusive` | boolean | Always emitted; no field-level overrides. |
| `count` | integer (usize) | Always emitted; no field-level overrides. |
| `predicted` | number (f64) or null | Always emitted; no field-level overrides. |
| `observed` | number (f64) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-bounds"></a>
## `loupe_core::task_metrics::Bounds`

Source: [crates/loupe-core/src/task_metrics.rs:104](../crates/loupe-core/src/task_metrics.rs#L104).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `minimum` | number (f64) | Always emitted; no field-level overrides. |
| `maximum` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-brierscale"></a>
## `loupe_core::task_metrics::BrierScale`

Source: [crates/loupe-core/src/task_metrics.rs:66](../crates/loupe-core/src/task_metrics.rs#L66).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `binary`

No payload.

### `multiclass`

No payload.


<a id="loupe-core-task-metrics-calibrationkind"></a>
## `loupe_core::task_metrics::CalibrationKind`

Source: [crates/loupe-core/src/task_metrics.rs:60](../crates/loupe-core/src/task_metrics.rs#L60).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `positive-probability`

No payload.

### `top-label`

No payload.


<a id="loupe-core-task-metrics-class"></a>
## `loupe_core::task_metrics::Class`

Source: [crates/loupe-core/src/task_metrics.rs:83](../crates/loupe-core/src/task_metrics.rs#L83).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `label` | string | Always emitted; no field-level overrides. |
| `support` | integer (usize) | Always emitted; no field-level overrides. |
| `predicted` | integer (usize) | Always emitted; no field-level overrides. |
| `true_positive` | integer (usize) | Always emitted; no field-level overrides. |
| `false_positive` | integer (usize) | Always emitted; no field-level overrides. |
| `false_negative` | integer (usize) | Always emitted; no field-level overrides. |
| `precision` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `recall` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `f1` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-parity"></a>
## `loupe_core::task_metrics::Parity`

Source: [crates/loupe-core/src/task_metrics.rs:625](../crates/loupe-core/src/task_metrics.rs#L625).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `fields` | integer (usize) | Always emitted; no field-level overrides. |
| `probabilities` | integer (usize) | Always emitted; no field-level overrides. |
| `max_absolute_error` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `mean_absolute_error` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `argmax_agreement` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-report"></a>
## `loupe_core::task_metrics::Report`

Source: [crates/loupe-core/src/task_metrics.rs:111](../crates/loupe-core/src/task_metrics.rs#L111).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `task` | [loupe_core::suites::Task](#loupe-core-suites-task) | Always emitted; no field-level overrides. |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `point_labels` | integer (usize) | Always emitted; no field-level overrides. |
| `ambiguous_labels` | integer (usize) | Always emitted; no field-level overrides. |
| `accepted` | integer (usize) | Always emitted; no field-level overrides. |
| `accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `macro_f1` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `f1` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `auroc` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `brier` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `brier_scale` | [loupe_core::task_metrics::BrierScale](#loupe-core-task-metrics-brierscale) | Always emitted; no field-level overrides. |
| `ece` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `calibration_kind` | [loupe_core::task_metrics::CalibrationKind](#loupe-core-task-metrics-calibrationkind) | Always emitted; no field-level overrides. |
| `reliability` | array of [loupe_core::task_metrics::Bin](#loupe-core-task-metrics-bin) | Always emitted; no field-level overrides. |
| `coverage` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `selective_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `confidence_gates` | array of [loupe_core::task_metrics::Selective](#loupe-core-task-metrics-selective) | Always emitted; no field-level overrides. |
| `mae` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `mae_bounds` | [loupe_core::task_metrics::Bounds](#loupe-core-task-metrics-bounds) or null | Always emitted; no field-level overrides. |
| `within_one_accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `per_label` | array of [loupe_core::task_metrics::Class](#loupe-core-task-metrics-class) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-retrieval"></a>
## `loupe_core::task_metrics::Retrieval`

Source: [crates/loupe-core/src/task_metrics.rs:687](../crates/loupe-core/src/task_metrics.rs#L687).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `cases` | integer (usize) | Always emitted; no field-level overrides. |
| `expected` | integer (usize) | Always emitted; no field-level overrides. |
| `selected` | integer (usize) | Always emitted; no field-level overrides. |
| `true_positive` | integer (usize) | Always emitted; no field-level overrides. |
| `exact_match` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `precision` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `recall` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `f1` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-selective"></a>
## `loupe_core::task_metrics::Selective`

Source: [crates/loupe-core/src/task_metrics.rs:96](../crates/loupe-core/src/task_metrics.rs#L96).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `min_confidence` | number (f64) | Always emitted; no field-level overrides. |
| `accepted` | integer (usize) | Always emitted; no field-level overrides. |
| `coverage` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `accuracy` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-timing"></a>
## `loupe_core::task_metrics::Timing`

Source: [crates/loupe-core/src/task_metrics.rs:578](../crates/loupe-core/src/task_metrics.rs#L578).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `samples` | integer (usize) | Always emitted; no field-level overrides. |
| `completed_units` | integer (usize) | Always emitted; no field-level overrides. |
| `elapsed_seconds` | number (f64) | Always emitted; no field-level overrides. |
| `p50_ms` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `p95_ms` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |
| `units_per_second` | [loupe_core::task_metrics::Value](#loupe-core-task-metrics-value) | Always emitted; no field-level overrides. |

<a id="loupe-core-task-metrics-undefined"></a>
## `loupe_core::task_metrics::Undefined`

Source: [crates/loupe-core/src/task_metrics.rs:13](../crates/loupe-core/src/task_metrics.rs#L13).

Container attributes: `#[serde(rename_all = "kebab-case")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `no-samples`

No payload.

### `no-point-labels`

No payload.

### `no-accepted-decisions`

No payload.

### `no-positive-labels`

No payload.

### `no-negative-labels`

No payload.

### `no-predicted-labels`

No payload.

### `no-true-labels`

No payload.

### `no-label-outcomes`

No payload.

### `undefined-class-f1`

No payload.

### `zero-elapsed`

No payload.

### `not-applicable`

No payload.


<a id="loupe-core-task-metrics-value"></a>
## `loupe_core::task_metrics::Value`

Source: [crates/loupe-core/src/task_metrics.rs:29](../crates/loupe-core/src/task_metrics.rs#L29).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `value` | number (f64) or null | Always emitted; no field-level overrides. |
| `support` | integer (usize) | Always emitted; no field-level overrides. |
| `undefined` | [loupe_core::task_metrics::Undefined](#loupe-core-task-metrics-undefined) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-tournament-bracket"></a>
## `loupe_core::tournament::Bracket`

Source: [crates/loupe-core/src/tournament.rs:146](../crates/loupe-core/src/tournament.rs#L146).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `index` | integer (usize) | Always emitted; no field-level overrides. |
| `candidate_ids` | array of [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `request` | [loupe_core::wire::Request](#loupe-core-wire-request) | Always emitted; no field-level overrides. |
| `question_order` | array of string | Always emitted; no field-level overrides. |
| `ensemble_min_agreement` | [loupe_core::policy::Probability](#loupe-core-policy-probability) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |
| `ensemble (storage only, skipped)` | `loupe_core::ensemble::Group` or null | `#[serde(skip)]` |

<a id="loupe-core-tournament-final"></a>
## `loupe_core::tournament::Final`

Source: [crates/loupe-core/src/tournament.rs:316](../crates/loupe-core/src/tournament.rs#L316).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `round` | integer (usize) | Always emitted; no field-level overrides. |
| `original_candidates` | integer (usize) | Always emitted; no field-level overrides. |
| `surviving_candidates` | array of [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `selected` | array of [loupe_core::tournament::Scored](#loupe-core-tournament-scored) | Always emitted; no field-level overrides. |
| `probability_scope` | string | Always emitted; no field-level overrides. |
| `ranking` | [loupe_core::tournament::Ranking](#loupe-core-tournament-ranking) or null | Always emitted; no field-level overrides. |

<a id="loupe-core-tournament-options"></a>
## `loupe_core::tournament::Options`

Source: [crates/loupe-core/src/tournament.rs:140](../crates/loupe-core/src/tournament.rs#L140).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `take` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |
| `max_rounds` | nonzero integer (NonZeroUsize) | Always emitted; no field-level overrides. |

<a id="loupe-core-tournament-ranking"></a>
## `loupe_core::tournament::Ranking`

Source: [crates/loupe-core/src/tournament.rs:177](../crates/loupe-core/src/tournament.rs#L177).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `ranked` | array of [loupe_core::tournament::Scored](#loupe-core-tournament-scored) | Always emitted; no field-level overrides. |
| `probabilities` | object mapping string keys to number (f64) | Always emitted; no field-level overrides. |
| `none_wins` | boolean | Always emitted; no field-level overrides. |
| `tied` | boolean | Always emitted; no field-level overrides. |
| `selection_override` | string or null | `#[serde(skip_serializing_if = "Option::is_none")]` |
| `binding (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-tournament-reduction"></a>
## `loupe_core::tournament::Reduction`

Source: [crates/loupe-core/src/tournament.rs:327](../crates/loupe-core/src/tournament.rs#L327).

Container attributes: `#[serde(tag = "stage", rename_all = "lowercase")]`.

Discriminator key: `stage`. Variant fields merge into the same object.

### `next`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `round` | [loupe_core::tournament::Round](#loupe-core-tournament-round) | Always emitted; no field-level overrides. |

### `final`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `result` | [loupe_core::tournament::Final](#loupe-core-tournament-final) | Always emitted; no field-level overrides. |


<a id="loupe-core-tournament-round"></a>
## `loupe_core::tournament::Round`

Source: [crates/loupe-core/src/tournament.rs:161](../crates/loupe-core/src/tournament.rs#L161).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `number` | integer (usize) | Always emitted; no field-level overrides. |
| `capacity` | integer (usize) | Always emitted; no field-level overrides. |
| `final_round` | boolean | Always emitted; no field-level overrides. |
| `brackets` | array of [loupe_core::tournament::Bracket](#loupe-core-tournament-bracket) | Always emitted; no field-level overrides. |
| `planner (storage only, skipped)` | string | `#[serde(skip)]` |

<a id="loupe-core-tournament-scored"></a>
## `loupe_core::tournament::Scored`

Source: [crates/loupe-core/src/tournament.rs:171](../crates/loupe-core/src/tournament.rs#L171).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | [loupe_core::candidates::Id](#loupe-core-candidates-id) | Always emitted; no field-level overrides. |
| `probability` | number (f64) | Always emitted; no field-level overrides. |

<a id="loupe-core-tournament-template"></a>
## `loupe_core::tournament::Template`

Source: [crates/loupe-core/src/tournament.rs:64](../crates/loupe-core/src/tournament.rs#L64).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `model` | string | Always emitted; no field-level overrides. |
| `state` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `question` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `presence_criteria` | object mapping string keys to arbitrary JSON value or null | Always emitted; no field-level overrides. |
| `descriptions` | object mapping [loupe_core::candidates::Id](#loupe-core-candidates-id) keys to arbitrary JSON value | Always emitted; no field-level overrides. |
| `companions` | object mapping string keys to [loupe_core::wire::Question](#loupe-core-wire-question) | Always emitted; no field-level overrides. |
| `ensemble` | [loupe_core::selection_ensemble::Ensemble](#loupe-core-selection-ensemble-ensemble) or null | `#[serde(skip_serializing_if = "Option::is_none")]` |

<a id="loupe-core-trigger-action"></a>
## `loupe_core::trigger::Action`

Source: [crates/loupe-core/src/trigger.rs:37](../crates/loupe-core/src/trigger.rs#L37).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `id` | integer (usize) | Always emitted; no field-level overrides. |
| `matcher` | [loupe_core::trigger::Matcher](#loupe-core-trigger-matcher) | Always emitted; no field-level overrides. |
| `target` | [loupe_core::trigger::Target](#loupe-core-trigger-target) | Always emitted; no field-level overrides. |

<a id="loupe-core-trigger-declaration"></a>
## `loupe_core::trigger::Declaration`

Source: [crates/loupe-core/src/trigger.rs:24](../crates/loupe-core/src/trigger.rs#L24).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `when` | string | Always emitted; no field-level overrides. |
| `target` | [loupe_core::trigger::Target](#loupe-core-trigger-target) | Always emitted; no field-level overrides. |

<a id="loupe-core-trigger-matcher"></a>
## `loupe_core::trigger::Matcher`

Source: [crates/loupe-core/src/trigger.rs:30](../crates/loupe-core/src/trigger.rs#L30).

Container attributes: `#[serde(tag = "kind", content = "value", rename_all = "snake_case")]`.

Discriminator key: `kind`. Payload key: `value`.

### `yes`

No payload.

### `no`

No payload.

### `uncertain`

No payload.

### `label`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |


<a id="loupe-core-trigger-outcome"></a>
## `loupe_core::trigger::Outcome`

Source: [crates/loupe-core/src/trigger.rs:114](../crates/loupe-core/src/trigger.rs#L114).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `code` | integer (i32) or null | Always emitted; no field-level overrides. |
| `signal` | integer (i32) or null | Always emitted; no field-level overrides. |
| `timed_out` | boolean | Always emitted; no field-level overrides. |
| `input_error` | string or null | Always emitted; no field-level overrides. |
| `stdout` | [loupe_core::process::Capture](#loupe-core-process-capture) | Always emitted; no field-level overrides. |
| `stderr` | [loupe_core::process::Capture](#loupe-core-process-capture) | Always emitted; no field-level overrides. |
| `unit_in_stdin_only` | boolean | Always emitted; no field-level overrides. |

<a id="loupe-core-trigger-target"></a>
## `loupe_core::trigger::Target`

Source: [crates/loupe-core/src/trigger.rs:19](../crates/loupe-core/src/trigger.rs#L19).

Container attributes: `#[serde(tag = "kind", content = "value", rename_all = "snake_case")]`.

Discriminator key: `kind`. Payload key: `value`.

### `exec`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string | Always emitted; no field-level overrides. |

### `append`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `0` | string (UTF-8 filesystem path) | Always emitted; no field-level overrides. |


<a id="loupe-core-truncate-mode"></a>
## `loupe_core::truncate::Mode`

Source: [crates/loupe-core/src/truncate.rs:13](../crates/loupe-core/src/truncate.rs#L13).

Container attributes: `#[serde(rename_all = "lowercase")]`.

Default external tagging: unit variants are strings; variants with payloads are one-key objects.

### `head`

No payload.

### `tail`

No payload.

### `middle`

No payload.


<a id="loupe-core-truncate-range"></a>
## `loupe_core::truncate::Range`

Source: [crates/loupe-core/src/truncate.rs:34](../crates/loupe-core/src/truncate.rs#L34).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `byte_start` | integer (usize) | Always emitted; no field-level overrides. |
| `byte_end` | integer (usize) | Always emitted; no field-level overrides. |
| `token_start` | integer (usize) | Always emitted; no field-level overrides. |
| `token_end` | integer (usize) | Always emitted; no field-level overrides. |

<a id="loupe-core-truncate-view"></a>
## `loupe_core::truncate::View`

Source: [crates/loupe-core/src/truncate.rs:41](../crates/loupe-core/src/truncate.rs#L41).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `operation` | string | Always emitted; no field-level overrides. |
| `mode` | [loupe_core::truncate::Mode](#loupe-core-truncate-mode) | Always emitted; no field-level overrides. |
| `path` | string | Always emitted; no field-level overrides. |
| `coordinate_space` | string | Always emitted; no field-level overrides. |
| `original_counts` | [loupe_core::encoder::Counts](#loupe-core-encoder-counts) | Always emitted; no field-level overrides. |
| `retained` | array of [loupe_core::truncate::Range](#loupe-core-truncate-range) | Always emitted; no field-level overrides. |
| `separator` | string | Always emitted; no field-level overrides. |

<a id="loupe-core-wire-question"></a>
## `loupe_core::wire::Question`

Source: [crates/loupe-core/src/wire.rs:12](../crates/loupe-core/src/wire.rs#L12).

Container attributes: `#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]`.

Discriminator key: `type`. Variant fields merge into the same object.

### `noul`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `instructions` | arbitrary JSON value or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `criteria` | object mapping string keys to arbitrary JSON value or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

### `choice`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `instructions` | arbitrary JSON value or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `criteria` | object mapping string keys to arbitrary JSON value | Always emitted; no field-level overrides. |

### `score`

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `instructions` | arbitrary JSON value or null | `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `criteria` | array of arbitrary JSON value | Always emitted; no field-level overrides. |


<a id="loupe-core-wire-request"></a>
## `loupe_core::wire::Request`

Source: [crates/loupe-core/src/wire.rs:118](../crates/loupe-core/src/wire.rs#L118).

Container attributes: `#[serde(deny_unknown_fields)]`.

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `state` | arbitrary JSON value | Always emitted; no field-level overrides. |
| `model` | string | Always emitted; no field-level overrides. |
| `questions` | object mapping string keys to [loupe_core::wire::Question](#loupe-core-wire-question) | Always emitted; no field-level overrides. |

<a id="loupe-core-wire-response"></a>
## `loupe_core::wire::Response`

Source: [crates/loupe-core/src/wire.rs:149](../crates/loupe-core/src/wire.rs#L149).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `model` | string | Always emitted; no field-level overrides. |
| `answers` | object mapping string keys to arbitrary JSON value | Always emitted; no field-level overrides. |
| `usage` | [loupe_core::wire::Usage](#loupe-core-wire-usage) | Always emitted; no field-level overrides. |

<a id="loupe-core-wire-usage"></a>
## `loupe_core::wire::Usage`

Source: [crates/loupe-core/src/wire.rs:143](../crates/loupe-core/src/wire.rs#L143).

| JSON key / tuple position | Value | Serde attributes |
| --- | --- | --- |
| `input_tokens` | integer (u64) | Always emitted; no field-level overrides. |
| `output_tokens` | integer (u64) | Always emitted; no field-level overrides. |
