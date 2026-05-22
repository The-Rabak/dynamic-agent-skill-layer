---
status: complete
priority: p1
issue_id: "005"
tags: [code-review, redis, streaming, performance]
dependencies: ["002"]
---

# Harden Redis stream runtime behavior

## Problem Statement

Redis stream adapter has runtime hazards that can cause hidden delivery lag or unnecessary load.

## Findings

- Connection is opened per operation (`crates/infrastructure/src/streaming/redis.rs:102-104,125,147,175,186,195`).
- `XREADGROUP` only reads `>` and lacks pending reclaim path (`redis.rs:153-155`).
- `ack` ignores acknowledged count (`redis.rs:174-182`).
- BUSYGROUP detection is string-match brittle (`redis.rs:117-121`).

## Proposed Solutions

### Option 1: Add robust consumer lifecycle
**Approach:** Reuse multiplexed connection(s), add pending reclaim flow, verify ack count, and harden error matching.
**Pros:** Better throughput and correctness under failure/restart.
**Cons:** More adapter complexity.
**Effort:** Medium  
**Risk:** Medium

---

### Option 2: Minimal correctness-first patch
**Approach:** Keep connection strategy for now; fix ack validation and pending reclaim only.
**Pros:** Faster delivery.
**Cons:** Leaves performance debt.
**Effort:** Small  
**Risk:** Medium

## Recommended Action

Apply a correctness-first hardening pass: keep the existing connection lifecycle for now, add pending reclaim reads (`XREADGROUP ... 0`) before fresh reads, enforce single-message ack confirmation, and replace brittle BUSYGROUP string matching with deterministic Redis error-code handling.

## Acceptance Criteria

- [x] Pending messages are reclaimed after consumer interruption.
- [x] Ack path confirms expected acknowledgment count.
- [x] Adapter behavior is deterministic for consumer-group initialization errors.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Consolidated performance, rust, and simplicity reviewer stream findings.

**Learnings:**
- Correctness and scalability concerns overlap in the same adapter paths.

### 2026-05-22 - Applied Redis stream runtime hardening

**By:** Copilot CLI

**Actions:**
- Updated `RedisStreamsAdapter` to reclaim pending messages before fresh reads, enforce exact `XACK` count semantics, and expose deterministic consumer-group initialization failures via Redis error code handling.
- Added unit coverage for ack-count validation and expanded T02 Redis hard assertions to verify BUSYGROUP determinism and pending-message reclaim behavior.
- Ran `cargo fmt`, `cargo test -p infrastructure`, and `./scripts/run-t02-infrastructure-tests.sh`.

**Learnings:**
- Correctness-focused stream hardening can be delivered without full connection-lifecycle refactoring when reclaim and acknowledgment contracts are explicit.
