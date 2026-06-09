---
ticket_id: T05
title: Typed skill graph storage and cold-start edge proposals
kind: expansion
status: ready
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

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md`
- https://arxiv.org/abs/2606.03056
- https://github.com/Ericbai06/SkillDAG

## Coupling Notes

T06 consumes these edges for agent-facing graph search. Schema migrations are approval-sensitive.
