---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/10-pending-lifecycle-state-machine.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.5"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-26T19:59:38+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-26-195938
completed: 2026-05-26T20:14:20+03:00
---

## WHY Context

### Problem Narrative
Proposal files become untrustworthy over time unless lifecycle state is explicit, inspectable, and consistent across extraction, maintenance, and watcher interpretation.

### User Story
As a developer using approval-based skill proposals, I need `.pending` lifecycle metadata, warning behavior, rejection/tombstone handling, and explicit state transitions so the filesystem remains the reliable approval UI.

### Architectural Context
T10 hardens the proposal lifecycle contract spanning writer output (`session-extractor`), lifecycle vocabulary (`domain`), cleanup scanning (`maintenance`), and watcher transition semantics (`graph-builder`) while preserving human-gated mutations.

### Success Criteria
- SC-3: approval workflow with explicit lifecycle metadata
- SC-5: filesystem-observable draft/reject/retire/approval transitions

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`) + E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Constitution version: 1.0.0
- Relevant principles: local-first execution, human-gated mutations, filesystem-observable state
- Required approvals surfaced: no auto-delete/auto-approve/auto-retire; lifecycle transitions stay explicit and auditable
- Waivers: None

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature homes: lifecycle behavior spans `session-extractor`, `maintenance`, `graph-builder`, and shared lifecycle vocabulary in `domain`
- Shared/global decisions: `.pending` TTL warnings only; no silent deletion; `.rejected` tombstones are observation records
- Context tiers: ticket-local packet primary; architecture/constitution constrain mutation boundaries
- Deletion test: do not move approval policy into online handlers or weaken filesystem-as-UI guarantees
- Interfaces as test surfaces: pending frontmatter fields, cleanup warning behavior, watcher transition detection, lifecycle enums
- Seams/adapters/contracts: writer metadata contract, maintenance scanner contract, watcher event contract
- Review guidance: verify transitions remain explicit and safe under path/location drift

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T10: Pending lifecycle state machine | hardening | SC-3 + SC-5 lifecycle metadata and transition consistency | completed | 1 | docs/execution-sessions/work-2026-05-26-195938/unit-01-pending-lifecycle-state-machine.md |

## Learnings Brief
- [dependency] T08 implementation exists in repo history (`fc56c85`) and current codebase surfaces maintenance proposal flows needed by T10.
- [boundary] `.pending` and `.retired` are proposal/state artifacts under human gate; lifecycle hardening must not add silent mutation paths.
- [lifecycle] Proposal metadata now uses RFC3339 `created_at`/`warning_at`/`expires_at` across extraction and maintenance proposal writers.
- [safety] Reproposal is fail-closed when matching `.rejected` tombstones exist; pruning is tombstone-only and threshold-gated.
- [audit] Watcher diffing now distinguishes `ApprovedRename`, `RejectedRename`, and `RetiredRename` for explicit lifecycle transition observability.
