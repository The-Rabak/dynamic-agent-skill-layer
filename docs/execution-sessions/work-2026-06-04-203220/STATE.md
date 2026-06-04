---
source_type: todo
plan_file: "(legacy input: todos/ batch — #162, #141, #155)"
ticket_index: ""
ticket_file: ""
brainstorm_ref: ""
started: 2026-06-04T20:32:20Z
status: in_progress
execution_shape: fix-batch
current_unit: 3
total_units: 6
session_id: work-2026-06-04-203220
---

## WHY Context

### Problem Narrative
A 2026-06-04 honesty sweep removed hardcoded `AssertionResult::Passed` and stub embedders from the live data-plane
and dream-state suites. That surfaced real, previously-hidden gaps: a live extraction→draft path that never proved
itself (#162), a migration runner that re-executes every migration on every boot with no tracking table (#141), and
dream-state contract tests whose honesty was fixed but whose named contracts are still not genuinely exercised
end-to-end against real infra (#155). Green was never proof. This session makes each of these genuinely true on
live infra.

### User Story
As the maintainer of the dynamic-agent-skill-layer, I want the resilience/durability/extraction guarantees the test
suite *claims* to be backed by real mechanisms exercised against the live containerized stack, so that "green" means
the system actually works in production — not that a fake passed.

### Architectural Context
- Real ingest loop: `SKILL.md.pending → human-gate rename → graph-builder rebuild (768-dim Ollama, PG, graph_version
  bump) → XADD graph.rebuilt to Redis skill-layer-events → mcp-server reload_and_swap → compile_context`.
- CQRS read model: `docs/reference/online-retrieval-cqrs.md`. Outbox reconciler: `OutboxReconciler::reconcile_once`.
- Canonical real-app e2e harness: `tests/e2e/harness/` (contract `docs/reference/e2e-harness-contract.md`; reference
  test `tests/e2e/test_golden_path_real_app.rs`). Drives the REAL mcp-server over HTTP `:3001`. The dream-state
  tests currently bypass this via in-process `McpServerApp::from_environment` — the load-bearing purity gap.
- Live stack: `docker compose -f docker-compose.test.yml up -d` (6 services), currently UP and healthy.

### Success Criteria
- #162: the live inline extraction test produces a REAL `.pending` draft (`origin: session_extraction`) from a
  substantive transcript and passes — never by weakening the assertion; contract asserts `candidate_count > 0`.
- #141: a `schema_migrations` table records applied ids; the runner skips already-applied migrations (per-migration
  tx); fresh boot applies + records 001–005, second boot applies none; `cargo test -p infrastructure` green.
- #155: DS-003..007 are migrated off in-process `from_environment` onto the real HTTP harness; DS-004 uses a real
  container kill+restart and real ingest seed (replayed==enqueued, 0 lost/dupes, skills retrievable); DS-006 drives
  the real watcher+extraction loop with convergence asserts; DS-007 warm p95 < 500ms via embedding concurrency/pool;
  DS-003/005 store-count equality hard-asserted; all proven on live infra.

### TDD Contract
- Effective mode: Ralph-driven TDD (no plan override; project default).
- Effective loop: failing test first → minimal real implementation → refactor → post-refactor rerun.
- Required evidence: unit + e2e. e2e MUST run against the live containerized stack (`--ignored` live tests), never
  in-process simulation where the name implies real end-to-end.
- Exceptions: none. NO stubs/fakes/placeholders/hardcoded Passed anywhere outside `#[cfg(test)]` unit tests — fail
  loud instead. (Maintainer standing mandate, re-stressed this session: any placeholder is treated as lying.)

### Constitution Context
`docs/constitution.md` honesty/no-stub baselines apply. Machine-wide + project rule: zero stubs/fakes in production
logic paths or non-unit tests; e2e drives the real app end-to-end against real infra; outcomes derive from real
enforced assertions; poll real conditions, never fixed sleeps.

### Architecture Handoff
- Artifact: plan-derived handoff (no dedicated architecture artifact; boundaries are well-established in the existing
  e2e harness contract + CQRS reference).
- Feature homes: extraction (`crates/session-extractor`), persistence/migrations (`crates/infrastructure`),
  retrieval embedding hot-path (`crates/mcp-server` + `crates/infrastructure` embedding), e2e tests (`tests/e2e`).
- Interfaces as test surfaces: the real mcp-server HTTP `:3001`; `OutboxReconciler::reconcile_once`; the real ingest
  loop; the embedding service.
- Review guidance: `/workflows:review` later must verify NO reintroduced hardcoded Passed, NO in-process seam where
  the name implies real e2e, real fault injection (container kill, not in-process reconstruction), measured counts.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | #162 live extraction→draft proof | fix-item | extraction guarantee is real | completed | 2 | unit-01-162-live-extraction-draft.md |
| 2 | #141 schema_migrations tracking table | fix-item | migration safety at scale | completed | 2 | unit-02-141-schema-migrations-table.md |
| 3 | #155 DS-003..007 harness migration (HTTP) | hardening | kills the in-process purity gap | pending | -- | -- |
| 4 | #155 DS-004 real kill/restart + real seed | hardening | durability proof is real | pending | -- | -- |
| 5 | #155 DS-006 real watcher loop + convergence | hardening | eventual-consistency proof is real | pending | -- | -- |
| 6 | #155 DS-007 embedding concurrency/pool perf | hardening | warm p95 budget met for real | pending | -- | -- |

Note: DS-003/005 hard store-count asserts are folded into Unit 3 (harness migration) since they are confirmation
asserts on those same scenarios. #154 is DEFERRED this session (needs design — see todo). Filename housekeeping
(092–102 → done, 156–161 → resolved) deferred to session wrap-up.

## Learnings Brief
- [extraction] granite4:3b is too small/flaky for reliable extraction (returns empty candidates nondeterministically
  even from concrete content). Use gemma4:e4b + `temperature=0` for deterministic live extraction tests.
- [infra] Ollama optional inference params go in `options` with `skip_serializing_if = Option::is_none` so the wire
  format is unchanged when unset — the pattern for adding inference knobs without changing production behavior.
- [e2e/test-fragility] `infrastructure::health::tests::build_health_checker_always_injects_usage_write_enabled` was a
  sync `#[test]` building a sqlx pool outside a runtime → "requires a Tokio context" under scheduling pressure. FIXED
  in Unit 2 by converting to `#[tokio::test]`.
- [rust/sqlx] `&mut PgConnection: Executor` (i.e. `self.pool.begin()` + `.execute(&mut *tx)`) can tip rustc's trait
  solver into "Send is not general enough" for OTHER crates that hold `&PostgresAdapter` across `.await` (admin). It
  breaks the build in a SEEMINGLY unrelated crate. Prefer pool-executor batches for simple atomic work. ALWAYS run
  `cargo build --workspace` after touching `infrastructure` — a `-p infrastructure` build alone hides downstream breaks.
- [postgres] A multi-statement `raw_sql` simple-query message runs in ONE implicit transaction: atomic, auto-rollback
  on error, and leaves the connection CLEAN. An explicit `BEGIN` whose `COMMIT` isn't reached strands the connection
  in `25P02` aborted-tx. For atomic apply+record without a held Transaction, send both statements in one un-bracketed batch.
