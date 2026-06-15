---
unit: "Unit B — Author session-start stratum"
unit_number: 2
unit_kind: infra-packet
serves: "The priming distribution the T11 fixture lacks; the stratum T12's signals are measured on"
status: completed
attempt_count: 1
domains: [measurement, fixtures, python]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/18-priming-instrument-session-start-stratum.md
session_id: work-2026-06-14-224314-T18
---

## What Was Implemented
`scripts/build_t18_session_start_stratum.py` (offline authoring + validation) extends
`tests/fixtures/retrieval_quality_262_corpus_labeled.json` from 162 → 184 queries with a NEW
`session_start` stratum: 22 queries from the 24 sessions' genuine OPENING problem statements
(`session_problems.json`), 11 thin + 11 verbose substrata, multi-gold `relevant` = `skills_in_session`,
`fresh_golds` tagged (skill absent from any lexicographically-earlier session).

## Files Changed
- `scripts/build_t18_session_start_stratum.py` — created (authoring pipeline + anti-circularity probe + idempotency guard)
- `tests/fixtures/retrieval_quality_262_corpus_labeled.json` — extended (162→184; existing 162 verified untouched; `_strata`/`_counts` metadata updated)
- `tests/e2e/reports/retrieval/session_start_anticircularity.json` — created (per-query overlap artifact)

## Validation (orchestrator independent check)
- 184 total / 22 session_start / 162 other (matches; existing queries intact).
- substrata 11 thin / 11 verbose; splits 14 tuning / 8 held_out; multi-gold sizes 3–16 (mean 9.5);
  all 22 have ≥1 fresh gold; thin text mean 66 chars vs verbose 588 chars (verbose genuinely long →
  reproduces the Finding-2 distribution).
- Anti-circularity probe: mean Jaccard **0.024**, max 0.056, 0 drops (reject gate ≥0.6).

## Key flag carried to Unit C
Overlap 0.024 is FAR below the ~0.3 reference band. Correct for CIRCULARITY (no token-match shortcut),
but it sharpens the real validity question: are these conversational openings retrievable enough that
true-scope coverage SEPARATES from the permutation control? **Unit C's negative-control gate is the
empirical adjudicator** — if true-scope coverage does not crater the permuted control (no separation
`S`), the stratum is INSTRUMENT-FAILURE(priming-stratum) and the thin queries need slightly more
grounding. Proceeding to C as the pre-registration designed.

## Assumptions (agent-reported, accepted)
- Freshness heuristic = skill not in any prior session (lexicographic). Honest gradient; flagged.
- ~0.3 band was calibrated on task-shaped transcript/disjoint strata; conversational openings naturally
  sit far lower — the binding gate is the ≥0.6 reject, which held.
- 16/24 sessions had substantive openings (8 opened with meta-steering like "commit and push").

## Test Results
- Command: `python3 scripts/build_t18_session_start_stratum.py` → PASS (idempotency guard fails-loud on re-run).
- Attempts: 1
