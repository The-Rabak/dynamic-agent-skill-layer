---
ticket_id: T14
title: Prove the system is USEFUL — task-outcome A/B harness (layer ON vs OFF)
kind: efficacy
status: blocked
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Non-Goals (efficacy is downstream of V1.7 retrieval); promoted into the ticket set per owner decision 2026-06-09"
source_packet_ref: "promoted from todo #205 (P0)"
feature_home: "efficacy harness (tests/e2e or scripts) over the live stack"
depends_on:
  - T10
  - T13
dependency_type: hard
serves:
  - The one unmeasured number — does the skill layer make a coding agent measurably better
files:
  - scripts/
  - tests/e2e/
  - docs/assessments/
test_command: "layer ON vs OFF A/B run over ≥10 tasks on the live stack + recorded report with per-task scores"
tdd_mode: ralph
---

# Prove the system is USEFUL — task-outcome A/B harness (layer ON vs OFF)

## Serves

Every artifact proves correctness; none proves efficacy. The whole value proposition rests on the one number nobody has measured. This builds the A/B experiment: same tasks with the skill layer ON vs OFF, plus a draft-acceptance-rate metric, with an explicit efficacy gate.

## Scope

- Efficacy harness that runs the same task set with layer ON and OFF against the live stack.
- Committed deterministic scoring rubric (judge prompt recorded in-repo if used).
- Reproducible report: measured ON-vs-OFF delta over ≥10 representative tasks, with per-task scores.
- Draft-acceptance-rate over ≥10 `.pending` drafts from REAL captured sessions (not synthetic).
- An explicit efficacy threshold in the release gate; run summary reports PASS/FAIL.
- **(Amended 2026-06-11, from `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`):**
  - **Pre-registration:** the pass/fail criterion is committed to this ticket BEFORE any measured run (e.g. "ON beats OFF on ≥ X of N paired tasks with no catastrophic regression" — owner sets X/N). Post-hoc readings of noisy means are not a verdict.
  - **Paired design:** every task runs both ON and OFF (same task, same rubric); the report shows per-task paired win/loss/tie and a sign test, not just aggregate means. At N≈10 the paired structure is the only thing that gives the result teeth.
  - **Three honest outcomes, not two:** PASS / FAIL / **UNDERPOWERED**. If the paired result can't distinguish the arms at the pre-registered bar, the report says "underpowered," which is distinct from "no effect" — and neither is spun.
  - **Placebo arm:** a third arm injecting irrelevant-but-plausible skill context at matched token mass (the real server serving a deliberately mismatched scope/corpus — an explicitly-labeled experimental control in the measurement harness, not a production fake). Without it, "relevant skills help" cannot be separated from "any extra context helps/hurts," and that distinction decides T12's investment.
  - **Attribution from the start:** every retrieval pull during measured runs is labeled (SessionStart priming vs mid-session `find_skill`) and lands in the per-task report — do not wait for T15 to start capturing this.

## Scope Fence

- Tasks/corpus come from T10 (real ingestion), not hand-authored shortcuts.
- No faked infrastructure (depends on T13). The placebo arm is an explicitly-labeled measurement control configured on the real stack, recorded as such in the report — never a silent fallback or a production path.
- A null/negative delta is documented honestly with raw data — not hidden or gamed.
- No changing the pre-registered criterion after data exists; a criterion change voids the run.

## Acceptance Criteria

- [ ] Pass/fail criterion pre-registered in this ticket before any measured run; the final report cites it verbatim.
- [ ] Efficacy harness runs the same task set with layer ON and OFF against the live stack, paired per task.
- [ ] Committed deterministic scoring rubric with the judge prompt (if used) recorded in-repo.
- [ ] Reproducible report shows the measured delta (ON vs OFF) on ≥10 tasks, with per-task paired scores, win/loss/tie counts, and a sign test.
- [ ] Placebo arm (matched-mass irrelevant context) run and reported alongside ON/OFF; the ON-vs-placebo comparison is stated explicitly.
- [ ] Per-pull retrieval attribution (priming vs `find_skill`) captured and reported per task.
- [ ] Draft-acceptance-rate over ≥10 `.pending` drafts from REAL captured sessions.
- [ ] Release gate includes an explicit, justified efficacy threshold; run reports PASS / FAIL / UNDERPOWERED.
- [ ] If the delta is null/negative/underpowered, documented in `docs/assessments/` with raw data.

## Local Context

- WHY source: plan `## Non-Goals` names efficacy as downstream; owner decision 2026-06-09 promoted it into the V1.7 ticket set.
- Depends on T10 (real corpus) and T13 (no-fakes substrate). Consumes T08's measured-retrieval handoff.
- T15 (#218 SWE-bench) is the flagship compounding variant of this.

## Source

Promoted 2026-06-09 from todo #205 (P0). Original analysis in git of `todos/205-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
