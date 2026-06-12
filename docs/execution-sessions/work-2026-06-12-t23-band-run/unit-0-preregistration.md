---
unit: "Pre-registration amendment — auto-gate, roster lock, unattended policy"
unit_number: 0
unit_kind: infra-packet
serves: "Legal foundation for the automated band — the amendment that makes auto-accept-all legitimate must land BEFORE the first paired band datum (the only window in which protocol amendments are valid)."
status: completed
attempt_count: 1
domains: [pre-registration, docs, protocol]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/23-automated-clband-run.md
session_id: work-2026-06-12-t23-band-run
---

## What Was Implemented

Authored and committed the **CL Acquisition-Band AUTO-GATE Amendment** — the pre-registration that
makes the unattended overnight run legitimate. Committed BEFORE any band data exists (the only valid
amendment window; after the first paired datum, changes VOID the affected run).

Two documents amended:
1. **T14 ticket** (`14-efficacy-task-outcome-ab-harness.md`): new LOCKED block after the existing
   "CL Acquisition-Band Pre-Registration Deltas". Six numbered clauses:
   - (1) `gate_mode=auto-accept-all`, `clband-*` scopes ONLY, via the REAL rename acceptance action +
     real scope rebuild; production human gate + 262 dogfood corpus UNTOUCHED; hard scope assertion,
     fail loud on non-clband; post-run dogfood re-probe is a T23 AC.
   - (2) Why accept-all not a filter: a filter = unvalidated judge in the pipeline (unreproducible);
     accept-all is reproducible AND conservative (ON faces the unpruned draft set; ON-win = lower
     bound vs human-gated production).
   - (3) `gate_mode` recorded verbatim in every run report + the verdict; auto-gate log lists every
     acceptance with its scope assertion.
   - (4) Roster + substitution unchanged (8 full + 3 alternates; substitution only via OFF pre-gate).
   - (5) Unattended continue/stop policy pre-committed (harness breakage ⇒ STOP+checkpoint;
     per-context INSTRUMENT-FAILURE ⇒ record+continue; OFF-pass sibling ⇒ drop; context losing all
     siblings ⇒ next alternate; standing laws hold).
   - (6) Solver re-pin: claude-code 2.1.175 (smoke was 2.1.173); OFF pre-gate re-runs per context so
     the solver bump is satisfied by construction; dataset sha pinned.
   Explicitly does NOT touch the ≥7/10 criterion, the INSTRUMENT-FAILURE taxonomy, the roster, the
   instruments, or the dogfood/production gates.
2. **CL-band plan §4 Step 2** (`2026-06-12-t14-cl-acquisition-band-plan.md`): "human gate" → "gate";
   added a blockquote noting the band's `gate_mode=auto-accept-all` (clband-* only) pointing at the
   LOCKED T14 amendment; "the smoke used the human gate; the band uses auto-accept-all."

STATE.md also carries the roster lock + unattended policy (operational copy); the LEGAL
pre-registration lives in the T14 ticket.

## Files Changed
- `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md` — modified (auto-gate amendment block)
- `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` — modified (§4 Step 2 gate-mode note)
- `docs/execution-sessions/work-2026-06-12-t23-band-run/STATE.md` — created
- `docs/execution-sessions/work-2026-06-12-t23-band-run/unit-0-preregistration.md` — created

## Problems Encountered
None. (One Edit line-wrap mismatch on the plan, corrected by re-reading the exact lines.)

## Patterns Discovered
- The pre-registration amendments in this project follow a consistent "LOCKED <date> — committed
  BEFORE <X>" header convention with a void-on-change clause; the new amendment matches it verbatim.
- The structural acceptance definition is centralized in `scripts/efficacy_draft_acceptance.py`
  (accepted iff sibling `.md` without `.pending` exists) — the auto-gate must drive exactly that
  rename, never a DB shortcut.

## TDD Evidence
Docs-only unit (pre-registration). No code; no Ralph cycle. Evidence = the committed amendment lands
before any band datum (verified: no band run has executed; `tests/e2e/reports/efficacy/clband-band/`
does not yet exist). The commit hash is the timestamped proof of the amendment window.

## Test Results
- Command: n/a (docs pre-registration)
- Result: amendment committed BEFORE first band datum — PASS (window honored)
- Attempts: 1
