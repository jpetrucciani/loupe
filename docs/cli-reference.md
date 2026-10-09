# CLI reference

See the [subcommand cookbook](subcommand-examples.md) for concrete use cases and
shell completion installation.

The binary's grammar is the source of truth for option names, aliases, defaults and allowed enum values. `loupe --help` lists commands; `loupe COMMAND --help` lists their arguments. `loupe man` emits one complete roff manual containing every command and nested command. `loupe man pick` and `loupe man presets trust` emit individual pages. These commands work offline without configuration, credentials or a tokenizer. The CLI and JSON contracts remain versioned development interfaces until the 1.0 freeze.

The [complete flag reference](../crates/loupe-cli/tests/compatibility/v0.1/flags.md) is generated from every declared command, including nested commands. Its [grammar snapshot](../crates/loupe-cli/tests/compatibility/v0.1/grammar.json) records option names, aliases, defaults, value arity, enum values, environment variable names, groups and conflicts. [Compatibility checks](compatibility.md) pin parsed defaults, exit precedence, shared output bytes and the actual MCP tool schemas. Current environment values never enter these generated artifacts.

```sh
target/debug/loupe man > loupe.1
target/debug/loupe man pick
target/debug/loupe presets trust --help
```

Shared options appear in multiple command help pages. A syntactically accepted option still has to apply to that operation; inapplicable combinations return 64. For example, selection reads finite whole documents and rejects classification windows, while numeric features emit rows without uncertainty filtering.

| Operation | Commands | Contract |
| --- | --- | --- |
| Boolean test and filters | `is`, `keep`, `drop` | `is` tests one whole input. Filters preserve original unit bytes; yes/no/uncertain are disjoint. |
| Labels and scores | `tag`, `rate`, `sort`, `count`, `route` | Sort is stable ascending, with `-r` for descending. Count emits one aggregate. Route appends to confined declared destinations. |
| Raw API and planning | `ask`, `tokens`, `doctor`, `models` | Ask preserves the API response shape. Tokens/dry-run expose actual encoded plans. Doctor diagnoses optional server capabilities. |
| Source selection | `find`, `pick`, `extract` | Finite source-backed candidates, original IDs/offsets, presence/none gates and budgeted tournaments. |
| Matching | `join`, `dedupe` | Join streams left records over a finite right index; finite dedupe clusters after EOF; windowed dedupe greedily keeps first representatives. |
| Numeric reports | `featurize`, `explain` | Ordered numeric feature columns or weighted factor contributions with unchanged raw answers. |
| Human/process actions | `review`, `--review`, `on`, `hook` | Owned TTY review, deferred queues, streaming declared actions and trusted Git hook checks. |
| Rubrics and measurements | `presets`, `cache`, `eval`, `calibrate` | Trusted named rubrics, raw-response cache administration, fixed partitions, reports and identity-pinned calibration writes. |
| Integrations and documentation | `mcp`, `completions`, `man` | Read-only MCP stdio, shell grammar artifacts and complete manuals. |

Global `--config`, `--profile`, `--url`, `--path`, `-m/--model`, `-j/--concurrency`, `-q/--quiet` and repeatable `-v/--verbose` select trusted runtime settings. `-q` conflicts with verbosity. Question sources are a positional question, `-p/--preset`, or `-f/--question-file`. With a preset/question file, positional operands are input files. Without file operands, commands read stdin; `-` explicitly selects stdin where supported.

Input formats use `-I/--input-format`; output uses `-O/--output-format`. `--json` means `-O json`. Plain filters preserve original delimiters, CSV/TSV headers and unterminated final units. Annotated/numeric commands have their own display rules. TSV escapes backslash, tab, CR and LF. JSON unit output is JSONL. Only `featurize` supports output CSV and optional Parquet. `ask` and `count` produce single objects.

Custom value parsers supplement the enum values shown in help:

| Value | Syntax |
| --- | --- |
| `--pack`, `--sweep-pack` | `unit`, `whole`, `auto`, `window` (16 units), `window:N`; a positive integer is also accepted as a window size. Sweep values are comma-separated. |
| `--combine`, `--sweep-combine` | Noul: `any`, `all`, `noisy-or`, `mean`. Choice: `mean`, `max-confidence`, `vote`. Score: `mean`, `max`, `min`. A mixed question file must use a mode valid for every split field. |
| Classification `--truncate` | `head`, `tail`, `middle`. Route instead has a boolean `--truncate` for its recoverable reset. |
| `--candidates` | `dates`/`date`, `money`, `semver`/`versions`, `emails`/`email`, `hostnames`/`hostname`, `tickers`/`ticker`. |
| `--decimal-separator`, `--date-order` | `dot`/`comma` and `mdy`/`dmy`, respectively; require `--normalize`. |
| Join `--cardinality`, `--blocking` | `1:1`, `1:n`, `n:m`; `tokens`, `lsh`, `hybrid`. |
| Dedupe `--keep` | `first`, `last`, `longest`; streaming supports `first`. |
| Eval `--question-id-source` | `lines`, `records`, `files`, `units`; their initial-capital spellings are also accepted. |
| Calibration `--target` | `precision=RATE` or `precision:LABEL=RATE`, with a finite rate in `[0,1]`. |
| Calibration `--max-uncertain`, review `--audit-rate` | Probability in `[0,1]` or percentage such as `2%`. |
| `--deadline` and inference/process timeouts | Finite nonnegative seconds, optionally followed by `ms`, `s` or `m`, such as `250ms` or `0.5m`. Individual operations can require a positive timeout. |

`--review` alone means interactive review. `--review=defer` requires the equals sign; a separate following operand remains an input operand. Bare join `--unmatched` means `both`. `--arg NAME=VALUE` repeats named literal template arguments. `--to LABEL=COMMAND` repeats route process destinations; `on` pairs each ordered `--when` with its following actions. Durations for cache retention and durable claim windows follow their command-specific contracts.

| Flag group | Application and detailed contract |
| --- | --- |
| `-I`, `--records`, `--field`, `--select`, `--keep-blank`, `--with-context`, `--files` | Readers, subjects, projections and context. [Input examples](../README.md). Selection accepts whole documents; join accepts structured input. |
| `--pack`, `--split`, `--combine`, `--overlap`, `--max-chunks`, `--focus`, `--truncate`, `--truncate-field` | Explicit request views. Structured records never split. Selection uses its own candidate brackets; route's boolean truncate is a separate operation. [Input view provenance](json-interfaces.md#planning-and-explicit-input-views). |
| `--threshold`, `--margin` | Noul decisions and selection/pair presence gates. Equality at the threshold is yes; uncertainty uses strict distance. |
| `--min-confidence`, `--min-gap` | Choice/Score decisions and selection ranking. Sorted Choice keys break exact ties, which remain uncertain. [Selection policy](selection-policy.md). |
| `--on-uncertain`, `--uncertain` | Decision emission/capture. Default filters emit only their certain bucket. `is` reports uncertainty as exit 2; count retains an uncertain bucket. |
| `--fallback`, `--fallback-timeout`, `--on-fallback-error` | Trusted external decisions, enabled by the effective fallback action. Gate-first pair/selection processing, typed output, deadlines and per-head resume. [Fallback](fallback.md). |
| `--ensemble`, `--min-agreement` | Joint named variants, mean distributions and vote agreement. Feature/factor means retain raw answers and annotate uncertainty without filtering numeric rows. Selection generates aligned presence/Choice/relevance heads; pair means drive join ranking and dedupe edges/suppression. All heads affect exact limits, and recovery pins agreement. [Ensembles](ensembles.md). |
| `--why` | Declared companion-signal diagnostics and JSON provenance. [Signals](signals.md). |
| `--review`, `--review-queue`, `--review-labels`, `--audit-rate` | Owned TTY review/deferred capture and uniform audit sampling. [Review](review.md). |
| `--budget`, `--deadline`, `--strict`, `--summary` | Per-attempt token admission, overall cancellation, usage parity and operational metrics. [Summaries](run-summary.md). |
| `--no-cache`, `--refresh` | Bypass cache, or bypass reads while permitting verified writes. Fresh authoritative identity is required for lookup. [Server identity](server-identity.md). |
| `--keep-going`, `--on-error` | Completed-unit failure handling; reader/auth/budget/deadline/cancellation failures remain fatal. [Failure policy](failure-policy.md). |
| `--failed`, `--resume` | Original input/run validation and saved-work recovery. Ordinary stdout has a flush/acknowledgement gap. [Recovery](recovery.md). |

Command-specific flags and JSON/provenance fields are documented together:

| Interface | Format and recovery details |
| --- | --- |
| Find | [Line IDs, byte bounds, probability scope, bracket history](find.md) |
| Pick | [Choice marginals, independent Score values and calculation source](pick.md) |
| Extract | [Candidate families, verbatim spans and deterministic normalization](extract.md) |
| Featurize | [Columns, row IDs, raw answers/bindings, ensemble reports and Parquet schema](features.md) |
| Explain | [Composite formula, factor contributions, assessment and raw request/response](explain.md) |
| Join | [Blocking, cardinality, pair evidence, unmatched rows and recovery](join.md) |
| Dedupe | [Finite clusters and representative rules](dedupe.md), [bounded streaming semantics](streaming-dedupe.md) |
| Route | [Confined files/processes and crash recovery manifest](route-recovery.md) |
| On | [Ordered declarations, action metadata and outcomes](on.md), [durable claim boundary](on-state.md) |
| Review | [Queue identities, human decisions and capture split provenance](review.md) |
| Eval | [Fixed partitions, reports, comparisons, sweeps and raw measurements](eval.md), [catalog join stages](join-evaluation.md) |
| Selection eval | [Retrieval/absence, original offsets, normalization and bracket calibration](selection-evaluation.md) |
| Calibrate | [Support requirements, view grids/winners, pinning and atomic writes](calibration.md) |
| Acceptance | [Human-ratified baseline records, held-out gates, identity and exits](baselines.md) |
| MCP | [Tool schemas, protocol, deadlines and allowlists](mcp.md), [filesystem grants](mcp-access.md) |

For ordinary decision JSONL, the stable shared fields are `{index, unit, answer, model, cached}`. `answer` always retains the raw API field. External, ensemble and human decisions have separate provenance. Raw `ask` returns exactly `{model, answers, usage}`. Histogram JSON is `{counts}` with an additional error count when applicable. Kept failures use null answer/model and an explicit error object. Selection, features, factors and actions add their documented source/provenance fields; their layouts are part of the pending 1.0 freeze.

| Exit | Meaning |
| --- | --- |
| `0` | Successful output; `is` yes; a downstream broken pipe is treated as successful early consumption. |
| `1` | No selected/emitted result; `is` no. |
| `2` | `is` uncertain. |
| `3` | Explicit uncertainty failure. |
| `4` | Token budget exhaustion. |
| `64` | Invalid arguments or an inapplicable flag. |
| `65` | Invalid/oversized input, schema overflow or incompatible recovery data. |
| `69` | Inference/protocol failure, overall deadline or fatal fallback failure. |
| `74` | Filesystem/output/process I/O failure. |
| `77` | Authentication failure. |
| `78` | Invalid configuration. |
| `130`, `143` | SIGINT and SIGTERM. |

Fatal errors take precedence over normal match/absence/uncertainty exits and flush only the documented completed prefix. `--keep-going` handles eligible complete-unit failures through its explicit policy; it cannot authorize more work after budget exhaustion or deadline expiry. Sort, finite dedupe and each selection document defer their output until the required finite operation completes. Route file transactions and `on --state` actions have their own durable boundaries.
