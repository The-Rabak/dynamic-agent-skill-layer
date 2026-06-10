---
ticket_id: T05
title: Typed skill graph storage and cold-start edge proposals
kind: expansion
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 5"
feature_home: "crates/graph-builder and persistence graph schema"
depends_on:
  - T03
dependency_type: hard
serves:
  - SkillDAG-style structural retrieval without graph-as-blind-boost
files:
  - crates/graph-builder/src/graph/
  - crates/infrastructure/src/persistence/
  - crates/infrastructure/migrations/
  - docs/reference/retrieval-contract.md
test_command: "cargo test -p graph-builder && cargo test -p infrastructure persistence"
tdd_mode: ralph
---

# Typed skill graph storage and cold-start edge proposals

## Serves

Persist typed inter-skill edges so graph structure can be returned as separate evidence instead of a ranking multiplier.

## Scope

- Add typed edge storage and history with `depends_on`, `specializes`, `composes_with`, `similar_to`, and `conflicts_with`.
- Store origin, confidence, reason, evidence, and timestamps.
- Validate directed backbone constraints and conflict semantics.
- Add cold-start edge proposal generation using structured fields, without requiring external APIs.

## Scope Fence

- Do not reintroduce community boost or graph scalar multipliers.
- Do not traverse `conflicts_with` as a positive edge.
- Do not require external LLM API keys for default graph construction.
- Do not commit agent-classified edges without evidence/proposal semantics.

## Acceptance Criteria

- Edges persist with type, origin, reason, and evidence.
- Invalid cycles or contradictory backbone edges fail clearly.
- `conflicts_with` is returned/pruned separately by later retrieval logic and is not walkable.
- Cold-start proposals are observable and do not silently mutate trusted graph state without the chosen commit path.

## Shared / Global Notes

Typed edges are shared graph infrastructure. Keep relation semantics centralized so retrieval, MCP tools, and future maintenance commands do not diverge.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: port the useful SkillDAG idea, typed graph evidence, without repeating the #208 multiplier mistake.
- SkillDAG separates `matches`, `neighbors`, and `conflicts`; use that as the conceptual contract.
- Important unknown: whether edge proposals should be filesystem-exported as well as stored in Postgres.

## Inherited Changes — V1.7 batch 1-2 triage (todos 228-244)

These landed on `feat/v-1-7` during the 228-243 triage swarm (2026-06-09) and bind this ticket (schema-heavy):

- **Migration floor is now 008.** This ticket's typed-edge migrations are `009_*`/`010_*` and MUST bump BOTH the ordering test `migration_set_is_ordered_001_through_008` AND the live-count test `live_run_migrations_applies_then_skips_on_second_boot` (now asserts all 8 IDs — hardcoded count gate, #238). Migrations stay approval-sensitive (see #233's drift precedent: adjacent-feature schema must not ride in unannounced).
- **`TRUNCATE_ALL_TABLES_SQL` is now the single source of truth** for runtime truncate + its guard test (#228/#238). Add every new typed-edge / edge-history table to that const, or cross-suite e2e isolation silently breaks (a stale row from suite A pollutes suite B).
- **Migration 007 (`generality` columns) is active write-ahead** (#233) — if cold-start edge proposals key off generality, note it flows via `.pending` frontmatter today, not the `skills` row.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md`
- https://arxiv.org/abs/2606.03056
- https://github.com/Ericbai06/SkillDAG

## Coupling Notes

T06 consumes these edges for agent-facing graph search. Schema migrations are approval-sensitive.
