---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/02-infrastructure-adapters-and-schema.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 1.1b"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-22T10:56:43+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-22-105643
completed: 2026-05-22T12:18:29+03:00
---

## WHY Context

### Problem Narrative
Developers lose time through manual skill selection and fragmented harness context. The system needs a local-first, self-growing layer with durable graph state and concrete infrastructure to power reliable retrieval, compilation, and lifecycle workflows.

### User Story
As a solo developer using multiple coding harnesses, I need a zero-touch skill context layer that retrieves from project and global scopes, compiles context quickly, and continuously grows/maintains the graph through extraction and maintenance workflows.

### Architectural Context
T02 owns the `infrastructure` feature home: concrete adapters for Ollama, Claude/Ollama extraction, PostgreSQL persistence + migrations, Redis streams, scope resolvers, resilience helpers, health checks, and structured logging. It depends on pure `domain` contracts from T01 and must not contain business logic.

### Success Criteria
- SC-4 data-integrity foundation for graph writes and future maintenance
- SC-5 filesystem-observable workflows backed by durable schema and events
- SC-7 concrete resilience and connectivity adapters

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit command/result (`cargo test --workspace`), E2E command/result (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
Constitution version `1.0.0` applies. Local-first execution, filesystem-observable state, and human-gated mutations are required. Schema changes and infrastructure configuration changes require explicit approval handling. No waivers are declared in the parent plan.

### Architecture Handoff
- Artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Feature homes: `domain` remains pure; `infrastructure` owns all concrete external adapters for downstream crates
- Shared / global decisions: outbox and rebuild-lock contracts are canonical; dependency direction stays `domain <- infrastructure <- service crates`
- Context tiers: ticket-local packet is primary; plan and architecture are deeper-dive context
- Deepening candidates to preserve: outbox ordering, event contract integrity, UUIDv7 contract, scope resolver seams
- Deletion test: keep concrete adapters in `infrastructure`; do not collapse into `domain` or service crates
- Interfaces as test surfaces: `EmbeddingService`, `TranscriptSkillExtractionService`, `ScopeResolver` implementations and PG schema contracts
- Seams / adapters / contracts: Ollama embedding/extraction, PG pool + migration, Redis streams pub/sub, GraphWriteCoordinator, RebuildCoordinator
- Review guidance: verify no business logic in adapters, verify migration/index/trigger contracts, verify resilience/degraded-path semantics

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Infrastructure adapters and schema (T02) | tracer-bullet | SC-4, SC-5, SC-7 foundation for downstream retrieval/graph/maintenance work | completed | 3 | `unit-01-infrastructure-adapters-and-schema.md` |

## Learnings Brief
- [persistence] `sqlx` 0.8 `PgPoolOptions` does not expose `connect_timeout`; wrap pool connect in `tokio::time::timeout` to enforce connect SLA.
- [validation] T02 now has a repeatable hard-E2E entrypoint at `./scripts/run-t02-infrastructure-tests.sh` that validates topology, migration contracts, outbox/rebuild DB invariants, and Redis stream behavior.
