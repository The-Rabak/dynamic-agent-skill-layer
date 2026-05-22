---
status: pending
priority: p3
issue_id: "010"
tags: [code-review, simplicity, maintainability, quality-improvement]
dependencies: []
---

# Apply low-risk code simplicity cleanups in infrastructure

## Problem Statement

Several low-severity readability and maintainability issues were flagged that are safe to defer but worth cleaning up.

## Findings

- Misleading index name suggests trigram behavior but index is plain (`001_initial_schema.sql:122-123`).
- Duplicate extraction flow patterns risk drift across providers.
- Test script contains repeated assertion blocks that can be factored.

## Proposed Solutions

### Option 1: Consolidated cleanup pass
**Approach:** Rename misleading index or implement true trigram index; extract shared helpers in extraction/script code.
**Pros:** Cleaner maintenance baseline.
**Cons:** Non-trivial touch across unrelated files.
**Effort:** Small-Medium  
**Risk:** Low

---

### Option 2: Opportunistic cleanup only when touching each area
**Approach:** Fix each item during nearby feature work.
**Pros:** Minimal interruption to roadmap.
**Cons:** Debt may linger longer.
**Effort:** Small  
**Risk:** Low

## Recommended Action

To be filled during triage.

## Acceptance Criteria

- [ ] Naming and helper structure no longer mislead readers.
- [ ] No behavior change introduced by cleanup-only edits.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Grouped uncle-bob and code-simplicity-reviewer P3 improvements.

**Learnings:**
- These are quality improvements, not blockers, and should remain capped as P3 scope.

