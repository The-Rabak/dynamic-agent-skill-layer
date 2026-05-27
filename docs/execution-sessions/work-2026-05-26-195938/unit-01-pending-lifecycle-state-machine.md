---
unit: "T10: Pending lifecycle state machine"
unit_number: 1
unit_kind: hardening
serves: "SC-3 + SC-5 lifecycle metadata and transition consistency"
status: completed
attempt_count: 1
domains: [backend, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/10-pending-lifecycle-state-machine.md
session_id: work-2026-05-26-195938
---

## What Was Implemented

- Hardened the `.pending` lifecycle contract end-to-end across extraction writer, maintenance cleanup/merge flows, watcher transition classification, and shared domain lifecycle vocabulary.
- Added lifecycle metadata emission in proposals: `created_at`, `warning_at` (+30d), `expires_at` (+90d), with provenance fields retained.
- Added fail-closed reproposal blocking when a matching `.rejected` tombstone exists.
- Added cleanup behavior for stale warning scans (non-destructive for `.pending`), reproposal-block detection, and tombstone-only pruning.
- Extended watcher semantics to explicitly classify `RejectedRename` and `RetiredRename` alongside `ApprovedRename`.
- Added integration coverage in `tests/integration/test_pending_lifecycle.rs`.

## Files Changed

- `crates/session-extractor/src/writer.rs` -- lifecycle frontmatter timestamps and tombstone block behavior
- `crates/session-extractor/src/lib.rs` -- new writer reason-code mapping
- `crates/session-extractor/Cargo.toml` -- `chrono` dependency for lifecycle timestamps
- `crates/maintenance/src/cleanup.rs` -- frontmatter-aware warning scan + tombstone block/prune
- `crates/maintenance/src/merge.rs` -- merge proposal lifecycle timestamps
- `crates/maintenance/Cargo.toml` -- pending lifecycle integration test target and dev dependency
- `crates/graph-builder/src/watcher.rs` -- explicit rejected/retired rename classification
- `crates/domain/src/types.rs` -- lifecycle vocabulary updated to draft/active/retired/rejected/deleted
- `tests/integration/test_pending_lifecycle.rs` -- lifecycle contract integration tests
- `Cargo.lock` -- dependency graph update

## Problems Encountered

### Problem 1: rustfmt edition mismatch
- **Error:** `async fn is not permitted in Rust 2015`
- **Root cause:** direct `rustfmt` invocation used an older edition default.
- **Fix:** reran formatting using `rustfmt --edition 2024`.

### Problem 2: incorrect compose attach target in proof command
- **Error:** `cannot attach to services not included in up: mcp-server`
- **Root cause:** test compose topology does not include `mcp-server`.
- **Fix:** switched evidence attachment to `topology-check`.

## Patterns Discovered

- Lifecycle metadata should stay frontmatter-driven and consistent across extraction and maintenance proposal generators.
- Watcher auditing is clearer when rename transitions are modeled as explicit kinds rather than inferred from generic create/delete pairs.

## TDD Evidence

- **Red**
  - Command: `cp crates/maintenance/Cargo.toml /home/rabak/projects/dasl-red/crates/maintenance/Cargo.toml && cp tests/integration/test_pending_lifecycle_frontmatter_contract.rs /home/rabak/projects/dasl-red/tests/integration/test_pending_lifecycle_frontmatter_contract.rs && cd /home/rabak/projects/dasl-red && cargo test -q -p maintenance --test test_pending_lifecycle_frontmatter_contract`
  - Result: FAIL
  - Evidence: the contract test failed on baseline because pending proposals lacked `created_at`/`warning_at`/`expires_at` lifecycle timestamps, proving missing behavior rather than target/setup noise.
- **Green**
  - Command: `cargo test -q -p maintenance --test test_pending_lifecycle_frontmatter_contract`
  - Result: PASS
  - Evidence: the same frontmatter contract test passed once lifecycle timestamps were emitted in pending proposals.
- **Post-Refactor Green**
  - Command: `cargo test -q -p maintenance --test test_pending_lifecycle_frontmatter_contract`
  - Result: PASS
  - Evidence: rerun stayed green with no additional cleanup, proving behavior stability.

## Test Results

- Command: `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 1
