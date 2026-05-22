---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/01-compose-and-domain-foundation.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_packet_ref: "## Execution Slices > Slice 1.1a"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-22T00:13:19+03:00
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-05-22-001319
completed: 2026-05-22T00:27:00+03:00
---

## WHY Context

### Problem Narrative
A developer using multiple coding agent harnesses loses time every session to manual skill selection and scope management. The system needs a local-first foundation that supports zero-touch behavior and future growth without architecture drift.

### User Story
As a solo developer, I need a zero-touch, self-growing skill context layer with shared domain vocabulary and local runtime infrastructure, so downstream services can compose safely and evolve toward V2 team scope without rework.

### Architectural Context
This unit establishes the first tracer-bullet foundation: local Docker Compose topology and the pure `domain` crate as the innermost layer with no infrastructure dependencies. All later crates and services depend on this boundary.

### Success Criteria
- SC-7: Graceful degrade foundation through local container topology.
- SC-8: V2 readiness through pure domain boundaries and shared traits.

### TDD Contract
- Effective mode: Ralph-driven TDD.
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun.
- Required evidence: Unit (`cargo test --workspace`) and E2E (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`).
- Exceptions: None.

### Constitution Context
- Version: 1.0.0 (`docs/constitution.md`).
- Applicable principles: local-first execution, zero-touch session start, filesystem-observable state, portable scope, human gate for mutations.
- Approvals surfaced: infrastructure configuration changes and other mutation-sensitive areas require explicit operator intent; this ticket is explicitly requested by the user.
- Waivers: none.

### Architecture Handoff
- Artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Feature homes: `crates/domain/` for pure contracts; compose files at repo root for local runtime shell.
- Shared/global decisions: `domain` owns types/traits/config/errors only; concrete adapters stay out of this unit.
- Context tiers: global constitution + domain boundary; ticket-local files and scope fence.
- Deepening candidates to preserve: enforce domain dependency purity via `cargo tree -p domain --depth 1`.
- Deletion test: keep `domain` separate from adapter code; do not collapse boundaries.
- Interfaces as test surfaces: `EmbeddingService`, `TranscriptSkillExtractionService`, `ScopeResolver`, `ContextCompiler`.
- Seams/adapters/contracts: only trait contracts in this unit; no concrete clients.
- Review guidance: confirm no `sqlx`, `qdrant-client`, `redis`, or `reqwest` in `domain`.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Compose and domain foundation (T01) | tracer-bullet | SC-7 local runtime foundation, SC-8 pure domain boundary | completed | 1 | `unit-01-compose-and-domain-foundation.md` |

## Learnings Brief
- [infrastructure] Compose topology checks are most stable through a short-lived probe container with socket checks.
- [domain] Keep core crate dependency-pure and verify with `cargo tree -p domain --depth 1`.
- [execution] Reserve high default host ports in `.env.example` to reduce local conflict risk.
- [execution] Use `./scripts/run-t01-foundation-tests.sh` as the repeatable T01 validation entrypoint.
