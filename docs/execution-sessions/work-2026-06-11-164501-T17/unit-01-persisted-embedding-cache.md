---
unit: "Persisted embedding cache — kill the boot/reload re-embed"
unit_number: 1
unit_kind: infra-persistence
serves: "T17 AC2 (precomputed-vector load + changed-only re-embed + fail-loud model/dim) + AC3 (~7min→seconds on unchanged corpus)"
status: completed
attempt_count: 1
domains: [infrastructure, persistence, mcp-server, migration, embeddings]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/17-mcp-server-boot-readiness-honesty.md
session_id: work-2026-06-11-164501-T17
---

## What Was Implemented
Postgres-backed persisted embedding cache that eliminates the unconditional full-corpus re-embed in `build_graph_from_pg` (the boot AND background-reload snapshot builder).

- **Migration 011** `skill_embeddings(skill_id, view_kind, model_name, dimension, content_hash, vector BYTEA, updated_at, PK(skill_id,view_kind,model_name))` — additive, idempotent, write-ahead, human-gate APPROVED comment; index on model_name. REGISTERED in the compile-time `MIGRATIONS` array (was initially missed — caught by orchestrator; migration is directory-inert without registration) and added to `TRUNCATE_ALL_TABLES_SQL` for cross-suite hygiene.
- **`EmbeddingCacheStore`** (crates/infrastructure/src/persistence/embedding_cache.rs): `load_for_model(model,dim)` → map keyed (skill_id,view_kind)→(content_hash,vector); fail-loud `DimensionMismatch` on any stored row whose dim ≠ active dim (#235 semantics). `upsert_many` ON CONFLICT DO UPDATE. `encode/decode_f32_vector` (little-endian BYTEA, exact roundtrip incl. NaN). `content_hash_for_view_text` (blake3). VIEW_KIND_* constants + `subunit_view_kind(pos)`.
- **`build_graph_from_pg` + `embed_dense_view_with_cache`** (crates/mcp-server/src/lib.rs): threads `EmbeddingModelInfo` through boot + `PostgresGraphReloader`; for each (skill,view) and (skill,subunit) computes content_hash, reuses cached vector on (hash,model,dim) match, embeds only misses, upserts fresh vectors. Blank views still skipped (never cached) — preserves T09 `embed_dense_view_skipping_blank` semantics (that now-dead fn deleted). Cold cache = identical end-to-end behavior; no ranking change (exact f32 roundtrip).

## Files Changed
- `crates/infrastructure/migrations/011_skill_embeddings.sql` — created
- `crates/infrastructure/src/persistence/embedding_cache.rs` — created (store + helpers + 13 tests)
- `crates/infrastructure/src/persistence/postgres.rs` — modified (MIGRATION_011 const + MIGRATIONS entry + TRUNCATE + ordering test 001..011 + new migration_011 declares-table test + live test ten→eleven)
- `crates/infrastructure/src/lib.rs` — modified (export embedding_cache module + types)
- `crates/mcp-server/src/lib.rs` — modified (cache-aware build_graph_from_pg, embed_dense_view_with_cache, model_info threading, deleted dead embed_dense_view_skipping_blank)

## Problems Encountered
### Problem 1: migration .sql created but not registered (orchestrator-caught)
- **Root cause:** migrations are a compile-time `MIGRATIONS: &[(&str,&str)]` array (include_str! consts), NOT directory-scanned. The agent created 011.sql but didn't register it → table never created at runtime.
- **Fix:** added `MIGRATION_011` const + `("011_skill_embeddings", MIGRATION_011)` array entry + `skill_embeddings` in TRUNCATE_ALL_TABLES_SQL + updated ordering/count/live tests 010→011.

### Problem 2: clippy -D warnings from this unit (orchestrator-caught)
- **Root cause:** dead `embed_dense_view_skipping_blank`, unused `SubunitEntry.skill_idx`, `% 4 != 0` vs is_multiple_of, `&[x.clone()]` vs from_ref, 3 collapsible nested ifs.
- **Fix:** all six resolved; lib clippy clean on infrastructure + mcp-server.

## Patterns Discovered
- Migrations register in `postgres.rs` MIGRATIONS array + need TRUNCATE_ALL_TABLES_SQL entry + an ordering test + a per-migration declares-table test (008/009/010 convention). A `.sql` file alone is inert.
- Exact f32 roundtrip (LE BYTEA) is mandatory — any precision drift would shift qwen3 scores and break the regression gate.

## Test Results
- Command: `cargo build -p infrastructure -p mcp-server --features test-utils` → OK (clean after warning fixes)
- Command: `cargo test -p infrastructure embedding_cache --features test-utils` → **11 passed; 0 failed; 2 ignored (live-PG)**
- Command: `cargo test -p infrastructure migration --features test-utils` → **7 passed (incl. migration_set_is_ordered_001_through_011, migration_011_declares_skill_embeddings_cache_table); 0 failed; 2 ignored**
- Command: `cargo clippy -p infrastructure -p mcp-server --lib --features test-utils` → **0 warnings/errors in touched lib code**
- Live-PG roundtrip (2 ignored: live_pg_roundtrip_upsert_and_load, live_pg_dimension_mismatch_fails_loud): DEFERRED to the consolidated stack bring-up in Unit 3 (PG not currently up at 127.0.0.1:15432).

## TDD Evidence
- **Red:** `cargo test -p infrastructure embedding_cache` before impl — FAIL (module/file absent → compile error); `cargo build -p mcp-server` before impl — FAIL (build_graph_from_pg lacked model_info param). Proves the cache + wiring were genuinely missing.
- **Green:** 11 embedding_cache unit tests + 7 migration tests PASS; both crates build clean. Proves cache hit/miss, dim-mismatch fail-loud, exact BYTEA roundtrip, and migration registration now work.
- **Post-Refactor Green:** after clippy cleanup (dead-fn delete, let-chain collapse, is_multiple_of, from_ref) and migration registration — re-ran the same suites: still 11+7 PASS, lib clippy clean. Cleanup preserved behavior.

## Pre-existing blockers observed (NOT from this unit — out of scope)
- `compile_context_bench` fails to compile: `retrieval::SeededSkill` missing T09 fields `e_task/e_needs/e_negative_embedding` in a bench fixture — pre-existing T09 debt, surfaces only under `--all-targets`.
- tests/e2e harness dead-code (ENV_LOCK, ScopeEnvGuard, QdrantObserver, RedisObserver, configure_scope_env*) — the documented `feat/v-1-7` workspace `-D warnings` final-gate blocker (memory: workspace-clippy-e2e-harness-deadcode-blocker), pre-existing.
