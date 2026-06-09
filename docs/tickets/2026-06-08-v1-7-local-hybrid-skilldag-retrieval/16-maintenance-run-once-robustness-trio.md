---
ticket_id: T16
title: Maintenance run-once robustness trio
kind: hardening
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "constitution: fail-loud, no silent no-ops, no arbitrary caps"
source_packet_ref: "promoted from todo #222 (P2)"
feature_home: "crates/maintenance"
depends_on: []
dependency_type: none
serves:
  - A maintenance worker that boots provider-agnostically and never silently no-ops the drain
files:
  - crates/maintenance/
test_command: "cargo test --workspace (maintenance trio covered) — no fakes"
tdd_mode: ralph
---

# Maintenance run-once robustness trio

## Serves

Three robustness gaps found live while driving the real maintenance drain. Independent of retrieval; fail-loud / no-silent-no-op hardening.

## Scope

- A: claude-code provider boots without requiring `OLLAMA_URL`.
- B: run-once with an undrained queue either drains or fails loud — never a silent no-op.
- C: empty/oversized embedding inputs are guarded; one skill's embed failure does not abort the whole run.

## Scope Fence

- No silent fallbacks; fail loud at the seam per the standing rule.
- No fakes in the covering tests (live infra).

## Acceptance Criteria

- [ ] A: claude-code provider boots without requiring `OLLAMA_URL`.
- [ ] B: run-once with an undrained queue either drains or fails loud — never a silent no-op.
- [ ] C: empty/oversized embedding inputs guarded; one skill's embed failure does not abort the run.
- [ ] Tests cover each; `cargo test --workspace` green; no fakes.

## Local Context

- WHY source: discovered live driving the real drain; constitution fail-loud mandate.
- Independent of the retrieval/efficacy spine — parallel-safe, slot anywhere.

## Source

Promoted 2026-06-09 from todo #222 (P2). Original analysis in git of `todos/222-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
