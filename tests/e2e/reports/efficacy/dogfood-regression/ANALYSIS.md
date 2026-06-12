# T22 Unit C — dogfood extraction-regression gate (analysis)

**Question (hard guardrail).** The taught-knowledge prompt change (`EXTRACT_TEACH_CAPTURE`, default ON)
produces the SAME 262 organic corpus and may NOT degrade it. Does default-ON degrade organic extraction?

**Method.** 3 organic session transcripts re-extracted through the REAL maintenance-worker
(`EXTRACT_SESSION_PROVIDER=claude-code`, frontier) under BOTH arms — `EXTRACT_TEACH_CAPTURE=off`
(pre-T22 prompt, byte-for-byte) and `=on` (candidate default) — into isolated scratch scopes. The flag
is the only variable. Raw: `regression.json`, `run.log`, `logs/*.log`. Driver: `scripts/dogfood_regression.py`.

## Result: CLEAN — no degradation

| session | off drafts | on drafts | Δ |
|---|---|---|---|
| 273eadb7 (40 KB) | 11 | 12 | **+1** |
| 52894a16 (51 KB) | 3 | 3 | 0 |
| 254281af (65 KB) | 7 | 7 | 0 |
| **total** | 21 | 22 | **+1** |

**Draft COUNT is flat** (+1 across 3 sessions). The feared failure mode — taught-capture making the
extractor manufacture filler from non-taught organic content — did NOT occur. The taught section's
explicit fence ("does NOT lower the bar for ORGANIC sessions … a throwaway is STILL an empty result")
held.

**Draft QUALITY is equivalent.** The `only_off`/`only_on` name-level differences are almost entirely
the SAME skills reworded — e.g.:
- off `enforce-invariants-structurally-not-procedurally` ↔ on `Enforce invariants mechanically, not by convention`
- off `Use real infrastructure (docker compose setup) for all` ↔ on `Always use the project's Docker Compose setup for`
- off `comprehension-before-commitment` ↔ on `Complete Context is a Hard Prerequisite Before Any Action`
- off `proxy-metric-validation-antipattern` ↔ on `Never Accept a Proxy Signal as Proof of the Real Invariant`

No good skill was dropped; no literal-dump or verbose junk appeared in the ON arm. The organic
sessions remained ordinary preference/best-practice/anti-pattern extractions.

## Honesty caveat (scope of the claim)

The OFF and ON arms are SEPARATE nondeterministic claude-code runs, so a share of the name-level
differences is run-to-run variance, NOT attributable to the prompt change. The robust signals are
(a) **count stability** (+1 total) and (b) **semantic equivalence** of the two skill sets. A fully
controlled diff would need N runs per arm; for a smoke-level regression gate over 2-3 organic sessions
(per the ticket) this is sufficient to clear "no degradation," and is reported as such — not as a
zero-variance proof.

## Verdict

**Regression gate: PASS.** Default-ON does not degrade organic dogfood extraction. The owner-approval
decision for the default (DP-1) can proceed on this evidence.
