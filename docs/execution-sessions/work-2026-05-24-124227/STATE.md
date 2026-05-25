---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/05-watcher-driven-graph-rebuild.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.1"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-24T12:42:27+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-24-124227
completed: 2026-05-24T12:52:14+03:00
---

## WHY Context

### Problem Narrative
Developers still have to manually seed and maintain skills unless filesystem changes automatically rebuild the graph. The self-growing loop is incomplete until file mutations become durable graph updates.

### User Story
As a solo developer, I need filesystem-driven graph rebuilds so newly added or approved skills automatically become retrievable context without manual indexing.

### Architectural Context
T05 is owned by `graph-builder`: watcher/reconciliation, extraction, embeddings, communities, rebuild ordering, and event publication must stay together and must honor the invalidation contract (`graph.rebuilt` only after durable PG + outbox state).

### Success Criteria
- SC-4: incremental graph rebuild on filesystem changes
- SC-5: filesystem-observable state drives graph updates and audit trails

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`) and E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Version: 1.0.0 (`docs/constitution.md`)
- Principles: local-first execution, zero-touch session flow, human-gated mutations, portable scope, filesystem-observable state
- Required approvals surfaced: mutation-sensitive areas (schema/event/infrastructure changes) require explicit user direction; this ticket is explicitly requested
- Waivers: none

### Architecture Handoff
- Artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Feature home: `crates/graph-builder/`
- Shared/global decisions: canonical event set and invalidation ordering are frozen; watcher reconciliation is mandatory
- Context tiers: ticket-local packet primary, plan + architecture for deep context
- Deepening candidates: outbox ordering, idempotent watcher recovery, graph version durability
- Deletion test: keep graph construction in graph-builder; do not fold merge/retire/admin logic into this unit
- Interfaces as test surfaces: filesystem-to-event flow, graph rebuild completion signal, audit persistence
- Seams/adapters/contracts: watcher events, extraction fallback, embeddings pipeline, community assignment, GraphWriteCoordinator ordering
- Review guidance: verify no policy-layer leakage and no invalidation contract drift

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T05 watcher-driven graph rebuild | expansion | SC-4 and SC-5 file-driven graph mutation path | completed | 2 | `unit-01-t05-watcher-driven-graph-rebuild.md` |

## Learnings Brief
- [architecture] Keep transport/thin MCP logic separate from feature-home rebuild logic.
- [contracts] Preserve canonical event contracts and invalidation ordering when expanding offline flows.
- [watcher] `notify-debouncer-full` setup should use direct debouncer watch calls with `FileIdMap` for stable rename handling.
- [testing] Graph-builder integration tests fit repository patterns via crate-level `[[test]]` entries into `tests/integration/`.
