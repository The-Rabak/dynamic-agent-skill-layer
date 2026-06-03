---
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
execution_shape: vertical-slices
ticket_set_status: in_progress # ready | in_progress | blocked | completed
last_completed_batch: 8 # batches 1–5 done; batch 6 (T07) superseded → todo 103; batch 7 (T08) done 2026-06-03; batch 8 (T09) done 2026-06-03 (session work-2026-06-03-073851; migration renumbered 004→005). Post-batch commits bcfa9de/d8e45f3/295bfef extended T05 after batch 4 closed (see Batch 4 note below)
total_batches: 10 # re-batched 2026-05-31; T10b activation proof added 2026-06-02 as post-gate adoption slice
---

# Ticket Set: skill-layer-v1-5 — Close the Loop

- **Plan:** `docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md`
- **Architecture:** `docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`
- **Execution shape:** `vertical-slices` (tracer bullet first: T01)
- **Ticket count:** 11 (10 plan execution slices + T10b post-gate activation proof added from the 2026-06-02 assessment)

> Scope fence (V2 boundary): no SkillLens quality scoring, no SkillOpt, no team/remote scope, no LLM-guidance compiler, no multi-harness compilers, no self-healing, no counterfactual explainability, no outcome-learning. Recency/frequency is a *deterministic* prior + retirement input only. T10b is allowed because it is activation/proof packaging, not V2 product surface.

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
            └─ T10b (first-run activation proof; doctor + demo + time-to-wow)
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
| T10b | T10 |

## Execution Batches

`last_completed_batch` advances only after **every** ticket in the batch reaches `completed`.

| Batch | Tickets | Status | Gating reason |
|---|---|---|---|
| 1 | T01 | completed | Foundation. Sweeping cross-crate rename (`SeededGraph`→`RetrievalSnapshot`) + prod constructor. Must be alone — it edits files every downstream retrieval ticket also touches. |
| 2 | T03, **‖** T04 | completed | Both depend only on T01. **Parallel-safe** — file-disjoint (see safety note). T04 hooks.example.json human-gate approved. |
| 3 | T02 | completed | **Pulled forward 2026-05-31** (was paired with T05). Singleton — T02 and T03 both edit `retrieval/src/orchestrator.rs`, so T02 CANNOT run parallel with T03; it runs sequentially right after Batch 2. Dep T01 ✓. |
| 4 | T05 | completed | Singleton — was paired with T02; now solo. Dep T04 ✓ (done in B2). File-disjoint from T02 but separated by the re-batch; gated cloud dep on the opt-in `provider=claude` path. **Post-batch (2026-06-02):** commits `bcfa9de` (fix stale graph-rebuild skill count + Ollama healthcheck), `d8e45f3` (headless Claude Code CLI provider + default model bumps), `295bfef` (clarify Claude CLI provider credentials) extended T05's effective file set after batch 4 closed. Retro-authorized by ADR-0002 + constitution v2.1.0; model/healthcheck changes approved via `docs/execution-sessions/retro-2026-06-02-model-healthcheck-approval/retro-approval.md`. |
| 5 | T06 | completed | Singleton — shares `mcp-server/lib.rs`, `retrieval/orchestrator.rs`, `maintenance/runtime.rs` with other tickets. Done 2026-06-01: usage write (bounded-mpsc bg writer) + `UsagePersistencePort`/`UsageSampleStore` ports + deterministic `usage_prior` + retirement read + migration `003_usage_fields.sql` (renumbered from `002`; `002` taken by transcript-ingest queue). |
| 6 | ~~T07~~ | **superseded → todo 103** | **Folded into todo 103's durable PG transcript-ingest queue (2026-06-01).** The marker table + `reconcile_transcripts()` FS-scan are replaced by `transcript_ingest_queue` (migration `002`) + the maintenance queue drain. SessionEnd's broken absolute-`{{transcript_path}}` wiring is fixed at the same time. See `todos/103-...md`. |
| 7 | T08 | completed | Singleton — shares `tests/e2e/*` with T09/T10 and `run-e2e-tests.sh` with T10 (line-ownership fence). Deps T01–T03 satisfied. **Done 2026-06-03:** Qdrant REST/gRPC port fix (`run-e2e-tests.sh:76`→HTTP port; compose `:6334`→`:6333`), suppression local `clear_session` prefix fix + DashMap-first lookup, `DefaultBodyLimit` (4 MiB) on the MCP router, loopback-posture preflight. Human-gate compose/script edits approved. Live roundtrip **green under `MCP_USAGE_LOGGING=off`** (preflight connects, compile `Ok`, duplicate suppressed); default-config teardown deadlock handed to T10 (see Blockers). |
| 8 | T09 | completed | Singleton — shares `orchestrator.rs`/`dual_scope.rs` with T02/T03/T06, `persistence/rebuild.rs` with T01, and `tests/e2e/*` with neighbors. Owns the `skills.source_paths` column + human-gated migration (replaces T01's scope-root stand-in). Deps T01,T02,T03 satisfied. **Done 2026-06-03** (session `work-2026-06-03-073851`): real per-skill `source_paths` (write+boot-read, empty→scope-root fallback documented), shared `tests/fixtures/retrieval_corpus.json`, deterministic `### Why These Skills` match-reason. **Migration renumbered `004`→`005_skill_source_paths.sql`** (`004` slot taken by `004_session_logs_status_check.sql`); human-gate approved. **Side effect:** the pre-existing unwired/non-idempotent `004` migration was wired into `MIGRATIONS` + made idempotent (DO block) — review must confirm `session_logs.status` writers honor the now-active CHECK. Live roundtrip green under `MCP_USAGE_LOGGING=off`; strict clippy + fmt clean. |
| 9 | T10 | pending | Terminal integration gate. Deps T01,T02,T03,T05,T06,T08,T09 all satisfied. Retires all V1.5 rollback flags. |
| 10 | T10b | pending | Post-gate activation proof. Depends on T10 because it consumes the final green runner/report shape and T09's corpus; produces `doctor.sh`, `run-demo.sh`, and time-to-wow docs. |

> **Re-batch note (2026-05-31, maintainer direction):** T02 ("refresh on `graph.rebuilt`") was moved out of the old `T02 ‖ T05` batch into its own batch (now Batch 3) so it executes immediately after Batch 2 rather than later. It could not simply join Batch 2 as a parallel unit because **T02 and T03 both edit `retrieval/src/orchestrator.rs`** (T02 adds the `ArcSwap` swap; T03 relabels health markers) — parallel agents would race that file. So Batch 2 runs `T03 ‖ T04` in parallel, then Batch 3 runs `T02` sequentially. T05 becomes its own Batch 4. `total_batches` 8 → 9; downstream batch numbers shifted +1.

### File-overlap safety notes (every multi-ticket batch)

**Batch 2 — T03 ‖ T04 (parallel-safe):**
- T03 files: `retrieval/src/orchestrator.rs`, `retrieval/src/dual_scope.rs`, `infrastructure/src/vector/qdrant.rs`, `infrastructure/src/health.rs`, `docs/architecture/adr-0001-…md`, `docs/reference/online-retrieval-cqrs.md`.
- T04 files: `config/claude-code/hooks.example.json`, `docs/reference/capability-catalog.md`, `docs/runbooks/degraded-state.md`, `crates/mcp-server/src/tools/compile_context.rs`.
- **Disjoint:** no shared code file. Both write under `docs/reference/` but to **different files** (T03 → new `online-retrieval-cqrs.md`; T04 → existing `capability-catalog.md`). The two `compile_context` surfaces are distinct: T04 edits `tools/compile_context.rs` (pure query unit); no other ticket in this batch touches it. No shared mutable state, no migration, no shared adapter edited concurrently.

**Batch 3 — T02 (singleton, was T02 ‖ T05):**
- T02 files: `retrieval/src/orchestrator.rs`, `mcp-server/src/lib.rs`, `mcp-server/src/graph_refresh_subscriber.rs`, `infrastructure/src/events/mod.rs`, `graph-builder/src/rebuild.rs`, `graph-builder/src/main.rs`.
- **Why singleton, not parallel with Batch 2:** T02 edits `retrieval/src/orchestrator.rs` (adds the `ArcSwap` swap path), and so does T03 (relabels health markers, `orchestrator.rs:168–186`). Two agents editing one file race → T02 runs sequentially after Batch 2 completes, building on T03's committed health-marker change.
- T02 remains file-disjoint from T05 (the old pairing was safe); they are now simply sequential singletons after the re-batch.

**Batch 4 — T05 (singleton, was T02 ‖ T05):**
- T05 files: `session-extractor/src/worker_pool.rs`, `session-extractor/src/lib.rs`, `infrastructure/src/extraction/ollama.rs`, `infrastructure/src/extraction/claude.rs`.
- Disjoint from T02; separated only by the re-batch so the maintainer's "T02 next" direction is honored without bundling T05's heavier cloud-provider work into the same round.
- **T05 file-set extended (2026-06-02, ADR-0002):** Post-ticket commits `bcfa9de`/`d8e45f3`/`295bfef` added `crates/infrastructure/src/extraction/claude_code.rs` (687 lines) and `crates/session-extractor/src/providers/claude_code.rs` (45 lines) to T05's effective file set. These files are retro-authorized by ADR-0002 and constitution v2.1.0. **The T07–T10 batch-overlap safety analysis must account for `claude_code.rs` now living in T05's files.** Any ticket in batches 7–9 that touches `infrastructure/src/extraction/` or `session-extractor/src/` must verify it does not conflict with these new T05 files.

All other batches are singletons by the default-to-sequential rule (shared `lib.rs` / `orchestrator.rs` / `runtime.rs` / `persistence/rebuild.rs` / `tests/e2e/*` / `run-e2e-tests.sh` surfaces).

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
| T10b | First-run activation proof | hardening | SC-A, SC-B, SC-E | scripts |

## Blockers

- **T02 confirmed prerequisite (R-2):** graph-builder does NOT publish `graph.rebuilt` to Redis today (pushes to an un-drained in-memory `Vec`). T02 must add the publish path via `infrastructure::RedisStreamsAdapter` — net-new work folded into the ticket.
- **Human-gate checkpoints (stop and confirm before commit/apply):**
  - T04 — edits `config/claude-code/hooks.example.json` (infra-config).
  - T06 — `003_usage_fields.sql` migration (schema) — **applied 2026-06-01** (renumbered from `002`; that slot is held by `002_transcript_ingest_queue.sql`).
  - T07 — `003_processed_transcripts.sql` migration (schema).
  - T08 — `docker-compose.test.yml` / `scripts/run-e2e-tests.sh` env (infra-config).
  - T09 — `004_skill_source_paths.sql` migration (schema) — **added 2026-05-31** to store real per-skill source paths and retire T01's scope-root stand-in (maintainer direction). **Applied 2026-06-03, renumbered to `005_skill_source_paths.sql`** (`004` slot already held by `004_session_logs_status_check.sql`); human-gate approved. The previously-unwired `004_session_logs_status_check.sql` was also wired into `MIGRATIONS` and made idempotent at the same time.
  - T10 — CI workflow + `run-e2e-tests.sh` CI stanza (infra-config).
- **Gated cloud dependency (T05):** the opt-in `provider=claude` path calls the Anthropic API — a deliberate, human-vetoable local-first stretch. Ollama default needs no key.
- **T10 is an integration gate:** must not start until T01, T02, T03, T05, T06, T08, T09 are each individually green.
- **T10 BLOCKER — teardown TRUNCATE-vs-async-usage-writer deadlock (surfaced by T08, owned by T10/T06):** with T08's Qdrant preflight fix, `test_live_data_plane_roundtrip` now runs its full body and passes all assertions; it is **green under `MCP_USAGE_LOGGING=off`** but deadlocks in teardown under the default (`on`). Root cause: T06's fire-and-forget async usage-writer transactions hold RowExclusive locks on `session_logs`/`skill_usage` while `LiveServerComponents::teardown` → `truncate_all_tables` issues `TRUNCATE … CASCADE` (ACCESS EXCLUSIVE), with two live server instances alive. **Fix owner:** the T06/teardown seam — drain/await background usage writers (or close the app pool / abort writer tasks) before truncate, or make the test-only `truncate_all_tables` deadlock-resilient. Files: `crates/infrastructure/src/persistence/postgres.rs`, `crates/mcp-server/src/lib.rs::teardown` — **outside T08's declared file set.** T10 must resolve this for the full live suite to be green. Evidence: `docs/execution-sessions/work-2026-06-03-001046/unit-01-t08-qdrant-ports-suppression.md`.
- **T10 pre-existing cleanup (not introduced by T08, blocks SC-V1.5-F strict-clippy/fmt gate):** ~~(1) `crates/infrastructure/src/extraction/claude_code.rs` (T05) fails `cargo fmt --check` — **resolved by T09 (2026-06-03): strict fmt clean applied**;~~ (2) shared e2e helpers `tests/e2e/report.rs` (`if_same_then_else`) and `tests/e2e/test_concurrency_stress.rs` (`len_zero`) fail strict `clippy -D warnings --all-targets`; (3) `health::tests::build_health_checker_injects_usage_write_disabled_when_flag_is_off` env-var-contamination flake (passes isolated).
- **T10 handoff from T09 (concurrency-burst expectation now stale):** T09's real-provenance fix makes the burst prompts in `compile_context_parallel_burst_under_live_infra_stays_within_contract_statuses` (`tests/e2e/test_concurrency_stress.rs:558`) all return `Ok`, so its `assert!(no_match_count > 0, "at least one NoMatch response required")` now fails (while `ok_count > 0` correctly passes). This is a consequence of T09 succeeding, not a retrieval defect. **T10 owns `test_concurrency_stress.rs`** (Slice 3.3 DS-006/007) and must rebalance the burst so it deterministically includes a relevant→`ok` AND an irrelevant→`no_match` prompt (or relax the assertion), so the mixed-status contract proof survives working retrieval. Same file/`extract_session_parallel_burst*` may need the same review. Surfaced by session `work-2026-06-03-073851`.
- **T10b is a post-gate adoption proof:** must not start until T10 is green. It must not fix upstream behavior; it only packages the proven path into doctor/demo scripts and quickstart docs.
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
