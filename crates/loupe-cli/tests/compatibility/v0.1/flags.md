# Complete CLI flag reference

Generated from the native Clap grammar. This is the v0.1 development contract; the 1.0 freeze is pending. Every declared command is included below. Shared flags repeat where accepted. Runtime applicability, custom value syntaxes and JSON contracts are described in [the reference index](../../../../../docs/cli-reference.md). Environment variable names are shown; their current values are always suppressed.

## `loupe`

```text
Semantic Unix filters for System One decision models

Usage: loupe [OPTIONS] <COMMAND>

Commands:
  mcp          Serve read-only classification and selection tools over MCP stdio
  review       Review a deferred queue offline, capturing labels and emitting corrections
  is           Test one whole state: exit 0 yes, 1 no, 2 uncertain
  keep         Preserve units in the yes bucket
  drop         Preserve units in the no bucket
  tag          Annotate each unit with a choice label
  route        Send each choice-labeled unit to a confined file or declared process
  rate         Annotate each unit with its expected score
  sort         Stable ascending score order, requiring finite input
  count        Histogram of choice labels or yes/no/uncertain decisions
  ask          Evaluate all questions in a TOML/JSON file against one state
  featurize    Evaluate joint Noul/Score features per row, as CSV or JSONL
  explain      Decompose a decision into an auditable weighted Noul/Score factor index
  join         Stream semantic matches against a finite indexed right-side table
  dedupe       Cluster duplicate pairs after finite input, keeping a representative
  on           Run declared actions on streaming Noul or Choice decisions
  hook         Install or run trusted, budgeted Git pre-commit / commit-msg checks
  eval         Evaluate pinned labeled cases and measure uncached pipeline latency
  calibrate    Fit empirical precision/coverage policy on fixed calibration cases
  find         Locate original lines answering a query, with presence and uncertainty gates
  pick         Rank candidate lines by Choice marginals or separate relevance Scores
  extract      Select source-backed date, money, version, address, host or ticker spans
  tokens       Plan requests and count tokens, with no inference
  doctor       Inspect endpoint health, limits and tokenizer readiness
  models       List models, or report an unsupported endpoint
  presets      Inspect, create, edit, trust and lint reusable rubrics
  cache        Inspect and manage the local raw-response cache without inference
  completions  Generate shell completions from this grammar
  man          Generate the complete roff manual, or one command's page
  help         Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

  -V, --version
          Print version

```

## `loupe mcp`

```text
Serve read-only classification and selection tools over MCP stdio

Usage: loupe mcp [OPTIONS]

Options:
      --config <CONFIG>
          

      --deadline <DEADLINE>
          Overall deadline per inference tool call
          
          [default: 30s]

      --budget <BUDGET>
          Exact input tokens per tool call, charging every retry

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe review`

```text
Review a deferred queue offline, capturing labels and emitting corrections

Usage: loupe review [OPTIONS] <FILE>

Arguments:
  <FILE>
          

Options:
      --config <CONFIG>
          

      --labels-dir <LABELS_DIR>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe is`

```text
Test one whole state: exit 0 yes, 1 no, 2 uncertain

Usage: loupe is [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe keep`

```text
Preserve units in the yes bucket

Usage: loupe keep [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe drop`

```text
Preserve units in the no bucket

Usage: loupe drop [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe tag`

```text
Annotate each unit with a choice label

Usage: loupe tag [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe route`

```text
Send each choice-labeled unit to a confined file or declared process

Usage: loupe route [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --out <OUT>
          Append to DIR/<sanitized-label>; process destinations override their label

      --to <TO>
          Feed one persistent shell command per declared label: LABEL=COMMAND

      --truncate
          Start fresh through a recoverable reset of owned files and route state

      --to-timeout <TO_TIMEOUT>
          Maximum wait for each process write and EOF shutdown (default 30s)

  -h, --help
          Print help

```

## `loupe rate`

```text
Annotate each unit with its expected score

Usage: loupe rate [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe sort`

```text
Stable ascending score order, requiring finite input

Usage: loupe sort [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -r, --reverse
          

  -h, --help
          Print help

```

## `loupe count`

```text
Histogram of choice labels or yes/no/uncertain decisions

Usage: loupe count [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe ask`

```text
Evaluate all questions in a TOML/JSON file against one state

Usage: loupe ask [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe featurize`

```text
Evaluate joint Noul/Score features per row, as CSV or JSONL

Usage: loupe featurize [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe explain`

```text
Decompose a decision into an auditable weighted Noul/Score factor index

Usage: loupe explain [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe join`

```text
Stream semantic matches against a finite indexed right-side table

Usage: loupe join [OPTIONS] --on <ON> <LEFT> <RIGHT>

Arguments:
  <LEFT>
          Structured left input, or - for a streaming stdin source

  <RIGHT>
          Finite right input, fully loaded before the left side is read

Options:
      --config <CONFIG>
          

      --ensemble
          Jointly evaluate the pair rubric's named variants

      --min-agreement <MIN_AGREEMENT>
          Minimum pair-ensemble agreement, default 1.0 or preset policy

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --on <ON>
          Comma-separated scalar field paths for local candidate blocking

      --url <URL>
          [env: LOUPE_URL]

      --max-candidates <MAX_CANDIDATES>
          [default: 20]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --blocking <BLOCKING>
          [default: hybrid]

  -I, --input-format <INPUT_FORMAT>
          [default: jsonl]
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

      --dry-run
          Emit candidate diagnostics without model inference

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -p, --preset <PRESET>
          

  -q, --quiet
          

  -f, --question-file <QUESTION_FILE>
          

  -v, --verbose...
          

      --arg <ARG>
          

      --context <CONTEXT>
          

      --cardinality <CARDINALITY>
          [default: 1:1]

      --unmatched [<UNMATCHED>]
          Emit unmatched left/right records; bare --unmatched means both
          
          [possible values: left, right, both]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

      --min-confidence <MIN_CONFIDENCE>
          

      --min-gap <MIN_GAP>
          

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain pair gates/scores using a trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --why
          

      --strict
          

      --no-cache
          

      --refresh
          

      --summary
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --keep-going
          

      --on-error <ON_ERROR>
          [possible values: drop, keep, fail]

      --failed <FAILED>
          

      --resume <RESUME>
          

  -h, --help
          Print help

```

## `loupe dedupe`

```text
Cluster duplicate pairs after finite input, keeping a representative

Usage: loupe dedupe [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

      --keep <KEEP>
          [default: first]

      --max-pairs <MAX_PAIRS>
          Refuse a finite catalog requiring more than N pair requests

      --window <WINDOW>
          Retain emitted representatives from the previous N physical input units

      --window-time <WINDOW_TIME>
          Retain representatives for a monotonic duration, such as 10m

      --index-capacity <INDEX_CAPACITY>
          Maximum live representatives in a time window; overflow fails explicitly

      --max-index-bytes <MAX_INDEX_BYTES>
          Maximum serialized bytes of retained streaming representatives

      --max-candidates <MAX_CANDIDATES>
          Maximum LSH candidates per arriving streaming unit

  -h, --help
          Print help

```

## `loupe on`

```text
Run declared actions on streaming Noul or Choice decisions

Usage: loupe on [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

      --when <WHEN>
          Start an action group for yes, no, uncertain, or a declared Choice label

      --exec <EXEC>
          Execute one shell command for each matching unit, without action retries

      --append <APPEND>
          Append matching original unit bytes to this file

      --exec-timeout <EXEC_TIMEOUT>
          [default: 30s]

      --dedupe-window <DEDUPE_WINDOW>
          

      --state <STATE>
          

      --max-fired <MAX_FIRED>
          Maximum live dedupe claims, default 65536

  -h, --help
          Print help

```

## `loupe hook`

```text
Install or run trusted, budgeted Git pre-commit / commit-msg checks

Usage: loupe hook [OPTIONS] <COMMAND>

Commands:
  install  Install both hooks, preserving existing files and core.hooksPath
  run      Run a hook check. Git supplies the message file to commit-msg
  help     Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe hook install`

```text
Install both hooks, preserving existing files and core.hooksPath

Usage: loupe hook install [OPTIONS] [HOOKS]...

Arguments:
  [HOOKS]...
          Select hooks; omission installs both
          
          [possible values: pre-commit, commit-msg]

Options:
      --config <CONFIG>
          

      --directory <DIRECTORY>
          Explicit existing hooks directory; defaults to Git's effective path

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe hook run`

```text
Run a hook check. Git supplies the message file to commit-msg

Usage: loupe hook run [OPTIONS] <KIND> [MESSAGE]

Arguments:
  <KIND>
          [possible values: pre-commit, commit-msg]

  [MESSAGE]
          

Options:
      --config <CONFIG>
          

      --diff <DIFF>
          Evaluate an existing patch file instead of the staged Git diff

      --dry-run
          Emit the exact joint request/counts without credentials or inference

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe eval`

```text
Evaluate pinned labeled cases and measure uncached pipeline latency

Usage: loupe eval [OPTIONS] [DATASET]

Arguments:
  [DATASET]
          Labeled JSONL input; requires --preset and --dataset-revision

Options:
      --config <CONFIG>
          

      --ensemble
          Evaluate mean decisions from jointly encoded named preset variants

      --min-agreement <MIN_AGREEMENT>
          Minimum ensemble vote agreement (default 1.0 or preset policy)

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --labels <LABELS>
          Explicit labeled JSONL input, equivalent to the positional dataset

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --suite <SUITE>
          Suite directory/manifest or name under --suites-dir; repeat for several suites

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --suites-dir <SUITES_DIR>
          [default: evals/suites]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -p, --preset <PRESET>
          Trusted preset; also overrides a suite's preset for rubric comparisons

      --dataset-revision <DATASET_REVISION>
          

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

      --split-file <SPLIT_FILE>
          

      --partition <PARTITION>
          Fixed partition; defaults to test, or calibrate for sweeps
          
          [possible values: test, calibrate, captured]

  -v, --verbose...
          

      --init-splits
          Explicitly create missing fixed split files; existing assignments are validated

      --report <REPORT>
          Write private JSON/Markdown reports and a publication manifest under DIR

      --baseline <BASELINE>
          Human-ratified baseline file; evaluate held-out absolute/regression gates

      --compare <COMPARE> <COMPARE>
          Two report/profile/preset targets, or standalone/ensemble, over identical cases

      --sweep-pack <SWEEP_PACK>
          Calibration-only packing candidates, separated by commas

      --sweep-combine <SWEEP_COMBINE>
          Calibration-only combine candidates; implies explicit text splitting

      --sweep-focus <SWEEP_FOCUS>
          Calibration-only selected-evidence counts; zero requests split/combine without focus

      --json
          

      --allow-unverified
          Emit provisional diagnostics when fresh full model identity is unavailable

      --warmup <WARMUP>
          Uncached passes over every selected packing group before latency measurement
          
          [default: 1]

      --dry-run
          

      --no-cache
          

      --refresh
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          

      --question-id-source <QUESTION_ID_SOURCE>
          Window question ID family; defaults to records or the calibrated preset's family

      --split
          

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          

      --truncate <TRUNCATE>
          

      --truncate-field <TRUNCATE_FIELD>
          

      --arg <ARG>
          

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

      --min-confidence <MIN_CONFIDENCE>
          

      --min-gap <MIN_GAP>
          

  -h, --help
          Print help

```

## `loupe calibrate`

```text
Fit empirical precision/coverage policy on fixed calibration cases

Usage: loupe calibrate [OPTIONS] <--target <TARGET>|--max-uncertain <MAX_UNCERTAIN>> [DATASET]

Arguments:
  [DATASET]
          Labeled JSONL input; requires --preset and --dataset-revision

Options:
      --config <CONFIG>
          

      --ensemble
          Evaluate mean decisions from jointly encoded named preset variants

      --min-agreement <MIN_AGREEMENT>
          Minimum ensemble vote agreement (default 1.0 or preset policy)

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --labels <LABELS>
          Explicit labeled JSONL input, equivalent to the positional dataset

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --suite <SUITE>
          Suite directory/manifest or name under --suites-dir; repeat for several suites

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --suites-dir <SUITES_DIR>
          [default: evals/suites]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -p, --preset <PRESET>
          Trusted preset; also overrides a suite's preset for rubric comparisons

      --dataset-revision <DATASET_REVISION>
          

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

      --split-file <SPLIT_FILE>
          

      --partition <PARTITION>
          Fixed partition; defaults to test, or calibrate for sweeps
          
          [possible values: test, calibrate, captured]

  -v, --verbose...
          

      --init-splits
          Explicitly create missing fixed split files; existing assignments are validated

      --report <REPORT>
          Write private JSON/Markdown reports and a publication manifest under DIR

      --baseline <BASELINE>
          Human-ratified baseline file; evaluate held-out absolute/regression gates

      --compare <COMPARE> <COMPARE>
          Two report/profile/preset targets, or standalone/ensemble, over identical cases

      --sweep-pack <SWEEP_PACK>
          Calibration-only packing candidates, separated by commas

      --sweep-combine <SWEEP_COMBINE>
          Calibration-only combine candidates; implies explicit text splitting

      --sweep-focus <SWEEP_FOCUS>
          Calibration-only selected-evidence counts; zero requests split/combine without focus

      --json
          

      --allow-unverified
          Emit provisional diagnostics when fresh full model identity is unavailable

      --warmup <WARMUP>
          Uncached passes over every selected packing group before latency measurement
          
          [default: 1]

      --dry-run
          

      --no-cache
          

      --refresh
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          

      --question-id-source <QUESTION_ID_SOURCE>
          Window question ID family; defaults to records or the calibrated preset's family

      --split
          

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          

      --truncate <TRUNCATE>
          

      --truncate-field <TRUNCATE_FIELD>
          

      --arg <ARG>
          

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

      --min-confidence <MIN_CONFIDENCE>
          

      --min-gap <MIN_GAP>
          

      --field <FIELD>
          Select one field from a mixed rubric; a single-field preset needs no selector

      --target <TARGET>
          Empirical precision=RATE or precision:LABEL=RATE; repeat for several labels

      --max-uncertain <MAX_UNCERTAIN>
          Maximum uncertain fraction, expressed as a probability or percentage

      --min-support <MIN_SUPPORT>
          Minimum accepted/predicted support for every fitted precision target
          
          [default: 1]

      --write
          Atomically write the fitted policy with complete current identity

  -h, --help
          Print help

```

## `loupe find`

```text
Locate original lines answering a query, with presence and uncertainty gates

Usage: loupe find [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

  -n, --take <TAKE>
          Retain up to N candidates, ranked within the final Choice population
          
          [default: 1]

      --max-rounds <MAX_ROUNDS>
          Stop before submitting a round beyond this bound
          
          [default: 64]

  -h, --help
          Print help

```

## `loupe pick`

```text
Rank candidate lines by Choice marginals or separate relevance Scores

Usage: loupe pick [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

  -n, --take <TAKE>
          Retain up to N candidates, ranked within the final Choice population
          
          [default: 1]

      --max-rounds <MAX_ROUNDS>
          Stop before submitting a round beyond this bound
          
          [default: 64]

      --independent
          Evaluate each candidate in its own Score/gate request

  -h, --help
          Print help

```

## `loupe extract`

```text
Select source-backed date, money, version, address, host or ticker spans

Usage: loupe extract [OPTIONS] --candidates <CANDIDATES> [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

  -n, --take <TAKE>
          Retain up to N candidates, ranked within the final Choice population
          
          [default: 1]

      --max-rounds <MAX_ROUNDS>
          Stop before submitting a round beyond this bound
          
          [default: 64]

      --candidates <CANDIDATES>
          Parse dates, money, semver, emails, hostnames or tickers from original text

      --normalize
          Normalize the selected source span in code, retaining verbatim provenance

      --now <NOW>
          Explicit RFC 3339 clock for relative/yearless dates

      --tz <TZ>
          IANA timezone for relative and offset-free dates

      --currency <CURRENCY>
          ISO 4217 code to resolve ambiguous money symbols

      --decimal-separator <DECIMAL_SEPARATOR>
          

      --date-order <DATE_ORDER>
          

  -h, --help
          Print help

```

## `loupe tokens`

```text
Plan requests and count tokens, with no inference

Usage: loupe tokens [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
      --config <CONFIG>
          

  -p, --preset <PRESET>
          

  -f, --question-file <QUESTION_FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

  -c, --criteria <CRITERIA>
          

      --url <URL>
          [env: LOUPE_URL]

      --criteria-file <CRITERIA_FILE>
          

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

      --context <CONTEXT>
          

      --arg <ARG>
          

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -t, --threshold <THRESHOLD>
          

      --margin <MARGIN>
          

  -q, --quiet
          

      --min-confidence <MIN_CONFIDENCE>
          

  -v, --verbose...
          

      --min-gap <MIN_GAP>
          

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          

  -I, --input-format <INPUT_FORMAT>
          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          [possible values: plain, annotate, tsv, json, csv, parquet]

      --json
          

      --keep-blank
          

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          

      --field <FIELD>
          

      --select <SELECT>
          

      --with-context <WITH_CONTEXT>
          

      --files
          

      --dry-run
          

      --strict
          

      --budget <BUDGET>
          

      --deadline <DEADLINE>
          

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          

      --overlap <OVERLAP>
          

      --max-chunks <MAX_CHUNKS>
          

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          

      --refresh
          

      --summary
          

      --review[=<REVIEW>]
          Review uncertainty plus a uniform audit sample using /dev/tty, or defer
          
          [possible values: interactive, defer]

      --review-queue <REVIEW_QUEUE>
          Durable queue path (default PROJECT/.loupe/review.jsonl)

      --review-labels <REVIEW_LABELS>
          Captured labels directory (default PROJECT/.loupe/labels)

      --audit-rate <AUDIT_RATE>
          Uniform review probability, default 2%; percentage or fraction

      --keep-going
          Continue after unit preflight or inference failures (default action: drop)

      --on-error <ON_ERROR>
          Handle failed units independently of uncertain answers
          
          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist admissions, exact plans, results and output progress in a new JSONL journal

      --resume <RESUME>
          Validate original input and resume a run journal (or route's manifest)

      --truncate <TRUNCATE>
          Retain the head/tail, or remove the middle while retaining both ends

  -h, --help
          Print help

```

## `loupe doctor`

```text
Inspect endpoint health, limits and tokenizer readiness

Usage: loupe doctor [OPTIONS]

Options:
      --config <CONFIG>
          

      --prefetch
          

      --json
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe models`

```text
List models, or report an unsupported endpoint

Usage: loupe models [OPTIONS]

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets`

```text
Inspect, create, edit, trust and lint reusable rubrics

Usage: loupe presets [OPTIONS] <COMMAND>

Commands:
  list   
  show   
  new    
  edit   
  trust  
  lint   
  help   Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets list`

```text
Usage: loupe presets list [OPTIONS]

Options:
      --config <CONFIG>
          

      --json
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets show`

```text
Usage: loupe presets show [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets new`

```text
Usage: loupe presets new [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
      --config <CONFIG>
          

      --type <TYPE>
          [default: noul]
          [possible values: noul, choice, score]

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --project
          

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets edit`

```text
Usage: loupe presets edit [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets trust`

```text
Usage: loupe presets trust [OPTIONS] [NAME]

Arguments:
  [NAME]
          

Options:
      --config <CONFIG>
          

  -f, --file <FILE>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe presets lint`

```text
Usage: loupe presets lint [OPTIONS] [NAME]

Arguments:
  [NAME]
          

Options:
      --config <CONFIG>
          

      --live
          

      --arg <ARG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --json
          

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe cache`

```text
Inspect and manage the local raw-response cache without inference

Usage: loupe cache [OPTIONS] <COMMAND>

Commands:
  stats  
  prune  
  clear  
  help   Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe cache stats`

```text
Usage: loupe cache stats [OPTIONS]

Options:
      --config <CONFIG>
          

      --json
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe cache prune`

```text
Usage: loupe cache prune [OPTIONS]

Options:
      --config <CONFIG>
          

      --older <OLDER_THAN>
          [default: 30d]

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe cache clear`

```text
Usage: loupe cache clear [OPTIONS]

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe completions`

```text
Generate shell completions from this grammar

Usage: loupe completions [OPTIONS] <SHELL>

Arguments:
  <SHELL>
          [possible values: bash, elvish, fish, powershell, zsh]

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```

## `loupe man`

```text
Generate the complete roff manual, or one command's page

Usage: loupe man [OPTIONS] [COMMAND]...

Arguments:
  [COMMAND]...
          Optional command path, such as "pick" or "presets trust"

Options:
      --config <CONFIG>
          

      --profile <PROFILE>
          [env: LOUPE_PROFILE]

      --url <URL>
          [env: LOUPE_URL]

      --path <PATH>
          [env: LOUPE_PATH]

      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions
          
          [env: LOUPE_API_FORMAT]

  -m, --model <MODEL>
          [env: LOUPE_MODEL]

  -j, --concurrency <CONCURRENCY>
          [env: LOUPE_CONCURRENCY]

  -q, --quiet
          

  -v, --verbose...
          

  -h, --help
          Print help

```
