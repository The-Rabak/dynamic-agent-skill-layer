---
status: complete
priority: p1
issue_id: "002"
tags: [code-review, persistence, outbox, protects-user-story]
dependencies: []
---

# Fix outbox state machine and idempotency contract gaps

## Problem Statement

Outbox persistence paths allow illegal transitions, ambiguous idempotency behavior, and silent no-op updates that can break durable event delivery.

## Findings

- `mark_outbox_published` / `mark_outbox_failed` update by `event_id` only and do not validate rows affected (`crates/infrastructure/src/persistence/outbox.rs:149-194`).
- `append_outbox_event` conflicts only on `event_id`, while schema uniqueness is also on `idempotency_key` (`outbox.rs:60-74`, `001_initial_schema.sql:87`).
- Schema supports `failed` status but code never uses terminal failure path (`001_initial_schema.sql:90`, `outbox.rs:168-194`).
- Retry conversion uses lossy cast (`outbox.rs:174`).

## Proposed Solutions

### Option 1: Enforce explicit state machine and typed failures
**Approach:** Add transition guards (`processing -> published/failed`), rows_affected checks, retry limit, and typed idempotency conflict handling.
**Pros:** Strong correctness and observability; aligns with architecture contracts.
**Cons:** Requires coordinated updates to relay consumers/tests.
**Effort:** Medium  
**Risk:** Medium

---

### Option 2: Keep flow permissive but add warnings/logs
**Approach:** Keep SQL behavior, only add instrumentation and soft warnings.
**Pros:** Smaller code change.
**Cons:** Does not remove corruption/duplication risk.
**Effort:** Small  
**Risk:** High

## Recommended Action

Harden `PostgresGraphWriteCoordinator` so outbox writes fail loudly on duplicates/illegal state transitions, enforce `processing`-gated publish/fail transitions with explicit `failed` terminal handling after bounded retries, and codify transition/idempotency assertions in the T02 infrastructure test script.

## Technical Details

**Affected files:**
- `crates/infrastructure/src/persistence/outbox.rs`
- `scripts/run-t02-infrastructure-tests.sh`

## Acceptance Criteria

- [x] State transitions are guarded and illegal transitions fail explicitly.
- [x] Idempotency behavior is deterministic for duplicate events.
- [x] Retry scheduling is overflow-safe.
- [x] Tests cover transition/order and duplicate scenarios.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Merged overlapping P1 findings from uncle-bob, rabak-rust-reviewer, constitution-guardian, and data-integrity reviewers.

**Learnings:**
- Most critical risk is silent success on invalid event-state operations.

### 2026-05-22 - Enforced outbox state machine contracts

**By:** Copilot CLI

**Actions:**
- Hardened `PostgresGraphWriteCoordinator` with explicit transition guards (`processing -> published|pending|failed`), rows-affected enforcement, typed not-found/illegal-transition/idempotency errors, and overflow-safe retry scheduling.
- Added bounded terminal failure semantics with `MAX_OUTBOX_RETRIES = 3` so repeated failures transition to `failed` instead of infinite `pending` retries.
- Extended `scripts/run-t02-infrastructure-tests.sh` with deterministic outbox state-machine assertions for guarded publish transitions and retry-to-failed progression.
- Ran `cargo fmt`, `cargo test -p infrastructure`, and `./scripts/run-t02-infrastructure-tests.sh`.

**Learnings:**
- Explicit state constraints with hard failure modes make outbox corruption paths visible immediately instead of silently masking contract violations.
