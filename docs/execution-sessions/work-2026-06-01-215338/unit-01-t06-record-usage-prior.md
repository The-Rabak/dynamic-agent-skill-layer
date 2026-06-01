---
unit: "T06 — Record usage; feed retirement + a deterministic ranking prior"
unit_number: 1
unit_kind: expansion
serves: "SC-V1.5-D — usage data exists and feeds retirement + a deterministic ranking prior"
status: completed
attempt_count: 1
domains: [mcp-server, infrastructure, retrieval, maintenance, database]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/06-record-usage-feed-retirement-and-prior.md
session_id: work-2026-06-01-215338
execution_model: sonnet
---

## What Was Implemented
Closed the usage signal loop (SC-V1.5-D):
- **Prior (retrieval, pure):** `usage_prior(usage_count, age_days) -> f32 = min(ln(1+count)·e^(-age/30), 0.15)` as a pure `#[inline] fn` in `scoring.rs` (+ `UsagePriorInputs`). `usage_count=0 ⇒ 0.0`. Populated into the snapshot at graph-load/refresh from one batched usage query; no learned tuning, no `skill_prior_overrides` write. Replaces hardcoded `prior:0.1`/`community_boost:0.2`.
- **Ports (infrastructure):** new `crates/infrastructure/src/persistence/usage.rs` — `UsagePersistencePort` (write) + `UsageSampleStore` (read), `PostgresUsageWriter`/`PostgresUsageSampleStore` adapters. Writer wraps `session_logs` + N `skill_usage` rows in ONE transaction. Reader uses one batched `unnest($1::uuid[])` LEFT JOIN (zero-row cold start safe). Append-log model: one immutable row per selected skill, no UNIQUE/UPSERT.
- **Write trigger (mcp-server):** background writer (`usage_writer.rs`) fed by a bounded `mpsc` (cap 128); `try_send` → drop + `health["usage_write"]="failed"` + warn on full/failure; never propagates to caller, never affects latency. Triggered at the `McpServerApp::compile_context` coordination layer (NOT in `CompileContextTool`). Prompt stored as BLAKE3 hash (security P3), never raw text. `MCP_USAGE_LOGGING=off` rollback flag.
- **Retirement (maintenance):** replaced the empty-slice `propose(&skills, &[], now)` with a real `UsageSampleStore::recent_usage` read (driven from the sync trait via `block_in_place`). `maintenance` does NOT import `mcp-server`.
- **Migration:** `003_usage_fields.sql` (nullable `ADD COLUMN`: `skill_usage.relevance_score REAL`, `session_logs.{prompt_hash TEXT, latency_ms BIGINT, status TEXT}`; `metadata JSONB` kept as overflow tail; no new indexes). Applied to live `skill_layer_test`.
- **Truncate fix:** `truncate_all_tables` now includes `session_logs` + `skill_usage` (E2E row-leakage fix).

## Files Changed
- `crates/retrieval/src/scoring.rs` — `usage_prior` pure fn + `UsagePriorInputs` + 4 tests
- `crates/retrieval/src/lib.rs` — re-exports
- `crates/infrastructure/migrations/003_usage_fields.sql` — created (human-gated, applied)
- `crates/infrastructure/src/persistence/usage.rs` — created (ports + adapters + 2 tests)
- `crates/infrastructure/src/persistence/postgres.rs` — MIGRATION_003, truncate_all_tables, tests
- `crates/infrastructure/src/lib.rs` — usage module + re-exports
- `crates/mcp-server/src/usage_writer.rs` — created (bounded-mpsc writer + 3 tests)
- `crates/mcp-server/src/lib.rs` — usage_sender/health fields, compile_context coordination, build_graph_from_pg prior population, build_live_server wiring, BLAKE3 prompt hash
- `crates/mcp-server/src/tools/compile_context.rs` — `invoke_and_capture_outcome` (pure additional return; no side effects)
- `crates/mcp-server/Cargo.toml` — uuid + tokio sync feature
- `crates/maintenance/src/runtime.rs` — usage_store field, new_with_usage_store, real usage read

## Problems Encountered
### Problem 1: Migration number conflict
- **Error:** ticket calls for `002_usage_fields.sql` but `002_transcript_ingest_queue.sql` already exists (added by todo-103 in a prior batch).
- **Root cause:** ticket authored before todo-103 landed `002`.
- **Fix:** created `003_usage_fields.sql` (sequential, correct for the current set). **Index/plan still reference `002` — see "Discrepancies to reconcile" below.**

### Problem 2: `sqlx::query!` macro unavailable
- **Root cause:** no `.sqlx` compile-time cache; `sqlx::query!` validates against a live DB at compile time.
- **Fix:** used runtime `sqlx::query` + `Row::try_get`, matching the existing `transcript_queue.rs`/`rebuild.rs` pattern.

### Problem 3: sync retirement trait vs async usage read
- **Root cause:** `RetirementPassRunner::run_retirement_pass` is sync; `UsageSampleStore::recent_usage` is async.
- **Fix:** `tokio::task::block_in_place` + `Handle::block_on` inside the sync impl (standard multi-thread-runtime idiom).

## Patterns Discovered
- `sqlx::query!` macros are NOT usable here (no `.sqlx` cache) — use runtime `sqlx::query` + `Row::try_get`.
- `McpServerApp` builder methods (`with_usage_writer`, `with_transcript_ingest`) are acceptable and do not violate the "two constructors" rule (which is about `from_environment` + `with_explicit_graph`).
- `CompileContextTool` stays pure: adding `invoke_and_capture_outcome` returns the `RetrievalOutcome` to the coordination layer without touching tool state.
- `build_session_usage_record` silently skips non-UUID skill IDs (debug log) rather than failing the whole write.

## TDD Evidence
- **Red** — `usage_prior`, `usage_writer`, and the ports did not exist → references fail to compile (genuinely-new-code red). The observability seam is proven by a behavioral test: `usage_writer::tests::write_failure_sets_health_marker_to_failed_and_never_propagates` (failing `UsagePersistencePort` ⇒ `health["usage_write"]="failed"`, no panic, no propagation). Baseline `git stash` confirmed the only failing workspace test (`watcher_detects_pending_approval...`) is pre-existing and unrelated.
- **Green** — `cargo test -p retrieval` 23 passed (4 new prior tests); `usage_writer` 3 passed; infrastructure 59 passed (5 new usage contract tests); `cargo test -p maintenance --test test_maintenance_e2e` 2 passed.
- **Post-Refactor Green** — `cargo fmt` on touched files; `cargo test --workspace --exclude graph-builder` 30 suites, 0 failures; clippy 0 warnings on touched-crate library code; `cargo tree` confirms retrieval/domain purity (no sqlx/redis/qdrant).

## Test Results
- Command: `cargo test -p maintenance --test test_maintenance_e2e`
- Result: PASS (2 passed)
- Attempts: 1
- Orchestrator-verified: rustfmt `--check` clean on touched crates (exit 0); clippy exit 0 (only pre-existing warnings in `tests/integration/test_admin_tools.rs`); retrieval + domain purity clean.

## Bench
- `cargo bench -p mcp-server --bench compile_context_bench` baseline established: ~1.4–1.5ms across 100/1000/5000 skills. Usage write is off-path; `invoke_and_capture_outcome` adds one `RetrievalOutcome` clone. Well under <500ms warm budget.

## Discrepancies to reconcile (surfaced, not silently fixed)
1. **Migration number:** delivered as `003_usage_fields.sql`; index.md Blockers + plan reference `002_usage_fields.sql`. The `002` slot is legitimately taken by `002_transcript_ingest_queue.sql`. Index updated to reflect `003`.
2. **`scope` derivation heuristic:** `build_session_usage_record` derives scope as `repo_path.is_empty() ? "global" : "project"` — a V1.5 approximation. A future improvement threads the resolved scope through `CompileContextResponse`. Documented in-code. Not a blocker for SC-V1.5-D.

## Pre-existing issue flagged (NOT a T06 regression)
- `graph-builder` test `watcher_detects_pending_approval_and_rebuild_respects_invalidation_order` (`tests/integration/test_watcher_rebuild.rs:137`) fails `left: 4, right: 2` — reproduced identically on the T06-stashed baseline. Unrelated to T06 (no graph-builder code touched). Should be triaged separately (candidate for T10's integration gate or a dedicated fix).
