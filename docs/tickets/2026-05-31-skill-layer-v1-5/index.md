---
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
execution_shape: vertical-slices
ticket_set_status: ready # ready | in_progress | blocked | completed
last_completed_batch: 0
total_batches: 8
---

# Ticket Set: skill-layer-v1-5 — Close the Loop

- **Plan:** `docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md`
- **Architecture:** `docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`
- **Execution shape:** `vertical-slices` (tracer bullet first: T01)
- **Ticket count:** 10 (1:1 with the plan's 10 execution slices)

> Scope fence (V2 boundary): no SkillLens quality scoring, no SkillOpt, no team/remote scope, no LLM-guidance compiler, no multi-harness compilers, no self-healing, no counterfactual explainability, no outcome-learning. Recency/frequency is a *deterministic* prior + retirement input only.

## Dependency Graph

```
T01 (tracer: prod retrieves live graph; SeededGraph→RetrievalSnapshot rename)
 ├─ T02 (refresh on graph.rebuilt; graph-builder Redis publish + ArcSwap swap)
 ├─ T03 (Qdrant role = Option A; honest health; ADR + CQRS docs)
 ├─ T04 (Claude Code lifecycle hooks: inject + SessionEnd extract trigger)
 │   └─ T05 (reliable extraction: MPMC worker pool + Anthropic provider)
 │       └─ T07 (crash-safe transcript reconciliation)  [also needs T04]
 ├─ T06 (record usage; retirement + deterministic prior; migration 002)
 ├─ T08 (Qdrant ports + suppression isolation; needs T01,T02,T03)
 └─ T09 (retrieval quality: real skills match; needs T01,T02,T03)
        └─ T10 (green live suite + CI gate; needs T01,T02,T03,T05,T06,T08,T09)
```

> **Dependency-type convention:** the plan labels blocking dependencies "real"; tickets encode this as `dependency_type: hard` per the contract enum (`none | hard | soft | parallel-safe`). T03→T09 and T03→T10 are `hard` because T03 edits `orchestrator.rs`/`dual_scope.rs` earlier and defines the DS-003 CQRS contract those tickets consume.

Explicit `depends_on` edges:

| Ticket | depends_on |
|---|---|
| T01 | — |
| T02 | T01 |
| T03 | T01 |
| T04 | T01 |
| T05 | T04 |
| T06 | T01 |
| T07 | T04, T05 |
| T08 | T01, T02, T03 |
| T09 | T01, T02, T03 |
| T10 | T01, T02, T03, T05, T06, T08, T09 |

## Execution Batches

`last_completed_batch` advances only after **every** ticket in the batch reaches `completed`.

| Batch | Tickets | Status | Gating reason |
|---|---|---|---|
| 1 | T01 | pending | Foundation. Sweeping cross-crate rename (`SeededGraph`→`RetrievalSnapshot`) + prod constructor. Must be alone — it edits files every downstream retrieval ticket also touches. |
| 2 | T03, **‖** T04 | pending | Both depend only on T01. **Parallel-safe** — file-disjoint (see safety note). |
| 3 | T02, **‖** T05 | pending | T02 dep T01 ✓; T05 dep T04 ✓ (done in B2). **Parallel-safe** — file-disjoint. |
| 4 | T06 | pending | Singleton — shares `mcp-server/lib.rs`, `retrieval/orchestrator.rs`, `maintenance/runtime.rs` with other tickets. |
| 5 | T07 | pending | Singleton — shares `maintenance/runtime.rs` with T06. Deps T04+T05 satisfied. |
| 6 | T08 | pending | Singleton — shares `tests/e2e/*` with T09/T10 and `run-e2e-tests.sh` with T10 (line-ownership fence). Deps T01–T03 satisfied. |
| 7 | T09 | pending | Singleton — shares `orchestrator.rs`/`dual_scope.rs` with T02/T03/T06 and `tests/e2e/*` with neighbors. Deps T01,T02,T03 satisfied. |
| 8 | T10 | pending | Terminal integration gate. Deps T01,T02,T05,T06,T08,T09 all satisfied. Retires all V1.5 rollback flags. |

### File-overlap safety notes (every multi-ticket batch)

**Batch 2 — T03 ‖ T04 (parallel-safe):**
- T03 files: `retrieval/src/orchestrator.rs`, `retrieval/src/dual_scope.rs`, `infrastructure/src/vector/qdrant.rs`, `infrastructure/src/health.rs`, `docs/architecture/adr-0001-…md`, `docs/reference/online-retrieval-cqrs.md`.
- T04 files: `config/claude-code/hooks.example.json`, `docs/reference/capability-catalog.md`, `docs/runbooks/degraded-state.md`, `crates/mcp-server/src/tools/compile_context.rs`.
- **Disjoint:** no shared code file. Both write under `docs/reference/` but to **different files** (T03 → new `online-retrieval-cqrs.md`; T04 → existing `capability-catalog.md`). The two `compile_context` surfaces are distinct: T04 edits `tools/compile_context.rs` (pure query unit); no other ticket in this batch touches it. No shared mutable state, no migration, no shared adapter edited concurrently.

**Batch 3 — T02 ‖ T05 (parallel-safe):**
- T02 files: `retrieval/src/orchestrator.rs`, `mcp-server/src/lib.rs`, `mcp-server/src/graph_refresh_subscriber.rs`, `infrastructure/src/events/mod.rs`, `graph-builder/src/rebuild.rs`, `graph-builder/src/main.rs`.
- T05 files: `session-extractor/src/worker_pool.rs`, `session-extractor/src/lib.rs`, `infrastructure/src/extraction/ollama.rs`, `infrastructure/src/extraction/claude.rs`.
- **Disjoint:** no shared file. Both edit under `crates/infrastructure/` but in different subtrees (`events/` vs `extraction/`). No shared mutable state; T02's Redis publish and T05's extraction provider do not co-edit any module.

All other batches are singletons by the default-to-sequential rule (shared `lib.rs` / `orchestrator.rs` / `runtime.rs` / `tests/e2e/*` / `run-e2e-tests.sh` surfaces).

## Ticket Table

| ID | Title | Kind | Serves | Feature home |
|---|---|---|---|---|
| T01 | Production server retrieves from the live graph | tracer-bullet | SC-A, SC-F | crates/mcp-server |
| T02 | Live graph refreshes on graph.rebuilt without restart | expansion | SC-A | crates/mcp-server (+graph-builder) |
| T03 | Resolve Qdrant's online role (Option A + CQRS docs) | hardening | SC-F | crates/retrieval |
| T04 | Wire the full Claude Code session lifecycle | expansion | SC-B | config/claude-code |
| T05 | Reliable real extraction — worker-pool + provider | hardening | SC-C, SC-F | crates/session-extractor |
| T06 | Record usage; feed retirement + deterministic prior | expansion | SC-D | crates/mcp-server (+maintenance/retrieval) |
| T07 | Crash-safe transcript reconciliation | hardening | SC-B | crates/maintenance |
| T08 | Fix Qdrant ports + suppression test isolation | hardening | SC-E | crates/infrastructure |
| T09 | Retrieval quality — real/seeded skills match | hardening | SC-E, SC-A | crates/retrieval |
| T10 | Green live suite + CI gate (integration gate) | hardening | SC-E | tests/e2e |

## Blockers

- **T02 confirmed prerequisite (R-2):** graph-builder does NOT publish `graph.rebuilt` to Redis today (pushes to an un-drained in-memory `Vec`). T02 must add the publish path via `infrastructure::RedisStreamsAdapter` — net-new work folded into the ticket.
- **Human-gate checkpoints (stop and confirm before commit/apply):**
  - T04 — edits `config/claude-code/hooks.example.json` (infra-config).
  - T06 — `002_usage_fields.sql` migration (schema).
  - T07 — `003_processed_transcripts.sql` migration (schema).
  - T08 — `docker-compose.test.yml` / `scripts/run-e2e-tests.sh` env (infra-config).
  - T10 — CI workflow + `run-e2e-tests.sh` CI stanza (infra-config).
- **Gated cloud dependency (T05):** the opt-in `provider=claude` path calls the Anthropic API — a deliberate, human-vetoable local-first stretch. Ollama default needs no key.
- **T10 is an integration gate:** must not start until T01, T02, T03, T05, T06, T08, T09 are each individually green.
- **Ratified `ScopeRoot` relocation** (plan decision #5) is homed in **T01** (same cross-crate type-move surface as the rename); defer only if the diff > ~50 lines, with the deferral logged in T01's completion note.
- **Cross-cutting P1 security items are ticketized:** `DefaultBodyLimit` on the MCP router → **T08** (`protocol.rs:428`); strip absolute paths from `draft_paths` in the `extraction.completed` event → **T05** (`session-extractor/src/lib.rs`). Prompt-hash P3 → T06 (`002` migration); Redis-unauthenticated P2 stays document-only per the plan.

## Review Summary

`ticket-flow-auditor` ticket-set audit run 2026-05-31. **5 blocking gaps + 6 recommendations reported; all blocking gaps resolved, all recommendations adopted.** Batch-safety verdict: Batch 2 (T03‖T04) and Batch 3 (T02‖T05) confirmed genuinely file-disjoint and parallel-safe; all other batches correctly serialized. Plan→ticket 1:1 mapping confirmed (10 slices → 10 tickets, no slice lost); all WHY reassessments R-1…R-6 and human-gate checkpoints honored.

### Blocking gaps — ALL RESOLVED
- **BG-1 (ScopeRoot had no ticket home):** RESOLVED — homed in T01 (files + AC + scope, with ≤50-line defer-and-log escape).
- **BG-2 (architecture doc stale `SkillSnapshot` ×5):** RESOLVED — corrected to `RetrievalSnapshot` in `docs/architecture/2026-05-31-…-architecture.md` with an inline R-4 correction note; T01 Local Context flags the precedence.
- **BG-3 (T01 `test_command` over-reached to the full roundtrip, contradicting R-3):** RESOLVED — narrowed to a seed-and-retrieve smoke (`boot_time_live_retrieval`); roundtrip moved to Phase-3 evidence (T09/T10); AC reworded.
- **BG-4 (T10 `depends_on` omitted T03 which defines the DS-003 contract):** RESOLVED — T03 added to T10 `depends_on`.
- **BG-5 (two P1 security items unticketized):** RESOLVED — `DefaultBodyLimit` → T08 (`protocol.rs:428`); `draft_paths` absolute-path strip → T05 (`session-extractor/src/lib.rs`), each as a named AC.

### Recommendations — ALL ADOPTED
- **R-1 (T05↔T04 coupling is delivery-ordering, not compiler-hard):** noted — kept `hard` for conservative batch ordering; the dependency-type convention note documents the "real"→`hard` mapping.
- **R-2 (T09 missing T03 dep on shared `orchestrator.rs`):** adopted — T03 added to T09 `depends_on` + a "read T03's diff first" note.
- **R-3 (CI purity gate absent):** adopted — added `cargo tree` purity AC to T10.
- **R-4 (rename completeness):** adopted — added a `grep -rn 'SeededGraph'` zero-hit check to T01 Local Context.
- **R-5 (`real`→`hard` mapping undocumented):** adopted — convention note added under the dependency graph.
- **R-6 (shared `run-e2e-tests.sh` line-ownership not enforced by AC):** adopted — added line-ownership ACs to both T08 (port/env only) and T10 (CI stanza only).

**Execution readiness: 0 blocking gaps remain. Ticket set is execution-ready.**
