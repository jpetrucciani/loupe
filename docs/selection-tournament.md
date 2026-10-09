# Candidate budgeting and tournament reduction

The core tournament planner sizes and reduces candidate sets for `find`, `pick` and `extract`. It preserves the candidate table's original IDs and provenance. Each request includes a presence Noul, a Choice with the reserved `none` option, and optional companion fields. The [shared selection-policy session](selection-policy.md) applies presence/uncertainty decisions before reduction. Held-out retrieval quality remains a separate qualification task.

Candidate evidence appears in both state and the Choice descriptions. Caller-supplied reference context stays intact. State contains a reference section and individually serialized candidate blocks separated by `<|file_sep|>`. The pinned Clef tokenizer registers this separator as an unnormalized token that strips no surrounding whitespace, so state blocks cannot merge across its boundary. This permits exact additive state budgeting even with NFC normalization and embedded special tokens. The encoder itself still follows the reference's fragment layout. A complete request that already fits needs no tournament boundary guarantee; overflow under an exact tokenizer requires the registered boundary. Jev profiles remain estimates.

The planner validates base fields and every candidate alone before admitting inference. A candidate that cannot fit alone is exit 65; descriptions and reference context are not silently truncated or split. Contextual descriptions can carry surrounding evidence while output provenance continues to refer to the original verbatim span.

If the full set fits, it becomes one final bracket. Otherwise, the planner derives a uniform capacity from the most expensive candidates. It checks total input tokens and state tokens independently, as well as question, per-field option and aggregate option limits. Each Choice reserves one option for `none`, each presence Noul consumes two aggregate options, and companions consume their own question/options. Ensembles add one of each generated head per query variant; candidate capacity accounts for their repeated Choice options. A 255-option Choice cap therefore permits at most 254 candidates before other constraints. Exact profiles use actual encoded costs; estimated profiles rank unrounded bytes and account for separate integer rounding. Every materialized bracket is encoded and checked again before submission.

Each preliminary eligible bracket carries up to top-k IDs from its own Choice distribution. A winning `none` contributes no survivors. A bracket resolved by external Choice fallback carries only that single winner, so final top-N may contain fewer than N candidates. Raw model distributions remain intact. The raw planner reduces Choice alone; the selection session additionally applies presence and uncertainty policy. Survivors enter the next round in original source order and receive a new joint request. Prior scores never determine the final ranking. Ties use sorted option-key order and retain the tie flag. Round/ranking bindings include explicit question order and reject results from another plan or bracket.

Overflow requires capacity greater than top-k so at least one full bracket can reduce the set. A non-reducing configuration fails before inference. `Options.max_rounds` bounds the core operation, with 64 used by current consumers/tests. Reaching that bound with candidates still needing comparison is exit 65. When every preliminary bracket is absent, reduction finishes immediately without an extra request, including at the round limit.

Final results label their probability population:

| Scope | Meaning |
| --- | --- |
| `all-candidates` | One Choice included every original candidate plus `none`. |
| `final-surviving-bracket` | The final Choice compared only the tournament survivors plus `none`. |
| `no-surviving-candidates` | Preliminary brackets retained no candidates; no final distribution exists. |

The final output retains that bracket's complete distribution and the original candidate count. It does not assemble or normalize earlier distributions into a global probability. Top-k marginals remain conditional Choice rankings; independent multi-select calibration is a different operation.
