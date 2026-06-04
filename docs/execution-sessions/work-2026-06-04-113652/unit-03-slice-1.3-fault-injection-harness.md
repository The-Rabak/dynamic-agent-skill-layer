---
unit: "Slice 1.3 — Real-infra fault-injection harness"
unit_number: 3
unit_kind: hardening
serves: "Unlocks DS-003..008 brutal rewrites (test-support capability)"
status: completed
attempt_count: 2
domains: [rust, e2e, test-harness, fault-injection, docker]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
New reusable `tests/e2e/support/` module:
- `infra.rs` — `compose_stop_service` / `compose_start_services` (wrap `docker compose -f docker-compose.test.yml stop/start <svc>`; never edit the file).
- `poll.rs` — `poll_until(predicate, timeout, interval)` + `poll_until_sync` (replaces fixed sleeps).
- `drift.rs` — `inject_pg_skills_without_qdrant_vectors`, `inject_qdrant_vectors_without_pg_rows`, `remove_*`, `pg_active_skill_ids`; injected Qdrant point IDs land near `u64::MAX/2` (FNV-1a) to avoid colliding with real IDs.
- `load.rs` — `write_skill_files_to_sandbox(sandbox, count)`; `fire_concurrent_compile_context(app, repo, prefix, K)` → `Vec<CallSample{status, duration_ms}>` via `JoinSet` (callers prove parallelism: `max(dur) < sum(dur)`).
Smoke test `tests/e2e/test_support_harness_smoke.rs` (4 non-live pass, 3 live `#[ignore]`).

## Files Changed
- `tests/e2e/support/{mod,infra,poll,drift,load}.rs` — created
- `tests/e2e/test_support_harness_smoke.rs` — created
- `crates/mcp-server/Cargo.toml` — registered `[[test]] test_support_harness_smoke`

## Problems Encountered
- `list_point_ids` needs `use infrastructure::OutboxVectorStore;` in scope — added. `#![allow(dead_code)]` must precede doc comment — reordered.

## Patterns Discovered (for siblings)
- **Include incantation:** `#[path = "support/mod.rs"] mod support;` then `support::infra::*`, `support::poll::*`, `support::drift::*`, `support::load::*`.
- Server launched via `McpServerApp::from_environment(retrieval_config)` reading `DATABASE_URL`/`QDRANT_URL`/`OLLAMA_URL`/`REDIS_URL`/`SKILL_GLOBAL_PATHS`/`SKILL_GLOBAL_ALLOWED_ROOTS`; `env_guard` sets them for test scope.
- PG: `sqlx::PgPool` via `PostgresAdapter::pool()`. Qdrant: `QdrantAdapter` (reqwest REST: PUT points?wait=true / POST points/delete?wait=true). `OutboxVectorStore::list_point_ids` for gap measurement.
- Compose path from mcp-server crate: `CARGO_MANIFEST_DIR/../../docker-compose.test.yml`.

## TDD Evidence
- **Red:** smoke test referenced non-existent `support/mod.rs` → compile error "No such file or directory".
- **Green:** `cargo test -p mcp-server --features test-utils --test test_support_harness_smoke` → 4 passed, 3 ignored.
- **Post-Refactor Green:** same → 4 passed; clippy on new target clean; fmt clean.

## Test Results
- Non-live: 4 passed. Live (3 `#[ignore]`): PENDING-LIVE — `... --test test_support_harness_smoke -- --include-ignored` with stack up.
