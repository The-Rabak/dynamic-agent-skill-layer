---
unit: "#141 — schema_migrations tracking table"
unit_number: 2
unit_kind: fix-item
serves: "migration safety at scale — migrations run once, tracked, atomic apply+record"
status: completed
attempt_count: 2
domains: [database, persistence, migrations]
plan_file: "todos/141-pending-p2-schema-migrations-tracking-table.md"
session_id: work-2026-06-04-203220
---

## What Was Implemented
Added a `schema_migrations (id TEXT PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())` tracking table.
`MIGRATIONS` is now `&[(stable_id, sql)]` keyed on the migration filename stems. `run_migrations` bootstraps the
table, reads applied ids once, and applies only unapplied migrations. Each migration is applied **atomically** with
its id record via `apply_and_record`: `strip_begin_commit_wrapper` removes the migration file's own `BEGIN;`/`COMMIT;`
(fail-loud if absent) so the runner's `pool.begin()` transaction genuinely spans the DDL + the `INSERT` + commit.

## Files Changed
- `crates/infrastructure/src/persistence/postgres.rs` — `schema_migrations` bootstrap; `(id, sql)` MIGRATIONS;
  `strip_begin_commit_wrapper` (fail-loud); `apply_and_record` (atomic); skip logic; adapted ordering test; live
  skip-proof + live rollback-proof tests.
- `crates/infrastructure/src/health.rs` — regression fix: converted `build_health_checker_always_injects_usage_write_enabled`
  from a sync `#[test]` (which built sqlx pools OUTSIDE a runtime → "requires a Tokio context") to `#[tokio::test]`.

## Problems Encountered
### Problem 1 (caught in orchestrator review): false atomicity claim
- **Root cause:** first implementation wrapped each migration in `pool.begin()`, but every migration file self-runs
  `COMMIT;`, which ends the outer tx early — so the `INSERT` ran outside it. Proven on live PG: a `COMMIT;` inside an
  open tx ends it; a following `ROLLBACK` reported "no transaction in progress" and the row survived. The doc comment
  claiming "single transaction... no half-applied migration recorded" was a lie.
- **Fix:** `strip_begin_commit_wrapper` (fail-loud if a migration isn't `BEGIN;`/`COMMIT;`-wrapped) so the runner owns
  the transaction; added `live_failing_migration_rolls_back_atomically` proving a failing migration records no id and
  leaves no partial DDL.
### Problem 2 (regression exposed by the new tests): health test "requires a Tokio context"
- **Root cause:** `build_health_checker_always_injects_usage_write_enabled` was a sync `#[test]` calling the pool-
  building factory before entering a runtime. Passed on clean HEAD by scheduling luck; #141's added tests shifted
  in-binary scheduling and made it fail deterministically.
- **Fix:** made it `#[tokio::test]` so the factory runs within an ambient runtime. This also resolved the cross-crate
  parallel flake noted in Unit 1.

### Problem 3 (caught in orchestrator review): committed work broke the `admin` crate build
- **Root cause:** the first atomic implementation used `self.pool.begin()` + `.execute(&mut *tx)`. The
  `&mut PgConnection: Executor` higher-ranked obligation tipped rustc's trait solver into "Send is not general
  enough" for `admin`'s pre-existing borderline async-trait methods (`list_skills`/`list_communities`/
  `trigger_full_rebuild`). `cargo build --workspace` went red — bisected by reverting only `postgres.rs`.
- **Fix:** rewrote `apply_and_record` to run the DDL body + id INSERT as ONE multi-statement `raw_sql` batch on the
  pool (no held `sqlx::Transaction`, no `&mut *tx`). Postgres runs a multi-statement simple-query message in a single
  IMPLICIT transaction: on error the whole batch rolls back AND the connection stays clean (no `25P02` aborted-tx —
  verified live; an explicit `BEGIN` whose `COMMIT` is never reached strands the connection aborted). Stripping the
  migration's own `BEGIN;`/`COMMIT;` is essential so there's no explicit transaction control in the batch.

## Patterns Discovered
- Postgres has NO nested transactions: a `BEGIN;` inside an open tx warns and is a no-op; a `COMMIT;` ends the whole
  tx (NOT a savepoint). Any runner executing self-`BEGIN;/COMMIT;`-wrapped SQL inside an outer tx silently loses
  atomicity. Strip the wrapper and let the runner own the tx.
- Scratch-schema isolation via `?options=-csearch_path%3D<schema>` is the clean pattern for full-DDL live tests.
- Sync `#[test]`s that build sqlx pools/reqwest clients must be `#[tokio::test]` — pool construction needs an ambient runtime.

## Test Results (orchestrator-verified, live PG)
- `cargo test -p infrastructure --lib` (alone): **101 passed, 0 failed** (stable across 2 runs).
- `cargo test -p infrastructure --lib -- --ignored`: live skip-proof + live rollback-proof + claude-cli — **3 passed, 0 failed**.
- `cargo test -p infrastructure -p session-extractor --lib` (former flake repro): **101 + 39 passed, 0 failed**.

## TDD Evidence
- **Red:** live PG psql proof that the prior outer-tx was ended by the migration's own COMMIT (row survived ROLLBACK);
  new rollback test failed to compile against the non-atomic code (helpers absent).
- **Green:** `live_failing_migration_rolls_back_atomically` + `live_run_migrations_applies_then_skips_on_second_boot`
  both pass on live PG; full infra suite green after the health fix.
- **Post-Refactor Green:** infra suite re-run twice — 101/0 each time.
