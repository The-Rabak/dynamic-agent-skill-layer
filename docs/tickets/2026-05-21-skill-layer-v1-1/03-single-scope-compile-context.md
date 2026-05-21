---
ticket_id: T03
title: Single-scope compile_context tracer bullet
kind: tracer-bullet
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.2"
feature_home: crates/mcp-server/
depends_on:
  - T02
dependency_type: hard
serves:
  - SC-1: zero-touch context injection tracer bullet
  - SC-2: retrieval and compilation pipeline proof in one scope
  - SC-6: subunit-aware compilation with rescue-attached context
files:
  - crates/mcp-server/Cargo.toml
  - crates/mcp-server/src/main.rs
  - crates/mcp-server/src/tools/compile_context.rs
  - crates/mcp-server/src/tools/find_skill.rs
  - crates/mcp-server/src/state.rs
  - crates/retrieval/Cargo.toml
  - crates/retrieval/src/lib.rs
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/scoring.rs
  - crates/retrieval/src/qdrant_search.rs
  - crates/retrieval/src/graph_search.rs
  - crates/retrieval/src/fusion.rs
  - crates/compiler/Cargo.toml
  - crates/compiler/src/lib.rs
  - crates/compiler/src/template.rs
  - crates/compiler/src/rescue.rs
  - tests/integration/test_compile_context.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Single-scope compile_context tracer bullet

## Serves

- SC-1 by making the first prompt produce task-specific compiled context.
- SC-2 by proving the retrieval, scoring, rescue, and compilation path before dual-scope widening.

## Scope

Deliver the first end-to-end online path: MCP transport receives a prompt, retrieval ranks seeded skills in one scope, compiler formats structured markdown, and result semantics stay faithful to the v1.1 contract.

## Scope Fence

- Do not add project-scope retrieval, filesystem watcher behavior, session-end extraction, or admin tools.
- Keep tool handlers thin; retrieval and compilation logic belong in their own crates.
- Keep compilation template-only; do not add LLM-synthesized guidance.

## Acceptance Criteria

- `compile_context` and `find_skill` are registered and callable through the MCP server.
- `compile_context` produces the canonical result envelope: `ok`, `no_match`, `degraded`, and `duplicate_suppressed`.
- Single-scope retrieval uses real embeddings, scoring, MMR, rescue attachment, and template compilation over seeded test data.
- Healthy outcomes set suppression state; degraded-first attempts do not.
- The tracer bullet hits the latency target and remains explicit about degraded vs healthy-empty behavior.

## Shared / Global Notes

- The result contract at the plan's `compile_context` section is frozen and must not drift.
- Session suppression semantics are part of the tool contract, not an implementation detail.
- Retrieval and compiler crates stay transport-agnostic; MCP remains an orchestration layer only.

## Local Context

WHY link: this is the first user-visible proof that the skill layer helps at session start instead of remaining a paper design.

Focus on three feature homes working together:

- `mcp-server` bootstraps and delegates only.
- `retrieval` owns scoring, graph search, Qdrant search, MMR, and orchestration.
- `compiler` owns markdown template output and rescue formatting.

Use seeded graph data for this ticket. The graph is still manually loaded; watcher and automatic graph construction come later.

Unknowns: none beyond implementation details already fenced by the result contract and latency target.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 1.2`
- Frozen contracts: `#### compile_context result contract`, `## Canonical V1.1 Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#interfaces-as-test-surfaces`

## Coupling Notes

- MCP transport, retrieval, and compilation stay in one ticket because the outcome is only honest when the full path works end to end.
- Splitting this tracer bullet would create partial infrastructure or isolated modules with no user-visible value.
