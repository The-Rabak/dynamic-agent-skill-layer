---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/08-maintenance-merge-retire-and-cron.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.3"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-25T23:59:44+03:00
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-25-235944
completed: 2026-05-26T00:18:14+03:00
status: completed
---

## WHY Context

### Problem Narrative
Without an offline maintenance loop, duplicate and stale skills accumulate, retrieval quality decays, and the skill graph drifts away from useful context over time.

### User Story
As a solo developer using multiple coding-agent harnesses, I need periodic merge/retire proposal workflows that keep the skill graph clean without removing human approval.

### Architectural Context
This unit implements the maintenance policy feature home (`crates/maintenance/`) that consumes graph state and emits filesystem-observable proposals (`.pending`, `.retired`) while preserving the scalar `scope` + `merged_from_scopes` contract.

### Success Criteria
- SC-4: offline maintenance for duplicate merge and stale-skill retirement
- SC-5: filesystem-observable proposals for merge and retirement

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`) + E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Constitution version: 1.0.0
- Relevant principles: local-first execution, human gate for mutations, portable scope, filesystem-observable state
- Required approvals surfaced: skill mutation workflows remain proposal-only (`.pending`/`.retired`) with explicit human confirmation
- Waivers: None

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature home: `crates/maintenance/` (policy workflows)
- Shared/global decisions: keep scalar `scope` plus `merged_from_scopes`; preserve event/state contracts and auditability
- Context tiers: apply global guardrails, ticket-local files, and scope fence only
- Deletion test: keep maintenance separate from graph construction and admin tooling
- Interfaces as test surfaces: merge proposal generation, retirement proposal generation, cron scheduling entrypoint, audit record emission
- Seams/adapters/contracts: usage history inputs, graph state inputs, filesystem proposal outputs, durable outbox-aware consistency assumptions from T07
- Review guidance: proposals only (no auto-approve), deterministic canonical scope policy for merged skills, retired items excluded from retrieval but observable

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T08: Maintenance merge, retire, and cron proposals | expansion | SC-4 + SC-5 maintenance policy loop and observable proposals | completed | 3 | docs/execution-sessions/work-2026-05-25-235944/unit-01-maintenance-merge-retire-and-cron-proposals.md |

## Learnings Brief
- [dependency] T07 already established durable outbox/reconciliation guarantees for PG-to-Qdrant consistency; do not bypass those assumptions.
- [policy] Canonical merged skill scope policy: prefer `project` when any source skill is project, otherwise `global`.
- [retirement] Keep retirement proposal generation non-destructive (`SKILL.md` remains), with marker-based lifecycle state and explicit human reversibility.
- [retrieval] Active graph build should skip `SKILL.md` when a sibling `SKILL.md.retired` marker exists.
