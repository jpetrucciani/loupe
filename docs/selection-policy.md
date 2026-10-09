# Selection presence and uncertainty

The shared selection session applies a presence Noul and a Choice policy to each original joint response before tournament reduction. It preserves raw probabilities, locally computed confidence/gap, original candidate IDs and bracket bindings. `find`, `pick` and `extract` use this session to submit requests, emit diagnostics and write uncertainty files.

Presence uses the existing strict margin rule and includes threshold equality as yes. Choice uses local maximum probability, minimum confidence/gap and sorted-key tie handling. A deterministic provisional choice remains uncertain when tied. With `--ensemble`, each head applies this policy to its mean distribution, with an additional agreement gate. Raw member answers remain separate; mean confidence does not replace the original model field confidence.

The precedence is:

| Decision | Result |
| --- | --- |
| Presence falls within its uncertainty margin | Uncertain, including when Choice selects `none`. |
| Presence is a certain no | Absent, regardless of Choice's confidence or winner. |
| Presence is a certain yes and `none` wins | Absent, regardless of a candidate confidence gate. |
| Presence is a certain yes, a candidate wins and Choice is uncertain | Uncertain. |
| Presence is a certain yes and a candidate passes Choice policy | Selected. |

A certain absence produces no span. Uncertainty follows the same actions as unit commands: `keep` retains the provisional candidate ranking, `drop` omits it, `stderr` produces diagnostics, and `fail` returns exit 3. `fallback` resolves the gate first, then an eligible uncertain Choice or independent Score, using a trusted external command. An external Choice carries only its single declared winner; a failed external Choice stays uncertain even when the raw answer chose `none`. See [fallback.md](fallback.md). Keeping uncertainty never substitutes a candidate for a winning `none`. An uncertain presence gate with `none` therefore still has no selected output; its raw decision remains available for review or capture.

These rules apply to preliminary brackets as well as the final bracket. Only eligible brackets carry their top-k candidates. A dropped uncertain bracket remains recorded as uncertain; when no candidates survive, the outcome distinguishes that uncertainty from confirmed absence and does not invent a final distribution. If a later bracket selects a candidate, it reports that final population's probabilities and separately retains the earlier uncertainty count. Confidence/gap thresholds must be qualified with the same packing and tournament policy.

Successful selected output has exit 0. Absence or omitted uncertainty has exit 1. Explicit uncertainty failure has exit 3. Transport, protocol, budget and other fatal errors retain their existing precedence and are not converted to absence. The session rejects incomplete/mismatched rankings, changed policies, replayed rounds and attempts to restart an advanced session. Empty candidate sets finish without inference.

`Decision` serialization records status/reason, the original presence probability, both local assessments and the original Choice ranking. `Outcome` records tournament population, output destination, exit code and uncertain bracket count. Internal plan/policy bindings are excluded from diagnostic serialization; these internal workspace APIs are not a published stable library.
