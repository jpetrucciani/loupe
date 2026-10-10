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
  featurize    Emit joint Boolean/Score features as CSV, JSONL or Parquet
  explain      Decompose a decision into an auditable weighted Noul/Score factor index
  join         Stream semantic matches against a finite indexed right-side table
  dedupe       Keep representatives of finite duplicate clusters or a bounded stream
  on           Run declared actions on streaming Noul or Choice decisions
  hook         Install or run trusted, budgeted Git pre-commit / commit-msg checks
  eval         Evaluate pinned labeled cases and measure uncached pipeline latency
  calibrate    Fit empirical precision/coverage policy on fixed calibration cases
  find         Locate original lines answering a query, with presence and uncertainty gates
  pick         Rank candidate lines by Choice marginals or separate relevance Scores
  extract      Select original date, money, version, email, hostname or ticker spans
  tokens       Plan requests and count tokens, with no inference
  doctor       Inspect endpoint health, limits and tokenizer readiness
  models       List models, or report an unsupported endpoint
  presets      Inspect, create, edit, trust and lint reusable rubrics
  cache        Inspect and manage the local raw-response cache without inference
  completions  Generate shell completions from this grammar
  man          Generate the complete roff manual, or one command's page
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help

  -V, --version
          Print version

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe mcp`

```text
Serve read-only classification and selection tools over MCP stdio

Usage: loupe mcp [OPTIONS]

Options:
      --deadline <DEADLINE>
          Overall deadline per inference tool call

          [default: 30s]

      --budget <BUDGET>
          Exact input tokens per tool call, charging every retry

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe review`

```text
Review a deferred queue offline, capturing labels and emitting corrections

Usage: loupe review [OPTIONS] <FILE>

Arguments:
  <FILE>
          Deferred JSONL review queue to open offline

Options:
      --labels-dir <LABELS_DIR>
          Write captured labels under DIR (default: PROJECT/.loupe/labels)

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe is`

```text
Test one whole state: exit 0 yes, 1 no, 2 uncertain

Usage: loupe is [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe keep`

```text
Preserve units in the yes bucket

Usage: loupe keep [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe drop`

```text
Preserve units in the no bucket

Usage: loupe drop [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe tag`

```text
Annotate each unit with a choice label

Usage: loupe tag [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe route`

```text
Send each choice-labeled unit to a confined file or declared process

Usage: loupe route [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe rate`

```text
Annotate each unit with its expected score

Usage: loupe rate [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe sort`

```text
Stable ascending score order, requiring finite input

Usage: loupe sort [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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
          Sort highest scores first instead of ascending

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe count`

```text
Histogram of choice labels or yes/no/uncertain decisions

Usage: loupe count [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe ask`

```text
Evaluate all questions in a TOML/JSON file against one state

Usage: loupe ask [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe featurize`

```text
Emit joint Boolean/Score features as CSV, JSONL or Parquet

Usage: loupe featurize [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Numeric feature rows (default: csv); json emits JSONL.

          [possible values: csv, json, parquet]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe explain`

```text
Decompose a decision into an auditable weighted Noul/Score factor index

Usage: loupe explain [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

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
      --ensemble
          Jointly evaluate the pair rubric's named variants

      --min-agreement <MIN_AGREEMENT>
          Minimum pair-ensemble agreement, default 1.0 or preset policy

      --on <ON>
          Comma-separated scalar field paths for local candidate blocking

      --max-candidates <MAX_CANDIDATES>
          Maximum blocked right-side candidates evaluated per arriving left record

          [default: 20]

      --blocking <BLOCKING>
          Local candidate strategy: tokens, lsh or hybrid

          [default: hybrid]

  -I, --input-format <INPUT_FORMAT>
          Structured input format for both tables

          [default: jsonl]
          [possible values: jsonl, csv, tsv]

      --dry-run
          Emit candidate diagnostics without model inference

  -p, --preset <PRESET>
          Trusted pair rubric with same_entity and similarity questions

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON pair-question file instead of a named preset

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

      --context <CONTEXT>
          UTF-8 context file included in every pair request

      --cardinality <CARDINALITY>
          Greedy assignment mode: 1:1, 1:n or n:m; input order affects assignment

          [default: 1:1]

      --unmatched [<UNMATCHED>]
          Emit unmatched left/right records; bare --unmatched means both

          [possible values: left, right, both]

  -O, --output-format <OUTPUT_FORMAT>
          Matched records (default: json); tsv emits escaped columns.

          [possible values: json, tsv]

      --json
          Emit matched-record JSONL (the default)

  -t, --threshold <THRESHOLD>
          Boolean same_entity threshold in [0,1]; equality is yes

      --margin <MARGIN>
          Boolean same_entity uncertainty distance in [0,1]

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning similarity Score probability in [0,1]

      --min-gap <MIN_GAP>
          Minimum top-two similarity Score probability gap in [0,1]

      --on-uncertain <ON_UNCERTAIN>
          Uncertain pair handling; defaults to drop

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain pair gates/scores using a trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default: 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --why
          Print pair scores, gates and declared companion-signal diagnostics

      --strict
          Require exact Clef counts and matching reported input-token usage

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --keep-going
          Continue after eligible complete-record failures

      --on-error <ON_ERROR>
          Failed-record handling with --keep-going; defaults to drop

          [possible values: drop, keep, fail]

      --failed <FAILED>
          Persist source admissions, pair plans and progress in a new JSONL journal

      --resume <RESUME>
          Validate original tables and resume a join journal

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe dedupe`

```text
Keep representatives of finite duplicate clusters or a bounded stream

Usage: loupe dedupe [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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
          Finite representative rule: first, last or longest; streaming requires first

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe on`

```text
Run declared actions on streaming Noul or Choice decisions

Usage: loupe on [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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
          Maximum duration per child action, including its output handling

          [default: 30s]

      --dedupe-window <DEDUPE_WINDOW>
          Suppress repeated actions within this duration, such as 10m

      --state <STATE>
          Persist action claims in this database across restarts

      --max-fired <MAX_FIRED>
          Maximum live dedupe claims, default 65536

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

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
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

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
      --directory <DIRECTORY>
          Explicit existing hooks directory; defaults to Git's effective path

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe hook run`

```text
Run a hook check. Git supplies the message file to commit-msg

Usage: loupe hook run [OPTIONS] <KIND> [MESSAGE]

Arguments:
  <KIND>
          Git hook to evaluate

          [possible values: pre-commit, commit-msg]

  [MESSAGE]
          Commit message file; required for commit-msg

Options:
      --diff <DIFF>
          Evaluate an existing patch file instead of the staged Git diff

      --dry-run
          Emit the exact joint request/counts without credentials or inference

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe eval`

```text
Evaluate pinned labeled cases and measure uncached pipeline latency

Usage: loupe eval [OPTIONS] [DATASET]

Arguments:
  [DATASET]
          Labeled JSONL input; requires --preset and --dataset-revision

Options:
      --ensemble
          Evaluate mean decisions from jointly encoded named preset variants

      --min-agreement <MIN_AGREEMENT>
          Minimum ensemble vote agreement (default 1.0 or preset policy)

      --labels <LABELS>
          Explicit labeled JSONL input, equivalent to the positional dataset

      --suite <SUITE>
          Suite directory/manifest or name under --suites-dir; repeat for several suites

      --suites-dir <SUITES_DIR>
          Base directory used to resolve named evaluation suites

          [default: evals/suites]

  -p, --preset <PRESET>
          Trusted preset; also overrides a suite's preset for rubric comparisons

      --dataset-revision <DATASET_REVISION>
          Immutable revision label for an explicit dataset

      --split-file <SPLIT_FILE>
          Fixed split assignments; defaults to DATASET.splits.json

      --partition <PARTITION>
          Fixed partition; defaults to test, or calibrate for sweeps

          [possible values: test, calibrate, captured]

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
          Emit the evaluation report as JSON instead of Markdown

      --allow-unverified
          Emit provisional diagnostics when fresh full model identity is unavailable

      --warmup <WARMUP>
          Uncached passes over every selected packing group before latency measurement

          [default: 1]

      --dry-run
          Emit request plans without discovery, credentials or inference

      --no-cache
          Disable response-cache reads and writes during accuracy evaluation

      --refresh
          Bypass accuracy-cache reads while allowing verified response writes

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap across every phase and retry; Clef only

      --deadline <DEADLINE>
          Overall discovery/evaluation deadline, such as 30s or 2m

      --pack <PACK>
          Request packing: unit, whole, auto or window:N

      --question-id-source <QUESTION_ID_SOURCE>
          Window question ID family; defaults to records or the calibrated preset's family

      --split
          Explicitly split oversized text using --combine

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks and re-ask over at most N selected evidence chunks

      --truncate <TRUNCATE>
          Explicit text view: head, tail or middle (retain both ends)

      --truncate-field <TRUNCATE_FIELD>
          Shorten only the named string field in structured input

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1]

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1]

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1]

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe calibrate`

```text
Fit empirical precision/coverage policy on fixed calibration cases

Usage: loupe calibrate [OPTIONS] [DATASET]
       --target SPEC or --max-uncertain RATE

Arguments:
  [DATASET]
          Labeled JSONL input; requires --preset and --dataset-revision

Options:
      --ensemble
          Evaluate mean decisions from jointly encoded named preset variants

      --min-agreement <MIN_AGREEMENT>
          Minimum ensemble vote agreement (default 1.0 or preset policy)

      --labels <LABELS>
          Explicit labeled JSONL input, equivalent to the positional dataset

      --suite <SUITE>
          Suite directory/manifest or name under --suites-dir; repeat for several suites

      --suites-dir <SUITES_DIR>
          Base directory used to resolve named evaluation suites

          [default: evals/suites]

  -p, --preset <PRESET>
          Trusted preset; also overrides a suite's preset for rubric comparisons

      --dataset-revision <DATASET_REVISION>
          Immutable revision label for an explicit dataset

      --split-file <SPLIT_FILE>
          Fixed split assignments; defaults to DATASET.splits.json

      --partition <PARTITION>
          Fixed calibration partition (default: calibrate); captured uses reviewed development
          cases.

          [possible values: calibrate, captured]

      --init-splits
          Explicitly create missing fixed split files; existing assignments are validated

      --report <REPORT>
          Write private JSON/Markdown reports and a publication manifest under DIR

      --sweep-pack <SWEEP_PACK>
          Calibration-only packing candidates, separated by commas

      --sweep-combine <SWEEP_COMBINE>
          Calibration-only combine candidates; implies explicit text splitting

      --sweep-focus <SWEEP_FOCUS>
          Calibration-only selected-evidence counts; zero requests split/combine without focus

      --json
          Emit the evaluation report as JSON instead of Markdown

      --allow-unverified
          Emit provisional diagnostics when fresh full model identity is unavailable

      --warmup <WARMUP>
          Uncached passes over every selected packing group before latency measurement

          [default: 1]

      --dry-run
          Emit request plans without discovery, credentials or inference

      --no-cache
          Disable response-cache reads and writes during accuracy evaluation

      --refresh
          Bypass accuracy-cache reads while allowing verified response writes

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap across every phase and retry; Clef only

      --deadline <DEADLINE>
          Overall discovery/evaluation deadline, such as 30s or 2m

      --pack <PACK>
          Request packing: unit, whole, auto or window:N

      --question-id-source <QUESTION_ID_SOURCE>
          Window question ID family; defaults to records or the calibrated preset's family

      --split
          Explicitly split oversized text using --combine

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks and re-ask over at most N selected evidence chunks

      --truncate <TRUNCATE>
          Explicit text view: head, tail or middle (retain both ends)

      --truncate-field <TRUNCATE_FIELD>
          Shorten only the named string field in structured input

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1]

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1]

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1]

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe find`

```text
Locate original lines answering a query, with presence and uncertainty gates

Usage: loupe find [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Read each file as a complete UTF-8 document (default: whole).

          [possible values: whole]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe pick`

```text
Rank candidate lines by Choice marginals or separate relevance Scores

Usage: loupe pick [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Read each file as a complete UTF-8 document (default: whole).

          [possible values: whole]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe extract`

```text
Select original date, money, version, email, hostname or ticker spans

Usage: loupe extract [OPTIONS] [OPERANDS]...
       --candidates KIND

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Read each file as a complete UTF-8 document (default: whole).

          [possible values: whole]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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
          Resolve ambiguous numeric separators: dot or comma

      --date-order <DATE_ORDER>
          Resolve ambiguous slash-date order: mdy or dmy

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe tokens`

```text
Plan requests and count tokens, with no inference

Usage: loupe tokens [OPTIONS] [OPERANDS]...

Arguments:
  [OPERANDS]...
          Positional question followed by files; with -p/-f, all operands are files

Options:
  -p, --preset <PRESET>
          Trusted named question rubric; positional operands then identify inputs

  -f, --question-file <QUESTION_FILE>
          Trusted TOML/JSON question file; positional operands then identify inputs

  -c, --criteria <CRITERIA>
          Comma-separated Choice labels or ordered Score levels for a positional question

      --criteria-file <CRITERIA_FILE>
          TOML/JSON criteria map or array instead of inline labels or levels

      --context <CONTEXT>
          UTF-8 context file included in every request

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

  -t, --threshold <THRESHOLD>
          Boolean yes threshold in [0,1]; equality is yes (default: 0.5)

      --margin <MARGIN>
          Boolean uncertainty distance from the threshold in [0,1] (default: 0)

      --min-confidence <MIN_CONFIDENCE>
          Minimum winning Choice/Score probability in [0,1] (default: 0)

      --min-gap <MIN_GAP>
          Minimum top-two Choice/Score probability gap in [0,1] (default: 0)

      --ensemble
          Jointly evaluate named preset variants and average their distributions

      --min-agreement <MIN_AGREEMENT>
          Minimum vote agreement for ensemble certainty (default 1.0)

      --on-uncertain <ON_UNCERTAIN>
          Handle uncertain answers; default filters emit only their certain bucket

          [possible values: keep, drop, fail, stderr, fallback]

      --fallback <FALLBACK>
          Resolve uncertain primary decisions with this trusted shell command

      --fallback-timeout <FALLBACK_TIMEOUT>
          Maximum duration per fallback command (default 30s)

      --on-fallback-error <ON_FALLBACK_ERROR>
          Failed fallback handling; defaults to uncertain

          [possible values: uncertain, fail]

      --uncertain <UNCERTAIN>
          Append original uncertain input to FILE

  -I, --input-format <INPUT_FORMAT>
          Input unit format; normally lines, but is/ask/explain read whole input

          [possible values: lines, whole, null, paragraphs, records, jsonl, csv, tsv]

  -O, --output-format <OUTPUT_FORMAT>
          Result representation (default: plain); json emits JSONL for unit results

          [possible values: plain, annotate, tsv, json]

      --json
          Shorthand for --output-format json; ask/count emit a single JSON object

      --keep-blank
          Preserve blank input units instead of skipping them

      --why
          Show declared companion signal probabilities and their separate policies

      --records <RECORDS>
          Regular expression separating text records; requires --input-format records

      --field <FIELD>
          Record field used as the output subject; other fields remain visible to the model

      --select <SELECT>
          Comma-separated table columns to send to the model

      --with-context <WITH_CONTEXT>
          Include N preceding input lines as context; preserve the original subject

      --files
          Treat each named file as a whole unit and emit its path

      --dry-run
          Emit request plans without credentials or model inference

      --strict
          Require exact Clef counts and matching reported input-token usage

      --budget <BUDGET>
          Exact submitted input-token cap, including retries; Clef only

      --deadline <DEADLINE>
          Overall run deadline, such as 30s or 2m; includes input waits

      --pack <PACK>
          Pack independent units, whole input, or joint windows (window:N)

      --split
          Explicitly split oversized text under the selected combine semantics

      --combine <COMBINE>
          Split-answer rule: any/all/noisy-or/mean, vote or min/max, by question type

      --overlap <OVERLAP>
          Number of overlap tokens between adjacent text chunks

      --max-chunks <MAX_CHUNKS>
          Maximum number of chunks permitted for one input

      --focus <FOCUS>
          Score chunks, then re-ask over at most K selected evidence chunks

      --truncate-field <TRUNCATE_FIELD>
          Shorten only this string field in a JSONL/CSV/TSV record, retaining its head

      --no-cache
          Disable response-cache reads and writes

      --refresh
          Bypass cache reads while allowing verified response writes

      --summary
          Print operational metrics on stderr after the run

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

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe doctor`

```text
Inspect endpoint health, limits and tokenizer readiness

Usage: loupe doctor [OPTIONS]

Options:
      --prefetch
          Download and validate the pinned Clef tokenizer when needed

      --json
          Emit the diagnostic report as JSON

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe models`

```text
List models, or report an unsupported endpoint

Usage: loupe models [OPTIONS]

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets`

```text
Inspect, create, edit, trust and lint reusable rubrics

Usage: loupe presets [OPTIONS] <COMMAND>

Commands:
  list   List built-in, user and project rubrics with origin and trust status
  show   Show a resolved preset's complete definition and trust status
  new    Create a rubric in the user directory, or in this project with --project
  edit   Edit with VISUAL/EDITOR; changing content revokes an existing trust grant
  trust  Trust inspected content; omission of NAME/-f trusts all project presets
  lint   Validate rubric schemas and templates; --live also evaluates examples
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets list`

```text
List built-in, user and project rubrics with origin and trust status

Usage: loupe presets list [OPTIONS]

Options:
      --json
          Emit the preset catalog as JSON

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets show`

```text
Show a resolved preset's complete definition and trust status

Usage: loupe presets show [OPTIONS] <NAME>

Arguments:
  <NAME>
          Preset name to inspect

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets new`

```text
Create a rubric in the user directory, or in this project with --project

Usage: loupe presets new [OPTIONS] <NAME>

Arguments:
  <NAME>
          New preset's simple name

Options:
      --type <TYPE>
          Question type: Boolean (noul), labeled choice or ordered score

          [default: noul]
          [possible values: noul, choice, score]

      --project
          Create under PROJECT/.loupe/presets instead of the user directory

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets edit`

```text
Edit with VISUAL/EDITOR; changing content revokes an existing trust grant

Usage: loupe presets edit [OPTIONS] <NAME>

Arguments:
  <NAME>
          Preset name; editing a built-in first creates a user copy

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets trust`

```text
Trust inspected content; omission of NAME/-f trusts all project presets

Usage: loupe presets trust [OPTIONS] [NAME]

Arguments:
  [NAME]
          One preset to trust; omission trusts every resolved project preset

Options:
  -f, --file <FILE>
          Trust this explicit TOML/JSON file instead of a named preset

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe presets lint`

```text
Validate rubric schemas and templates; --live also evaluates examples

Usage: loupe presets lint [OPTIONS] [NAME]

Arguments:
  [NAME]
          Preset to check; omission checks all resolved presets

Options:
      --live
          Run declared examples against the configured API after local checks

      --arg <ARG>
          Literal template binding NAME=VALUE; repeat for several names

      --json
          Emit validation reports as JSON

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe cache`

```text
Inspect and manage the local raw-response cache without inference

Usage: loupe cache [OPTIONS] <COMMAND>

Commands:
  stats  Report cached response counts and disk usage without inference
  prune  Delete responses older than the selected age
  clear  Clear all saved responses; retain the database and downloaded tokenizers
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe cache stats`

```text
Report cached response counts and disk usage without inference

Usage: loupe cache stats [OPTIONS]

Options:
      --json
          Emit cache statistics as JSON

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe cache prune`

```text
Delete responses older than the selected age

Usage: loupe cache prune [OPTIONS]

Options:
      --older <OLDER_THAN>
          Retention age as integer seconds/minutes/hours/days, such as 7d

          [default: 30d]

  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe cache clear`

```text
Clear all saved responses; retain the database and downloaded tokenizers

Usage: loupe cache clear [OPTIONS]

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe completions`

```text
Generate shell completions from this grammar

Usage: loupe completions [OPTIONS] <SHELL>

Arguments:
  <SHELL>
          Shell whose completion script is written to stdout

          [possible values: bash, elvish, fish, powershell, zsh]

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```

## `loupe man`

```text
Generate the complete roff manual, or one command's page

Usage: loupe man [OPTIONS] [COMMAND]...

Arguments:
  [COMMAND]...
          Optional command path, such as "pick" or "presets trust"

Options:
  -h, --help
          Print help

Global options:
      --api-format <API_FORMAT>
          Request/response format: systemone (default) or decisions

          [env: LOUPE_API_FORMAT]

      --config <CONFIG>
          Read configuration from FILE instead of the XDG config directory

  -j, --concurrency <CONCURRENCY>
          Maximum concurrent inference requests; must be positive

          [env: LOUPE_CONCURRENCY]

  -m, --model <MODEL>
          Override the model name sent to the API

          [env: LOUPE_MODEL]

      --path <PATH>
          Override the request path, such as /v1/systemone or /v1/decisions

          [env: LOUPE_PATH]

      --profile <PROFILE>
          Select a named configuration profile

          [env: LOUPE_PROFILE]

  -q, --quiet
          Suppress progress and diagnostic messages on stderr

      --url <URL>
          Override the decision API's base URL

          [env: LOUPE_URL]

  -v, --verbose...
          Increase diagnostics; repeat for more detail

```
