---
status: complete
priority: p2
issue_id: "029"
tags: [code-review, performance, graph-builder, watcher]
dependencies: []
---

# Bound watcher recovery idempotency memory

## Problem Statement

`WatcherRecovery` stores every seen idempotency key in an ever-growing in-memory set, which can grow unbounded in long-running graph-builder processes.

## Findings

- `crates/graph-builder/src/watcher_recovery.rs:7-8` stores a `HashSet<String>` of all seen keys.
- `crates/graph-builder/src/watcher_recovery.rs:21-24` only inserts; no eviction/rotation strategy exists.
- Long-lived rebuild services with frequent changes will accumulate memory indefinitely.

## Proposed Solutions

### Option 1: Time-windowed eviction (recommended)

**Approach:** Store idempotency keys with timestamp and evict entries older than bounded TTL.

**Pros:**
- Preserves dedup semantics for realistic replay windows.
- Prevents unbounded growth.

**Cons:**
- Adds clock-based cleanup logic.

**Effort:** 2-4 hours

**Risk:** Low

---

### Option 2: Scope by snapshot epoch/version and clear between rebuild windows

**Approach:** Track cache per snapshot generation and drop old generations after successful rebuild cycles.

**Pros:**
- Simpler memory bounds with deterministic lifecycle.

**Cons:**
- Needs careful handling around delayed duplicate events.

**Effort:** 2-3 hours

**Risk:** Medium

## Recommended Action

Implement generation-windowed eviction in `WatcherRecovery` so idempotency keys are
deduplicated for recent reconciliation cycles while stale keys are deterministically
evicted to bound memory growth.

## Technical Details

**Affected files:**
- `crates/graph-builder/src/watcher_recovery.rs`
- `tests/integration/test_watcher_rebuild.rs` (idempotency behavior updates)

**Database changes (if any):**
- No

## Resources

- `crates/graph-builder/src/watcher_recovery.rs`

## Acceptance Criteria

- [x] Idempotency cache has bounded memory behavior
- [x] Existing idempotent reconciliation guarantees remain true
- [x] Regression tests cover duplicate suppression across reconciliation cycles

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Audited watcher recovery dedup cache lifecycle and growth behavior.

**Learnings:**
- Correctness is currently favored over lifecycle bounds; production path needs both.

### 2026-05-25 - Implemented bounded cache lifecycle

**By:** Copilot CLI

**Actions:**
- Replaced unbounded watcher recovery dedup set with generation-windowed eviction logic in `crates/graph-builder/src/watcher_recovery.rs`.
- Added deterministic unit tests for repeated-snapshot idempotency and stale-key eviction after generation-window advancement.
- Ran focused validation with `cargo test -p graph-builder watcher_recovery::tests -- --nocapture`.

**Learnings:**
- Tracking last-seen generation per idempotency key preserves short-horizon idempotency while keeping long-running memory usage bounded.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
