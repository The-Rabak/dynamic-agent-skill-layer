---
unit: "T12: Session persistence and context cache"
unit_number: 1
unit_kind: hardening
serves: "SC-1 (no duplicate first-prompt injection after restart), SC-7 (suppression and cache behavior survive process restart and graph changes)"
status: completed
attempt_count: 1
domains: backend, testing
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/12-session-persistence-and-context-cache.md
session_id: work-2026-05-28-201847
---

## What Was Implemented

Added `CompiledContextCache` struct to `crates/mcp-server/src/state.rs` implementing dual-tier cache (DashMap + optional Redis) keyed by prompt hash + scope fingerprint + graph_version. Cache checked before suppression state and retrieval pipeline. Populated on healthy outcomes (Ok, NoMatch) only. `clear_session()` method for both suppression and cache state. `CompileContextTool::new()` now takes cache param. `invoke()` checks cache first.

## Files Changed

- `crates/mcp-server/src/state.rs` -- added `CompiledContextCache` (get/set/clear_session, redis-backed), `SessionSuppressionState::clear_session()`, made `CachedContext` public, unit tests
- `crates/mcp-server/src/tools/compile_context.rs` -- wired cache into `invoke()`, added `scope_fingerprint()`, updated `new()` signature
- `crates/mcp-server/src/lib.rs` -- instantiated `CompiledContextCache`, passed to `CompileContextTool`
- `crates/mcp-server/Cargo.toml` -- moved `blake3` from dev-deps to deps, registered `test_session_persistence`
- `tests/integration/test_session_persistence.rs` -- created (5 tests: repeated prompt cache hit, version mismatch invalidation, degraded no-cache, healthy no-match cache, cached context equivalence)
- `tests/integration/env_guard.rs` -- fixed poison mutex recovery
- `tests/integration/test_compile_context.rs` -- updated two tests for new cache-first behavior

## TDD Evidence

### Red
- Command: `cargo test -p mcp-server --lib`
- Result: FAIL (6 test compilation errors from incomplete integration)
- Evidence: Tests expected `CompileContextTool::new()` with 3 params, code changed to 4 params

### Green
- Command: `cargo test -p mcp-server --lib && cargo test -p mcp-server --test test_session_persistence && cargo test -p mcp-server --test test_compile_context`
- Result: PASS (6 lib + 5 integration + 8 integration = 19 tests)

### Post-Refactor Green
- Command: `cargo test --workspace -- --test-threads=1`
- Result: PASS (all workspace tests, 102+ passing)

## Test Results
- Command: `cargo test --workspace -- --test-threads=1`
- Result: PASS
- Attempts: 1