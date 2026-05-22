---
artifact: migration-runbook
status: active
version: 1
owned_by: infrastructure
applies_to:
  - T02
  - T07
---

# Schema Migration Verification and Rollback Contract

## Approval Gate (Constitution)

`docs/constitution.md` requires explicit human approval for graph schema migrations. Capture approval evidence before running migration commands.

| Evidence field | Required value |
|---|---|
| Requested by | Operator running the migration |
| Approved by | Human approver name/handle |
| Scope | Target database and migration (`001_initial_schema.sql`) |
| Timestamp (UTC) | Approval time |
| Execution command set | Exact commands run |
| Verification evidence | Pre/post SQL output references |
| Rollback evidence | Backup + restore confirmation |

## Pre-Deploy Verification SQL

Run against the target database before applying the migration:

```sql
SELECT current_database() AS database_name;
SELECT current_user AS migration_actor;
SELECT to_regclass('public.skills') AS skills_table_before;
SELECT to_regclass('public.outbox_events') AS outbox_table_before;
```

## Post-Deploy Verification SQL

Run immediately after applying `crates/infrastructure/migrations/001_initial_schema.sql`:

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

## Rollback / Restore Contract

1. Create a pre-migration backup (`pg_dump -Fc`) of the target database.
2. Apply migration.
3. If any post-check fails, stop deploy and restore from backup:
   1. Drop target database.
   2. Recreate target database.
   3. Restore backup with `pg_restore`.
4. Re-run the pre-deploy SQL to confirm baseline restored.

Stop conditions:
- Any post-deploy SQL check is `false`.
- Migration command exits non-zero.
- Restore command exits non-zero.

## Tested Procedure

Use `./scripts/run-t02-infrastructure-tests.sh` for a repeatable contract test that now includes:
- Pre-migration snapshot in a rollback probe database.
- Migration apply in probe database.
- Full restore from snapshot.
- Verification that migrated tables are absent after restore.

## Evidence Log

| Date (UTC) | Operator | Approval | Command | Result |
|---|---|---|---|---|
| 2026-05-22 | Copilot CLI | @The-Rabak (explicit request to execute open P1 todos) | `./scripts/run-t02-infrastructure-tests.sh` | PASS (includes rollback probe restore check) |
