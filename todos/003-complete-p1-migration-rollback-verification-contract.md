---
status: complete
priority: p1
issue_id: "003"
tags: [code-review, migration, deployment, constitution-violation]
dependencies: []
---

# Define production-safe migration rollback and verification contract

## Problem Statement

Schema changes are implemented, but deployment-safe rollback and production verification evidence are not explicitly codified.

## Findings

- No explicit backup/restore evidence and no down-migration path surfaced by deployment verification review.
- Migration strategy currently runs embedded SQL directly (`crates/infrastructure/src/persistence/postgres.rs:85-90`).
- Reviewers flagged missing formal post-deploy checks and stop conditions in ticket/session artifacts.

## Proposed Solutions

### Option 1: Introduce explicit migration runbook + verification SQL + rollback contract
**Approach:** Document pre/post checks, stop conditions, restore path, and required operators; wire to ticket/execution docs.
**Pros:** Immediate safety/traceability improvements; merge-confidence increase.
**Cons:** Documentation and ops coordination overhead.
**Effort:** Medium  
**Risk:** Low

---

### Option 2: Adopt versioned migration tooling with immutable revisions
**Approach:** Replace raw SQL execution path with migration ledger-based runner and immutable files.
**Pros:** Strong long-term safety and drift control.
**Cons:** Larger implementation and migration of current workflow.
**Effort:** Large  
**Risk:** Medium

## Recommended Action

Add an explicit migration safety runbook with pre/post verification SQL and approval-evidence fields, wire those executable SQL checks into both T02 and T07 ticket artifacts, and enforce rollback/restore verification in the T02 infrastructure test harness.

## Acceptance Criteria

- [x] T02/T07 artifacts include executable pre/post verification SQL.
- [x] Rollback/restore path is explicit and tested.
- [x] Schema-mutation approval evidence is captured per constitution requirements.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Consolidated deployment-verification-agent and migration reviewer blockers.

**Learnings:**
- Biggest gap is not schema syntax; it is production recoverability evidence.

### 2026-05-22 - Added migration safety and rollback contract

**By:** Copilot CLI

**Actions:**
- Added `docs/runbooks/schema-migration-verification-and-rollback.md` with constitution-gated approval evidence requirements, executable pre/post verification SQL, explicit stop conditions, and rollback/restore procedure.
- Updated T02 and T07 ticket artifacts to include executable migration verification SQL and direct references to the runbook contract.
- Extended `scripts/run-t02-infrastructure-tests.sh` with an automated rollback probe that snapshots a pre-migration database, applies migration, restores from snapshot, and verifies migrated artifacts are removed after restore.
- Ran `./scripts/run-t02-infrastructure-tests.sh` with the new rollback verification stage.

**Learnings:**
- Treating rollback as an executable contract (not just prose) closes the production-recoverability gap reviewers flagged.
