---
unit: "T08: Maintenance merge, retire, and cron proposals"
unit_number: 1
unit_kind: expansion
serves: "SC-4 + SC-5 maintenance policy loop with filesystem-observable proposal workflows"
status: completed
attempt_count: 3
domains: [maintenance, filesystem, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/08-maintenance-merge-retire-and-cron.md
session_id: work-2026-05-25-235944
---

## What Was Implemented
Implemented a new `maintenance` feature-home crate with proposal-only merge/retire workflows and cron orchestration:
- Cross-scope duplicate merge proposal generation with cosine threshold + semantic verifier seam.
- Deterministic merged-scope policy: prefer `project` when any source is project, else `global`.
- Retirement proposal markers as non-destructive `.retired` artifacts (active file preserved).
- Active retrieval build now excludes `SKILL.md` files when sibling `SKILL.md.retired` marker exists, while keeping both files filesystem-observable.
- Configurable maintenance cron pass and pending-warning scanner support.

## Files Changed
- `Cargo.toml` -- added `crates/maintenance` workspace member.
- `Cargo.lock` -- added `maintenance` crate lock entry.
- `crates/maintenance/Cargo.toml` -- created maintenance crate manifest and test targets.
- `crates/maintenance/src/lib.rs` -- created crate exports.
- `crates/maintenance/src/merge.rs` -- implemented merge candidate/proposal workflow.
- `crates/maintenance/src/retire.rs` -- implemented retirement scoring + non-destructive proposal marker flow.
- `crates/maintenance/src/cron.rs` -- implemented interval-based maintenance orchestrator.
- `crates/maintenance/src/cleanup.rs` -- implemented stale `.pending` warning scanner.
- `crates/graph-builder/src/graph/build.rs` -- skip active skill ingestion when sibling `.retired` marker exists.
- `tests/integration/test_merge_workflow.rs` -- added merge workflow integration coverage.
- `tests/integration/test_retire_workflow.rs` -- added retirement workflow integration coverage.
- `crates/infrastructure/src/persistence/outbox_reconciler.rs` -- rustfmt-only formatting.
- `crates/infrastructure/src/vector/qdrant.rs` -- rustfmt-only formatting.
- `tests/integration/test_outbox_consistency.rs` -- rustfmt-only formatting.

## Problems Encountered
### Problem 1: Clippy blockers in new maintenance code
- **Error:** `clippy::manual-contains` and `clippy::ptr-arg` failures in `merge.rs` and `retire.rs`.
- **Root cause:** Initial implementation used non-idiomatic membership/path argument patterns under strict linting.
- **Fix:** Switched to `contains(&ScopeType::Project)` and changed helper signature from `&PathBuf` to `&Path`.

### Problem 2: Human-gate contract violation in retirement path
- **Error:** Retirement logic renamed `SKILL.md` to `SKILL.md.retired`, mutating active skill state before human review.
- **Root cause:** Proposal generation path incorrectly coupled proposal emission with immediate file-state transition.
- **Fix:** Replaced rename with non-destructive marker write, preserved active file, and enforced retrieval exclusion through sibling retired-marker detection in graph-builder.

## Patterns Discovered
- Repository uses strict rustfmt/clippy expectations even for unaffected files; full-format runs may introduce formatting-only drift in touched long-line blocks.
- Graph-builder active-skill ingestion can safely honor lifecycle markers by sibling-file checks without changing watcher observability behavior.

## TDD Evidence
- **Red**
  - Command: `git worktree add /home/rabak/projects/dynamic-agent-skill-layer-baseline-red HEAD && cd /home/rabak/projects/dynamic-agent-skill-layer-baseline-red && cargo test -p maintenance`
  - Result: FAIL
  - Evidence: Baseline state had no `maintenance` package (`package ID specification 'maintenance' did not match any packages`), proving missing behavior before implementation.
- **Green**
  - Command: `cargo test -p maintenance`
  - Result: PASS
  - Evidence: New maintenance crate tests and integration workflows executed successfully for merge/retire proposal behavior.
- **Post-Refactor Green**
  - Command: `cargo fmt --all && cargo clippy -p maintenance --no-deps -- -D warnings && cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
  - Result: PASS
  - Evidence: After review-driven fixes and cleanup, workspace tests and topology checks still passed end-to-end.

## Test Results
- Command: `cargo fmt --all && cargo clippy -p maintenance --no-deps -- -D warnings && cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 3
