# Numeric features

`loupe featurize -f questions.toml` evaluates every primary Noul and Score question jointly for each input row. It defaults to JSONL input and CSV output. Multi-question presets also work with `-p NAME`, under the normal trust/template rules. Choice questions and command-line criteria overrides are exit 64.

```sh
loupe --profile server featurize -f examples/postmortem-features.toml --field id < incidents.jsonl > features.csv
loupe --profile server featurize -f examples/postmortem-features.toml --field id --json < incidents.jsonl > features.jsonl
loupe --profile server featurize -f examples/postmortem-features.toml --field id -O parquet < incidents.jsonl > features.parquet
```

The question source has an ordered `questions` map. JSON and TOML declaration order is preserved. IDs are prompt content, and changing them can change both probabilities and token counts. Feature schema construction rejects duplicate generated column names before discovery/inference. For example, Score `x` and Noul `x_0` would both generate `p_x_0`.

| Column | Value |
| --- | --- |
| `index` | Global 1-based input unit index, including earlier files and skipped line units. |
| `row_id` | Explicit selected ID, or stable source ID `R<n>`, `L<n>`, `F<n>` or `U<n>`. |
| `p_<question>` | Noul true probability as returned. |
| `s_<question>` | Score expected level index as returned, on its declared `0..levels-1` scale. |
| `p_<question>_<level>` | Score probability for that numeric level, in ascending numeric order. |
| `error` | Empty on success, otherwise the retained unit error message. |

Primary questions contribute columns in declaration order. Each Score's expected value precedes all its level probabilities. Feature values preserve the returned numbers, including their rounding; Score values are not normalized to 0..1 and rounded probabilities are not renormalized. Scores with different rubrics need corresponding downstream interpretation. All rows in a run share one schema. CSV uses proper comma/quote/newline escaping, UTF-8 and LF record endings through the existing [csv writer](https://docs.rs/csv/1.4.0/csv/struct.WriterBuilder.html). Empty input emits the header and exits 1.

For JSONL, `--field PATH` selects the row ID using the existing dotted/JSON-pointer rules. The entire original object remains model state. The ID must be a nonempty string or a number, otherwise exit 65 during input reading, before that row's inference. JSON retains the ID's original scalar type. Without `--field`, IDs derive from the input mode and global index. Explicit duplicate IDs remain duplicate, with `index` identifying distinct rows; the command does not accumulate an unbounded uniqueness set. CSV represents IDs as text. Table input uses generated `R<n>` IDs and the ordinary `--select` state projection; `--field` remains specific to JSONL.

`-O csv` is the default. `-O json` and `--json` emit one object per row with `index`, `row_id`, `unit`, ordered numeric `features`, logical-ID `answers`, logical-to-actual `bindings`, `model`, `cached` and original `location`. Each raw answer is preserved unchanged. Window output additionally records `indices`; explicit split/truncate/focus views retain their view plans and answer provenance. The `unit` is the existing input subject, including an explicitly selected ID. Plain/annotate/TSV output is exit 64. Other commands reject `-O csv`.

Declared companion signals are always sent and counted, alongside all feature questions. They remain separate from primary feature columns. `--why` adds their own probabilities/policies to JSON and stderr; quiet suppresses diagnostics while preserving requested JSON metadata. CSV columns stay unchanged by `--why`. Primary threshold/margin/confidence/uncertainty flags are exit 64, since features emit numeric values without decision filtering. Preset decision policies do not filter rows. Normal calibrated-request identity checks still apply.

Unit packing performs one joint schema request per row, optionally using the server's batch transport for several independent states. Explicit `--pack window:N` binds every feature and signal to its own source record in a joint request. Both modes produce identical column definitions and stable row IDs, while probabilities can differ because window fields and state attend jointly. JSON bindings record the actual prompt field IDs. All complete requests use the normal schema/state/token limits, exact per-attempt budgets, deadlines, retries and fresh-identity cache rules. An oversized schema fails rather than being divided into independent feature groups.

Structured records never split. Text splitting requires explicit combine semantics for every field; probabilities then describe the declared part aggregation. Truncation/focus are explicit evidence views, with JSON/raw-journal provenance. `--dry-run` emits complete request plans/counts and `feature_columns`, without inference or CSV rows. State, fields, context, signal definitions and packing remain part of complete-request cache identity; changing output format does not alter prompt content.

Fatal failures flush only the completed input-order prefix. `--failed FILE` records the normal durable source/request/response journal. `--keep-going --on-error keep` emits a row with every feature missing, represented by blank CSV cells or JSON nulls, plus the error. Drop omits failed rows; fail stops. JSON error rows have null `answers`, `bindings` and `model`, `cached=false` and the ordinary `{code,message}` error. Invalid structured records or row IDs are reader failures and remain fatal.

A new CSV run writes one header. `--resume FILE` writes only unfinished rows, without another header, so append resumed stdout to the same output file. A completed resume writes nothing and needs no inference budget. Resume validates the original schema, output mode, packing, input and saved complete results. Stdout acknowledgements retain the general journal's documented crash gap; this command does not promise transactional external stdout delivery. See [failure-policy.md](failure-policy.md).

Memory is bounded by input-channel capacity, configured concurrency/window size, schema columns and individual records/responses. Unit mode does not retain the complete input. Window mode retains only in-flight windows. Whole input and explicit text views retain their finite document data as documented by those modes.

Build with `--features parquet` to enable `-O parquet`. The approved Apache writer uses an uncompressed flat schema: required unsigned 64-bit `index`, required UTF-8 `row_id`, ordered nullable double feature columns, and nullable UTF-8 `error`. IDs use the same textual rendering as CSV, preserving large numeric spellings. Numeric values use the shared projection without another inference or rounding step. Retained failures have null features and an error string.

Row groups hold at most 256 rows or approximately 8 MiB, with a 16 MiB maximum individual row. Completed groups stream to stdout; footer metadata scales with groups/columns. Normal completion and fatal/signal exits finalize the completed prefix into a valid file. Abrupt SIGKILL or a broken output pipe can leave an incomplete file. Parquet stdout requires a complete file, so `--failed`/`--resume` are exit 64; use journaled CSV/JSON when resumable external output is required. Empty input emits a valid zero-row file and exits 1. Dry-run still emits JSON plans.

`--ensemble` averages named variants for each feature that declares them, keeping the same column names and leaving other fields unchanged. Noul features use mean true probability; Score features use normalized mean level probabilities and their expected index. JSON `answers` remains the raw primary map and adds `ensembles` with member answers/distributions, mean values, votes and policy/agreement assessment. Confidence and agreement annotate these reports without filtering rows. CSV and Parquet use exactly the same numeric projection. Variants are jointly evaluated and correlated; the reports claim no calibration or independence. Scalar calibration pins are rejected for ensemble features.

```sh
loupe --profile server featurize -f examples/ensemble-features.toml \
  --ensemble --min-agreement 0.75 --json --why < incidents.jsonl
```

Qualification covers saved distributions, CSV/JSON numeric parity, quoting, schema collisions, scalar IDs, multi-file indexing, real server joint/window bindings, signals, missing rows, ordered partial output and inference-free resume. Actual Parquet file-reader tests verify row groups, UTF-8 IDs, unsigned indices and nullable features/errors. Fresh server checks compare the actual Parquet schema/values with JSON and read finalized fatal-prefix files. Held-out feature usefulness, full live identity/cache and native/other-platform release packaging remain separate.
