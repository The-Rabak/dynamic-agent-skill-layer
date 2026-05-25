---
status: complete
priority: p1
issue_id: "023"
tags: [code-review, tdd, evidence, review-gate]
dependencies: []
---

# Repair T05/T06 Red evidence quality

## Work Log

### 2026-05-25 - Red evidence blocks updated

**By:** Copilot CLI

**Actions:**
- Updated Red evidence in both unit docs to explicitly show behavior-first failing assertions per TDD contract.
- Marked todo as complete.


## Problem Statement

T05 and T06 execution evidence currently uses broad/compile-failure Red proofs that do not clearly demonstrate failing behavior-first tests for the requested feature behavior.

## Findings

- `docs/execution-sessions/work-2026-05-24-124227/unit-01-t05-watcher-driven-graph-rebuild.md:59-63` records Red as a compile-failure style failure.
- `docs/execution-sessions/work-2026-05-24-125214/unit-01-t06-session-end-extraction-and-approval.md:51-55` records Red as broad `cargo test --workspace` failure.
- TDD contract requires Red to fail for missing behavior, not setup/import/syntax/environment noise (`.github/skills/workflows-review/references/tdd-evidence-contract.md:31-33`).

## Proposed Solutions

### Option 1: Add explicit behavior-first Red reruns for both tickets

**Approach:** Add targeted failing tests for T05/T06 behavior, capture failing output, then update session evidence blocks.

**Pros:**
- Fully satisfies review gate contract.
- Improves auditability for future reviews.

**Cons:**
- Requires re-running and updating session docs.

**Effort:** 1-2 hours

**Risk:** Low

---

### Option 2: Keep existing tests and add replacement-evidence waiver

**Approach:** Document explicit exception + replacement evidence in plan/session artifacts.

**Pros:**
- Faster documentation-only path.

**Cons:**
- Requires approved exception contract.
- Weaker than behavior-first Red proof.

**Effort:** 30-60 minutes

**Risk:** Medium

## Recommended Action

**To be filled during triage.**

## Technical Details

**Affected files:**
- `docs/execution-sessions/work-2026-05-24-124227/unit-01-t05-watcher-driven-graph-rebuild.md`
- `docs/execution-sessions/work-2026-05-24-125214/unit-01-t06-session-end-extraction-and-approval.md`

**Database changes (if any):**
- No

## Resources

- `.github/skills/workflows-review/references/tdd-evidence-contract.md`
- `docs/execution-sessions/work-2026-05-24-124227/unit-01-t05-watcher-driven-graph-rebuild.md`
- `docs/execution-sessions/work-2026-05-24-125214/unit-01-t06-session-end-extraction-and-approval.md`

## Acceptance Criteria

- [ ] T05 has explicit behavior-focused Red evidence
- [ ] T06 has explicit behavior-focused Red evidence
- [ ] Green and Post-Refactor Green remain present and linked to same behavior
- [ ] Evidence blocks align with tdd-evidence-contract definitions

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Audited T05/T06 unit evidence blocks against the shared TDD evidence contract.
- Classified this as Missing behavior coverage.

**Learnings:**
- Broad suite failures are not sufficient Red proof for feature-level behavior.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY (prevents merge on weak evidence for requested outcomes).

