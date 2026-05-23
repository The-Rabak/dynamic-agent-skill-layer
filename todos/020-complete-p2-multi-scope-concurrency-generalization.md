---
status: complete
priority: p2
issue_id: "020"
tags: [code-review, t04, retrieval, architecture, drift-risk]
dependencies: ["017"]
---

# Generalize scope concurrency beyond exactly two scopes

## Problem Statement

`search_scopes_concurrently` special-cases one and two scopes, then falls back to sequential execution for any other scope count. This is a drift risk for future team-scope expansion.

## Findings

- Function name implies general concurrency.
- Implementation is concurrent only for the two-scope case.
- Architecture and roadmap anticipate additional scope extensibility.
- WHY impact: not immediate blocker for current T04 behavior, but increases future change risk.

## Proposed Solutions

### Option 1: N-scope bounded parallel search

**Approach:** Replace match-branch special casing with task-set / `join_all` bounded by max concurrency.

**Pros:**
- Honest behavior and easier extension.
- Consistent latency characteristics across scope counts.

**Cons:**
- Slightly more orchestration complexity.

**Effort:** Medium

**Risk:** Low

---

### Option 2: Explicit dual-scope API

**Approach:** Rename function/type semantics to dual-scope only; reject >2 scopes.

**Pros:**
- Clears ambiguity immediately.

**Cons:**
- Requires later rework for team scope.

**Effort:** Small

**Risk:** Medium

---

### Option 3: Leave as-is with docs note

**Approach:** Document >2 scope fallback.

**Pros:**
- No code churn.

**Cons:**
- Keeps latent behavior trap.

**Effort:** Small

**Risk:** Medium

## Recommended Action

Adopt Option 1: run all scope searches concurrently (not just the 2-scope branch), while preserving existing timeout/failure behavior per scope.

## Technical Details

**Affected files:**
- `crates/retrieval/src/dual_scope.rs`
- related retrieval tests

## Resources

- Architecture context tier notes for future scope expansion.

## Acceptance Criteria

- [x] Function name and behavior are aligned.
- [x] >2 scope behavior is either truly concurrent or explicitly unsupported.
- [x] Tests cover behavior for 1, 2, and >2 scopes.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Logged drift-risk finding from simplicity/performance/architecture synthesis.

**Learnings:**
- Naming-contract mismatch is a predictable source of future defects.

## Notes

- WHY classification: ⚠️ DRIFT RISK.

### 2026-05-23 - Multi-scope concurrency generalized

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Updated `search_scopes_concurrently` so the `>2` scope path executes per-scope searches concurrently via Tokio tasks instead of sequentially.
- Preserved existing behavior for `1` and `2` scopes, and preserved per-scope timeout/failure mapping semantics.
- Added retrieval unit coverage for a 3-scope latency envelope and verified retrieval crate tests pass.
- Marked this todo complete with acceptance criteria checked.

**Learnings:**
- A function name that implies generalized concurrency should avoid hidden cardinality-specific fallbacks; adding explicit >2 latency tests helps prevent regressions.
