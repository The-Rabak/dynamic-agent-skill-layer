---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/04-dual-scope-retrieval-and-hooking.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 1.3"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-23T12:24:14+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-23-122414
completed: 2026-05-23T12:42:00+03:00
---

## WHY Context

### Problem Narrative
Developers still lose time at session start when retrieval remains single-scope, because project-specific and global skills are not fused together and hook behavior is easy to drift.

### User Story
As a solo developer, I need zero-touch dual-scope retrieval (project + global) with correct first-prompt hook semantics, so I consistently get relevant compiled context without manual scope curation.

### Architectural Context
T04 expands the existing online path while keeping `mcp-server` transport-thin. Retrieval owns dual-scope orchestration and fusion; infrastructure owns scope resolution adapters; MCP enforces result-status and suppression semantics.

### Success Criteria
- SC-2: concurrent project/global retrieval with per-scope MMR then weighted RRF fusion.
- SC-1: Claude hook semantics reflected in behavior/docs: inject on `ok`, suppress only after healthy outcomes, retry allowed after `degraded`.

### TDD Contract
- Effective mode: Ralph-driven TDD.
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun.
- Required evidence: unit (`cargo test --workspace`) and e2e (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`).
- Exceptions: None.

### Constitution Context
- Version: 1.0.0 (`docs/constitution.md`).
- Applicable principles: local-first execution, zero-touch session start, portable scope, filesystem-observable state, human gate for mutations.
- Approvals surfaced: no schema/event/model changes are planned for this unit; if scope expands into those areas, execution must pause for explicit approval.
- Waivers: none.

### Architecture Handoff
- Artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Feature homes: `crates/retrieval/`, `crates/infrastructure/src/scope.rs`, `crates/mcp-server/src/tools/compile_context.rs`
- Shared / global decisions: `ScopeResolver` stays the seam; `compile_context` contract statuses (`ok`, `no_match`, `degraded`, `duplicate_suppressed`) must remain canonical.
- Context tiers: ticket-local packet primary, architecture/plan as deeper references.
- Deepening candidates to preserve: dual-scope concurrent retrieval with weighted project bias; healthy/degraded suppression distinction.
- Deletion test: keep retrieval logic out of MCP handlers; keep resolver logic out of ad-hoc env/git access in retrieval.
- Interfaces as test surfaces: `SkillRetriever`, `ScopeResolver`, and `compile_context` response envelope semantics.
- Seams / adapters / contracts: `GitRootProjectResolver`, `EnvPathGlobalResolver`, retrieval fusion layer, MCP suppression state.
- Review guidance: confirm no business-logic drift into MCP transport and no weakening of status semantics.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T04 dual-scope retrieval + hook semantics | expansion | SC-2 dual-scope fusion and SC-1 first-prompt hook semantics | completed | 1 | `unit-01-t04-dual-scope-retrieval-and-hook-semantics.md` |

## Learnings Brief
- [backend] Keep MCP handlers delegation-only and place retrieval logic in the retrieval crate.
- [testing] Status-transition tests around suppression semantics are required to prevent degraded-vs-healthy regressions.
- [contracts] `duplicate_suppressed` responses must keep real graph/scopes metadata.
- [retrieval] Use per-scope MMR first, then weighted cross-scope RRF with project-priority defaults.
- [scope] Treat project/global resolution failures as degraded retrieval and preserve reason-code visibility.
- [hooks] Document first-prompt policy explicitly: inject on `ok`, suppress only on healthy outcomes, retry after `degraded`.
