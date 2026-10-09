# Join candidate blocking

This document describes the local blocking stage used by [semantic join](join.md). `--dry-run` emits candidate diagnostics and complete pair request plans as JSONL, without accepting candidates as semantic matches.

```sh
loupe join left.jsonl right.jsonl --on name,location --dry-run
loupe join - right.csv -I csv --on name,location --max-candidates 20 --dry-run < left.csv
```

Both sides use the existing real JSONL/CSV/TSV reader. The right side is finite and fully loaded before the left reader opens. One side may be `-` for stdin, but both cannot use stdin. Each left record streams independently, and its complete candidate row flushes before reading the next record. Structured reader validation, original full record values and physical byte/line/source locations are retained, including quoted multiline table rows. Empty left input exits 1; nonempty left input with no candidates still emits diagnostic rows and exits 0.

`--on` requires distinct scalar field paths. Dotted paths and JSON pointers use the existing parser, including array traversal. Equivalent duplicate paths, malformed/empty paths, missing fields and array/object blocking values are errors. Null and punctuation-only field values normalize to empty and never match other missing data. String, number and boolean scalars tokenize locally.

Normalization applies Unicode lowercase, splits at non-alphanumeric characters, removes duplicate words and sorts each field's token set. It does not remove accents, apply Unicode normalization, expand abbreviations or infer aliases. Each token is qualified by the configured field position, so a name token does not match a location token. Whole-field equality means equality of the normalized token sets, ignoring original word order and punctuation. It is blocking evidence, rather than a semantic identity assertion.

`--blocking hybrid` is the default and unions token-overlap and LSH postings. `tokens` uses only field-qualified token postings; `lsh` requires a shared MinHash band. Empty token sets have no bands. The version-1 MinHash sketch has 64 independently domain-separated BLAKE3 hash projections, selecting the minimum 64-bit hash of each token for each projection. It groups the sketch into eight bands of eight rows and hashes each band with a separate domain. Projection indices use fixed-width little-endian integers, giving reproducible signatures across supported architectures. The banding parameters and this implementation's actual recall require separate measurement.

The token fallback protects hybrid mode from LSH misses on short names or modest token overlap. It can admit common-word false positives. LSH-only mode can discard true matches, especially aliases or low-overlap strings. A candidate cap can also remove true matches. Held-out blocking recall must be measured independently of model pair quality and assignment quality.

Candidate pre-ranking uses:

1. More exactly equal nonempty normalized fields.
2. Larger field-qualified token Jaccard similarity, compared as exact integer ratios.
3. Earlier original right record index.

`--max-candidates K` defaults to 20 and must be positive. The index retains only the top K candidates while scanning eligible records; model work will consume only those retained records. There is no acceptance threshold at this stage. A candidate records `right_index`, `exact_fields`, token `intersection`/`union`, displayed `token_jaccard` and `matching_lsh_bands`. The diagnostic row also includes the original left subject/location, full right subjects/locations, configured fields/mode, pre-cap `eligible_records` and right-index statistics. Candidate handles bind to the complete right-index identity and cannot be resolved against different right input.

Memory scales with all right `Unit` values and their normalized field strings/token sets, distinct token/band keys and postings, eight band digests per nonempty right record, and one generation-marker slot per right record. Input units retain their normal raw/text/state/subject/header representations, so `raw_bytes` is a source byte count rather than total allocator usage. Left-side memory holds the current record, its token/sketch data and at most K candidate results. A generation counter reuses right-sized scratch slots and resets them on wraparound; it does not retain past left records or query results.

Every query traverses the right marker array after marking relevant postings. Time can therefore scale with total right-record count plus the relevant token/band postings and scored token-set comparisons. Token-heavy or common-token inputs can be expensive even with K=20. The cap bounds retained candidate/model work, not all local blocking computation. No sublinear query-time claim is made.

Six core tests cover normalization and field isolation, scalar/path refusal, exact fields/Jaccard/ties, original offsets, LSH-only matching, candidate ownership, candidate caps and 1,000 repeated queries without growing right-index state. Three real CLI process tests cover files/stdin, quoted CSV rows, right-before-left validation, output before EOF, invalid grammar, empty inputs and unsupported output modes. The actual server probe records all three local modes and sends retained synthetic pairs through existing `ask` with one Score and companion Noul fields, preserving raw distributions and exact token accounting. Those historical calls prepared pair evidence; the subsequent semantic join stage is qualified separately in `join.md`. They do not prove held-out recall or ratify match thresholds. Full identity/cache, refreshed release packages and target-platform qualification remain open.
