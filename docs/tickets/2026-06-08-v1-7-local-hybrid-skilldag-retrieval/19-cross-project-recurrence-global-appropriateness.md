---
ticket_id: T19
title: Global appropriateness via cross-project recurrence (#180) — DEFERRED, unmeasurable on a single-project corpus
kind: expansion
status: deferred
status_note: "Deferred 2026-06-12 (restructure): the entire 262-skill corpus comes from 24 sessions of ONE project; cross-project recurrence has zero data to measure against, so this cannot 'earn its place via measured delta' as the scope fence demands. Gate: ≥2 project corpora ingested through the real pipeline."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Agent usefulness targets (global appropriateness)"
source_packet_ref: "extracted from T12 (restructure 2026-06-12); originally todo #180 via #220"
feature_home: "crates/retrieval scope classification + crates/graph-builder recurrence aggregation"
depends_on: []
dependency_type: none
serves:
  - Global-scope skill classification driven by observed cross-project recurrence instead of a static flag
files:
  - crates/retrieval/src/
  - crates/graph-builder/src/
test_command: "cross-project recurrence sweep over ≥2 real project corpora on the live stack (UNRUNNABLE until the data gate clears)"
tdd_mode: ralph
---

# Global appropriateness via cross-project recurrence (#180) — DEFERRED

## Serves

"Global appropriateness" (which skills deserve global scope) should be defined by observed
cross-project recurrence, not a static flag. Extracted from T12 so the priming work is not held
hostage by a signal that is currently unmeasurable.

## Why deferred (the data gate)

The constitution and the T12 scope fence require every signal to earn its place via measured quality
delta on the real corpus. Cross-project recurrence is a function of *multiple* project corpora; the
dogfood corpus (T10) is 262 skills from 24 sessions of this one repository. There is no second
project in the data layer, therefore no recurrence signal exists to measure, therefore any
implementation now would ship unmeasured — which the fence forbids. Deferral is the honest state.

**Unblock condition (explicit):** a second real project corpus (≥50 skills, ingested end-to-end
through the real pipeline per the T10 recipe) exists in the data layer under its own scope. When that
exists, revive this ticket with the T11/T18 instrument discipline: pre-registered recurrence
threshold, negative control (a project-unique skill must NOT classify global), paired verdicts.

## Scope (when revived)

- Define recurrence: the same skill (or merge-equivalent) independently extracted in ≥K distinct
  project scopes.
- Classification job proposes scope promotion to global; human gate approves (no auto-promotion —
  lifecycle governance unchanged).
- Measure: pre-registered precision bar on a labeled sample of proposed promotions; negative control
  as above.

## Scope Fence

- No static "global" flag heuristics shipped under this ticket's name.
- No auto-approval of scope promotion; the human lifecycle gate is untouched.
- Do not start before the data gate clears — building unmeasurable mechanism is the exact failure
  mode the restructure removed from T12.

## Acceptance Criteria

- [ ] Data gate cleared: ≥2 real project corpora live (recorded with corpus inventory evidence).
- [ ] Recurrence definition + thresholds pre-registered before any measured run.
- [ ] Negative control passes (project-unique skills do not classify global).
- [ ] Proposed promotions flow through the existing human lifecycle gate; measured precision recorded.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
