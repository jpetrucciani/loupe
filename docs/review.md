# Review and captured labels

`--review` opens Ratatui on `/dev/tty`, keeping stdin for source data and stdout for results. Review is included in the default build; `--no-default-features` omits the TUI. Interactive use without a controlling terminal exits 64 before credentials or inference. Ctrl-C/SIGINT/SIGTERM restore terminal attributes, cursor and screen, leaving pending tasks in the queue.

```sh
cat incidents.txt | loupe keep -p real-failure --margin 0.1 --review
cat incidents.txt | loupe keep -p real-failure --review=defer --review-queue review.jsonl
loupe review review.jsonl
```

Supported commands are `is`, `keep`, `drop`, `tag`, `rate`, `sort`, `route`, and `on`. Other commands and inference-free plans reject review with exit 64. Arrows and Enter select a declared option; 0–9 select an indexed option, and y/n select Noul labels. `s` skips; q or Ctrl-C cancels. The display retains the original unit/question and raw probabilities; control characters are inert.

Review includes every uncertain decision and an independent uniform 2% audit sample of all units. `--audit-rate 0.02` and `2%` are equivalent. Zero selects only uncertainty; 100% selects every unit. Uniform sampling happens before testing uncertainty, preserving the uniform sample's uncertain cases. Captures and summaries distinguish `uniform-audit` from `uncertainty`. Skipped audits are missing labels and require reporting completeness; pooling the two samples produces a biased estimate.

A human label changes the effective decision while preserving raw model answers. JSON adds `review` provenance and `decision` with `source=review` and `confidence=null`; TSV leaves confidence empty. Filters, sorting, routing and actions use the selected label. Exec receives `LOUPE_DECISION_SOURCE=review` and empty `LOUPE_CONFIDENCE`. Skips leave existing uncertainty policy in effect. Overall deadlines also bound inline review; standalone queue review has no inference deadline.

Deferred mode records tasks and leaves the model decision/policy in effect. The queue defaults to `PROJECT/.loupe/review.jsonl`; `--review-queue FILE` overrides it. `loupe review FILE --labels-dir DIR` works offline without credentials/network and emits JSONL corrections. Durable decisions and output receipts skip completed work; unfinished output reuses its label without prompting again. A crash between stdout and its receipt may repeat a row. Skips receive receipts and are not prompted again.

Tasks bind original units/locations, complete joint requests/raw responses, field/encoder/run identities and selection reasons. Checksummed frames reject corruption and repair only an incomplete final append. Each state file has one writer, rejects symlinks/parent traversal/hardlinks/nonregular objects, uses mode 0600 on creation, and has a 64 MiB cap. Memory scales with bounded queue/label indexes; choose new files when full.

Labels append to `PROJECT/.loupe/labels/PRESET.jsonl`; `--review-labels DIR` changes the inline destination. Positional/question-file rubrics use `question.jsonl`. Captures use the evaluator's case schema and retain original state, full reference request/response, and source/run/question provenance. Durable IDs prevent duplicate captures. Capture precedes the durable decision, so recovery reuses an already stored label. General `--failed`/`--resume` also retain the sampling seed and reviewed decisions.

Cases have `source=review`, `reviewed=true` and a sampling tag. The evaluator assigns them only to its separate `captured` split, for development/calibration. They never enter held-out test. Capture does not ratify a baseline or fit a scalar policy for another joint view.

Real PTY tests verify selection, idempotent capture/resume, skip, Ctrl-C/SIGTERM, terminal restoration and separate stdin/stdout pipes. Fresh server tests verify inline piped inference and deferred general-journal resume with zero inference budget. Core tests cover queue recovery, uniform sampling and separate raw-result/decision replay.
