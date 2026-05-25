---
unit: "T07: Outbox relay and reconciliation"
unit_number: 1
unit_kind: hardening
serves: "SC-4 durable merge/rebuild data integrity and SC-7 graceful degrade with replayable recovery"
status: completed
attempt_count: 3
domains: [infrastructure, graph-builder, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/07-outbox-relay-and-reconciliation.md
session_id: work-2026-05-25-222137
---

## What Was Implemented

Implemented an outbox relay path that claims pending events, parses vector upsert payloads, derives deterministic point IDs from content hash, writes to vector storage, and transitions outbox state to published/failed with explicit retry metadata. Added reconciliation that scans published vector-upsert events, re-enqueues missing vectors as repair work, and deletes orphaned vector points. Added integration tests that cover transient failure replay, reconciliation behavior, and guard the ordering contract so `graph.rebuilt` is not emitted when outbox drain fails.

## Files Changed

- `crates/infrastructure/src/persistence/outbox.rs` -- expanded with relay types/traits, payload parsing, hash->point-id function, and relay execution flow.
- `crates/infrastructure/src/persistence/outbox_reconciler.rs` -- created reconciler for missing/orphaned vector detection and repair enqueueing.
- `crates/infrastructure/src/vector/qdrant.rs` -- added vector-store operations used by relay/reconciliation.
- `crates/infrastructure/src/lib.rs` -- exported new relay/reconciliation APIs.
- `crates/infrastructure/Cargo.toml` -- added `blake3` dependency for deterministic point IDs.
- `crates/graph-builder/Cargo.toml` -- registered new integration test target and required dev-dependencies.
- `tests/integration/test_outbox_consistency.rs` -- created integration suite for relay/reconciliation/ordering contract.
- `Cargo.lock` -- lockfile update from dependency changes.

## Problems Encountered

### Problem 1: misplaced trait impl nesting
- **Error:** `implementation is not supported in traits or impls`
- **Root cause:** `OutboxInspection` implementation was accidentally nested inside another impl block.
- **Fix:** Moved the impl to module scope.

### Problem 2: integration test compile wiring
- **Error:** unresolved import/type/dependency mismatches in the new test target.
- **Root cause:** incorrect import path, missing `sqlx` in graph-builder test target deps, and assertion type mismatch.
- **Fix:** corrected imports, added required dev dependency, and fixed assertion typing.

### Problem 3: strict clippy baseline is currently red outside this ticket scope
- **Error:** `cargo clippy --workspace --all-targets -- -D warnings` fails on existing warnings in files outside T07 scope.
- **Root cause:** pre-existing `clippy::field_reassign_with_default` and `clippy::collapsible_if` findings in untouched modules.
- **Fix:** kept T07 scope focused and did not patch unrelated modules; functional T07 validation remains green via workspace + compose validation flow.

## Patterns Discovered

- The outbox state machine (`pending|processing|published|failed`) is the core audit/replay contract and should remain the source of truth for replay semantics.
- Rebuild ordering is intentionally strict: durable mutation -> outbox drain -> graph version bump -> `graph.rebuilt`.

## TDD Evidence

### Red
- **Command:** `cargo test -p graph-builder --test test_outbox_consistency graph_rebuilt_is_not_emitted_when_outbox_drain_fails -- --exact --nocapture` (attempt 1, before implementation)
- **Result:** FAIL
- **Evidence:** behavior assertion failed at `tests/integration/test_outbox_consistency.rs` with the message `graph.rebuilt must stay hidden when outbox drain fails`, proving `graph.rebuilt` visibility was incorrect before the outbox-drain guardrail was implemented.

### Green
- **Command:** `cargo test -p graph-builder --test test_outbox_consistency graph_rebuilt_is_not_emitted_when_outbox_drain_fails -- --exact --nocapture`
- **Result:** PASS
- **Evidence:** targeted T07 behavior now passes, proving rebuild visibility remains blocked until outbox drain succeeds.

### Post-Refactor Green
- **Command:** `cargo test -p graph-builder --test test_outbox_consistency graph_rebuilt_is_not_emitted_when_outbox_drain_fails -- --exact --nocapture`
- **Result:** PASS
- **Evidence:** reran the same behavior test after evidence cleanup-only edits; behavior remained green.

## Test Results

- Command: `cargo test -p graph-builder --test test_outbox_consistency`
- Result: PASS
- Attempts: 1
