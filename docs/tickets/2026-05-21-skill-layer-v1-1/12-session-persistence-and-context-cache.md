---
ticket_id: T12
title: Session persistence and context cache
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.3"
feature_home: crates/mcp-server/
depends_on:
  - T11
dependency_type: hard
serves:
  - SC-1: no duplicate first-prompt injection after restart
  - SC-7: suppression and cache behavior survive process restart and graph changes
files:
  - crates/mcp-server/src/state.rs
  - tests/integration/test_session_persistence.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Session persistence and context cache

## Serves

- SC-1 by keeping healthy first-prompt suppression stable across MCP restarts.
- SC-7 by making cache invalidation and degraded-path behavior explicit.

## Scope

Implement the dual-tier session suppression state plus compiled-context caching in the MCP server, keyed by prompt, scope fingerprint, and graph version.

## Scope Fence

- Do not turn Redis into a graph source of truth.
- Do not write suppression state for degraded outcomes.
- Do not add general-purpose caching outside the `compile_context` path.

## Acceptance Criteria

- Session suppression survives MCP restart through Redis-backed recovery.
- Healthy first outcomes write suppression state; degraded outcomes do not.
- Repeated prompts on the same graph version can return cached context without rerunning the full pipeline.
- `graph.rebuilt` invalidates cache entries by version mismatch.
- Redis outages degrade to documented DashMap-only behavior without hiding the limitation.

## Shared / Global Notes

- This ticket relies on the final degraded/healthy semantics from T11; do not re-open those contracts.
- Session state is operational caching, not a graph mutation and not a filesystem approval flow.
- Cache invalidation is tied to `graph_version`, not ad hoc clears.

## Local Context

WHY link: the system is only "zero-touch" if it remains polite after restarts and repeated prompts instead of reinjecting context or rerunning heavy work unnecessarily.

Keep this work inside `crates/mcp-server/src/state.rs` and the related tests. The critical contract is that suppression and caching both respect graph invalidation and degraded behavior:

- state key uses `{session_id, repo_path}`,
- cache key uses prompt hash + scope fingerprint + `graph_version`,
- `SessionEnd` explicitly clears session suppression.

Unknowns: none beyond TTL tuning that stays within the documented cache contract.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 3.3`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#recommendations-for-workflowswork`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- `docs/constitution.md`

## Coupling Notes

- Session suppression and compiled-context caching stay together because both live in the same MCP state boundary and share invalidation rules.
- Splitting them would duplicate graph-version logic and invite divergence in healthy/degraded behavior.
