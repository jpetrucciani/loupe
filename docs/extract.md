# Source-span extraction

`loupe extract QUERY [FILE ...] --candidates KIND` selects verbatim spans from original UTF-8 documents. Each file is a finite document; stdin is read to EOF without operands. The parser assigns `C1`, `C2`, and subsequent IDs in document order. Equal text at distinct offsets remains separate. Every emitted span has original byte bounds and physical line coordinates.

```sh
loupe extract 'Which amount was paid as the settlement?' --candidates money < email.txt
loupe extract 'Which version first introduced the regression?' --candidates semver thread.md --json
loupe extract 'Which email address is for billing?' --candidates emails contacts.txt
```

Plain output defaults to the original selected span followed by LF. `--normalize` converts selected dates, money and versions in code. JSON preserves `unit` and original offsets, adding `normalized.value`, its display `text`, and date precision. Plain/annotate/TSV display normalized text when requested.

```sh
loupe extract 'Which date is the release date?' --candidates dates --normalize \
  --now 2026-10-05T23:30:00-04:00 --tz America/New_York < release.txt
loupe extract 'Which amount is the invoice total?' --candidates money --normalize \
  --currency USD --decimal-separator dot < invoice.txt
```

Offset-bearing timestamps normalize directly to RFC 3339. Offset-free dates/times require IANA `--tz`; day precision uses local midnight. Relative/yearless dates also require RFC 3339 `--now` with an offset. Days/weeks follow the local calendar; month/year offsets clamp to the target month's last valid day. `next`/`last` weekdays are strictly future/past; `this` uses the current ISO week. Ambiguous slash dates require `--date-order mdy|dmy`. Ambiguous/nonexistent local times during timezone changes fail with exit 65.

Money becomes `{amount: DECIMAL_STRING, currency: ISO_CODE}` without binary float arithmetic. Grouping, scientific notation and magnitudes preserve exact digits. Dollar/yen ambiguity requires `--currency`; explicit codes and supported unambiguous symbols supply it. Conflicting markers, unsupported codes and Bitcoin fail. ISO codes use the agency's [2026-09-17 list](https://www.six-group.com/dam/download/financial-information/data-center/iso-currrency/lists/list-one.xml). A lone separator followed by three digits requires `--decimal-separator dot|comma`. Invalid grouping fails. No currency conversion is applied.

Semver removes a leading `v`, preserving prerelease/build metadata. Other kinds preserve text. Inapplicable date/currency options or invalid configuration exit 64 before inference. Missing information in a selected span exits 65. Normalization options and timezone-data version bind journal replay; model requests/probabilities stay unchanged.

| Kind | Candidate forms |
| --- | --- |
| `dates` | ISO dashed dates; RFC 3339 timestamps; ISO local date/time; slash dates with a four-digit year; English month/day and day/month forms with optional ordinal/day punctuation/year; today/yesterday/tomorrow and adjacent-day phrases; next/last/this weekday; `in N days|weeks|months|years` and `N … ago`. |
| `money` | Currency symbols `$ € £ ¥ ₹ ₩ ₽ ₿` or uppercase three-letter code-shaped markers before/after an amount; signs, parentheses, decimal/group separators, apostrophe/NBSP grouping, scientific notation, `k` and spelled thousand/million/billion/trillion magnitudes. |
| `semver` | Three-component SemVer, optional lowercase `v`, prerelease and build metadata. Core numbers and numeric prerelease identifiers reject leading zeros. |
| `emails` | ASCII dot-atom local parts, quoted local parts and dotted ASCII domains or IP literals. Address detection does not establish deliverability. |
| `hostnames` | Dotted ASCII labels and literal lowercase `localhost`; each label is at most 63 characters and the full name at most 253. The final label contains a letter, avoiding version/IP substrings. Domains inside URLs and email addresses can be candidates. |
| `tickers` | Uppercase alphanumeric symbol-shaped tokens, optional `$` or exchange prefix, and dot/dash suffixes. Detection does not consult an exchange listing. Capitalized prose/currency abbreviations can therefore be candidates for the model to reject. |

Aliases are `date`, `versions`, `email`, `hostname` and `ticker`. Token boundaries reject valid-looking fragments of malformed longer tokens. Sentence periods delimit spans. ISO dates/times and fully specified English calendar dates are validated. Yearless dates retain the need for a year; ambiguous slash dates retain locale ambiguity. Money parsing preserves punctuation and currency markers without inferring their meaning. Unsupported syntax is outside this parser catalog, and an empty catalog returns no selected output with exit 1.

Each parsed candidate includes its containing source line as model evidence, plus the preceding N physical lines when `--with-context N` is given. Output remains the exact original span. Candidates and context appear in state and Choice descriptions, contributing to every token/schema limit. A containing line that cannot fit even for one candidate fails with exit 65; it is never silently shortened. Source files are separate documents and IDs restart within each, so source path and byte range identify results across files.

Selection uses the [shared tournament](selection-tournament.md), [presence/none gates](selection-policy.md), exact or explicitly estimated counts, budget admission, fresh identity/cache policy, deadlines and cancellation. `-n/--take N` defaults to one. Overflow carries top-k eligible candidates and retains only final-bracket marginals. Noul query files/trusted presets and presence criteria are accepted; Choice/Score rubrics are exit 64. Threshold/margin govern presence and confidence/gap govern Choice. `--keep-blank`, independent ranking, structured/record input, classification packing, split/combine/focus/truncation and keep-going flags are inapplicable.

Annotated/TSV output has location, candidate ID, marginal probability and verbatim text; TSV escapes embedded controls. JSON retains common `index`, `unit`, `answer`, `model`, `cached` fields, plus `candidate_type`, `id`, `location`, `probability`, `probability_scope`, uncertainty/restoration and complete request/response history. `answer` is the original final Choice API answer. `index` is the candidate's starting physical line; the ID and offsets distinguish multiple spans on one line. `--dry-run` shows the actual catalog and first-round requests/counts without inference, marking later rounds conditional.

Uncertainty capture appends the original whole document once, retaining original bytes. The [document run journal](failure-policy.md) saves complete original responses for validated `--resume`, including explicit uncertainty failure. Replay binds candidate parser kind/version as well as original input, query, policy and request plans. Completed responses need no new inference; generic stdout/capture acknowledgement gaps remain as documented. Parser changes require updating their journal identity version.

Memory grows with document size, candidate metadata, containing-line evidence and bounded round history. The containing-line pass is linear in lines plus candidates, before serialization/encoding. Repeated context can make request/history storage larger than the original document. This is a finite-document command.

Qualification includes direct parser tests, real process/file contracts and actual server selection for all six families, original Unicode/CRLF offsets, punctuation, malformed spans, absence, uncertainty actions and zero-inference journal replay. These synthetic fixtures do not establish held-out extraction quality, normalization coverage or exchange/address validity. Full live identity/cache and release packaging remain separate qualification work.

`--on-uncertain fallback` resolves presence before an eligible uncertain Choice. The trusted command receives compact JSON of the actual bracket state plus LF and prints one declared ID or `none`. Each external bracket contributes one winner, so top-N can return fewer than N. JSON preserves raw answers, adds per-field `fallback`/`selection_decisions`, and sets external `probability:null` with the original value in `model_probability`. Annotate/TSV leave that probability empty. External confidence is null. See [fallback](fallback.md) for bounds, deadline and failure policy.
