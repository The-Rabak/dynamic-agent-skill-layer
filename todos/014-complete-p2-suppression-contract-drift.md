---
status: complete
priority: p2
issue_id: "014"
tags: [code-review, contracts, mcp-server, compile-context]
dependencies: []
---

# compile_context suppression metadata drifts from runtime state

## Problem Statement

`duplicate_suppressed` responses hardcode `scopes_considered` to `["global"]`, and suppression checks do not use stored `graph_version` for freshness. This can return stale or misleading envelope metadata and weaken suppression contract clarity.

## Findings

- `duplicate_suppressed` path sets `scopes_considered` to hardcoded `global` (`crates/mcp-server/src/tools/compile_context.rs:70`).
- Healthy responses pass through actual runtime scopes from retrieval outcome (`compile_context.rs:106,128`).
- `SessionSuppressionState` stores `graph_version`, but `is_suppressed` ignores it (`crates/mcp-server/src/state.rs:21-27`, `34-43`).
- Reviewers flagged this as contract drift risk against canonical result semantics.

## Proposed Solutions

### Option 1: Make suppression decision/version-aware and return real scopes (Recommended)

**Approach:** Persist and compare suppression `graph_version` against current graph version; return scope metadata derived from current retrieval config/state rather than hardcoded value.

**Pros:**
- Restores envelope consistency.
- Prevents stale suppression behavior after graph changes.

**Cons:**
- Requires passing current graph version/scope context into suppression check.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Keep current behavior and document this as intentional temporary simplification

**Approach:** Add explicit note in ticket/architecture for temporary hardcoded scope and non-versioned suppression.

**Pros:**
- No code change now.

**Cons:**
- Maintains contract ambiguity and future drift risk.

**Effort:** Small

**Risk:** Medium

## Recommended Action


## Technical Details

**Affected files:**
- `crates/mcp-server/src/tools/compile_context.rs`
- `crates/mcp-server/src/state.rs`
- `crates/retrieval/src/orchestrator.rs` (scope source reference)

**Related components:**
- `compile_context` result envelope contract
- Session suppression semantics

**Database changes (if any):**
- No

## Resources

- Ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/03-single-scope-compile-context.md`
- Architecture contract references in `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`

## Acceptance Criteria

- [x] `duplicate_suppressed` returns scope metadata consistent with runtime configuration.
- [x] Suppression freshness behavior is explicitly version-aware or documented with approved exception.
- [x] Tests cover stale-version suppression edge cases.

## Work Log

### 2026-05-23 - Review synthesis (full working tree)

**By:** Copilot CLI

**Actions:**
- Correlated suppression branch response fields with state implementation.
- Captured metadata/version drift as P2 contract-quality finding.

**Learnings:**
- Small envelope defaults can silently diverge from retrieval reality over time.

### 2026-05-23 - Implementation complete

**By:** Copilot CLI

**Actions:**
- Made suppression checks graph-version-aware in `SessionSuppressionState::is_suppressed`.
- Persisted and returned `scopes_considered` in suppression state.
- Updated `compile_context` duplicate-suppressed path to use stored/current runtime scopes instead of hardcoded `global`.
- Added/updated tests to assert duplicate-suppressed scope parity with healthy first response.

**Learnings:**
- Persisting response metadata in suppression state avoids subtle contract drift as runtime configuration evolves.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT
- Severity rationale: important contract consistency issue, not immediate blocker for core tracer-bullet behavior.
