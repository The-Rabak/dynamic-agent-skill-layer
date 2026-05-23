---
unit: "T04 dual-scope retrieval + hook semantics"
unit_number: 1
unit_kind: expansion
serves: "SC-2 dual-scope concurrent retrieval with weighted RRF and SC-1 first-prompt hook semantics"
status: completed
attempt_count: 1
domains: [backend, testing, documentation]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/04-dual-scope-retrieval-and-hooking.md
session_id: work-2026-05-23-122414
---

## What Was Implemented
- Added concurrent dual-scope retrieval orchestration (`project` + `global`) in retrieval.
- Added resolver seam wiring (`DualScopeResolver`) so retrieval consumes resolver output instead of ad-hoc scope discovery.
- Added weighted reciprocal-rank fusion after per-scope MMR with project-priority weighting defaults.
- Kept `compile_context` suppression semantics healthy-only (`ok`/`no_match` suppress; `degraded` does not).
- Added a Claude hook example config capturing inject/suppress/retry policy for first-prompt behavior.
- Added integration tests for dual-scope retrieval ordering and suppression/session isolation semantics.

## Files Changed
- `crates/retrieval/src/dual_scope.rs` -- created concurrent scope search + timeout handling + per-scope candidate preparation.
- `crates/retrieval/src/scope_resolution.rs` -- created dual resolver orchestration with degradation reason propagation.
- `crates/retrieval/src/fusion.rs` -- added weighted RRF fusion primitives and tests.
- `crates/retrieval/src/orchestrator.rs` -- extended orchestrator with dual-scope flow, resolver integration, and fusion selection.
- `crates/retrieval/src/lib.rs` -- exported new dual-scope and scope-resolution modules.
- `crates/retrieval/Cargo.toml` -- enabled tokio features required by new async test/runtime usage.
- `crates/mcp-server/src/lib.rs` -- wired seeded server to dual-scope resolver + dual-scope retriever constructor.
- `crates/mcp-server/Cargo.toml` -- registered new integration test target.
- `crates/infrastructure/src/scope.rs` -- added delimiter-flexible env parsing and tests for SKILL_GLOBAL_PATHS defaults.
- `tests/integration/test_dual_scope.rs` -- created dual-scope integration tests.
- `tests/integration/test_compile_context.rs` -- aligned tests with resolver env setup under dual-scope server wiring.
- `config/claude-code/hooks.example.json` -- created Claude hook example policy contract.

## Problems Encountered
### Problem 1: seeded server became dual-scope and required resolver env in tests
- **Error:** integration tests that build seeded server without scope env setup degraded unexpectedly.
- **Root cause:** seeded server now resolves both project and global scopes by default.
- **Fix:** test fixtures now set `SKILL_GLOBAL_PATHS` and `SKILL_GLOBAL_ALLOWED_ROOTS` before invoking server calls.

## Patterns Discovered
- Keep resolver behavior behind `ScopeResolver` seam and pass resolved scope descriptors into retrieval.
- Keep MCP compile tool strictly orchestration/status behavior; retrieval/fusion stays in retrieval crate.
- For scope fusion determinism, use explicit scope-priority tie-breakers after weighted RRF.

## TDD Evidence
- **Red**
  - Command: `cargo test --workspace --test test_dual_scope`
  - Result: FAIL
  - Evidence: new dual-scope behavior and weighted-fusion APIs were absent; expected dual-scope outcomes were not produced.
- **Green**
  - Command: `cargo test --workspace`
  - Result: PASS
  - Evidence: new dual-scope integration tests and retrieval/fusion unit tests pass with project/global scope coverage.
- **Post-Refactor Green**
  - Command: `cargo clippy --workspace -- -D warnings && cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
  - Result: PASS
  - Evidence: lint, full workspace tests, and compose topology/e2e command all succeeded after final cleanup.

## Test Results
- Command: `cargo clippy --workspace -- -D warnings && cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 1
