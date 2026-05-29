---
ticket_id: T15a
title: Live harness factory and roundtrip validation
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.1, 2.2, 2.4"
feature_home: crates/mcp-server/
depends_on:
  - T07
  - T11
  - T13
dependency_type: hard
serves:
  - SC-1: full runtime context injection flow is validated against live dependencies
  - SC-4: PG-to-Qdrant durability and replay behavior are validated
files:
  - crates/mcp-server/src/lib.rs
  - docker-compose.test.yml
  - tests/fixtures/test-skills/global/rust-file-io/SKILL.md
  - tests/fixtures/test-skills/global/async-tokio/SKILL.md
  - tests/fixtures/test-skills/global/auth-playbook/SKILL.md
  - tests/fixtures/sample-transcript.jsonl
  - tests/e2e/report.rs
  - tests/e2e/test_live_data_plane_roundtrip.rs
  - scripts/run-e2e-tests.sh
test_command: ./scripts/run-e2e-tests.sh
tdd_mode: inherit
---
# Live harness factory and roundtrip validation

## Serves

- SC-1 by creating the factory that wires real infrastructure into `McpServerApp` — the bridge between live adapters and the test surface.
- SC-4 by proving the factory can drive a full filesystem-to-retrieval roundtrip against real PG and Qdrant before any stress or failure tests depend on it.

## Scope

Add `build_live_server()` to `crates/mcp-server/src/lib.rs` alongside `build_seeded_server()`, extend `docker-compose.test.yml` with service containers and test fixture volumes, seed test fixtures with known SKILL.md content, build the report infrastructure (`tests/e2e/report.rs`), and add one live roundtrip E2E test to prove the harness works end-to-end.

## Scope Fence

- Do not build extraction, degradation/recovery, concurrency stress, watcher churn, or dream-state contract tests — those belong to T15b.
- Do not implement the 10-question judge checklist or report aggregation — those belong to T15b.
- Do not change any domain traits or adapter implementations; wire what already exists in `crates/infrastructure/`.
- Do not re-implement existing unit or integration assertions; the roundtrip test validates full-flow behavior only.

## What to Build — `build_live_server()` Factory

Add to `crates/mcp-server/src/lib.rs` alongside `build_seeded_server()`. Both functions share the same return type (`McpServerApp`) so existing test code using `McpServerApp` methods works with either factory without modification.

### Factory Function Signature

```rust
use std::sync::Arc;
use infrastructure::{
    OllamaEmbeddingService, PostgresGraphWriteCoordinator,
    QdrantVectorStore, RedisStreamPublisher,
    PostgresAdapter, RebuildCoordinator,
};
use retrieval::RetrievalOrchestrator;

pub struct LiveServerComponents {
    pub app: McpServerApp,
    pub embedding_service: Arc<OllamaEmbeddingService>,
    pub write_coordinator: Arc<PostgresGraphWriteCoordinator>,
    pub vector_store: Arc<QdrantVectorStore>,
    pub event_publisher: Arc<RedisStreamPublisher>,
    pub pg_adapter: PostgresAdapter,
    pub rebuild_coordinator: RebuildCoordinator,
}

pub async fn build_live_server(
    retrieval_config: RetrievalConfig,
) -> Result<LiveServerComponents, Box<dyn std::error::Error>> {
    // 1. Connect to all live infrastructure via env vars
    // 2. Run PG migrations (idempotent)
    // 3. Create Qdrant collection with payload indexes (idempotent)
    // 4. Wire OllamaEmbeddingService (real Ollama, 768-dim nomic-embed-text)
    // 5. Wire PostgresGraphWriteCoordinator (real PG outbox)
    // 6. Wire RedisStreamPublisher (real Redis streams)
    // 7. Wire ScopeResolvers (GitRootProjectResolver + EnvPathGlobalResolver)
    // 8. Construct RetrievalOrchestrator with live connectors
    // 9. Construct McpServerApp with live retriever
    // 10. Return all components for test lifecycle management
}
```

### Key Design Constraints

- **Environment-driven:** All connection URLs, credentials, and paths come from Docker Compose environment variables — never hardcoded.
- **Idempotent setup:** Migrations and Qdrant collection creation must be safe to call repeatedly across test fixtures.
- **Teardown hooks:** Expose `async fn teardown(self)` that truncates PG tables, deletes Qdrant collection points, and flushes Redis streams.
- **Timeout-gated:** Every infrastructure connection uses `tokio::time::timeout` (5s PG, 3s Qdrant, 3s Redis, 5s Ollama) with clear error messages.
- **Dual-mode support:** The factory accepts an optional `mode` parameter — `Live` (default, real infra) or `Deterministic` (seeded in-memory). Mark live tests with `#[ignore = "requires live containers"]` so `cargo test` runs fast seeded tests and the E2E script runs live tests explicitly.
- **No mock infra:** If infra is unreachable, return an error. Tests needing deterministic behavior use `build_seeded_server()`.

## What to Build — Test Container Topology

Extend `docker-compose.test.yml` with service containers and test fixture volumes. The file currently has infrastructure containers only (postgres, redis, qdrant, ollama, topology-check, ollama-model-check).

### Service Definitions

```yaml
  mcp-server:
    build:
      context: ..
      dockerfile: Dockerfile
      args:
        BIN: mcp-server
    image: skill-layer/mcp-server:test
    container_name: skill-layer-test-mcp-server
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
      qdrant:
        condition: service_started
      ollama:
        condition: service_started
    ports:
      - "13001:3001"
    environment:
      - RUST_LOG=debug
      - DATABASE_URL=postgres://skill_layer:skill_layer@postgres:5432/skill_layer_test
      - REDIS_URL=redis://redis:6379
      - QDRANT_URL=http://qdrant:6334
      - OLLAMA_URL=http://ollama:11434
      - SKILL_GLOBAL_PATHS=/skills/global
      - SKILL_GLOBAL_ALLOWED_ROOTS=/skills/global
      - CLAUDE_TRANSCRIPT_ROOT=/transcripts
    volumes:
      - test-global-skills:/skills/global:ro
      - test-transcripts:/transcripts:ro
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://localhost:3001/health"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 15s
    restart: "no"

  graph-builder:
    build:
      context: ..
      dockerfile: Dockerfile
      args:
        BIN: graph-builder
    image: skill-layer/graph-builder:test
    container_name: skill-layer-test-graph-builder
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
      qdrant:
        condition: service_started
      ollama:
        condition: service_started
    environment:
      - RUST_LOG=debug
      - DATABASE_URL=postgres://skill_layer:skill_layer@postgres:5432/skill_layer_test
      - REDIS_URL=redis://redis:6379
      - QDRANT_URL=http://qdrant:6334
      - OLLAMA_URL=http://ollama:11434
      - GRAPH_BUILDER_PROJECT_ROOT=/skills/project
      - GRAPH_BUILDER_GLOBAL_ROOT=/skills/global
    volumes:
      - test-project-skills:/skills/project
      - test-global-skills:/skills/global:ro
    restart: "no"

  live-e2e-check:
    build:
      context: ..
      dockerfile: Dockerfile
      args:
        BIN: mcp-server
    image: skill-layer/mcp-server:test
    container_name: skill-layer-test-e2e-check
    depends_on:
      mcp-server:
        condition: service_healthy
      graph-builder:
        condition: service_started
    entrypoint:
      - sh
      - -c
      - |
        wget -qO- http://mcp-server:3001/health | grep -q healthy
        echo "live E2E topology ready"
    restart: "no"
```

Add named volumes:

```yaml
volumes:
  postgres_data:
  redis_data:
  qdrant_data:
  ollama_data:
    external: true
  test-global-skills:
  test-transcripts:
  test-project-skills:
```

### Test Fixture Lifecycle

- **Test global skills volume** is pre-seeded from `tests/fixtures/test-skills/global/` at container start via graph-builder's initial scan.
- **Test project skills volume** starts empty. Each test writes SKILL.md files to it through the graph-builder container.
- **Test transcripts volume** is pre-seeded from `tests/fixtures/sample-transcript.jsonl`.
- No `restart: always` — containers run once and exit.

## What to Build — Test Fixtures

Extend `tests/fixtures/test-skills/global/` with three pre-seeded global skills for deterministic live assertions:

```
tests/fixtures/test-skills/global/
  rust-file-io/SKILL.md     # Real Rust file handling patterns
  async-tokio/SKILL.md       # Async Tokio conventions
  auth-playbook/SKILL.md     # Auth workflow patterns
```

Each SKILL.md must have real, high-quality content (not stubs) so live retrieval returns meaningful context. The file `async-tokio/SKILL.md` already exists; create the other two. Content should contain enough domain-specific tokens to generate distinct embedding vectors against `nomic-embed-text:768d`.

The existing `tests/fixtures/sample-transcript.jsonl` is sufficient for extraction tests.

## What to Build — Report Infrastructure

Add `tests/e2e/report.rs` with the full type hierarchy:

```rust
pub struct E2EReport {
    pub test_name: String,
    pub test_id: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub outcome: ReportOutcome,
    pub sections: Vec<ReportSection>,
    pub environment: EnvironmentSnapshot,
    pub contract_assertions: Vec<ContractAssertion>,
    pub degradation_events: Vec<DegradationEvent>,
    pub latency_samples: Vec<LatencySample>,
}
```

All sub-types with `#[derive(Serialize)]`: `ReportOutcome`, `ReportSection`, `ReportedAction`, `AssertionResult`, `SideEffect`, `EnvironmentSnapshot`, `ContractAssertion`, `DegradationEvent`, `LatencySample`. A `ReportBuilder` that embeds directly in test execution — each test function pushes actions as they execute. Reports must carry full payloads via `serde_json::Value`, never truncated.

## What to Build — Live Roundtrip Test

Add one live roundtrip test in `tests/e2e/test_live_data_plane_roundtrip.rs`:

1. Connects to live containers via `build_live_server()`
2. Writes a project-scoped SKILL.md to the test project skills volume
3. Triggers a graph rebuild through the graph-builder container
4. Waits for outbox drain and Qdrant consistency
5. Calls `compile_context` with a prompt related to the seeded skill
6. Asserts `CompileContextStatus::Ok` and verifies context content contains the seeded skill name
7. Calls again with same session_id → asserts `DuplicateSuppressed`
8. Produces a full `E2EReport` JSON to `tests/e2e/reports/{test_name}__{timestamp}.json`

Test is marked `#[ignore = "requires live containers"]` so `cargo test` skips it. The E2E runner script invokes explicitly.

## What to Build — Runner Script Updates

Extend `scripts/run-e2e-tests.sh`:
- Build test images (`skill-layer/mcp-server:test`, `skill-layer/graph-builder:test`) via `docker compose build`
- Start the full test topology including service containers
- Run `live-e2e-check` as a topology gate before tests
- Run live roundtrip test via `cargo test --test test_live_data_plane_roundtrip -- --ignored`
- Teardown includes both infra and service containers
- Preserve existing `--skip-infra` and `--include-dream` flags

## Acceptance Criteria

- `build_live_server()` exists in `crates/mcp-server/src/lib.rs` alongside `build_seeded_server()`, wiring real `OllamaEmbeddingService`, `PostgresGraphWriteCoordinator`, `QdrantVectorStore`, `RedisStreamPublisher`, `PostgresAdapter`, and live scope resolvers.
- `build_live_server()` accepts a `mode` parameter (`Live` or `Deterministic`). Live tests marked `#[ignore = "requires live containers"]`.
- `build_live_server()` setup is idempotent — PG migrations and Qdrant collection creation safe to call repeatedly.
- `build_live_server().teardown()` truncates tables, deletes Qdrant points, flushes Redis streams.
- `docker-compose.test.yml` includes service definitions for `mcp-server` and `graph-builder`, and a `live-e2e-check` gate.
- Test fixture volumes (`test-global-skills`, `test-transcripts`) are pre-seeded with known content.
- `tests/fixtures/test-skills/global/` contains three complete SKILL.md files with real content.
- `tests/e2e/report.rs` contains all report types with `#[derive(Serialize)]`.
- One live roundtrip test passes: filesystem change → rebuild → outbox drain → `compile_context` `Ok` → `DuplicateSuppressed`.
- The live roundtrip test writes a complete JSON report to `tests/e2e/reports/`.
- `./scripts/run-e2e-tests.sh` builds test images, starts the full topology, runs the live roundtrip test, and tears down.

## Shared / Global Notes

- This ticket creates the bridge between live infrastructure adapters (built in T02, hardened in T07/T11) and the test surface. Without `build_live_server()`, every test must manually wire 6+ adapters.
- The factory lives alongside `build_seeded_server()` so tests can switch modes by swapping one constructor call.
- Docker build time may require caching the cargo-chef dependency layer in CI.
- The invalidation order (`PG commit → outbox drain → graph_version visibility → graph.rebuilt`) remains canonical and must be asserted by the roundtrip test.

## Local Context

WHY: the autonomous loop is only trustworthy if the complete live path behaves correctly under realistic dependency conditions. This ticket delivers the gating infrastructure — factory, topology, fixtures, reports — that T15b needs to build the full stress/resilience suite.

This ticket intentionally separates harness creation from test suite expansion because:
1. The harness + topology + report system is independently shippable — you get a working live test harness with a smoke test before building the full suite.
2. T15b's stress/resilience tests are coverage multipliers that consume the harness; they can be developed in parallel once this ships.
3. The cross-cutting invariants (invalidation ordering, reason-code completeness, outbox ordering) are enforced by the shared report infrastructure, not by test colocation.

Unknowns: Ollama model pull time for `nomic-embed-text` in CI may require pre-warming the `ollama_data` volume. Docker build caching strategy for multi-stage `cargo-chef` images may need CI tuning.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.1, 2.2, 2.4`
- Original parent ticket (split from): `docs/tickets/2026-05-21-skill-layer-v1-1/15-live-data-plane-e2e-and-stress-suite.md`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#canonical-v11-contracts`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- T02: `docs/tickets/2026-05-21-skill-layer-v1-1/02-infrastructure-adapters-and-schema.md`
- T07: `docs/tickets/2026-05-21-skill-layer-v1-1/07-outbox-relay-and-reconciliation.md`
- T11: `docs/tickets/2026-05-21-skill-layer-v1-1/11-graceful-degrade-and-health-checks.md`

## Coupling Notes

- `build_live_server()`, Docker topology, test fixtures, and report infrastructure ship together because they form a single gating dependency: without any one of them, no live test can run. The roundtrip test is included as the minimal proof that the harness isn't dead on arrival.
- Stress, resilience, churn, extraction live, and dream-state tests are intentionally deferred to T15b — they consume the harness but are independently valuable coverage additions.
- The report infrastructure types (`E2EReport` etc.) are defined here because they are the contract surface both T15a and T15b consume. T15b adds report aggregation and judge validation on top of this foundation.