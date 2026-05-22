---
ticket_id: T07
title: Outbox relay and reconciliation
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.4"
feature_home: crates/infrastructure/
depends_on:
  - T05
dependency_type: hard
serves:
  - SC-4: durable merge and rebuild data integrity
  - SC-7: graceful degrade through replayable vector sync
files:
  - crates/infrastructure/src/persistence/outbox.rs
  - crates/infrastructure/src/persistence/outbox_reconciler.rs
  - tests/integration/test_outbox_consistency.rs
  - docs/runbooks/schema-migration-verification-and-rollback.md
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Outbox relay and reconciliation

## Serves

- SC-4 by making PG-to-Qdrant synchronization durable and repairable.
- SC-7 by ensuring failures degrade into retryable backlog instead of silent drift.

## Scope

Harden the graph write path with an async outbox relay, idempotent Qdrant upserts, retry/failure bookkeeping, and reconciliation scans that repair stale or orphaned vectors.

## Scope Fence

- Do not add distributed transactions or invent a second persistence path.
- Keep this limited to PG/Qdrant consistency, not general caching or unrelated event plumbing.
- Do not emit `graph.rebuilt` before the rebuild's outbox work is durably processed.

## Acceptance Criteria

- The relay polls pending outbox entries, claims them safely, writes vectors, and marks processed rows.
- Failures retain retry metadata and remain replayable.
- Qdrant upserts are idempotent on content hash.
- Reconciliation finds stale or missing vectors and re-enqueues them.
- Rebuild completion only becomes visible after the outbox backlog for that rebuild is drained.

## Migration Safety Contract Dependency

This ticket consumes outbox tables introduced by T02. Before enabling relay/reconciliation in an environment, execute T02 migration verification SQL and keep rollback evidence attached to the deployment record.

Pre-deploy SQL:

```sql
SELECT current_database() AS database_name;
SELECT current_user AS migration_actor;
SELECT to_regclass('public.outbox_events') AS outbox_table_before;
```

Post-deploy SQL:

```sql
SELECT to_regclass('public.outbox_events') IS NOT NULL AS outbox_exists;
SELECT EXISTS (
  SELECT 1
  FROM pg_constraint c
  JOIN pg_class t ON c.conrelid = t.oid
  WHERE t.relname = 'outbox_events'
    AND c.contype = 'u'
    AND pg_get_constraintdef(c.oid) ILIKE '%idempotency_key%'
) AS outbox_idempotency_unique;
```

Rollback/restore and approval evidence requirements:
- `docs/runbooks/schema-migration-verification-and-rollback.md`

## Shared / Global Notes

- This ticket consumes the outbox contract introduced earlier; it does not redefine it.
- Eventual consistency is acceptable, but hidden consistency gaps are not.
- `graph.rebuilt` timing is part of the global invalidation contract used by the MCP server cache later.

## Local Context

WHY link: the skill graph cannot be trusted if filesystem-triggered rebuilds leave PostgreSQL and Qdrant out of sync after a transient failure.

Focus on the outbox worker and reconciliation surfaces only. Important constraints:

- Use content-hash-based idempotency for Qdrant point IDs.
- Keep failure data explicit (`attempts`, `status`, `last_error`) so maintenance and operations have something real to inspect.
- Design the worker to cooperate with graph-builder rebuild boundaries instead of becoming a hidden side channel.

Unknowns: none beyond worker polling/backoff tuning within the existing outbox contract.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.4`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#deletion-test`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#interfaces-as-test-surfaces`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Relay and reconciliation stay together because both belong to the same "PG and Qdrant stay honest" outcome.
- Splitting replay from repair would leave one half of the consistency contract unowned.
