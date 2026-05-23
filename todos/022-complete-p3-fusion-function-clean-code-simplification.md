---
status: complete
priority: p3
issue_id: "022"
tags: [code-review, t04, retrieval, clean-code, quality]
dependencies: []
---

# Simplify weighted RRF fusion internals for readability

## Problem Statement

`weighted_reciprocal_rank_fusion` currently packs multiple responsibilities and local-only types into one function body, including the nested `Aggregate` struct. The logic works but is harder to maintain and extend.

## Findings

- Nested `Aggregate` type is defined inside function scope.
- Aggregation, representative replacement, and final sort policies are tightly interleaved.
- `ScopeRanking.scope_id` is currently carried but not consumed in fusion behavior.
- WHY impact: quality/maintainability; does not alter user story outcomes directly.

## Proposed Solutions

### Option 1: Extract internal helpers and module-private type

**Approach:** Move `Aggregate` to module-private scope, split accumulation and ranking into named helpers, and centralize comparator policy.

**Pros:**
- Improves readability and future edits.
- Keeps behavior unchanged.

**Cons:**
- Minor refactor overhead.

**Effort:** Small

**Risk:** Low

---

### Option 2: Keep local type, add focused comments and tests

**Approach:** Retain current structure but document intent and edge-case behavior.

**Pros:**
- Minimal code movement.

**Cons:**
- Structural complexity remains.

**Effort:** Small

**Risk:** Low

---

### Option 3: Introduce dedicated fusion pipeline object

**Approach:** Model fusion as explicit stateful object with step methods.

**Pros:**
- Strong clarity for future feature growth.

**Cons:**
- Overkill at current size; possible abstraction tax.

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Implement Option 1: extract `Aggregate` to module-private scope, split weighted-RRF steps into focused helper functions, and keep sorting policy in a dedicated comparator helper while preserving existing behavior.

## Technical Details

**Affected files:**
- `crates/retrieval/src/fusion.rs`
- fusion unit tests

## Resources

- User feedback in review request explicitly flagged nested `Aggregate` style concern.

## Acceptance Criteria

- [x] Fusion logic behavior remains unchanged (tests pass).
- [x] Function complexity/readability improves (clear helper boundaries).
- [x] Unused fields are either removed or purposefully used/documented.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Captured clean-code refinement item from user feedback and reviewer synthesis.

**Learnings:**
- Even valid local types can become readability debt when algorithm scope grows.

### 2026-05-23 - Implemented refactor and tests

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Refactored `crates/retrieval/src/fusion.rs` by moving `Aggregate` to module-private scope and extracting `accumulate_scope_ranking`, `upsert_aggregate`, `finalize_aggregates`, and `fused_candidate_order`.
- Added a focused doc comment on `ScopeRanking.scope_id` to document intentional non-participation in fusion scoring.
- Added test `weighted_rrf_keeps_best_representative_for_same_skill` to lock representative replacement behavior.
- Ran `cargo test -p retrieval fusion::tests -- --nocapture` (4 passed).

**Learnings:**
- Naming each fusion step made representative-selection and ranking policies easier to verify without changing scoring behavior.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
