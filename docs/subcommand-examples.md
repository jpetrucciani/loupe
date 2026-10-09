# Loupe cookbook: every subcommand

Use this as a menu of practical jobs. The [usage guide](usage-guide.md) explains
input formats, uncertainty, configuration, and pipeline behavior in more detail.
Install/configure Loupe using [install.md](install.md) first. The examples assume
`loupe` is on PATH and your default profile selects the API you want to use.
H2O needs the Jev profile, estimated counts, and an endpoint. An API key is needed
only if your endpoint requires authentication. Loupe sends the selected input to
that endpoint.

## Sample inputs

Create these files in a scratch directory. Other examples below explicitly name
files you supply, such as reviewed labels, a Git patch, or an application log.

```sh
cat > events.log <<'DATA'
Checkout is down. Customers cannot place orders.
The health check completed successfully.
A retry restored the connection; requests now succeed.
DATA

cat > comments.txt <<'DATA'
Please validate the signature before accepting the webhook.
This feels a little odd.
Rename response_data to result so it matches the surrounding code.
DATA

cat > reports.txt <<'DATA'
Checkout is unavailable; customers cannot place orders.
Customers cannot complete purchases because checkout is down.
The scheduled health check completed successfully.
The routine heartbeat passed.
DATA

cat > release.txt <<'DATA'
Production release is scheduled for 2026-10-12.
Version 2.4.1 introduced the checkout regression.
The previous release was version 2.4.0.
DATA

cat > customers.jsonl <<'DATA'
{"name":"Acme Corporation","location":"Indianapolis"}
{"name":"Northwind","location":"Chicago"}
DATA

cat > accounts.jsonl <<'DATA'
{"name":"ACME Corp.","location":"Indianapolis"}
{"name":"Northwind Traders","location":"Chicago"}
DATA

cat > questions.toml <<'DATA'
[questions.outage]
type = "noul"
instructions = "Is checkout currently unavailable?"

[questions.urgency]
type = "score"
instructions = "How urgent is the current event?"
criteria = ["Routine or recovered", "Degraded service", "Customer-facing outage"]
DATA
```

Results below describe intended use, not guaranteed model predictions. A filter
or selection with no matches can exit 1; `is` also uses 2 for uncertainty.

## `is`: branch a shell script on meaning

Check an entire incident report before paging someone:

```sh
loupe is 'Does this report unresolved customer impact?' events.log
status=$?
printf 'Decision exit status: %s\n' "$status"
```

Exit 0 means yes, 1 means no, and 2 means uncertain. Handle other exits as errors.

## `keep`: retain actionable log lines

```sh
loupe keep -p real-failure events.log > failures.log
```

Preserves the original bytes of yes decisions. Uncertain lines are excluded by
default; add `--uncertain needs-review.log` to capture them separately.

## `drop`: retain the no bucket

Remove lines classified as current failures when preparing a routine activity log:

```sh
loupe drop -p real-failure events.log > routine.log
```

`keep` and `drop` both exclude uncertainty by default.

## `tag`: add incident severity labels

```sh
loupe tag -p severity events.log --json > labeled-events.jsonl
```

Emits each input unit with its predicted `page`, `ticket`, or `noise` answer,
probabilities, model, and cache status. `-O tsv` is convenient for shell pipelines.

## `route`: create separate queues by severity

```sh
loupe route -p severity events.log --out queues --dry-run
loupe route -p severity events.log --out queues
```

Appends to label files beneath `queues`, with a journal for recovery. Inspect the
plan first; this command writes files. See [route recovery](route-recovery.md).

## `rate`: score code-review comments

```sh
loupe rate -p actionable comments.txt -O tsv
```

Assigns an expected rubric score to each comment. Scores express the rubric,
not a calibrated probability that a comment is correct.

## `sort`: put concrete review suggestions first

```sh
loupe sort -p actionable comments.txt --reverse
```

Buffers finite input and sorts by score. `--reverse` places high scores first.

## `count`: summarize a batch of alerts

```sh
loupe count -p severity events.log
```

Produces a histogram of labels. Boolean questions instead count yes/no/uncertain.

## `ask`: answer several structured questions about one document

```sh
loupe ask -f questions.toml events.log --json
```

Returns the API object `{model, answers, usage}`. Questions share one API request;
Clef decides them jointly, while the H2O shim evaluates each question separately.

## `featurize`: build a table for analysis

```sh
loupe featurize -f questions.toml events.log -I lines -O csv > features.csv
loupe featurize -f questions.toml events.log -I lines -O parquet > features.parquet
```

Creates numeric Boolean/Score features per row. The portable build includes
Parquet; custom builds need `--features parquet` for the second command.

## `explain`: inspect weighted incident factors

Use the shipped factor preset to see customer impact, scope, and recovery
contributions. Run from the checkout or extracted archive root:

```sh
loupe explain -f examples/incident-factors.toml events.log --json
```

Returns the composite index, factor weights/contributions, and raw answers.
The factors explain the configured rubric, not the model's internal reasoning.

## `join`: reconcile differently named customer tables

```sh
loupe join customers.jsonl accounts.jsonl --on name,location \
  --cardinality 1:1 --max-candidates 20 --json > matches.jsonl
```

Matches each arriving left record to its best available right candidate.
The right table is loaded into memory; blocking and input order affect results.

## `dedupe`: consolidate repeated incident reports

```sh
loupe dedupe 'Do these records describe the same incident?' reports.txt \
  --keep first --json
```

Finite input forms clusters and selects representatives. For a live alert stream,
use `tail -F application.log | loupe dedupe --window 1000`; streaming keeps the
first match within its bounded window.

## `on`: collect paging events from a live stream

```sh
tail -F application.log | loupe on -p severity \
  --when page --append pages.log --when ticket --append tickets.log \
  --dedupe-window 10m --state triggers.redb
```

Writes only declared actions and remembers claims across restart. The state file
is not the source stream's cursor. Exec actions are at-most-once and never retried.

## `hook`: check staged changes or commit messages

This feature requires an exact-count **Clef profile**, not the current H2O
estimate profile. With a working Clef `local` profile and a prepared tokenizer:

```sh
loupe --config examples/config.toml --profile local hook run pre-commit \
  --diff change.patch --dry-run
loupe --config examples/config.toml --profile local hook install pre-commit commit-msg
```

The first command plans checks for your supplied patch. The second installs Git
hooks in the current repository, preserving existing hooks. Commit-message checks
can be invoked with `loupe hook run commit-msg .git/COMMIT_EDITMSG` using that same
Clef configuration. See [hooks](hooks.md) before installing.

## `eval`: measure a rubric on reviewed examples

Supply independently labeled cases in the [case format](evaluation-suites.md):

```sh
loupe eval --labels labels.jsonl -p real-failure --dataset-revision reviewed-v1 \
  --init-splits --allow-unverified --report reports --json
```

Creates fixed calibration/test partitions and reports held-out metrics and
uncached latency. `--allow-unverified` permits provisional H2O diagnostics;
these cannot ratify a baseline. Small toy samples do not establish production quality.

## `calibrate`: explore thresholds for your labeled data

```sh
loupe calibrate --labels labels.jsonl -p real-failure \
  --dataset-revision reviewed-v1 --target precision=0.98 \
  --allow-unverified --json
```

Uses the calibration partition created above. The requested target may be unmet.
The current H2O endpoint lacks authoritative identity, so keep this provisional.
Publishing a policy with `--write` requires verified identity and a supported
execution view; see [calibration](calibration.md).

## `find`: locate a line answering a question

```sh
loupe find 'Which line describes the current checkout outage?' events.log --json
```

Returns original source lines and offsets, rather than generated prose.
Presence/none/uncertainty gates can produce an empty result.

## `pick`: shortlist code-review suggestions

```sh
loupe pick 'Which comments request concrete code changes?' comments.txt -n 2 --json
loupe pick 'Which comments request concrete code changes?' comments.txt \
  -n 2 --independent --json
```

The first ranks Choice marginals; the second scores candidates independently.
Gating can yield fewer than two results. Neither method guarantees calibrated relevance.

## `extract`: select and normalize a value from its source

```sh
loupe extract 'Which version introduced the checkout regression?' release.txt \
  --candidates semver --normalize --json
loupe extract 'Which date is the production release?' release.txt \
  --candidates dates --normalize --now 2026-10-08T12:00:00-04:00 \
  --tz America/Indiana/Indianapolis --json
```

Selects a verbatim span, retains its byte offsets, and normalizes it in code.
Supported candidate types also include money, emails, hostnames, and tickers.

## `tokens`: inspect a request without running inference

```sh
loupe tokens -f questions.toml events.log --json
```

Shows the planned request and token counts. H2O counts are estimates; exact Clef
counts require the pinned tokenizer. No classification request is sent.

## `doctor`: inspect connectivity and profile limitations

```sh
loupe doctor --json
```

Reports health, discovery, limits, encoder readiness, and cache eligibility.
The current H2O profile can serve inference while doctor exits 69 because full
identity is unavailable. Clef's `doctor --prefetch` prepares its local tokenizer.

## `models`: discover the endpoint's model names

```sh
loupe models
```

Lists models if the server implements discovery. An unsupported endpoint returns
an error rather than inventing a model list.

## `presets`: reuse and maintain your questions

```sh
loupe presets list
loupe presets show severity
loupe presets new billing-defect --type noul --project
loupe presets edit billing-defect
loupe presets lint billing-defect --json
loupe presets trust billing-defect
```

Create/edit writes a project preset; edit launches your editor. Inspect the
instructions and permissions before trusting it. Editing a trusted preset changes
its content hash and revokes the grant. Add `--live` to lint after trust to run
its declared examples against your endpoint. See [preset trust](mcp-access.md).

## `cache`: inspect or remove saved responses

```sh
loupe cache stats --json
loupe cache prune --older 7d
loupe cache clear
```

Prune and clear delete local cache entries. Inference cache reuse requires fresh,
authoritative identity; the current H2O deployment disables reuse.

## `review`: correct a deferred queue in your terminal

```sh
loupe keep -p real-failure events.log --margin 0.1 \
  --review=defer --review-queue review.jsonl --audit-rate 100%
loupe review review.jsonl
```

The first command runs inference and queues every example for review.
For normal use, omit `--audit-rate 100%` to review uncertain decisions and the
default uniform 2% audit sample. The second
opens a TUI on `/dev/tty` to capture corrections offline. Review labels remain
separate from held-out test data. This needs the review feature, included by default.

## `mcp`: expose classification to an assistant

```sh
loupe mcp
```

Configure your MCP client to launch the absolute binary path with these arguments.
It speaks MCP on stdio and waits for a client. It exposes read-only tools, with
explicit preset/path grants, not route or exec actions. See [client setup](mcp.md).

## `completions`: enable tab completion

Bash, for the current session:

```sh
source <(loupe completions bash)
```

Fish, persistently:

```sh
mkdir -p ~/.config/fish/completions
loupe completions fish > ~/.config/fish/completions/loupe.fish
```

Zsh, persistently, using a user completion directory:

```sh
mkdir -p ~/.zsh/completions
loupe completions zsh > ~/.zsh/completions/_loupe
```

Add `fpath=(~/.zsh/completions $fpath)` before `autoload -Uz compinit; compinit`
in `.zshrc`. Completion generation uses `clap_complete`, works offline, and also
supports PowerShell and Elvish. The release archive already includes Bash/Fish/Zsh
scripts; the native Nix package installs those scripts automatically.

## `man` and `help`: read the grammar offline

```sh
loupe man > loupe.1
man -l loupe.1
loupe man presets trust > loupe-presets-trust.1
loupe help extract
loupe extract --help
```

Generates the complete manual or one command's page, including nested commands.
No endpoint, credentials, or tokenizer is needed.
