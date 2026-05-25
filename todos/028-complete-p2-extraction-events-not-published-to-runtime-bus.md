---
status: complete
priority: p2
issue_id: "028"
tags: [code-review, events, session-extractor, architecture]
dependencies: []
---

# Publish extraction lifecycle events beyond in-memory list

## Problem Statement

Extraction lifecycle events are currently stored only in an in-memory vector inside `SessionExtractor`, limiting observability/integration with the broader event-driven architecture.

## Findings

- `crates/session-extractor/src/lib.rs:40-60` defines in-memory event store.
- `crates/session-extractor/src/lib.rs:158-183` and `211-221` push lifecycle events only to local memory.
- No adapter is wired here to publish these lifecycle events to shared runtime messaging/outbox surfaces.

## Proposed Solutions

### Option 1: Introduce event publisher seam and wire infrastructure adapter

**Approach:** Add a trait-based event publisher dependency and publish canonical envelopes to the shared bus (or outbox-backed adapter).

**Pros:**
- Aligns with architecture event contracts.
- Improves operational observability.

**Cons:**
- Requires wiring and tests for adapter failures.

**Effort:** 3-5 hours

**Risk:** Medium

---

### Option 2: Keep in-memory for now but explicitly scope as test-only and document deferred runtime publishing

**Approach:** Restrict in-memory collector to tests and annotate production gap in ticket/session docs.

**Pros:**
- Honest interim state with low change cost.

**Cons:**
- Leaves runtime event integration incomplete.

**Effort:** 1-2 hours

**Risk:** Medium

## Recommended Action

Implemented Option 1 with bounded scope:
- Added explicit `ExtractionEventPublisher` seam to `SessionExtractor`.
- Wired runtime publication through `RedisExtractionEventPublisher` in `from_environment`.
- Preserved `lifecycle_events()` as in-memory secondary sink for tests and diagnostics.

## Technical Details

**Affected files:**
- `crates/session-extractor/src/lib.rs`
- `crates/infrastructure` (publisher adapter wiring, if chosen)
- integration tests for extraction lifecycle publication

**Database changes (if any):**
- Possibly (if outbox path selected)

## Resources

- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- `crates/session-extractor/src/lib.rs`

## Acceptance Criteria

- [x] Lifecycle events are observable through shared runtime channel/outbox path
- [x] Tests assert requested/completed/failed event publication
- [x] In-memory collector is test-only or clearly secondary

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Reviewed lifecycle event path and event envelope creation points.

**Learnings:**
- Event creation exists, but integration boundary is still local-only.

### 2026-05-25 - Implemented runtime lifecycle publication seam

**By:** Copilot CLI

**Actions:**
- Added `ExtractionEventPublisher` seam and `RedisExtractionEventPublisher` adapter in `crates/session-extractor/src/lib.rs`.
- Routed requested/completed/failed lifecycle events through a shared publication path while keeping local in-memory capture as secondary.
- Updated `tests/integration/test_extract_session.rs` to assert requested/completed/failed publication through the publisher seam.
- Ran focused validation with `cargo test --test test_extract_session`.

**Learnings:**
- Keeping the in-memory lifecycle list as a secondary sink preserves deterministic tests while making runtime publication explicit and replaceable.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
