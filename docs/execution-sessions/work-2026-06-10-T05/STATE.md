---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/05-typed-skill-graph-edge-storage.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "## Execution Slices > Slice 5"
brainstorm_ref: none
started: 2026-06-10
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-10-T05
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: SkillDAG-style structural retrieval — typed graph edges returned as SEPARATE evidence (matches/neighbors/conflicts), never a ranking multiplier (the #208 lesson).
- Success-criteria focus: "Edges persist with type/reason/origin; invalid cycles or contradictory backbone edges fail; `conflicts_with` exists but is not traversed as a positive neighbor."

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit tests for edge validation/persistence/proposal behavior (`cargo test -p graph-builder && cargo test -p infrastructure persistence`); migration tests (ordering + live apply/skip count gate + TRUNCATE guard). No real-server retrieval e2e needed here (T05 is storage; the agent surface is T06).
- Exceptions: None.

### Constitution Context
- Schema migrations are approval-sensitive — owner approved this batch.
- Edge mutations need an observable audit trail (Postgres + history table). Owner decision: Postgres-only storage (no filesystem export this slice).
- Owner decision: high-confidence DETERMINISTIC cold-start edges auto-commit during rebuild (origin + evidence + confidence recorded); lower-confidence stay as proposals. Conflicts are never auto-walked.
- Local-first preserved: cold-start classification uses structured fields only, no external API keys.

### Architecture Handoff
- Artifact: plan-derived handoff (parent plan ## Architectural Context, ## Proposed V1.7 Architecture > Graph, ## Design Decisions #4/#5).
- Feature homes: `crates/graph-builder/src/graph/` (edge construction/validation) + `crates/infrastructure/src/persistence/` + `crates/infrastructure/migrations/` (typed-edge schema/history). Shared edge-relation semantics belong in `crates/domain`.
- Shared / global decisions: relation semantics centralized so retrieval (T06), MCP tools, and maintenance do not diverge. Migration floor is 009; new migrations are 010_*+.
- Deletion test: typed edges + history are concrete persistence now; the agent-facing graph search surface stays in T06 (do not build MCP tools here).
- Interfaces as test surfaces: edge validation (acyclicity/conflict semantics) and persistence roundtrip are the behavioral contracts.
- Seams / adapters / contracts: edge persistence flows through the same outbox/rebuild path as skills; `conflicts_with` is one-hop prune-only, never traversed.
- Review guidance: `/workflows:review` must verify no community/scalar boost reintroduced, conflicts not walkable, migrations approval-clean + drift-free, TRUNCATE const updated.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T05 Typed skill graph storage and cold-start edge proposals | expansion | Typed graph evidence for SkillDAG-style retrieval (unlocks T06) | completed (unit + live PG green) | 2 | unit-01-typed-skill-graph-edge-storage.md |

## Resume Log
- 2026-06-10: First execution-agent dispatch DIED before completing. On-disk salvage: `crates/infrastructure/migrations/010_skill_edges.sql` was written and is well-formed (keep it). Nothing else landed (no domain types, persistence, graph code, or tests).
- Decision: orchestrator implements T05 directly + serially (honors WSL2 no-concurrent-heavy-build rule; avoids agent-death loop). Builds run one at a time under orchestrator control.

## Remaining Work Checklist
- [x] `010_skill_edges.sql` migration (salvaged from dead agent)
- [ ] Optional `skill_edge_history` audit table (ticket scope: "storage AND history")
- [ ] Domain: `SkillEdge`, `EdgeType`, `EdgeOrigin` types + relation semantics (walkable set, conflicts) in `crates/domain`
- [ ] Edge construction + cold-start proposal generation (deterministic, structured-field based) in `crates/graph-builder/src/graph/`
- [ ] Backbone directed-acyclicity validation + conflict semantics (fail loud)
- [ ] Persistence: edge upsert/read in `crates/infrastructure/src/persistence/`
- [ ] Register migration 010 in postgres.rs MIGRATIONS + include_str! const
- [ ] Bump `migration_set_is_ordered_001_through_009` -> 010
- [ ] Bump live `live_run_migrations_applies_then_skips_on_second_boot` count gate -> 010
- [ ] Add new edge tables to `TRUNCATE_ALL_TABLES_SQL` + its guard test
- [ ] Unit tests: edge validation, acyclicity, conflict non-walkable, cold-start proposal, persistence roundtrip
- [ ] Build serially + run `cargo test -p graph-builder && cargo test -p infrastructure persistence`

## Learnings Brief
_No learnings yet._
