---
unit: "3-arm A/B runner + scoring + attribution + gate + draft-acceptance scorer"
unit_number: 3
unit_kind: expansion
serves: "The harness that turns the battery into the single honest PASS/FAIL/UNDERPOWERED/INSTRUMENT-FAILURE verdict; reuses T20 stats"
status: completed
attempt_count: 2
domains: [efficacy, testing, scripts, stats]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
session_id: work-2026-06-12-122502-T14
---

## What Was Implemented
- `scripts/efficacy_ab.py` — 3-arm runner + gate: spec validation against CONTRACT, verifier exit→win/loss
  mapping, attribution parser (session_start_priming vs mid_session_find_skill), `classify_efficacy_verdict`
  implementing the LOCKED gate (reuses `retrieval_metrics.sign_test`, no stats fork), report emitter
  (criterion printed verbatim), `--dry-run` (no model calls), `--self-test`.
- `scripts/efficacy_draft_acceptance.py` — draft-acceptance scorer; FAILS LOUD on <10 real `.pending` drafts.
- `scripts/settings-efficacy-on.json` / `scripts/settings-efficacy-placebo.json` — ON + PLACEBO claude-code
  wiring (SessionEnd capture intentionally OFF during measured runs to avoid corpus contamination; placebo
  explicitly labeled as a measurement control, not a production fake).
- `scripts/test_efficacy_ab.py` — 51 unit tests.

## Files Changed
- scripts/efficacy_ab.py, scripts/efficacy_draft_acceptance.py, scripts/settings-efficacy-on.json,
  scripts/settings-efficacy-placebo.json, scripts/test_efficacy_ab.py — created

## Problems Encountered
### Problem 1: UNDERPOWERED vs FAIL edge case
- **Error:** 5 ON-wins vs 5 OFF-wins classified FAIL.
- **Root cause:** FAIL guard used `off_wins >= on_wins`; the LOCKED text says a tie (sign test cannot
  distinguish) is UNDERPOWERED. FAIL requires OFF to strictly beat ON.
- **Fix:** FAIL guard → `off_wins > on_wins`; documented the invariant.

## Patterns Discovered
- `retrieval_metrics.sign_test(n_a_better, n_b_better)` is the clean reuse point — no adapter.
- Keep the claude-code invocation behind one function so Unit 4 runs it for real and unit tests stub it.
- `_comment` keys in settings JSON are ignored by claude-code → use them to label the placebo control.

## TDD Evidence
- **Red:** `python3 scripts/test_efficacy_ab.py` → ModuleNotFoundError (51 tests, module absent).
- **Green:** 51/51 after the UNDERPOWERED-vs-FAIL fix; `--dry-run` over the real battery exit 0.
- **Post-Refactor Green:** removed unused/duplicate imports; 51/51 + `--dry-run` still green.
- Live e2e (real claude-code solves over HTTP) intentionally DEFERRED to Unit 4 (orchestrator-serialized);
  `--dry-run` is this unit's e2e stand-in.

## Test Results
- Command: `python3 scripts/efficacy_ab.py --dry-run --tasks tests/e2e/efficacy/tasks/ && python3 scripts/test_efficacy_ab.py`
- Result: PASS (dry-run exit 0; 51/51 tests). Orchestrator re-verified independently: same.
- Attempts: 2
</content>
