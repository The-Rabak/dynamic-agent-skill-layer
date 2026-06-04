---
source_type: plan
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
ticket_index: null
ticket_file: null
tickets_ref: null
source_packet_ref: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
brainstorm_ref: null
started: 2026-06-04T11:36:52Z
status: in_progress
execution_shape: vertical-slices
current_unit: 3
total_units: 11
session_id: work-2026-06-04-113652
---

## WHY Context

### Problem Narrative
The V1.5 live e2e suite is GREEN but the 2026-06-04 dream-state evaluation (5 opus assessors) found
DS-004/005/006/007 pass WITHOUT exercising their named contracts (no real restart, zero drift injected,
`ok=0/no_match=24` counted as success, a serialized latency ramp mislabeled "high QPS"), and `report.rs`
records `outcome: Passed` independent of any assertion. The containerized mcp-server has no `git`, so
project-scope retrieval ALWAYS degrades in-container. Green CI gives false confidence — a real resilience
regression would ship undetected.

### User Story
As the maintainer/operator, I need every dream-state e2e scenario to drive a real fault or load through
the real production code paths and real infrastructure and assert measured, fail-able outcomes, so that a
GREEN suite is trustworthy proof that the system survives Qdrant/Ollama/PG/Redis failures, replays its
outbox without loss, reconciles store drift, stays consistent under saturation, and meets its latency budget.
Secondary: the containerized server must resolve project scope (`Ok`, not `degraded`).

### Architectural Context
Lives in `tests/e2e/` (dream-state contract tests + report harness + runner) plus two production crates
(scope resolver in `crates/infrastructure`, embedding read-path). Option-A CQRS: read = in-memory
`RetrievalSnapshot`; Qdrant = write-side only. Real infra via `docker-compose.test.yml`. Data flow under
test: SKILL.md → watcher/ingest → queue → drain → `.pending` → human approve (rename) → graph rebuild →
Redis `graph.rebuilt` → snapshot swap → `compile_context` retrieval.

### Success Criteria
1. No fake passes: report outcome derived from recorded assertions; zero-assertion scenario = ERROR.
2. Every brutal scenario demonstrably fail-able (RED when fault/condition violated; GREEN when system behaves).
3. Real project-scoped `Ok` through containerized server (#154).
4. DS-004: real kill + ≥2 restarts; replayed==enqueued, lost==0, duplicated==0, seeded skills retrievable.
5. DS-005: N injected divergences (both directions) → reconcile_once → store-count equality, scale ≥100.
6. DS-006: sustained churn through full loop → final count==expected, zero dupes, queue drained, ok_count>0.
7. DS-007: genuine concurrency, p95 threshold + error budget, warm vs cold separated, bottleneck removed/bounded.
8. DS-003: recovery latency recorded; qdrant_write_side degradation hard-asserted; bounded polling; PG+Redis faults.
9. Coverage: DS-002 + DS-008 promoted; hostile-input promoted; deferrals recorded.
10. Guardrails: Ollama/`cloud_calls:none`; existing GREEN suite stays green; any compose/Dockerfile/env/migration
    change human-gated and surfaced (target: none).

### TDD Contract
- Effective mode: Ralph-driven TDD.
- Effective loop: failing test first → minimal implementation → refactor → post-refactor rerun.
- Required evidence: unit (required) + e2e (required). For test-hardening slices, "Red" = the new brutal
  assertion FAILS when the contract is violated (disable fix / inject fault without guard), proving fail-ability.
- Exceptions: none.

### Constitution Context
v2.1.0. Local-First (P1): default path Ollama, `cloud_calls:none`, no cloud reach. Human Gate for Mutations (P3):
DS-006 drives real `.pending`→approve(rename); never auto-approve outside test sandbox. Required approvals
(human-gate, surface before applying): ANY docker-compose.test.yml/Dockerfile/env/migration change — target ZERO.
Fault injection uses `docker compose stop/start` of existing services (commands, not file edits). Waivers: none.

### Architecture Handoff
- Artifact: plan-derived handoff (docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md).
- Feature homes: `tests/e2e/` (brutal suite); `crates/infrastructure/src/scope.rs` (resolver); embedding read-path crate (DS-007).
- Shared/global: Option-A CQRS stays intact; report schema semantics (report.rs) are shared across all scenarios.
- Deletion test: `report.rs` outcome derivation is the linchpin — must derive from recorded assertions, concrete.
- Interfaces as test surfaces: `ScopeResolver` trait (1.1 swaps impl behind it); `OutboxReconciler::reconcile_once`
  (DS-005); real outbox + Redis Streams (DS-004); watcher + extraction worker pool (DS-006); embedding client (DS-007).
- Seams: trait-level rollback for resolver; feature-flag/bounded-pool revert for embedding concurrency.
- Review guidance: production fixes (resolver, embedding) need security-sentinel/performance-oracle/architecture-strategist.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | 1.1 Real project-scoped Ok in-container (#154 git-free resolver) | tracer-bullet | SC#3 + secondary story | completed | 1 | unit-01-slice-1.1-git-free-resolver.md |
| 2 | 1.2 Honest reporting — outcome from real assertions | hardening | SC#1 (prereq for all) | completed | 1 | unit-02-slice-1.2-honest-reporting.md |
| 3 | 1.3 Real-infra fault-injection harness | hardening | enables DS-003..008 | completed | 2 | unit-03-slice-1.3-fault-injection-harness.md |
| 4 | 2.1 DS-003 dependency_chaos_matrix — deepen Option-A proof | hardening | SC#8 | pending | -- | -- |
| 5 | 2.2 DS-004 outbox_backlog_replay — kill/restart + no loss | hardening | SC#4 | pending | -- | -- |
| 6 | 2.3 DS-005 qdrant_pg_drift — inject/reconcile/converge | hardening | SC#5 | pending | -- | -- |
| 7 | 2.4 DS-006 watcher_extraction_saturation — real loop convergence | hardening | SC#6 | pending | -- | -- |
| 8 | 2.5 DS-007 high_qps_compile_context — concurrency + bounded hot path | hardening+perf | SC#7 | pending | -- | -- |
| 9 | 3.1 Promote DS-002 (transport) + DS-008 (multi-repo isolation) | expansion | SC#9 | pending | -- | -- |
| 10 | 3.2 Promote hostile-input / trust-boundary scenario | expansion | SC#9 | pending | -- | -- |
| 11 | 4.1 Wire fail-able suite into runner/CI without regressing green | hardening | all SC | pending | -- | -- |

## Batches (dependency + file-overlap aware)
- Batch 1 (parallel-safe): 1.1, 1.2, 1.3 — non-overlapping files (1.1 e2e proof fenced to a NEW file).
- Batch 2 (sequential — all edit tests/e2e/test_dream_state_contract.rs): 2.1 → 2.2 → 2.3 → 2.4 → 2.5.
- Batch 3 (sequential — edit contract test): 3.1 → 3.2.
- Batch 4: 4.1.

## Learnings Brief
- **[e2e-harness]** Include shared modules via `#[path = "..."] mod x;`: `report.rs`, `../integration/env_guard.rs`,
  and the new `support/mod.rs`. Phase-2 scenarios add `#[path = "support/mod.rs"] mod support;`.
- **[reporting]** Use `report.assert_contract(name, passed: bool, expected, actual, details)` to record fail-able
  outcomes. `build()` now: any failed assertion/section ⇒ `Failed`; zero assertions+sections ⇒ `Failed` (not Passed).
  Every Phase-2 scenario MUST record real `contract_assertions` — a scenario that asserts nothing now reports Failed.
- **[harness API]** `support::infra::compose_stop_service / compose_start_services` (docker compose stop/start COMMANDS only);
  `support::poll::poll_until(pred, timeout, interval)` (NO fixed sleeps); `support::drift::inject_pg_skills_without_qdrant_vectors`
  / `inject_qdrant_vectors_without_pg_rows` / `remove_*` / `pg_active_skill_ids`;
  `support::load::write_skill_files_to_sandbox` + `fire_concurrent_compile_context(...) -> Vec<CallSample{status,duration_ms}>`
  (prove real concurrency: `max(dur) < sum(dur)`, not a monotonic ramp).
- **[infra wiring]** Server: `McpServerApp::from_environment(retrieval_config)` reads `DATABASE_URL/QDRANT_URL/OLLAMA_URL/
  REDIS_URL/SKILL_GLOBAL_PATHS/SKILL_GLOBAL_ALLOWED_ROOTS`; `env_guard` sets them. PG: `sqlx::PgPool` via `PostgresAdapter::pool()`.
  Qdrant: `QdrantAdapter` REST; `OutboxVectorStore::list_point_ids`. Compose path: `CARGO_MANIFEST_DIR/../../docker-compose.test.yml`.
- **[scope]** `FsMarkerProjectResolver` replaced `GitRootProjectResolver` (wired mcp-server/src/lib.rs:176,512). Project scope
  now resolvable in-container → DS-006/3.1 can assert real project-scoped `Ok`. `SKILL_PROJECT_MARKER` env is the no-`.git` fallback.
- **[clippy debt]** `cargo clippy --all-targets -D warnings` is RED on HEAD baseline (pre-existing dead_code/len_zero in
  test_concurrency_stress.rs, report.rs `record_degradation_event`, etc.). NOT our regression. Fix only what your slice touches;
  DS-003 rewrite (2.1) will naturally consume `record_degradation_event`. Validate per-target, not `--all-targets`.
- **[live e2e]** No docker stack in the agent sandbox → live tests are `#[ignore="requires live containers"]` and PENDING-LIVE;
  the real proof runs via `scripts/run-e2e-tests.sh`. Agents must NOT fake green; write correct code, run non-live checks, document the live command.
