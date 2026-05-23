---
status: complete
priority: p1
issue_id: "016"
tags: [code-review, t04, retrieval, resilience, protects-user-story]
dependencies: []
---

# Return partial results when one scope fails

## Problem Statement

The orchestrator currently returns fully degraded empty output when any one scope fails. This discards healthy scope results and can break zero-touch context injection.

## Findings

- In `retrieve`, any `scope_failures` leads to immediate degraded return.
- Healthy scope candidates are not fused or returned when peer scope fails.
- This behavior conflicts with dual-scope value delivery under partial outages.
- WHY impact: threatens SC-1/SC-2 and first-prompt success for user story.

## Proposed Solutions

### Option 1: Partial-success contract (recommended)

**Approach:** Fuse successful scopes, include degraded metadata for failed scopes, mark status as degraded-with-results.

**Pros:**
- Preserves useful context for users.
- Improves resilience without hiding failures.

**Cons:**
- Requires explicit response semantics and tests.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Config-gated fail-open behavior

**Approach:** Keep current behavior by default, add config flag to allow partial return.

**Pros:**
- Lower migration risk.

**Cons:**
- Semantic split increases complexity and drift risk.

**Effort:** Medium

**Risk:** Medium

---

### Option 3: Keep fail-closed, improve retry

**Approach:** Preserve current behavior but add aggressive retries/circuit logic.

**Pros:**
- Small logic change.

**Cons:**
- Still fails first-prompt value under partial scope outages.

**Effort:** Small

**Risk:** High

## Recommended Action

Implemented Option 1 (partial-success contract): preserve successful scope results while surfacing failed scopes as degraded metadata.

## Technical Details

**Affected files:**
- `crates/retrieval/src/orchestrator.rs`
- `crates/mcp-server/src/tools/compile_context.rs`
- `tests/integration/env_guard.rs`
- `tests/integration/test_dual_scope.rs`

## Resources

- Ticket T04 acceptance criteria (dual scope + suppression semantics)
- Execution session evidence docs in `docs/execution-sessions/work-2026-05-23-122414/`

## Acceptance Criteria

- [x] Retrieval returns available results when at least one scope succeeds.
- [x] Failed scopes remain visible in `degraded_scopes` and `reason_codes`.
- [x] Tests cover one-scope-fails/one-scope-succeeds behavior.
- [x] No regression to healthy suppression contract (`ok`/`no_match` only).

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Synthesized overlapping findings from architecture, performance, simplicity, and clean-code reviewers.
- Normalized to a single behavioral root cause.

**Learnings:**
- Partial failure handling is the highest-leverage reliability fix for user outcomes.

### 2026-05-23 - Implementation complete

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Updated retrieval orchestrator to return fused successful-scope results when at least one scope search succeeds, even if peer scopes degrade.
- Preserved degraded metadata (`degraded_scopes`, `reason_codes`) and degraded health markers on partial-success outcomes.
- Updated compile-context handling to return `degraded` responses with compiled context when retrieval includes usable results, while still suppressing only on healthy `ok`/`no_match`.
- Added integration coverage for partial scope failure (global resolution failure + project success) confirming degraded-with-context behavior and no suppression consumption.

**Learnings:**
- Partial-success responses can preserve first-prompt value while keeping failure visibility explicit for observability and retries.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY.
