---
unit: "T15a - Live harness factory and roundtrip validation"
unit_number: 2
unit_kind: hardening
serves: "SC-1 (live runtime context injection) + SC-4 (PG-to-Qdrant durability validation)"
status: completed
attempt_count: 1
domains: [backend, testing, docker, e2e]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/15a-live-harness-factory-and-roundtrip-validation.md
session_id: work-2026-05-29-230115
---

## What Was Implemented

Built `build_live_server()` alongside `build_seeded_server()`, extended Docker test topology, seeded skill fixtures, created E2E report infrastructure, and wrote one live roundtrip test.

### Live Server Factory
- `build_live_server()` in `crates/mcp-server/src/lib.rs` wires real `OllamaEmbeddingService`, PostgresGraphSnapshotStore, `PostgresAdapter`, Redis, Qdrant, scope resolvers
- `LiveServerComponents` struct holds `McpServerApp` + infrastructure handles for test lifecycle
- `teardown()` truncates PG tables (cascade), deletes Qdrant points, flushes Redis stream
- `ServerMode::Live` vs `ServerMode::Deterministic` — Deterministic returns Err (caller should use `build_seeded_server()` directly)
- `build_graph_from_pg()` loads `PersistedGraphSkillRecord` from PG via `PostgresGraphSnapshotStore`, embeds via Ollama, converts to `SeededGraph`
- All connection URLs from env vars with timeout gates (5s PG, 3s Qdrant, 3s Redis, 5s Ollama)

### Docker Topology
- Service containers added: `mcp-server`, `graph-builder`, `live-e2e-check` gate
- Named volumes: `test-global-skills`, `test-transcripts`, `test-project-skills`
- mcp-server healthcheck via wget on `/health` endpoint
- All services use env-var-driven connection URLs

### Test Fixtures
- `tests/fixtures/test-skills/global/rust-file-io/SKILL.md` — real Rust file I/O skill (tokio::fs, error handling, buffered reading, tempfile patterns)
- `tests/fixtures/test-skills/global/auth-playbook/SKILL.md` — real auth workflow skill (JWT validation, token refresh, session management, OAuth2 patterns)

### Report Infrastructure
- `tests/e2e/report.rs` — `E2EReport`, `ReportBuilder`, `ReportOutcome`, `ReportSection`, `ReportedAction`, `AssertionResult`, `SideEffect`, `EnvironmentSnapshot`, `ContractAssertion`, `DegradationEvent`, `LatencySample` — all with `#[derive(Serialize)]`

### Live Roundtrip Test
- `tests/e2e/test_live_data_plane_roundtrip.rs` — `#[ignore = "requires live containers"]`
- Seeds skill, compiles context, verifies Ok + skill name in output, verifies duplicate suppression, writes JSON report

### Runner Script
- Extended with `--skip-live` flag, test image builds, service lifecycle, trap-based cleanup

## Files Changed
- `crates/mcp-server/src/lib.rs` — added `ServerMode`, `LiveServerComponents`, `build_live_server()`, `build_graph_from_pg()` (176-390)
- `docker-compose.test.yml` — added mcp-server, graph-builder, live-e2e-check services + 3 named volumes
- `tests/fixtures/test-skills/global/rust-file-io/SKILL.md` — NEW
- `tests/fixtures/test-skills/global/auth-playbook/SKILL.md` — NEW
- `tests/e2e/report.rs` — NEW
- `tests/e2e/test_live_data_plane_roundtrip.rs` — NEW (ignored test)
- `scripts/run-e2e-tests.sh` — extended with builds, service lifecycle, live roundtrip

## Problems Encountered
- `RetrievalOrchestrator` only works with `SeededGraph` (in-memory) — resolved by building `build_graph_from_pg()` that loads from PG into SeededGraph
- `RedisStreamsAdapter.connection()` is private — used separate Redis client for teardown flush

## Patterns Discovered
1. "Live" mode means loading PG data + real embeddings into SeededGraph, not bypassing retrieval pipeline
2. `PostgresGraphSnapshotStore` has `list_graph_snapshot()` returning `Vec<PersistedGraphSkillRecord>`
3. `mcpserver::BuildLiveServerError` wrapper for live server initialization failures

## Test Results
- Unit tests: 33/33 pass (mcp-server integration + all crates)
- Dream-state contract: Compiles
- E2E live test: Compiles + `#[ignore]` — requires live Docker containers
- No regressions in existing tests
- Attempts: 1