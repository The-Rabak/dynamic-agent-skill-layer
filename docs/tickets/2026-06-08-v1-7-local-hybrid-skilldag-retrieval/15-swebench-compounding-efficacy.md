---
ticket_id: T15
title: Flagship efficacy proof — SWE-bench Lite compounding self-improvement
kind: efficacy
status: blocked
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Non-Goals (downstream); promoted into the ticket set per owner decision 2026-06-09"
source_packet_ref: "promoted from todo #218 (P0)"
feature_home: "efficacy harness + SWE-bench integration over the live stack (scripts/swebench)"
depends_on:
  - T10
  - T14
dependency_type: hard
serves:
  - The strongest evidence: the system COMPOUNDS — learns from its own sessions and gets transferably better
files:
  - scripts/swebench/
  - tests/e2e/
  - docs/assessments/
test_command: "SWE-bench Lite layer-ON vs OFF on a disjoint held-out test set, difference-of-differences with CI"
tdd_mode: ralph
---

# Flagship efficacy proof — SWE-bench Lite compounding self-improvement

## Serves

The thesis is not "context helps" — it is "the system compounds: it learns from its own sessions and gets better over time." This proves it against SWE-bench Lite, measuring transferable improvement (not memorization, not variance), and organically generates real corpus while exercising every observable surface.

## Scope

- Spike: confirm SWE-bench Lite runs through Claude Code with our hooks wired (or document the integration path).
- Commit metric + thresholds + instance/seed counts in advance.
- **(Amended 2026-06-11)** The advance commitment must include the **minimum detectable effect** at the chosen instance/seed counts: state up front what size of difference-of-differences the design can resolve. If the measured CI spans zero at that power, the report says **UNDERPOWERED** — a distinct outcome from "no effect," and neither is spun. (Same three-outcome honesty as T14.)
- Full per-run instrumentation: score, unique skills created, ranks, communities, scope, per-instance retrieval pulls (skill + timing + score), post-session store deltas.
- Retrieval-source attribution per pull (SessionStart priming vs mid-session `find_skill`), reported per passing instance (feeds T12/#217/#210).
- Headline: layer-ON vs OFF on a DISJOINT held-out TEST set with a control arm, as difference-of-differences with variance/CI.
- Same-set 3-run trajectory reported as narrative, flagged as memorization-susceptible and NOT the primary claim.

## Scope Fence

- #217 (cold-start) resolved before measured runs.
- No faked infrastructure (T13). Corpus from real runs (T10), not shortcuts.
- The primary claim is the disjoint-test difference-of-differences, not the same-set trajectory.

## Acceptance Criteria

- [ ] Spike confirms SWE-bench Lite runs through Claude Code with hooks wired (or documents the chosen path).
- [ ] #217 (cold-start) resolved before the measured runs.
- [ ] Committed-in-advance metric + thresholds + instance/seed counts + minimum detectable effect; final report classifies the outcome PASS / FAIL / UNDERPOWERED against that pre-registration.
- [ ] Full per-run instrumentation captured (scores, skills, ranks, communities, scope, retrieval pulls + timing + score, store deltas).
- [ ] Retrieval-source attribution captured per pull and reported per passing instance.
- [ ] Headline: layer-ON vs OFF on a disjoint held-out test set, control arm, difference-of-differences, variance/CI.
- [ ] Same-set 3-run trajectory reported as narrative, explicitly flagged as not the primary claim.

## Local Context

- WHY source: plan `## Non-Goals` (downstream); owner decision 2026-06-09 promoted into the V1.7 ticket set.
- Depends on T10 (corpus) and T14 (A/B harness it extends). Coordinates with T12 (priming attribution).
- **(Restructure 2026-06-12)** Conditional: if T14's invented-rule positive control shows the layer's
  value concentrating in non-pretrained knowledge, add a small CL-bench-shaped instance arm here
  (taught novel procedure → held-out reuse); otherwise the disjoint difference-of-differences stays
  the sole headline. Full DS-025–030 contracts remain parked until after this ticket either way.

## Source

Promoted 2026-06-09 from todo #218 (P0). Original analysis in git of `todos/218-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
