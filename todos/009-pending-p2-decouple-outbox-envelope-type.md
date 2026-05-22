---
status: pending
priority: p2
issue_id: "009"
tags: [code-review, architecture, persistence, quality]
dependencies: ["002"]
---

# Decouple outbox persistence contract from Redis transport type

## Problem Statement

Persistence coordinator APIs currently depend on a Redis transport envelope type, creating unnecessary cross-module coupling.

## Findings

- `crates/infrastructure/src/persistence/outbox.rs` imports and exposes `crate::streaming::redis::EventEnvelope`.
- Architecture review flagged this as seam leakage and future-evolution friction.

## Proposed Solutions

### Option 1: Introduce transport-neutral outbox event model
**Approach:** Define outbox event contract in persistence/shared module and map to Redis envelope at stream boundary.
**Pros:** Cleaner seam, easier future transport evolution.
**Cons:** Requires adapter translation code.
**Effort:** Medium  
**Risk:** Low

---

### Option 2: Keep coupled type and document intentional tradeoff
**Approach:** Accept current coupling and record rationale/limits.
**Pros:** No refactor now.
**Cons:** Higher long-term change cost.
**Effort:** Small  
**Risk:** Medium

## Recommended Action

To be filled during triage.

## Acceptance Criteria

- [ ] Persistence trait signatures no longer depend on streaming-specific types.
- [ ] Event serialization boundary is isolated to streaming adapter.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Extracted architecture-strategist P2 seam finding into focused todo.

**Learnings:**
- This is a design-quality improvement, not an immediate correctness blocker.

