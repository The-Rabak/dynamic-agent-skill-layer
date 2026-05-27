---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/09-admin-inspection-and-rebuild-tools.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.3"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-26T19:13:14+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-26-191314
completed: 2026-05-26T19:27:58+03:00
---

## WHY Context

### Problem Narrative
Operators need explicit visibility and manual recovery controls for the graph lifecycle without mixing policy logic into online request paths.

### User Story
As a solo developer operating the skill layer, I need read-only inspection and manual rebuild controls so I can diagnose graph state safely and recover without bypassing approval workflows.

### Architectural Context
T09 owns the `crates/admin/` feature home and composes that thin admin surface into the online binary. Admin must remain read-only + trigger-only, with graph construction staying in `graph-builder` and maintenance policy staying in `maintenance`.

### Success Criteria
- SC-4: operable graph inspection and manual rebuild trigger
- SC-5: read-only visibility into graph state

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`) + E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Constitution version: 1.0.0
- Relevant principles: local-first execution, human gate for mutations, filesystem-observable state
- Required approvals surfaced: admin tools must stay read-only/trigger-only and must not perform hidden approval or mutation workflows
- Waivers: None

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature home: `crates/admin/` (online debug tools)
- Shared/global decisions: keep admin separate from mcp-server and graph-builder while composing into online binary
- Context tiers: global guardrails + ticket-local files + scope fence only
- Deletion test: do not move merge/retire policy or graph construction logic into admin
- Interfaces as test surfaces: rebuild trigger contract, inspect-skill payload, list-communities payload, mcp tool registration and dispatch
- Seams/adapters/contracts: admin tool seam over graph-builder/retrieval state without direct policy mutations
- Review guidance: verify read-only/trigger-only boundary and thin online composition

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T09: Admin inspection and rebuild tools | expansion | SC-4 + SC-5 admin observability and manual rebuild trigger | completed | 1 | docs/execution-sessions/work-2026-05-26-191314/unit-01-admin-inspection-and-rebuild-tools.md |

## Learnings Brief
- [architecture] `admin` is a dedicated feature home and must not absorb maintenance policy or graph construction logic.
- [boundary] Admin tools are read-only or trigger-only; approval and mutation authority remain in filesystem workflows.
- [composition] MCP tool additions require updates in both registered tool names and JSON-RPC list/call dispatch.
- [rebuild] Admin rebuild trigger can delegate to `GraphRebuildOrchestrator` without copying graph-builder policy logic.
