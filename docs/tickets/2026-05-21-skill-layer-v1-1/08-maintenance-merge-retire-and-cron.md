---
ticket_id: T08
title: Maintenance merge, retire, and cron proposals
kind: expansion
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.3"
feature_home: crates/maintenance/
depends_on:
  - T05
  - T07
dependency_type: hard
serves:
  - SC-4: offline maintenance for duplicate merge and stale-skill retirement
  - SC-5: filesystem-observable proposals for merge and retirement
files:
  - crates/maintenance/Cargo.toml
  - crates/maintenance/src/lib.rs
  - crates/maintenance/src/merge.rs
  - crates/maintenance/src/retire.rs
  - crates/maintenance/src/cron.rs
  - crates/maintenance/src/cleanup.rs
  - tests/integration/test_merge_workflow.rs
  - tests/integration/test_retire_workflow.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Maintenance merge, retire, and cron proposals

## Serves

- SC-4 by scheduling and running duplicate-merge and stale-retirement proposal workflows.
- SC-5 by keeping those maintenance decisions visible as `.pending` and `.retired` filesystem artifacts.

## Scope

Implement the `maintenance` crate's policy workflows for merge proposal generation, retirement proposal generation, and scheduled full maintenance passes.

## Scope Fence

- Do not auto-approve merges or retirements.
- Do not put online admin tools or graph construction in this ticket.
- Do not introduce a many-to-many `skill_scopes` model; preserve scalar `scope` plus `merged_from_scopes`.

## Acceptance Criteria

- Similarity plus LLM semantic checks can propose merge candidates and emit merged `.pending` files.
- Retirement scoring uses usage history and proposes `.retired` markers without auto-applying deletion.
- The scheduled maintenance pass runs the merge and retire proposal workflows on a configurable interval.
- Approved merged skills preserve canonical scope plus provenance, and all proposal flows write audit records.
- Retired skills are excluded from retrieval while remaining observable in the graph state.

## Shared / Global Notes

- Human gate rules from the constitution are blocking here: `.pending` and `.retired` remain proposals.
- The architecture artifact freezes maintenance as a separate feature home from graph construction and admin tools.
- Use the existing graph data and outbox-hardened vector state rather than inventing a second analysis pipeline.

## Local Context

WHY link: without a cleanup loop, the skill library only grows noisier and retrieval quality decays over time.

Keep the work bounded to maintenance policy:

- Merge proposals compare project and global skills, then produce a new `.pending` artifact rather than mutating active skills directly.
- Retirement is a scored proposal flow, not a silent cleanup job.
- Cron is part of the same ticket because the maintenance feature is only complete when policy can run on a schedule rather than only on demand.

Open question to surface during execution if needed: choose the exact canonical-scope policy for approved merges, but do not reopen the stored `scope` plus `merged_from_scopes` contract.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.3`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Shared / Global Decisions`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#deletion-test`
- `.github/skills/workflows-to-issues/references/vertical-slice-architecture.md`

## Coupling Notes

- Merge, retire, and cron stay together because they are one maintenance-policy feature home with one reason to change.
- Admin tools were intentionally split out so this ticket can stay focused on offline policy instead of online inspection surfaces.

## Work Log Addendum

### 2026-05-26 - Reopened for dependency-order truth

**By:** Copilot CLI

**Actions:**
- Reconciled ticket status with hard-dependency ordering by explicitly keeping this ticket at `ready` while T07 remains incomplete.
- Kept the dependency contract explicit (`depends_on: T05, T07`) with no waiver because out-of-order completion was not approved.

**Learnings:**
- Hard dependency status must remain truthful across ticket metadata to preserve execution governance confidence.
