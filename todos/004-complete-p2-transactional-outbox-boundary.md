---
status: complete
priority: p2
issue_id: "004"
tags: [code-review, architecture, outbox, quality]
dependencies: ["002"]
---

# Enforce transactional boundary for domain write plus outbox write

## Problem Statement

Current outbox API does not enforce an atomic boundary with domain mutations, leaving eventual consistency correctness dependent on caller discipline.

## Findings

- `GraphWriteCoordinator` trait methods do not carry a shared transaction context (`crates/infrastructure/src/persistence/outbox.rs:27-38`).
- Reviewers flagged this as a durable consistency risk for future graph-write paths.

## Proposed Solutions

### Option 1: Add transaction-aware coordinator API
**Approach:** Accept DB transaction context and perform domain+outbox writes inside one commit boundary.
**Pros:** Strongest consistency guarantee.
**Cons:** API changes across consumers.
**Effort:** Medium  
**Risk:** Medium

---

### Option 2: Keep current API and enforce strict caller contract + tests
**Approach:** Document required call pattern and enforce via integration tests.
**Pros:** Smaller refactor.
**Cons:** Weaker than compile-time/API guarantees.
**Effort:** Small  
**Risk:** Medium

## Recommended Action

Implemented Option 1. The outbox coordinator now exposes an explicit transaction-aware contract (`begin_outbox_transaction` + `append_outbox_event_in_tx`) so domain writes and outbox writes can share one DB transaction boundary.

## Acceptance Criteria

- [x] Outbox/domain write atomicity is explicit in code contract.
- [x] Failure injection tests prove no split-brain write scenarios.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Captured data-integrity reviewer concern as a separate architecture-quality item.

**Learnings:**
- This item is coupled to state-machine hardening but can be sequenced after P1 outbox fixes.

### 2026-05-22 - Execution

**By:** Copilot CLI

**Actions:**
- Added explicit transactional outbox API surfaces in `crates/infrastructure/src/persistence/outbox.rs`.
- Kept single-call append semantics by internally opening + committing a transaction when no transaction is passed.
- Added transactional failure-injection assertions in `scripts/run-t02-infrastructure-tests.sh` to prove rollback of both domain row and outbox writes on idempotency conflict.

**Learnings:**
- Making transaction boundaries explicit at the persistence seam prevents future callers from accidentally splitting domain and outbox durability.
