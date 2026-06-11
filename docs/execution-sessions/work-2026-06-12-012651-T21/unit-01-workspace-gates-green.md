---
unit: "T21 — Workspace gates green (clippy -D warnings + fmt clean)"
unit_number: 1
unit_kind: hygiene
serves: "The honest-tree property every Phase B measured claim rests on; unblocks meaningful full-suite runs for T20/T14."
status: completed
attempt_count: 2
domains: [build, workspace-hygiene, rust]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/21-workspace-gates-green.md
session_id: work-2026-06-12-012651-T21
---

## What Was Implemented

Drove both workspace static gates to exit 0 with zero behavior change.

1. **fmt sweep** — `cargo fmt` cleaned 31 diffs across 7 files: `crates/graph-builder/src/graph/{edges.rs, rebuild.rs}`, `crates/infrastructure/src/{health.rs, persistence/embedding_cache.rs, persistence/rebuild.rs, vector/qdrant.rs}`, `crates/mcp-server/src/lib.rs`. Formatting only.
2. **`useless_vec`** (`crates/retrieval/src/cosine_rank.rs:64`) — test `vec![vec![…]]` → array literal `[vec![…]]`. This was the masking abort.
3. **`items_after_test_module`** (`crates/mcp-server/src/tools/search_skill_graph.rs`) — `classify_edges_for_matches` + `emit_edge` moved from after the `#[cfg(test)]` block to before it. Byte-identical relocation.
4. **Dead-code class** — the real masked offender was `crates/mcp-server/tests/env_guard.rs` being auto-discovered by Cargo as an orphan standalone integration-test binary (it is a `#[path]`-included helper). Relocated to `crates/mcp-server/tests/helpers/env_guard.rs` (byte-identical) and updated the three `#[path]` includes in `test_compile_context.rs`, `test_dual_scope.rs`, `test_session_persistence.rs`. NO blanket `#[allow(dead_code)]`. The previously-feared QdrantObserver/RedisObserver/run_docker/ScopeEnvGuard class was NOT the blocker.
5. **Bench compile-fix** (`tests/bench/compile_context_bench.rs`) — added the three T09 `e_task_embedding`/`e_needs_embedding`/`e_negative_embedding` empty-vec fields the bench was missing. Empty == absent per the fusion contract; zero behavior delta. One-line intent comment added.

## Files Changed
- `crates/retrieval/src/cosine_rank.rs` — modified (useless_vec)
- `crates/graph-builder/src/graph/edges.rs` — modified (fmt)
- `crates/graph-builder/src/graph/rebuild.rs` — modified (fmt)
- `crates/infrastructure/src/health.rs` — modified (fmt)
- `crates/infrastructure/src/persistence/embedding_cache.rs` — modified (fmt)
- `crates/infrastructure/src/persistence/rebuild.rs` — modified (fmt)
- `crates/infrastructure/src/vector/qdrant.rs` — modified (fmt)
- `crates/mcp-server/src/lib.rs` — modified (fmt)
- `crates/mcp-server/src/tools/search_skill_graph.rs` — modified (function move)
- `crates/mcp-server/tests/env_guard.rs` — deleted (moved)
- `crates/mcp-server/tests/helpers/env_guard.rs` — created (byte-identical move)
- `crates/mcp-server/tests/{test_compile_context,test_dual_scope,test_session_persistence}.rs` — modified (#[path] updated)
- `tests/bench/compile_context_bench.rs` — modified (3 missing SeededSkill fields)

## Problems Encountered
### Problem 1: clippy still RED after the useless_vec fix
- **Error:** second clippy run exited 101 with 7 errors in `env_guard.rs`, 1 `items_after_test_module` in `search_skill_graph.rs`, and 1 missing-fields error in `compile_context_bench.rs`.
- **Root cause:** the first error (`useless_vec`) aborted compilation and masked the rest from the original RED enumeration.
- **Fix:** drained all in one further pass (relocate env_guard, move functions, add bench fields).

## Patterns Discovered
- Cargo auto-discovers every `.rs` directly under `tests/` as a standalone integration-test binary, even `#[path]`-included helpers → trips dead-code under `--all-targets`. Put helpers in a `tests/<subdir>/`.
- `clippy::items_after_test_module`: keep non-test items above the `#[cfg(test)]` block.
- First clippy error masks the rest; drain top-down.

## TDD Evidence
- **Red**
  - Command: `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check`
  - Result: FAIL (clippy exit 101 at cosine_rank.rs:64; fmt 31 diffs) — orchestrator-captured baseline
  - Evidence: the gates demonstrably failed for the V1.7-session-introduced offenders before the unit.
- **Green**
  - Command: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
  - Result: PASS (exit 0) — orchestrator-verified independently
  - Evidence: both commands finish clean; the requested honest-tree state now holds.
- **Post-Refactor Green**
  - Command: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
  - Result: PASS (exit 0)
  - Evidence: final re-run after the full sweep + relocations; gates stable. Touched-test regression also green (`retrieval` cosine 2/2, `mcp-server` search_skill_graph 5/5).

## Test Results
- Command: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
- Result: PASS (both exit 0)
- Attempts: 2
