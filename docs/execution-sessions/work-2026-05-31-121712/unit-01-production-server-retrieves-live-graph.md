---
unit: "T01 — Production server retrieves from the live graph"
unit_number: 1
unit_kind: tracer-bullet
serves: "SC-V1.5-A boot-time half (clean deployment retrieves a skill that exists in the graph) + SC-V1.5-F (empty-graph production stub path removed)"
status: completed
attempt_count: 4
domains: [rust, mcp-server, retrieval, domain, infrastructure, graph-builder, testing]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/01-production-server-retrieves-live-graph.md
session_id: work-2026-05-31-121712
---

## What Was Implemented
- `main.rs` boots via `McpServerApp::from_environment(...)` (live graph) with an `MCP_RETRIEVAL_MODE=seeded|live` rollback flag (default `live`) carrying `// TODO(remove-after-v1.5-green)` + "first green CI on main" removal criterion. The `seeded` branch uses `with_explicit_graph` with an empty snapshot.
- `build_seeded_server` (free fn) deleted; replaced by `McpServerApp::with_explicit_graph`. `build_live_server` demoted to a private helper behind public `McpServerApp::from_environment` → exactly two public graph-assembly constructors.
- `SeededGraph` → `RetrievalSnapshot` rename across `retrieval` + all callers (~41 refs); `grep -rn 'SeededGraph' crates/ tests/` = 0.
- `build_graph_from_pg` reads real `graph_version` from `graph_state` via new `PostgresGraphSnapshotStore::current_graph_version()` (0 on cold start); `warn!`+truncate on >5000 skills instead of `Err`.
- **Boot-load fix (the actual loop-closer):** `build_graph_from_pg` now populates `source_paths` with the configured scope root (`SKILL_GLOBAL_PATHS`/cwd). Live-loaded skills had empty `source_paths`, so `seeded_skill_matches_scope` filtered every live skill out before scoring → `no_match` even with a populated graph. This is snapshot construction (boot wiring), not the ranking algorithm.
- `ScopeRoot` relocated to `domain::types` (re-exported from `domain`); `graph_builder::ScopeRoot` = transitional `pub use domain::ScopeRoot`; `maintenance` + `mcp-server` imports updated. Diff ~31 ins / ~22 del across 5 files — under the ~50-line escape hatch, so COMPLETED not deferred.

## Files Changed
- `crates/mcp-server/src/main.rs` — modified (live `from_environment` boot + rollback flag)
- `crates/mcp-server/src/lib.rs` — modified (constructor consolidation; real `graph_version`; warn+truncate; `source_paths` boot wiring)
- `crates/infrastructure/src/persistence/rebuild.rs` — modified (`current_graph_version`)
- `crates/retrieval/src/{orchestrator.rs,lib.rs,dual_scope.rs}` — modified (rename)
- `crates/domain/src/{types.rs,lib.rs}` — modified (new `ScopeRoot` home + export)
- `crates/graph-builder/src/watcher.rs` — modified (transitional alias)
- `crates/maintenance/src/runtime.rs`, `crates/mcp-server/src/admin_wiring.rs` — modified (`ScopeRoot` import from `domain`)
- `crates/mcp-server/Cargo.toml` — modified (registered test target)
- `tests/e2e/test_boot_time_live_retrieval.rs` — created (narrow seed-and-retrieve smoke)
- 8 existing test files (`tests/bench`, `tests/e2e/*`, `tests/integration/*`) — modified (rename + `with_explicit_graph` migration)

## Problems Encountered
### Problem 1: Qdrant connectivity parse error
- **Error:** `Connectivity(... url: "http://localhost:16334/collections" ... Parse(Version))`
- **Root cause:** HTTP connectivity check pointed at the gRPC port.
- **Fix:** use HTTP port 16333 for `QDRANT_URL` (matches `run-e2e-tests.sh` HTTP section).

### Problem 2: NoMatch persisted even at zero relevance threshold
- **Error:** `assertion left == right failed ... left: NoMatch ... right: Ok`
- **Root cause:** `build_graph_from_pg` set `source_paths: vec![]`; `seeded_skill_matches_scope` rejects empty `source_paths` against a scope with configured paths → live skill dropped before scoring. The real reason deployed retrieval returns `no_match` even with a populated graph.
- **Fix:** populate `source_paths` in the snapshot builder with the configured scope root (boot wiring; schema unchanged).

## Patterns Discovered
- `seeded_skill_matches_scope` (`crates/retrieval/src/dual_scope.rs:123`) gates skills by `scope_type`+`scope_id` AND requires `source_paths` within the scope's configured paths. Any PG-loaded-graph path MUST set `source_paths` to a scope-matching value or skills silently drop before scoring. **T09's threshold tuning depends on this load fix being present.**
- The persisted `skills` table has no source-path column → per-file provenance cannot be reconstructed at boot, only scope-root provenance. Surface to T09 if it expects finer-grained path matching.
- Test containers: PG 15432, Qdrant HTTP 16333 / gRPC 16334, Redis 16379, Ollama 11444; live DB is `skill_layer_test` (not `skill_layer`).
- The packet's `dependency_factory.rs` reference was speculative; the real shared factory is `DependencyFactory` in `crates/infrastructure/src/health.rs`. Live-wiring logic kept in `mcp-server`, not the factory.

## TDD Evidence
- **Red**
  - Command: `cargo test -p mcp-server --features test-utils boot_time_live_retrieval`
  - Result: FAIL — `error[E0599]: no function or associated item named 'from_environment' found for struct 'McpServerApp'` (production live-boot entry point did not exist); after compiling against the new API, failed at runtime with `NoMatch (reason "no_relevant_skills")` — the exact `no_match` defect this unit closes.
- **Green**
  - Command: `cargo test -p mcp-server --features test-utils --test test_boot_time_live_retrieval boot_time_live_retrieval -- --ignored` (env → test containers)
  - Result: PASS — `test boot_time_live_retrieval ... ok`; clean boot reads the seeded live graph and `compile_context` returns `Ok` containing `boot-time-rust-file-io`.
- **Post-Refactor Green**
  - Command: same, rerun after rename/constructor consolidation + `source_paths` wiring + fmt
  - Result: PASS — `test result: ok. 1 passed` + `cargo test --workspace --lib` all-green + `--features test-utils --no-run` 0 errors.

## Test Results
- Command: `cargo test -p mcp-server --features test-utils boot_time_live_retrieval`
- Result: PASS (independently re-verified by orchestrator on live containers, 0.48s)
- Attempts: 4

## Orchestrator Verification (independent)
- `grep -rn 'SeededGraph' crates/ tests/` = 0; `build_seeded_server` = 0; `RetrievalSnapshot` present (23 refs).
- `main.rs` boots via `from_environment` / `with_explicit_graph` / `MCP_RETRIEVAL_MODE` (TODO + removal criterion present).
- `ScopeRoot` in `domain/types.rs:121`; alias `graph-builder/src/watcher.rs:46`.
- `warn!`+truncate at `MAX_SKILLS_TO_LOAD = 5000` (`lib.rs:416-423`); `current_graph_version` reads `graph_state`.
- `cargo test --workspace --features test-utils --no-run` → all executables built, 0 errors.
- Containerized smoke re-run by orchestrator → PASS.
- fmt drift exists only in `crates/graph-builder/src/graph/rebuild.rs` — a file T01 did NOT touch (pre-existing repo drift, left untouched per scope fence).

## Scope-Fence Note for Review
- The `source_paths` boot-wiring change touches `build_graph_from_pg` (snapshot construction), not the ranking algorithm — within "where the graph comes from at boot." Flagged explicitly; ties to T09's "seeded skills load into the live retriever (T01/T02)" note. `/workflows:review` should confirm this stayed boot-wiring and did not pre-empt T09's ranking work.
