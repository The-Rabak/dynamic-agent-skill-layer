---
ticket_id: T10
title: Pending lifecycle state machine
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.5"
feature_home: crates/graph-builder/
depends_on:
  - T06
  - T08
dependency_type: hard
serves:
  - SC-3: approval workflow with explicit lifecycle metadata
  - SC-5: filesystem-observable draft, reject, retire, and approval transitions
files:
  - crates/graph-builder/src/maintenance/cleanup.rs
  - crates/session-extractor/src/writer.rs
  - crates/domain/src/types.rs
  - tests/integration/test_pending_lifecycle.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Pending lifecycle state machine

## Serves

- SC-3 by formalizing how proposed skills age, warn, get approved, or get rejected.
- SC-5 by making those lifecycle states explicit on the filesystem and in shared types.

## Scope

Complete the `.pending` lifecycle contract: frontmatter timestamps and origin metadata, warning scans, `.rejected` tombstones, lifecycle enum support, and audited state-transition detection.

## Scope Fence

- Do not auto-delete `.pending` skills.
- Do not auto-approve or auto-retire anything.
- Keep lifecycle metadata aligned across session extraction and maintenance proposals.

## Acceptance Criteria

- `.pending` files carry origin, creation, warning, and expiry metadata plus proposal provenance.
- Cleanup scans warn on stale pending files without deleting them.
- `.rejected` tombstones can prevent immediate reproposal and be pruned later as tombstone cleanup only.
- Rename and retire transitions emit the expected watcher/event/audit signals.
- Shared lifecycle vocabulary covers draft, active, retired, rejected, and deleted states.

## Shared / Global Notes

- This ticket operationalizes the constitution's human-gate rule; silent cleanup would be a blocking violation.
- Lifecycle metadata belongs in the proposal files themselves so the filesystem remains the approval UI.
- Rejected tombstones are observation records, not a bypass around approval.

## Local Context

WHY link: proposal files only stay trustworthy if their lifecycle is explicit, inspectable, and recoverable after long delays.

This packet crosses a few files but one contract:

- the writer must emit the right frontmatter,
- the domain needs a stable lifecycle vocabulary,
- cleanup and watcher logic must interpret those states consistently.

If execution uncovers an inconsistency in file paths between the plan and implementation, preserve the contract and resolve the location drift without weakening the lifecycle rules.

Unknowns: none beyond final file placement details for lifecycle helpers.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.5`
- Approval semantics: `## Canonical V1.1 Contracts`, `## Drift Checks`

## Deeper-Dive Refs

- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#context-tiers`

## Coupling Notes

- Writer metadata, lifecycle vocabulary, and cleanup behavior stay together because they define one proposal-state contract.
- Splitting them would make approval behavior drift between extraction and maintenance flows.
