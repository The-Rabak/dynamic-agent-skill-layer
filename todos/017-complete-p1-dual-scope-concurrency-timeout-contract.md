---
status: complete
priority: p1
issue_id: "017"
tags: [code-review, t04, retrieval, performance, concurrency, protects-user-story]
dependencies: []
---

# Make dual-scope search truly concurrent and timeout-effective

## Problem Statement

`search_scopes_concurrently` appears concurrent for project/global, but per-scope work is CPU-bound and non-yielding, making timeout and parallel-latency guarantees unreliable.

## Findings

- `run_project_and_global_concurrently` uses `tokio::join!`, but `perform_scope_search` has no internal awaits.
- Timeout wrapper can be ineffective for non-yielding futures.
- T04 acceptance includes expected parallel latency envelope.
- WHY impact: threatens SC-2 under real workload and scale.

## Proposed Solutions

### Option 1: Offload CPU-heavy scope work to blocking pool

**Approach:** Wrap per-scope compute in `spawn_blocking`, enforce timeout on join handles, and add cancellation semantics.

**Pros:**
- Restores real concurrency under CPU load.
- Makes timeout meaningful.

**Cons:**
- Requires careful ownership and allocation tuning.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Precompute + keep async path light

**Approach:** Precompute heavy graph artifacts and keep per-request work small enough that async join remains effective.

**Pros:**
- Improves both latency and memory churn.

**Cons:**
- Broader refactor across retrieval data flow.

**Effort:** Large

**Risk:** Medium

---

### Option 3: Narrow contract to dual-scope best effort

**Approach:** Keep implementation mostly as-is, relax latency claims in docs/tests.

**Pros:**
- Minimal code changes.

**Cons:**
- Conflicts with T04 acceptance intent.

**Effort:** Small

**Risk:** High

## Recommended Action

Implement Option 1 (offload per-scope compute to blocking pool): run each scope search on Tokio's
blocking runtime via `spawn_blocking`, enforce timeout over the join handle, and return scope-specific
failure codes on timeout/task failure.

## Technical Details

**Affected files:**
- `crates/retrieval/src/dual_scope.rs`
- `todos/017-complete-p1-dual-scope-concurrency-timeout-contract.md`

## Resources

- T04 ticket acceptance criteria
- Architecture retrieval seam and latency guidance

## Acceptance Criteria

- [x] Per-scope searches execute with demonstrable parallel behavior under CPU-heavy conditions.
- [x] Timeout behavior is effective and test-covered.
- [x] Latency envelope test reflects real retrieval path (not only synthetic sleeps).
- [x] No scope-result contract regressions.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Consolidated performance findings around concurrency and timeout semantics.
- Mapped impact to SC-2 user-facing behavior.

**Learnings:**
- Async syntax alone does not guarantee concurrency for CPU-bound retrieval code paths.

### 2026-05-23 - Implemented timeout-effective dual-scope execution

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Updated `crates/retrieval/src/dual_scope.rs` to run per-scope search work with `tokio::task::spawn_blocking`.
- Wrapped blocking handle completion with timeout enforcement and explicit abort on timeout.
- Added `timeout_is_effective_for_blocking_scope_work` unit test validating timeout contract for blocking scope work.
- Added `real_scope_search_path_meets_parallel_latency_envelope` unit test validating parallel latency on the real `search_scopes_concurrently` retrieval path.
- Verified retrieval dual-scope unit tests via:
  - `cargo test -p retrieval dual_scope::tests:: -- --nocapture`

**Learnings:**
- Timeouts become reliable for CPU-bound scope work when timeout is applied to a blocking task handle rather
  than a non-yielding async future.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY.
