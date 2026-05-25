---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/07-outbox-relay-and-reconciliation.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.4"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-25T22:21:37+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-25-222137
---

## WHY Context

### Problem Narrative
The system cannot be trusted if filesystem-triggered rebuilds leave PostgreSQL and Qdrant out of sync after transient failures. T07 exists to make PG→Qdrant synchronization durable, replayable, and auditable instead of silent drift.

### User Story
As a solo developer using multiple coding-agent harnesses, I need a zero-touch, self-growing skill context layer that remains correct even when dependencies fail, so context stays reliable without manual repair.

### Architectural Context
This unit hardens the offline graph write path by implementing outbox relay + reconciliation behavior in the `infrastructure` and `graph-builder` feature homes, while preserving the contract: durable writes -> outbox drain -> graph_version bump -> `graph.rebuilt`.

### Success Criteria
- SC-4: Offline graph maintenance data integrity is preserved through durable replay and reconciliation.
- SC-7: Infrastructure failure degrades into explicit retryable backlog and eventual consistency.

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`), E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Constitution version: 1.0.0
- Relevant principles: local-first execution, human gate for mutations, filesystem-observable state, zero-touch reliability.
- Execution baselines: explicit failures, no stubs, Docker Compose deployability, structured logs.
- Required approvals: schema migrations and event-contract changes require explicit approval evidence; this unit keeps migration verification/rollback traces aligned to the runbook.
- Waivers: None.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature homes: `crates/infrastructure/` (relay + adapters), `crates/graph-builder/` (rebuild ordering and outbox drain boundary).
- Shared / global decisions: event envelope contract remains stable; outbox pattern is authoritative for PG+Qdrant dual persistence.
- Context tiers: keep global guardrails global; execute with ticket-local files and scope fence.
- Deepening candidates to preserve: outbox replay durability, drift reconciliation, idempotency semantics.
- Deletion test: keep `infrastructure` boundary and outbox coordinator abstraction; do not fold policy into transport.
- Interfaces as test surfaces: `GraphWriteCoordinator`, rebuild ordering contract, and event publication contract.
- Seams / adapters / contracts: Qdrant adapter behavior, outbox state transitions, rebuild invalidation ordering.
- Review guidance: verify no direct dual-write bypasses the outbox; verify `graph.rebuilt` visibility only after outbox drain.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T07: Outbox relay and reconciliation | hardening | SC-4 + SC-7 durable replay and consistency repair | completed | 3 | docs/execution-sessions/work-2026-05-25-222137/unit-01-outbox-relay-and-reconciliation.md |

## Learnings Brief
- [infrastructure] Preserve outbox state-machine semantics (`pending|processing|published|failed`) as the audit/replay contract.
- [ordering] Keep rebuild invalidation ordering strict: durable write -> outbox drain -> graph_version bump -> `graph.rebuilt`.
