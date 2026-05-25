---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/06-session-end-extraction-and-approval.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 2.2"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-24T12:52:14+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-24-125214
completed: 2026-05-24T13:00:00+03:00
---

## WHY Context

### Problem Narrative
The loop is still incomplete if session knowledge does not become reviewable skill drafts at session end. Session extraction must preserve trust boundaries and human approval.

### User Story
As a solo developer, I need session-end extraction that produces `.pending` draft skills for review, so new knowledge enters the graph without bypassing human control.

### Architectural Context
T06 is owned by `session-extractor` plus a thin `mcp-server` tool route. It must validate `transcript_ref` under `CLAUDE_TRANSCRIPT_ROOT`, run asynchronously, route provider implementations behind the extraction seam, emit canonical extraction lifecycle events, and write `.pending` drafts that T05 watcher flow can consume after approval rename.

### Success Criteria
- SC-3: session-end extraction with human approval

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (`cargo test --workspace`) and E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- Exceptions: None

### Constitution Context
- Version: 1.0.0 (`docs/constitution.md`)
- Principles: local-first, zero-touch lifecycle, human-gated filesystem mutation, portable scope, filesystem-observable state
- Required approvals surfaced: skill mutations and tag proposals must remain `.pending` + human rename approval
- Waivers: none

### Architecture Handoff
- Artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Feature home: `crates/session-extractor/` with thin hook in `crates/mcp-server/`
- Shared/global decisions: transcript ingress contract uses `transcript_ref` under mounted transcript root; event catalog includes `skill.extraction_requested`, `extraction.completed`, `extraction.failed`
- Context tiers: ticket-local packet primary, plan/architecture for deeper constraints
- Deepening candidates: trust boundary path validation, async return semantics, writer compatibility with watcher approvals
- Deletion test: do not add auto-approval or runtime in-session transcript analysis
- Interfaces as test surfaces: extract_session transport contract, transcript parser, provider output schema, pending draft writer
- Seams/adapters/contracts: extraction provider seam, transcript ingress seam, filesystem writer seam, extraction lifecycle event seam
- Review guidance: verify trust boundary, async semantics, and canonical event/status behavior

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T06 session-end extraction and approval | expansion | SC-3 transcript->pending draft workflow | completed | 2 | `unit-01-t06-session-end-extraction-and-approval.md` |

## Learnings Brief
- [graph-builder] T05 provides watcher/rebuild activation path for `.pending` -> `.md` approvals.
- [contracts] Preserve canonical extraction and rebuild event semantics; avoid introducing ad hoc approvals.
- [testing] Integration env mutation helpers require lock-scoped setup and restore to prevent cross-test poisoning.
- [mcp] Keep extract_session transport-thin by delegating async lifecycle behavior to session-extractor crate.
