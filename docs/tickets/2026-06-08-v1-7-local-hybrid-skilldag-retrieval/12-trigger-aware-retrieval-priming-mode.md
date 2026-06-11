---
ticket_id: T12
title: Trigger-aware retrieval — priming mode + recurrence-based global + freshness slot
kind: expansion
status: blocked
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Agent usefulness targets + ## Retrieval Flow"
source_packet_ref: "promoted from todo #220 (P1)"
feature_home: "crates/retrieval and crates/compiler (SessionStart priming path)"
depends_on:
  - T10
  - T11
dependency_type: hard
serves:
  - SessionStart priming vs mid-session task retrieval as distinct, measured intents
files:
  - crates/retrieval/src/
  - crates/compiler/src/
  - scripts/retrieval_quality_live.py
  - tests/e2e/reports/
test_command: "real-server priming-mode quality sweep on the T10 corpus (#210 rig) + unit tests for ranker signals"
tdd_mode: ralph
---

# Trigger-aware retrieval — priming mode + recurrence-based global + freshness slot

## Serves

Retrieval currently treats SessionStart priming and mid-session `find_skill` identically. SessionStart should PRIME (centrality + recent usage + a freshness slot for high-value brand-new skills), while prompt/`find_skill` does task retrieval. "Global appropriateness" should be defined by cross-project recurrence (#180), not a static flag. Each signal must earn its place via measured quality delta on the real corpus.

## Scope

- Make retrieval trigger-aware: SessionStart → priming, prompt/`find_skill` → task retrieval; both documented and matching code.
- Implement a priming ranker (centrality + recent usage + bounded freshness slot, bounded N).
- Define global appropriateness via cross-project recurrence (#180).
- Measure each signal's MRR/nDCG impact on the T10 corpus (#210 rig); drop signals that don't help.
- **(Amended 2026-06-11)** Measure priming on the PRIMING query distribution: coordinate with T11's fixture authoring so it includes a session-start stratum (thin/vague session-opening prompts — the distribution priming actually serves), distinct from the specific task-query strata. Priming signals evaluated against task-shaped queries would answer the wrong question.
- **(Amended 2026-06-11)** Pre-register per-signal ROI thresholds BEFORE any measured sweep: for each signal (centrality, recent-use, freshness), record in this ticket what minimum paired quality delta keeps it. "Drop signals that don't help" only has teeth if "help" is defined before the data exists.

## Scope Fence

- Priming must stay within the constitutional 500ms SessionStart budget.
- No LLM call on the SessionStart hot path.
- Signals that don't measurably help are dropped, not shipped.
- Do not blindly inject more context — priming surfaces a bounded, high-value set.

## Acceptance Criteria

- [ ] Retrieval is trigger-aware: SessionStart → priming, prompt/`find_skill` → task retrieval; documented and matching code.
- [ ] Priming ranker (centrality + recent usage + freshness slot, bounded N) implemented; MRR/nDCG impact measured on the T10 corpus; a thin/empty session-start prompt surfaces high-value project baseline skills incl. a relevant brand-new one.
- [ ] Global appropriateness defined via cross-project recurrence (#180), measured, matching code.
- [ ] Each signal's measured quality delta recorded; non-helping signals dropped.
- [ ] Per-signal ROI thresholds were recorded in this ticket BEFORE the measured sweeps ran; the keep/drop decisions cite them.
- [ ] Priming measured on a session-start query stratum (from the T11 fixture), not only on task-shaped queries.
- [ ] T15 (#218) source-attribution (priming vs find_skill) reviewed and used to scope investment.

## Local Context

- WHY source: plan `## Agent usefulness targets`; referenced in the plan's source_docs (todo #220).
- Needs the T10 corpus to measure; the freshness slot connects to the cold-start concern (#217).
- Coordinates with T15 (#218) which captures retrieval-source attribution per pull.
- **Amendment 2026-06-11:** T11 added as a hard dependency — every measured claim in this ticket runs through the quality instrument, and T11's midpoint-assessment findings showed the prior instrument could not see arm differences (saturated fixture, mean-equality verdicts). Measuring priming signals on a broken ruler would ship noise as ROI. See `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`.

## Source

Promoted 2026-06-09 from todo #220 (P1). Original analysis in git of `todos/220-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
