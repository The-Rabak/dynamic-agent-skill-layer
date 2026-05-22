---
status: pending
priority: p2
issue_id: "006"
tags: [code-review, performance, async, infrastructure]
dependencies: []
---

# Improve async adapter performance and timeout discipline

## Problem Statement

Several adapter paths are functionally correct but likely to degrade under scale due to sequential execution, blocking calls, and incomplete timeout envelopes.

## Findings

- Embedding batch is sequential (`crates/infrastructure/src/embeddings/ollama.rs:155-160`).
- Timeout covers request send but not full decode path in extraction/embedding flows.
- Scope resolver uses blocking process call inside async method (`crates/infrastructure/src/scope.rs:22-27`).
- Outbox claim path index may not match ordering/filter pattern (`001_initial_schema.sql:138-141` vs query in `outbox.rs:103-106`).

## Proposed Solutions

### Option 1: Throughput-oriented async refactor
**Approach:** Bounded parallel batch embedding, end-to-end request timeout coverage, async process execution, and index tuning.
**Pros:** Better p95/p99 behavior and resilience.
**Cons:** Requires careful regression tests.
**Effort:** Medium  
**Risk:** Medium

---

### Option 2: Minimal risk targeted patch
**Approach:** Fix blocking scope resolver and timeout envelope first; defer batching/index changes.
**Pros:** Lower change surface.
**Cons:** Leaves known throughput debt.
**Effort:** Small  
**Risk:** Low

## Recommended Action

To be filled during triage.

## Acceptance Criteria

- [ ] Blocking calls are removed from async hot paths.
- [ ] Timeout contracts cover full request+decode operations.
- [ ] Performance-sensitive query/index alignment is validated.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Captured performance-oracle P1/P2 findings into one remediation item.

**Learnings:**
- This work is mostly quality/resilience hardening, not scope expansion.

