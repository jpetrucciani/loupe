# Candidate ranking with `pick`

`loupe pick QUERY [FILE ...] -n N` emits up to N original lines in rank order. Each file is a finite document; stdin is read to EOF when there are no file operands. Physical `L<n>` IDs, Unicode byte offsets, preceding-line context and source validation follow [find](find.md). Plain output is the original line, including its terminator; an unterminated selected line receives LF for display.

```sh
loupe pick 'Which lines describe checkout outages?' incidents.txt -n 3 --json
loupe pick 'Which lines describe checkout outages?' incidents.txt -n 3 --independent
loupe pick -p relevant --arg 'query=Which lines describe checkout outages?' incidents.txt --independent
```

The default uses the [shared Choice tournament](selection-tournament.md), carrying top-k from each eligible bracket when candidates exceed the token or option limits. Returned probabilities belong to the final bracket. JSON labels their scope `all-candidates` when one request covers the complete catalog, or `final-surviving-bracket` after overflow. They are conditional marginals, and the presence/`none` gates determine whether any candidates are emitted. Exact Choice ties use sorted key order and are flagged uncertain.

`--independent` preflights one request per candidate. Each contains that candidate's evidence, the fixed query/context, a presence Noul, a Choice between its ID and `none`, and a relevance Score. Candidates never see other candidates' evidence. Fields within each request are still decided jointly. This costs one submitted request per candidate and retains its own probabilities, rather than producing a normalized distribution across candidates.

The default Score has three levels: unrelated (0), related context (1), and direct evidence (2). A Score question file or preset supplies its own ordered levels. Ranking uses `sum(level * probability) / sum(probability)` from the returned probabilities. The division accommodates rounded API probability mass. Original probabilities and the backend's reported `score` remain unchanged in the raw answer. Candidates with equal derived values sort by source order. Only eligible candidates enter the top-N result.

Noul rubrics supply presence criteria/policy; Score rubrics supply Score levels/policy. Choice rubrics are exit 64. `--threshold` and `--margin` apply to presence. `--min-confidence` and `--min-gap` apply to Choice in default mode and Score in independent mode. Exact Choice ties remain uncertain in independent mode. Certain absence suppresses output even if Score is uncertain; an uncertain gate or eligible uncertain ranking follows `--on-uncertain keep|drop|stderr|fail`. Keeping uncertainty never turns `none` into a source span. `--uncertain FILE` appends the original document once when uncertainty occurs, preserving bytes.

| Output | Default Choice | Independent Score |
| --- | --- | --- |
| Plain | Original selected line | Original selected line |
| Annotate/TSV | Location, ID, marginal, verbatim text | Location, ID, derived Score mean, verbatim text |
| JSON `answer` | Complete raw final Choice answer | Complete raw Score answer for this candidate |
| JSON ranking field | `probability` | `ranking_score` |
| JSON `probability_scope` | `all-candidates` or `final-surviving-bracket` | `independent-candidate-score` |

JSON also includes `index`, `unit`, `model`, `cached`, `id`, `location`, `uncertain`, `restored` and `provenance`. Independent output includes `score_calculation=normalized_weighted_mean_of_returned_level_probabilities`. History retains every exact plan/count and original backend evaluation. `cached` refers to the response supplying that result's answer. A Score mean is not a probability of relevance, and independent candidate requests alone do not establish multi-select calibration.

`--dry-run` emits actual requests and counts without inference. Choice later rounds are conditional; independent plans include every singleton request. `--max-rounds` applies only to Choice mode and explicitly supplying it with `--independent` is exit 64. Input formats other than whole documents, classification packing, split/combine/focus/truncation and keep-going flags are inapplicable. Source aliases, calibration pins for incompatible question/view shapes and malformed rubrics are refused before inference.

Fresh identity discovery, caching, exact submitted-token admission, retries, shared deadlines, cancellation and document-order output use the existing [runtime contracts](failure-policy.md). Independent requests run within configured concurrency. No candidates from a partially failed document are emitted. Complete raw responses saved in `--failed FILE` can be validated and re-evaluated through `--resume FILE` without inference, including an explicit uncertainty failure. Replay binds mode, Score criteria, query/policy, original input and plans. Generic output/capture acknowledgement gaps remain documented in the run journal.

Memory grows with the finite document, candidate catalog and retained request history. Independent mode stores one request/evaluation per candidate; its cost is linear in candidate count. Token and schema limits are checked for every singleton before submission. Oversized singleton evidence is exit 65.

Qualification uses mathematical reducers, real CLI/filesystem processes and actual server inference. Positive/absent cases, both modes, Score preset levels, uncertainty actions, exact accounting, original spans and zero-inference replay are covered. These synthetic functional cases do not close the later held-out retrieval/out-of-scope quality gate. The measured noun-phrase presence false negative in [find evidence](find.md) also remains relevant to `pick`.

`--on-uncertain fallback` resolves presence before an eligible uncertain Choice. The trusted command receives compact JSON of the actual bracket state plus LF and prints one declared ID or `none`. Each external bracket contributes one winner, so top-N can return fewer than N. JSON preserves raw answers, adds per-field `fallback`/`selection_decisions`, and sets external `probability:null` with the original value in `model_probability`. Annotate/TSV leave that probability empty. Independent mode additionally resolves eligible uncertain Scores, ranks each external numeric value and sets `score_calculation:"external_score"`. External confidence is null. See [fallback](fallback.md) for bounds, deadline and failure policy.
