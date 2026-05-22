---
ticket_id: T02
title: Infrastructure adapters and schema
kind: tracer-bullet
status: completed
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.1b"
feature_home: crates/infrastructure/
depends_on:
  - T01
dependency_type: hard
serves:
  - SC-4: data-integrity foundation for graph writes and future maintenance
  - SC-5: filesystem-observable workflows backed by durable schema and events
  - SC-7: concrete resilience and connectivity adapters
files:
  - crates/infrastructure/Cargo.toml
  - crates/infrastructure/src/lib.rs
  - crates/infrastructure/src/embeddings/ollama.rs
  - crates/infrastructure/src/extraction/claude.rs
  - crates/infrastructure/src/extraction/ollama.rs
  - crates/infrastructure/src/persistence/postgres.rs
  - crates/infrastructure/src/persistence/outbox.rs
  - crates/infrastructure/src/persistence/rebuild.rs
  - crates/infrastructure/src/streaming/redis.rs
  - crates/infrastructure/src/vector/qdrant.rs
  - crates/infrastructure/src/scope.rs
  - crates/infrastructure/src/resilience.rs
  - crates/infrastructure/src/health.rs
  - crates/infrastructure/src/logging.rs
  - crates/infrastructure/migrations/001_initial_schema.sql
  - scripts/run-t02-infrastructure-tests.sh
  - docs/runbooks/schema-migration-verification-and-rollback.md
test_command: ./scripts/run-t02-infrastructure-tests.sh
tdd_mode: inherit
---

# Infrastructure adapters and schema

## Serves

- SC-4 by introducing the outbox and rebuild coordination needed for durable graph writes.
- SC-5 by giving filesystem-driven workflows a backed schema and event path.
- SC-7 by wiring concrete adapters for the local infra dependencies.

## Scope

Implement the shared `infrastructure` crate and the initial PostgreSQL schema. This ticket owns all concrete external adapters, migrations, and shared utilities that downstream service crates consume through stable interfaces.

## Scope Fence

- Do not put retrieval, compilation, MCP tool logic, graph construction, or maintenance policy in `infrastructure`.
- Do not leak business rules into concrete adapters.
- Reuse `domain` traits and types rather than redefining them here.

## Acceptance Criteria

- PostgreSQL migrations create the full baseline schema, outbox table, rebuild locks, triggers, and indexes.
- Redis Streams publish/consume paths, Ollama embeddings, and Qdrant connectivity work against the local stack.
- `GraphWriteCoordinator` and `RebuildCoordinator` exist as shared persistence contracts for later graph-builder work.
- Scope resolvers, resilience helpers, health checks, and structured logging bootstraps are available for downstream crates.
- Workspace build, test, format, and lint expectations are satisfiable from this shared layer.

## Migration Verification SQL (Executable)

Pre-deploy checks:

```sql
SELECT current_database() AS database_name;
SELECT current_user AS migration_actor;
SELECT to_regclass('public.skills') AS skills_table_before;
SELECT to_regclass('public.outbox_events') AS outbox_table_before;
```

Post-deploy checks:

```sql
SELECT to_regclass('public.outbox_events') IS NOT NULL AS outbox_exists;
SELECT to_regclass('public.rebuild_locks') IS NOT NULL AS rebuild_locks_exists;
SELECT EXISTS (SELECT 1 FROM graph_state WHERE singleton = TRUE) AS graph_state_seeded;
SELECT EXISTS (
  SELECT 1
  FROM pg_trigger
  WHERE tgname = 'trg_outbox_events_set_updated_at' AND NOT tgisinternal
) AS outbox_trigger_exists;
SELECT EXISTS (
  SELECT 1
  FROM pg_constraint c
  JOIN pg_class t ON c.conrelid = t.oid
  WHERE t.relname = 'outbox_events'
    AND c.contype = 'u'
    AND pg_get_constraintdef(c.oid) ILIKE '%idempotency_key%'
) AS outbox_idempotency_unique;
```

Rollback/restore procedure and approval evidence requirements are defined in:
- `docs/runbooks/schema-migration-verification-and-rollback.md`

## Shared / Global Notes

- The architecture artifact freezes `infrastructure` as the only place that directly instantiates Ollama, Qdrant, PostgreSQL, Redis, or Claude clients.
- The outbox pattern and rebuild-lock contracts are canonical v1.1 decisions; later tickets must consume them instead of inventing alternatives.
- Keep `domain <- infrastructure <- service crates` dependency direction intact.

## Local Context

WHY link: the user story needs real local dependencies and durable graph state, not mocks, before the first compile-context tracer bullet can be honest.

Work against `crates/infrastructure/` and the initial migration file. Concrete details to preserve:

- `EmbeddingService`, transcript extraction, and scope resolution are trait implementations here.
- The schema includes the graph tables, audit trail, usage data, and outbox/rebuild coordination tables.
- The open choice is only implementation detail, not contract shape: UUIDv7 may come from PostgreSQL support or application-side generation, but every caller should see a stable ID contract.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 1.1b`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#shared--global-decisions`

## Coupling Notes

- Adapters, shared utilities, and the baseline schema stay together because every later slice depends on the same concrete trust boundary.
- Splitting the schema from adapter work would leave downstream tickets without a durable event and persistence contract.
