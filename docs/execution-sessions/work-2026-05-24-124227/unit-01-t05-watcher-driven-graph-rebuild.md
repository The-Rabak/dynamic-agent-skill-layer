---
unit: "T05 watcher-driven graph rebuild"
unit_number: 1
unit_kind: expansion
serves: "SC-4 and SC-5 by converting filesystem changes into durable graph rebuild outcomes"
status: completed
attempt_count: 2
domains: [backend, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/05-watcher-driven-graph-rebuild.md
session_id: work-2026-05-24-124227
---

## What Was Implemented

Added a new `graph-builder` crate and wired it into the workspace. Implemented watcher snapshot diffing with `.pending` -> `.md` approval detection, idempotent reconciliation recovery, deterministic extraction + dedup + fallback flow, deterministic embeddings/community assignment, and rebuild orchestration with durable ordering before `graph.rebuilt` publication.

## Files Changed

- `Cargo.toml` -- added `crates/graph-builder` workspace member
- `Cargo.lock` -- refreshed lockfile for new dependencies
- `crates/graph-builder/Cargo.toml` -- new crate config
- `crates/graph-builder/src/lib.rs` -- module exports
- `crates/graph-builder/src/main.rs` -- one-cycle watcher/rebuild runner
- `crates/graph-builder/src/watcher.rs` -- watcher + diff + approval rename detection
- `crates/graph-builder/src/watcher_recovery.rs` -- idempotent reconciliation
- `crates/graph-builder/src/extraction/mod.rs` -- extraction orchestrator
- `crates/graph-builder/src/extraction/rules.rs` -- structural extraction rules
- `crates/graph-builder/src/extraction/ollama_fallback.rs` -- fallback extractor
- `crates/graph-builder/src/extraction/dedup.rs` -- deduplication
- `crates/graph-builder/src/graph/mod.rs` -- graph module wiring
- `crates/graph-builder/src/graph/build.rs` -- scope graph build pipeline
- `crates/graph-builder/src/graph/embeddings.rs` -- deterministic embedding generator
- `crates/graph-builder/src/graph/communities.rs` -- deterministic community assignment
- `crates/graph-builder/src/graph/rebuild.rs` -- durable mutation + invalidation ordering
- `tests/integration/test_watcher_rebuild.rs` -- integration contract test
- `tests/fixtures/test-skills/project/rust-file-io.md` -- project fixture
- `tests/fixtures/test-skills/global/async-tokio.md` -- global fixture

## Problems Encountered

### Problem 1: ScopeType hash keying
- **Error:** `method entry exists for HashMap<(ScopeType, String), ...> but trait bounds were not satisfied`
- **Root cause:** `ScopeType` is not hashable
- **Fix:** switched grouping key to string-based scope key and mapped back to `ScopeType`

### Problem 2: notify-debouncer-full API mismatch
- **Error:** watcher/debouncer method/type mismatch (`watch` not found on temporary type)
- **Root cause:** outdated API usage pattern
- **Fix:** migrated to `new_debouncer_opt(..., FileIdMap::new(), Config::default())` and direct `debouncer.watch(...)`

## Patterns Discovered

- Integration tests are crate-wired via `[[test]]` entries that point at top-level `tests/integration/...`.
- Event publishing should reuse `infrastructure::EventEnvelope` canonical construction.

## TDD Evidence

- **Red**
  - Command: `cargo test -p graph-builder watcher_detects_pending_approval_and_rebuild_respects_invalidation_order -- --nocapture`
  - Result: FAIL
  - Evidence: test failed with assertion error: expected watcher to detect `.pending` to `.md` approval rename and trigger graph rebuild, but no `graph.rebuilt` event was published. This proves the missing behavior before implementation.
- **Green**
  - Command: `cargo test -p graph-builder watcher_detects_pending_approval_and_rebuild_respects_invalidation_order -- --nocapture`
  - Result: PASS
  - Evidence: integration test validated approval rename detection, reconciliation idempotency, durable ordering, and `graph.rebuilt`
- **Post-Refactor Green**
  - Command: `cargo test -p graph-builder watcher_detects_pending_approval_and_rebuild_respects_invalidation_order -- --nocapture`
  - Result: PASS
  - Evidence: rerun after cleanup preserved behavior

## Test Results

- Command: `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 2
