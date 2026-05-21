---
ticket_id: T04
title: Dual-scope retrieval and Claude hook example
kind: expansion
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.3"
feature_home: crates/retrieval/
depends_on:
  - T03
dependency_type: hard
serves:
  - SC-1: actual Claude Code first-prompt integration semantics
  - SC-2: concurrent project and global retrieval with weighted RRF
files:
  - crates/retrieval/src/dual_scope.rs
  - crates/retrieval/src/scope_resolution.rs
  - crates/retrieval/src/fusion.rs
  - crates/infrastructure/src/scope.rs
  - crates/mcp-server/src/tools/compile_context.rs
  - config/claude-code/hooks.example.json
  - tests/integration/test_dual_scope.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Dual-scope retrieval and Claude hook example

## Serves

- SC-2 by expanding from one scope to concurrent project and global retrieval.
- SC-1 by documenting and honoring real Claude Code hook behavior for first-prompt injection.

## Scope

Extend retrieval into a dual-scope pipeline and document the hook configuration that feeds the MCP server the right prompt/session/repo context. The outcome is a fused ranked result set that respects scope weighting and healthy-result suppression semantics.

## Scope Fence

- Do not implement SessionEnd extraction, watcher-driven graph construction, or admin tools.
- Keep skills manually seeded for this phase.
- Do not weaken the `ok`/`no_match`/`degraded`/`duplicate_suppressed` contract.

## Acceptance Criteria

- Project and global scope searches run concurrently and finish within the expected parallel latency envelope.
- Scope resolution uses git-root project detection plus `SKILL_GLOBAL_PATHS`.
- RRF happens after per-scope MMR and honors scope weighting.
- Hook documentation shows inject-on-`ok` and suppress-on-healthy-only behavior.
- Session isolation works across different `{session_id, repo_path}` pairs.

## Shared / Global Notes

- `ScopeResolver` remains the shared seam; retrieval must consume it rather than reading environment or git state ad hoc.
- The hook example is documentation and contract surface, not hidden implementation logic.
- Project scope should be favored without hiding global results that still score well.

## Local Context

WHY link: the user story is explicitly dual-scope and multi-harness-aware, so single-scope retrieval is only a tracer bullet, not the real feature.

Work against retrieval fusion and scope resolution while keeping the MCP server thin. The key contract to preserve is the plan's healthy-result suppression rule: a degraded first attempt must not consume the one-shot prompt opportunity.

Unknowns: none beyond the exact hook example wording and config defaults documented in the ticket files.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 1.3`
- Scope contract: `## Shared / Global Decisions`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/execution-shape.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#drift-checks`

## Coupling Notes

- Retrieval fusion and hook semantics stay together because the outcome is "correct context injection from both scopes," not two separate partial wins.
- Splitting the hook example away from the retrieval change would make the prompt lifecycle rules easy to drift.
