---
unit: "Slice 1.1 — Real project-scoped Ok through the production container (#154 git-free resolver)"
unit_number: 1
unit_kind: tracer-bullet
serves: "SC#3 (real project-scoped Ok in-container) + secondary user story"
status: completed
attempt_count: 1
domains: [rust, infrastructure, scope-resolution, e2e]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
`FsMarkerProjectResolver` (git-free) in `crates/infrastructure/src/scope.rs`: walks ancestors for `.git` or the
path named by `SKILL_PROJECT_MARKER` env var, no subprocess; returns `ScopeDescriptor{ config:{"resolver":"fs-marker"} }`;
honest `ResolverUnavailable` at filesystem root with no match. Exported from `lib.rs:58` (kept `GitRootProjectResolver`
for trait-level rollback). Wired in place of `GitRootProjectResolver` at `crates/mcp-server/src/lib.rs:176` and `:512`.
New live e2e proof `tests/e2e/test_project_scope_container.rs` (`#[ignore="requires live containers"]`).

## Files Changed
- `crates/infrastructure/src/scope.rs` — added resolver + 3 unit tests
- `crates/infrastructure/src/lib.rs` — re-export
- `crates/mcp-server/src/lib.rs` — wiring swap at both sites, removed unused import
- `crates/mcp-server/Cargo.toml` — registered `[[test]] test_project_scope_container`
- `tests/e2e/test_project_scope_container.rs` — created (live proof)

## Problems Encountered
- clippy `ptr_arg` (`&PathBuf`→`&Path`) — fixed. Unused `GitRootProjectResolver` import in mcp-server — removed. fmt diffs — ran fmt.

## Patterns Discovered
- e2e shared modules included via `#[path="report.rs"] mod report;` / `#[path="../integration/env_guard.rs"] mod env_guard;`.
- Two resolver construction sites: `with_explicit_graph` (~176) and `build_live_server` (~512) — both must swap.
- `invalid_repo_path_degrades_but_preserves_global_context_contract` (in test_live_data_plane_roundtrip.rs) is the
  regression anchor for the ResolverUnavailable fallback — still passes.

## TDD Evidence
- **Red:** `cargo test -p infrastructure scope::tests::fs_marker` → E0433 undeclared `FsMarkerProjectResolver` (behavior absent).
- **Green:** same command → 3 passed.
- **Post-Refactor Green:** `cargo test -p infrastructure scope` → 8 passed (after `&Path` clippy fix + fmt).

## Test Results
- `cargo test -p infrastructure scope` → 8 passed. Compile of new live target clean.
- Live: `cargo test -p mcp-server --features test-utils --test test_project_scope_container -- --include-ignored` → PENDING-LIVE (needs stack).
