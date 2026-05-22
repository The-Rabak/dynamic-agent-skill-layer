---
status: complete
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

Implemented Option 1 as a bounded cleanup pass: corrected misleading index naming, extracted shared extraction helpers, and factored repeated SQL command blocks in the infrastructure test script.

## Acceptance Criteria

- [x] Naming and helper structure no longer mislead readers.
- [x] No behavior change introduced by cleanup-only edits.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Grouped uncle-bob and code-simplicity-reviewer P3 improvements.

**Learnings:**
- These are quality improvements, not blockers, and should remain capped as P3 scope.

### 2026-05-22 - Execution

**By:** Copilot CLI

**Actions:**
- Renamed misleading schema index name in `crates/infrastructure/migrations/001_initial_schema.sql` (`idx_skills_name_trgm` -> `idx_skills_name`).
- Added shared extraction helpers in `crates/infrastructure/src/extraction/http.rs` and expanded `limits.rs`, then reused them from both provider adapters.
- Factored repeated PostgreSQL execution blocks in `scripts/run-t02-infrastructure-tests.sh` into a shared `psql_exec` helper.

**Learnings:**
- Centralizing identical request/validation paths keeps provider adapters aligned without changing runtime behavior.
