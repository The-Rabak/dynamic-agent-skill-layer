---
status: complete
priority: p1
issue_id: "018"
tags: [code-review, t04, security, retrieval, scope, protects-user-story]
dependencies: []
---

# Enforce scope descriptor boundaries during candidate retrieval

## Problem Statement

Scope resolution validates global paths and returns descriptors, but retrieval filtering currently relies on `scope_type` only. This can permit scope-boundary drift if indexed metadata is inconsistent.

## Findings

- Candidate filtering in retrieval path checks enum scope type, not descriptor paths/scope_id.
- Path validation exists in infrastructure resolver but is not enforced downstream at candidate-level.
- This creates a potential cross-scope retrieval integrity gap.
- WHY impact: can degrade relevance and safety of injected context.

## Proposed Solutions

### Option 1: Candidate-level scope metadata enforcement

**Approach:** Require each indexed skill to carry scope_id/path metadata and validate against resolved `ScopeDescriptor` at query time.

**Pros:**
- Strong boundary correctness.
- Aligns retrieval with resolver contract intent.

**Cons:**
- Requires metadata propagation and migration/tests.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Ingestion-time invariant + runtime assertion

**Approach:** Validate and reject out-of-scope records at graph build time; keep lightweight runtime assertions.

**Pros:**
- Lower query-time overhead.

**Cons:**
- Defense-in-depth weaker than strict runtime filtering.

**Effort:** Medium

**Risk:** Medium

---

### Option 3: Document trust assumption only

**Approach:** Keep current behavior and document index trust assumptions.

**Pros:**
- Minimal work.

**Cons:**
- Leaves security/integrity concern unresolved.

**Effort:** Small

**Risk:** High

## Recommended Action

Option 1 implemented: candidate-level scope metadata enforcement with scope_id and path boundary checks.

## Technical Details

**Affected files:**
- `crates/retrieval/src/dual_scope.rs`
- `crates/retrieval/src/orchestrator.rs`
- graph/index metadata model files (as needed)

## Resources

- Security review findings from T04 review run
- Scope contracts in architecture artifact

## Acceptance Criteria

- [x] Retrieval enforces resolved scope boundary metadata beyond enum type.
- [x] Tests prove out-of-scope candidates are excluded.
- [x] No regression to legitimate project/global retrieval results.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Captured security-sentinel P1 as a concrete boundary contract task.
- Linked finding to retrieval and scope resolver seam ownership.

**Learnings:**
- Validating input scope paths is insufficient without downstream candidate-boundary enforcement.

### 2026-05-23 - Scope boundary enforcement implemented

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Added `scope_id` and `source_paths` metadata to `SeededSkill` and enforced it during dual-scope candidate filtering.
- Added a retrieval unit test to prove mismatched `scope_id`/path candidates are excluded.
- Updated seeded integration graphs to include explicit scope metadata and in-bound source paths.
- Verified with:
  - `cargo test -p retrieval dual_scope::tests`
  - `cargo test -p mcp-server --test test_compile_context`
  - `cargo test -p mcp-server --test test_dual_scope`

**Learnings:**
- Filtering by enum scope type alone is insufficient; per-candidate scope metadata must match the resolved descriptor to preserve scope boundaries.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY.
