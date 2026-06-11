---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/17-mcp-server-boot-readiness-honesty.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/17-mcp-server-boot-readiness-honesty.md
brainstorm_ref: null
started: 2026-06-11T16:45:01Z
status: in_progress
execution_shape: vertical-slices
current_unit: 1
total_units: 3
session_id: work-2026-06-11-164501-T17
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md (no brainstorm_ref)
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: A /health that never claims ready while the retrieval snapshot is warming, and a boot/reload that loads precomputed vectors in seconds instead of re-embedding the whole corpus — so T11's measured sweeps can trust readiness instead of probing around it.
- Success-criteria focus: T17 ACs 1-5 (readiness honesty; precomputed-vector load + changed-only re-embed + fail-loud model/dim; ~7min→seconds; live cold-boot test + workspace green; T11 can gate on the honest signal).

### TDD Contract
- Effective mode: Ralph-driven TDD (plan tdd.mode=ralph, plan_overrides_local).
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun.
- Required evidence: Unit tests (embedding-cache store, content-hash change detection, dim-mismatch fail-loud, readiness state machine, tool warming short-circuit) + e2e (live qwen3 262-skill cold-boot test driving the real server: no healthy-while-warming window, no tool-call hang, before/after timing).
- Exceptions: None.

### Constitution Context
- constitution_version: 2.1.0 (plan). Approval-sensitive: schema migrations + embedding model changes.
- OWNER APPROVAL OBTAINED 2026-06-11 (this session): migration 011 `skill_embeddings` cache table (additive, write-ahead, IF NOT EXISTS) APPROVED; readiness scope = "gate reload + readiness signal" (lighter, does not weaken bind-after-ready initial boot contract).
- Machine-wide + repo no-fakes mandate: zero stubs/fakes in production or non-unit-test paths; fail loud. No silent fallback to a stale/partial snapshot while claiming health. Do not weaken existing fail-loud boot contracts. Live cold-boot test drives the real server/corpus.

### Architecture Handoff
- Artifact: plan-derived handoff (parent plan ## Architectural Context + ## Proposed V1.7 Architecture; ADR-0001 read-model CQRS contract; #235 DimensionMismatch semantics).
- Feature homes: crates/mcp-server/src (boot/readiness) + crates/infrastructure/src (vector persistence read at boot). graph-builder is OUT OF SCOPE (do not change its rebuild/Qdrant write path).
- Shared / global decisions: Qdrant stays write-side-only per ADR-0001 (do NOT add a Qdrant boot read path). New embedding persistence is a Postgres cache table read at boot. ArcSwap snapshot stays lock-free.
- Confirmed mechanism: `build_graph_from_pg` (lib.rs:1145) unconditionally re-embeds e_summary + subunits + e_task/e_needs/e_negative on EVERY call; same fn is reused by `PostgresGraphReloader::reload_and_swap` (lib.rs:1048) on a detached task after `graph.rebuilt`. /health (infrastructure/health.rs:117) probes only connectivity → reports healthy during background re-embed; concurrent find_skill query-embed starves on the shared Ollama semaphore (16 permits) → 7-min hang. Both behaviors trace to the unconditional re-embed + missing readiness signal.
- Deletion test: the embedding-cache store must be real (Postgres-backed), not abstracted speculative layers. Readiness state is a concrete shared signal, not a generic framework.
- Interfaces as test surfaces: EmbeddingCacheStore (load_for_model / upsert_many / fail-loud dim mismatch); readiness state (NOT-ready while build/reload in flight); tool warming short-circuit (explicit fast response, no embed).
- Seams / adapters / contracts: EmbeddingService trait unchanged; PostgresGraphReloader + build_graph_from_pg are the boot/reload seam; #235 DimensionMismatch semantics for mismatch fail-loud.
- Review guidance: verify no silent stale-snapshot-while-healthy; verify fail-loud preserved (missing config still fails loud, not warming-forever); verify reload error → FAILED readiness, not stuck warming; verify migration is additive/idempotent/write-ahead; verify no fakes in the live test.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Persisted embedding cache: kill the boot/reload re-embed | infra-persistence | AC2 + AC3 (precomputed-vector load, changed-only re-embed, fail-loud model/dim, ~7min→seconds) | completed (code+unit; live-PG → Unit 3 stack) | 1 | unit-01-persisted-embedding-cache.md |
| 2 | Readiness honesty: snapshot-ready signal + fast tool warming | hardening | AC1 (no healthy-while-warming; tools warming-fast, no hang) | pending | -- | -- |
| 3 | Live qwen3 cold-boot test + workspace green + T11 gate signal | e2e-evidence | AC3/AC4/AC5 (live proof both behaviors; cargo test --workspace green; T11 gates on honest signal) | pending | -- | -- |

## Learnings Brief
- [migration] Migrations are a compile-time `MIGRATIONS` array in `crates/infrastructure/src/persistence/postgres.rs` (include_str! consts), NOT directory-scanned. A new `.sql` file is INERT until: (1) `MIGRATION_0NN` const added, (2) appended to `MIGRATIONS`, (3) added to `TRUNCATE_ALL_TABLES_SQL` if it holds live data, (4) ordering test `migration_set_is_ordered_001_through_0NN` updated, (5) per-migration declares-table test added, (6) live `..._applies_then_skips...` count updated. Always verify postgres.rs is in the diff when a migration is added.
- [embeddings] `build_graph_from_pg` (lib.rs ~1305) is the SINGLE boot + background-reload snapshot builder; it took an `EmbeddingModelInfo` param (boot has it at ~635; reload via `PostgresGraphReloader.model_info`). Cache keys: `e_summary`/`e_task`/`e_needs`/`e_negative` + `subunit:{position}`. Exact LE-BYTEA f32 roundtrip is mandatory (no score drift). Blank views never cached (T09 blank-skip preserved).
- [clippy] feat/v-1-7 has a PRE-EXISTING workspace `-D warnings` blocker: `compile_context_bench` SeededSkill missing T09 fields + tests/e2e harness dead-code (ENV_LOCK/ScopeEnvGuard/QdrantObserver/RedisObserver/configure_scope_env*). Surfaces under `--all-targets`. Not T17's; track separately. Use `--lib` clippy to isolate T17-owned warnings.
- [retrieval/state] Snapshot is `ArcSwap<GraphSnapshot>` (lock-free, never blocks readers). The "find_skill hang" is NOT a snapshot lock — it's the query embed starving on the shared Ollama semaphore (16 permits) saturated by the bulk re-embed during a background `graph.rebuilt` reload. Unit 2 readiness must short-circuit tool calls BEFORE the query embed.
