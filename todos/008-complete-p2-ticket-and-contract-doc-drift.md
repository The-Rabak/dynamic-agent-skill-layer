---
status: complete
priority: p2
issue_id: "008"
tags: [code-review, documentation, ticket-flow, drift-risk]
dependencies: []
---

# Resolve ticket/session/contract documentation drift

## Problem Statement

Several ticket and execution artifacts are out of sync with implementation and test reality, reducing review traceability.

## Findings

- T02 `files:` list omits new test script.
- STATE TDD command still references older E2E command while ticket points to script.
- Some downstream docs reference stale outbox/rebuild field names per data-migration review.
- Script uses `graph.updated` fixture name while architecture canon lists different event names.

## Proposed Solutions

### Option 1: Normalize all affected docs to current contracts
**Approach:** Update ticket, session state, and downstream references in one pass with citations.
**Pros:** Restores auditability and reduces execution confusion.
**Cons:** Documentation sweep effort.
**Effort:** Medium  
**Risk:** Low

---

### Option 2: Patch only T02 files now, defer downstream docs
**Approach:** Fix local ticket/session artifacts immediately and open follow-up for broader references.
**Pros:** Faster immediate cleanup.
**Cons:** Leaves residual drift.
**Effort:** Small  
**Risk:** Medium

## Recommended Action

Implemented Option 1 for active T02 artifacts and coupled contracts: normalized T02 ticket/session references, canonical event naming, and stale contract field names in active tickets/scripts.

## Acceptance Criteria

- [x] T02 ticket/session artifacts match actual implementation and test commands.
- [x] Canonical event naming and schema references are consistent across active docs.
- [x] No stale contract fields remain in relevant execution tickets.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Captured traceability drift findings from ticket-flow, architecture, and data-migration reviews.

**Learnings:**
- Artifact drift is currently non-blocking for runtime but high-impact for future ticket execution quality.

### 2026-05-22 - Execution

**By:** Copilot CLI

**Actions:**
- Added `scripts/run-t02-infrastructure-tests.sh` to T02 ticket `files:` for traceability.
- Updated T02 execution STATE required E2E command to the hardened script.
- Updated script fixture events from `graph.updated` to canonical `graph.rebuilt`.
- Corrected stale field naming in T07 (`retry_count` -> `attempts`).

**Learnings:**
- Documentation drift tends to accumulate at ticket/session boundaries, so execution artifacts should track test-entrypoint changes immediately.
