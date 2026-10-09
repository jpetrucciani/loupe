# Line retrieval with `find`

`loupe find QUERY [FILE ...]` selects original lines answering a query. With no files it reads stdin until EOF; `-` may appear once among file operands. Each file is a separate finite document. Physical `L<n>` IDs restart within each document and preserve gaps from skipped blank lines. Source path plus physical line number identifies results across files.

```sh
loupe find 'Which line explains feed failover?' runbook.md
loupe find -p answers --arg 'query=How can we fail over the feed?' runbook.md --json
loupe find 'Which lines describe checkout outages?' incidents.txt -n 3 --strict
```

`-p` and `-f` accept trusted Noul query rubrics, argument/context templates, true/false presence criteria and presence policy. Choice/Score rubrics are exit 64. Generated Choice options are original line IDs. Classification calibration pins are rejected because their question/view identity does not describe a selection tournament. A model identity pin still requires matching fresh discovery.

The command reads whole UTF-8 documents. Explicit `-I whole` overrides a preset default. Structured/record input, classification unit/window packing, split/combine/focus/truncation and keep-going flags are inapplicable and exit 64. `--pack auto|whole` describes document selection. `--files` requires named operands and adds their paths to reference state. `--with-context N` adds the preceding N physical lines, including blanks, to candidate evidence; output always refers to the original selected line.

`-n/--take N` defaults to one and ranks up to N candidates from the final Choice distribution. These are conditional marginals, not independent relevance probabilities. `--max-rounds` defaults to 64. Evidence, criteria and context count toward every limit. The [shared tournament](selection-tournament.md) reserves `none`, carries top-k from eligible brackets and stops on non-reducing capacity or the round bound. A line that cannot fit alone is exit 65 and is never silently truncated.

`--threshold`/`--margin` govern presence; `--min-confidence`/`--min-gap` govern Choice. The [shared policy](selection-policy.md) handles uncertainty and absence. `--uncertain FILE` appends the original whole document once when any bracket is uncertain, including before explicit failure. It preserves original bytes without inventing delimiters between documents. Source aliases, including hard links, are rejected. Dry runs never create this file.

| Format | Shape |
| --- | --- |
| `plain` | `LINE:original line`, or `PATH:LINE:original line` for named sources. CRLF is retained; an unterminated selected line receives LF for display. |
| `annotate` | Location, candidate ID, marginal probability and verbatim text separated by tabs. |
| `tsv` | Four escaped columns: location, candidate ID, marginal probability, verbatim text. Backslashes, tabs, CR and LF are escaped. |
| `json` | One object per selected line, retaining the fields below. |

JSON has the common `index`, `unit`, `answer`, `model`, `cached` fields. `index` is the physical line number within its source; `unit` is the verbatim span without its terminator. `answer` is the complete original final Choice API answer, including `none`. Additional fields are `id`, `location` (source and exclusive byte/inclusive line bounds), `probability`, `probability_scope`, `uncertain`, `restored` and `provenance`. Provenance retains document index, policy outcome, all round plans/counts and raw backend evaluations. Earlier scores are never assembled into a global distribution. `cached` describes the final response; earlier evaluations retain their own cache status.

`--dry-run` emits the catalog and exact or explicitly estimated first-round requests/counts without credentials or network access. Further rounds are marked conditional on actual answers. No selection is fabricated.

Fresh discovery precedes cache lookup and constrains limits. Cache stays disabled without verified identity. All documents/rounds share the per-invocation submitted-token budget and deadline, including retries and unknown usage. Brackets run concurrently up to the admission limit; completed documents emit in source order. Fatal errors retain the completed prefix. Summary units/uncertainty count documents, while logical records/attempts count bracket requests. Deadlines bound input waits too; SIGINT/SIGTERM cancel pending work.

`--failed FILE`/`--resume FILE` use the general [run journal](failure-policy.md). Resume validates original inputs/fingerprints and run identity before skipping handled documents or recomputing policy from saved responses. Saved plans/counts must match fresh rounds. Completed-but-unemitted responses, including an explicit uncertainty failure, need no inference. Budgets count newly submitted work. Generic stdout and uncertainty writes retain the documented acknowledgement gap and may repeat after a crash. Changed configuration, inputs, plans or verified identity are refused. Unavailable full identity keeps historical responses provisional and does not make them cache entries.

Memory includes original document/candidate metadata, bounded read-ahead and request history capped by `--max-rounds`. JSON/journals retain that history. Memory grows with document size and round count; this is a finite-document command.

Qualification covers real files/processes and server: UTF-8/CRLF offsets, the repository MIT license, absent answers, uncertainty capture/actions, hard zero-budget admission and inference-free journal replay. Synthetic cases establish behavior, not held-out retrieval accuracy. Full identity/cache and release-platform qualification remain separate.

`--on-uncertain fallback` resolves presence before an eligible uncertain Choice. The trusted command receives compact JSON of the actual bracket state plus LF and prints one declared ID or `none`. Each external bracket contributes one winner, so top-N can return fewer than N. JSON preserves raw answers, adds per-field `fallback`/`selection_decisions`, and sets external `probability:null` with the original value in `model_probability`. Annotate/TSV leave that probability empty. External confidence is null. See [fallback](fallback.md) for bounds, deadline and failure policy.
