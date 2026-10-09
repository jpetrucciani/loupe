# Using Loupe from the command line

Loupe asks a decision model questions about text and turns its answers into Unix
filters, labels, scores or selections. Start with explicit questions and a few
representative inputs, inspect JSON, then put the command into a larger pipeline.
Its decisions are model predictions. Exit status and uncertainty let your script
decide what to do with them.

For a concrete example of every command, see the [subcommand cookbook](subcommand-examples.md),
including presets, review, hooks, cache management, and shell completion setup.

## Connect to server

Install the [portable binary](install.md), or use the current checkout build:

```sh
export PATH="$PWD/target/debug:$PATH"
mkdir -p ~/.config/loupe
cp examples/mcp.toml ~/.config/loupe/config.toml
export LOUPE_URL=http://127.0.0.1:8015
export LOUPE_PATH=/v1/systemone
export LOUPE_MODEL=h2oai/h2o-lightning-4b
export LOUPE_ENCODER=estimate
printf '%s\n' 'Checkout is down.' | loupe keep -p real-failure
```

The copied profile selects the Jev backend; these variables select the endpoint
and model. With this checkout, use
`loupe --config examples/h2o.toml --profile server-h2o` for the equivalent bounded
profile. For MCP, use `--config examples/mcp.toml --profile server`.
Token counts are estimates, so omit `--strict` and hard `--budget` flags.
Caching is disabled until the server supplies authoritative model/encoder identity.
`doctor --json` reports readiness and missing identity separately; its current
exit 69 does not mean H2O inference is unavailable. No local tokenizer is required.
Other API variables and persistent configuration are in [install.md](install.md).
Commands below that require exact counting or caching are Clef-specific.

The examples below use these small input files. Create them in a scratch directory:

```sh
cat > events.log <<'EOF'
Checkout is down. Customers cannot place orders.
The health check completed successfully.
A retry restored the connection; requests now succeed.
EOF

cat > comments.txt <<'EOF'
Please validate the signature before accepting the webhook.
This feels a little odd.
Rename response_data to result so it matches the surrounding code.
EOF
```

## Filter lines by meaning

```sh
# Keep yes decisions using a built-in question.
loupe keep -p real-failure events.log

# Ask a custom question instead.
loupe keep 'Does this report unresolved customer impact?' events.log

# Keep the no bucket for the same question.
loupe drop 'Does this report unresolved customer impact?' events.log

# Stream a finite file through an ordinary pipe.
cat events.log | loupe keep -p real-failure
```

With the sample above, the verified server run printed only:

```text
Checkout is down. Customers cannot place orders.
```

The positional question comes before input files. With `-p` or `-f`, all positional
operands are files. No file operands means stdin. Filters preserve original
matching bytes. `keep` emits yes, `drop` emits no, and neither emits uncertain by
default. A matching phrase can appear inside an otherwise irrelevant line;
phrase your question around the event you actually want to keep.

## Use a decision in a shell script

`is` reads the whole input and exits 0 for yes, 1 for no or 2 for uncertain.
Other failures have separate exit codes.

```sh
if loupe is 'Does this report a current customer-facing outage?' events.log; then
  echo 'Outage detected'
else
  status=$?
  case "$status" in
    1) echo 'No outage detected' ;;
    2) echo 'Needs a human decision' ;;
    *) echo "Loupe failed with exit $status" >&2 ;;
  esac
fi

# Inspect a proposed change as one whole state.
git diff | loupe is 'Does this change remove an authentication check?'
```

An empty successful match set from filters/selection commonly exits 1. Account for
that when using `set -e` or `pipefail`; it is different from a transport failure.

## Label, count, score and sort

```sh
# Severity is a Choice preset with page, ticket and noise labels.
loupe tag -p severity events.log -O tsv
loupe count -p severity events.log

# Inspect original text, raw answer, model identity and cache status.
loupe tag -p severity events.log --json

# Actionable is a Score preset, with ordered rubric levels.
loupe rate -p actionable comments.txt -O tsv
loupe sort -p actionable comments.txt --reverse
```

`tag` assigns one declared label per unit. `rate` uses the returned probability
distribution to compute an expected rubric score. That score is not a probability
of correctness. `sort` reads finite input and orders by score; ascending is the
default, so `--reverse` puts high scores first.

Use `loupe presets list` to find built-ins and `loupe presets show severity` to
read the actual question/options. Inspecting presets does not require inference.

## Keep uncertainty visible

```sh
loupe keep -p real-failure events.log \
  --margin 0.1 --uncertain uncertain.log --summary

# Stop if a decision is uncertain.
loupe keep -p real-failure events.log --on-uncertain fail

# For Choice, require sufficient winning probability and separation.
loupe tag -p severity events.log --min-confidence 0.8 --min-gap 0.15 --json
```

For a Boolean question, `--margin` marks answers near the decision threshold
uncertain. For Choice/Score, confidence and top-two gap control uncertainty.
Flags must apply to the question type. `--on-uncertain` changes handling;
`--uncertain FILE` captures original uncertain input for inspection.

## Read paragraphs, records or entire files

```sh
# One multiline paragraph per decision.
loupe keep 'Does this paragraph describe a root cause?' postmortem.md -I paragraphs

# One whole document per decision, printing matching file paths.
loupe keep 'Does this document describe a checkout outage?' --files incidents/*.md

# CSV: send only the selected columns, preserving matching original rows/header.
loupe keep 'Does this ticket describe a billing defect?' tickets.csv \
  -I csv --select title,description

# JSONL: send each full object; select the subject shown in annotations/JSON.
loupe tag -p severity events.jsonl -I jsonl --field message --json
```

`-I` controls input units. `-O` controls output representation. `--json` is shorthand
for `-O json`; per-unit JSON output is JSONL. JSONL inputs must be objects, one per
line. CSV/TSV inputs need consistent headers and row widths. `--field` does not
hide the other JSON fields from the model; `--select` controls sent table columns.

## Find evidence, rank lines and extract spans

```sh
loupe find 'Which line describes the current checkout outage?' events.log
loupe pick 'Which comments request concrete code changes?' comments.txt -n 2 --json
loupe pick 'Which comments request concrete code changes?' comments.txt \
  -n 2 --independent --json

loupe extract 'Which version introduced the regression?' thread.md \
  --candidates semver --normalize --json
loupe extract 'Which date is the production release?' release.txt \
  --candidates dates --normalize --now 2026-10-07T12:00:00-04:00 \
  --tz America/Indiana/Indianapolis --json
```

These commands return original source lines/spans, with offsets in JSON, rather
than generated prose. They read finite documents to EOF. Use a complete question;
the model's presence gate can miss terse noun-phrase queries. `pick` normally ranks
Choice marginals over candidates; `--independent` scores candidates separately,
costing more requests. Presence/none/uncertainty gates can yield fewer than N
results. Date normalization uses the supplied reference time and timezone.

## Ask several questions jointly or make numeric features

```sh
cat > questions.toml <<'EOF'
[questions.outage]
type = "noul"
instructions = "Is checkout currently unavailable?"

[questions.urgency]
type = "score"
instructions = "How urgent is the current event?"
criteria = ["Routine or recovered", "Degraded service", "Customer-facing outage"]
EOF

# One whole-input response, shaped as {model, answers, usage}.
loupe ask -f questions.toml events.log

# One set of numeric features per input line.
loupe featurize -f questions.toml events.log -I lines -O csv
loupe featurize -f questions.toml events.log -I lines -O parquet > events.parquet
```

Clef decides all questions in a request jointly, so a companion question can
change the original answer. The H2O shim evaluates each question separately and
shares state prefixes through vLLM caching. Use stable question IDs and keep a consistent
question set when comparing runs. Parquet requires a build with that feature;
the portable artifact includes it.

## Route decisions into files or trigger actions

```sh
# Plan destinations before writing anything.
loupe route -p severity events.log --out routed --dry-run

# Append each line to routed/page, routed/ticket or routed/noise.
loupe route -p severity events.log --out routed

# Resume the same input/run without duplicating committed routed output.
loupe route -p severity events.log --out routed \
  --resume routed/.loupe-route.jsonl

# Stream decisions into declared append actions, with durable repeat suppression.
tail -F application.log | loupe on -p severity \
  --when page --append pages.log --when ticket --append tickets.log \
  --dedupe-window 10m --state triggers.redb
```

`route` appends by default; `--truncate` starts fresh. It confines files under its
output directory and journals appends for crash recovery. `on` acts only on the
declared matching groups. `--state` preserves claims across restart, and an explicit
dedupe window controls expiry. It does not save the source stream's cursor.
`--exec COMMAND` is also available; child actions are never automatically retried.
See [route recovery](route-recovery.md) and [trigger state](on-state.md).

## Match tables or remove semantic duplicates

```sh
loupe join customers.jsonl accounts.jsonl --on name,location \
  --cardinality 1:1 --max-candidates 20 --json

loupe dedupe 'Do these records describe the same incident?' reports.txt --keep first --json
tail -F alerts.log | loupe dedupe --window 1000
```

Join loads the finite right table and streams the left. Default 1:1 matching picks
the best available right candidate for each arriving left record, so input order
matters. Blocking and candidate caps can miss matches. Finite dedupe clusters
matching pairs; windowed streaming dedupe keeps the first representative and
cannot reconsider older output. Large finite dedupe inputs can require many
pair decisions, so start with a small sample.

## Plan costs, bound a run and reuse work

```sh
# Offline estimated plan/count. Clef exact counts require tokenizer preparation.
loupe tokens -p severity events.log --json

loupe tag -p severity events.log --deadline 30s --summary
loupe tag -p severity events.log --no-cache --summary
loupe cache stats --json

# Persist work for a later resume; keep input/configuration identical.
loupe keep -p real-failure events.log --failed run.jsonl > kept.log
loupe keep -p real-failure events.log --resume run.jsonl >> kept.log
```

For Clef only, add `--strict --budget 10000` to enforce an exact input budget.
H2O estimates cannot provide that guarantee.

Exact Clef counts include state, schema and context. Retries consume budget too;
unknown usage stays charged. Fresh model/tokenizer identity is discovered before
cache lookup. Cache keys cover the complete request; warmup/latency in eval always
bypass cache. Generic stdout resume has a crash acknowledgement gap, unlike the
route journal's coordinated file writes. See [failure/recovery](failure-policy.md).

Whole-document questions fail when oversized unless splitting is explicitly
allowed. For example, `--split --combine any` asks whether any chunk satisfies the
question. It changes the meaning of the original whole-document question.

## Measure and calibrate your own rubric

```sh
# labels.jsonl contains stable case IDs, unit text and independently reviewed labels.
loupe eval --labels labels.jsonl -p real-failure --dataset-revision reviewed-v1 \
  --init-splits --allow-unverified --report reports --json

loupe calibrate --labels labels.jsonl -p real-failure --dataset-revision reviewed-v1 \
  --target precision=0.98 --max-uncertain 10% --allow-unverified --json
```

Eval measures the held-out test split; calibration fits the separate calibration
split. A precision target needs enough accepted positive support and may be unmet.
H2O runs with `--allow-unverified` are provisional and cannot ratify baselines
or publish policies. With verified identity, `--write` publishes a policy pinned
to the actual model, tokenizer and request view. Treat measured precision as a property of your labeled sample. Review the
[evaluation](eval.md) and [calibration](calibration.md) data contracts before using
these commands with a real dataset.

## Use the same model through MCP

Configure your MCP client to launch the installed absolute Loupe path with arguments
`["mcp"]`, using the persistent config from [installation](install.md). From this
checkout, use `["--config", "/absolute/path/to/examples/mcp.toml", "mcp"]`.
The server exposes read-only classification, ask, token planning, preset access,
find, pick and extraction. Preset/path access needs explicit grants; it does not
expose routing or process actions. See [MCP setup](mcp.md).

For exact flag syntax, use `loupe COMMAND --help`, `loupe man COMMAND` or the
[complete CLI reference](cli-reference.md).
