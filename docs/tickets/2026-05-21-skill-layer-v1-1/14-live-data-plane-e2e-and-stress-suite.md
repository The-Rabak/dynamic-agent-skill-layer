---
ticket_id: T14
title: Live data-plane E2E and stress suite
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.1, 2.2, 2.4, 3.1, 3.2"
feature_home: tests/
depends_on:
  - T07
  - T11
  - T13
dependency_type: hard
serves:
  - SC-1: full runtime context injection flow is validated against live dependencies
  - SC-4: PG-to-Qdrant durability and replay behavior are validated as one data-plane flow
  - SC-7: degraded semantics stay explicit under dependency loss and recovery
files:
  - crates/mcp-server/src/lib.rs
  - tests/e2e/test_live_data_plane_roundtrip.rs
  - tests/e2e/test_concurrency_stress.rs
  - tests/e2e/test_watcher_churn_reconciliation.rs
  - tests/e2e/test_dream_state_contract.rs
  - tests/e2e/report.rs
  - scripts/run-e2e-tests.sh
  - scripts/run-t02-infrastructure-tests.sh
  - docker-compose.test.yml
test_command: ./scripts/run-e2e-tests.sh
tdd_mode: inherit
---

# Live data-plane E2E and stress suite

## Serves

- SC-1 by validating `compile_context` as a black-box runtime path, not only seeded or isolated test slices.
- SC-4 by proving graph mutations stay durable and replayable from filesystem change through PG and Qdrant consistency.
- SC-7 by verifying dependency outages produce explicit `degraded` behavior and clean recovery semantics.

## Scope

Add a dedicated end-to-end and stress harness that runs against temporary test containers (PostgreSQL, Qdrant, Ollama, Redis) and exercises the live runtime path: watcher/rebuild, outbox relay, retrieval, compile response, extraction, approval, degraded behavior, and recovery.

## Scope Fence

- Do not re-implement existing unit or integration assertions in this ticket; orchestrate full-flow behavior checks.
- Do not introduce distributed transactions or alternate persistence paths; validate the existing outbox and reconciliation contract.
- Keep stress scenarios deterministic and bounded so CI remains repeatable.

## What to Build — Live Test Harness Factory

All current E2E tests use `build_seeded_server()` — an in-memory mock harness that bypasses PostgreSQL, Qdrant, Redis, and Ollama entirely. This ticket adds `build_live_server()` in `crates/mcp-server/src/lib.rs` that wires real infrastructure adapters for use in live-container tests.

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
- **Idempotent setup:** Migrations and Qdrant collection creation must be safe to call repeatedly (used in every test fixture setup).
- **Teardown hooks:** Expose `async fn teardown(self)` that truncates PG tables, deletes Qdrant collection points, and flushes Redis streams so tests don't leak state between runs.
- **Timeout-gated:** Every infrastructure connection uses `tokio::time::timeout` (5s PG, 3s Qdrant, 3s Redis, 5s Ollama) with clear error messages on timeout.
- **Dual-mode support:** The factory should accept an optional `mode` parameter — `Live` (default, real infra) or `Deterministic` (seeded in-memory). This allows the same test file to run both fast seeded passes and slow live verification passes from one entrypoint. Mark live tests with `#[ignore = "requires live containers"]` so `cargo test` runs fast seeded tests and the E2E script runs live tests explicitly.
- **No mock infra:** `build_live_server()` must never fall back to in-memory mocks. If infra is unreachable, it returns an error. Tests that need deterministic behavior use `build_seeded_server()` instead.

### Where the Factory Lives

Add to `crates/mcp-server/src/lib.rs` alongside `build_seeded_server()`. Both functions share the same return type (`McpServerApp`) so existing test code using `McpServerApp` methods (`.compile_context()`, `.extract_session()`) works with either factory without modification.

## What to Build — Test Container Topology

`docker-compose.test.yml` currently only starts infrastructure containers (PG, Redis, Qdrant, Ollama) with a `topology-check` alpine container that verifies connectivity. This ticket extends it to also start the Rust service binaries for live E2E scenarios.

### Service Definitions for docker-compose.test.yml

Add these services below the existing infra definitions, using the same `Dockerfile` created in T11:

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

volumes:
  postgres_data:
  redis_data:
  qdrant_data:
  ollama_data:
  test-global-skills:
  test-transcripts:
  test-project-skills:
```

### Test Fixture Lifecycle

- **Test global skills volume** is pre-seeded from `tests/fixtures/test-skills/global/` at container start (via the graph-builder's initial scan). Contains real SKILL.md files with known content for deterministic assertions.
- **Test project skills volume** starts empty. Each test writes SKILL.md files to it through the graph-builder container, simulating watcher events.
- **Test transcripts volume** is pre-seeded from `tests/fixtures/sample-transcript.jsonl`. Used for `extract_session` live tests.
- No `restart: always` — test containers run once and exit. CI kills and recreates the stack per suite.

### Volume Mount Test Fixtures

Extend `tests/fixtures/` directory:
```
tests/fixtures/
  test-skills/
    project/          # empty — populated by watcher tests
    global/           # pre-seeded global skills for retrieval
      rust-file-io/SKILL.md
      async-tokio/SKILL.md
      auth-playbook/SKILL.md
  sample-transcript.jsonl   # valid Claude Code transcript for extraction tests
```

## What to Build — Full Report Output for Judge Evaluation

Every E2E test must produce a complete, structured, machine-parseable report of all inputs and outputs captured during execution. Reports are written to `tests/e2e/reports/` as timestamped JSON files. The judge consuming these reports must be able to reconstruct every decision point, every state transition, and every response shape without reading source code.

### Report Infrastructure

Add `tests/e2e/report.rs` with:

```rust
use serde::Serialize;
use std::time::Instant;

#[derive(Serialize)]
pub struct E2EReport {
    pub test_name: String,
    pub test_id: String,
    pub started_at: String,               // RFC3339
    pub duration_ms: u64,
    pub outcome: ReportOutcome,
    pub sections: Vec<ReportSection>,      // ordered by execution phase
    pub environment: EnvironmentSnapshot,
    pub contract_assertions: Vec<ContractAssertion>,
    pub degradation_events: Vec<DegradationEvent>,
    pub latency_samples: Vec<LatencySample>,
}

#[derive(Serialize)]
pub struct ReportOutcome {
    pub status: String,   // "pass" | "fail" | "degraded_pass"
    pub reason_code: Option<String>,
    pub failure_detail: Option<String>,
}

#[derive(Serialize)]
pub struct ReportSection {
    pub phase: String,              // "setup" | "watcher_scan" | "rebuild" | ...
    pub actions: Vec<ReportedAction>,
    pub phase_duration_ms: u64,
}

#[derive(Serialize)]
pub struct ReportedAction {
    pub label: String,
    pub input: serde_json::Value,      // full request payload
    pub output: serde_json::Value,     // full response payload
    pub status_code: Option<String>,   // compile_context status, HTTP code, etc.
    pub latency_ms: u64,
    pub assertions: Vec<AssertionResult>,
    pub side_effects: Vec<SideEffect>, // filesystem writes, events, DB rows
}

#[derive(Serialize)]
pub struct AssertionResult {
    pub description: String,
    pub passed: bool,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
}

#[derive(Serialize)]
pub struct SideEffect {
    pub kind: String,      // "event_published" | "file_written" | "file_renamed" |
                           // "pg_row_inserted" | "qdrant_point_upserted" | "cache_invalidated"
    pub timestamp: String, // RFC3339
    pub detail: serde_json::Value,
}

#[derive(Serialize)]
pub struct EnvironmentSnapshot {
    pub ollama_model: String,
    pub ollama_embedding_dim: u32,
    pub qdrant_collection_name: String,
    pub qdrant_point_count: u64,
    pub pg_skill_count: i64,
    pub pg_community_count: i64,
    pub pg_outbox_pending_count: i64,
    pub redis_stream_lengths: std::collections::HashMap<String, i64>,
    pub graph_version: i64,
    pub scope_paths: Vec<String>,
    pub containers_running: Vec<String>,
}

#[derive(Serialize)]
pub struct ContractAssertion {
    pub contract_name: String,             // e.g., "compile_context_result_contract"
    pub contract_rule: String,             // e.g., "status must be ok/no_match/degraded/duplicate_suppressed"
    pub observed_value: serde_json::Value,
    pub passes: bool,
    pub violation_detail: Option<String>,
}

#[derive(Serialize)]
pub struct DegradationEvent {
    pub timestamp: String,
    pub dependency: String,        // "ollama" | "qdrant" | "postgres" | "redis"
    pub status: String,            // "healthy" | "degraded" | "unreachable"
    pub compile_context_status: Option<String>,
    pub reason_code: Option<String>,
    pub recovery_duration_ms: Option<u64>,
}

#[derive(Serialize)]
pub struct LatencySample {
    pub phase: String,
    pub stage: String,             // "embed_prompt" | "qdrant_search" | "pg_cte" | ...
    pub elapsed_ms: u64,
    pub percentile_label: Option<String>,  // "p50" | "p95" | "p99"
}
```

### What Every Report Must Capture

**For `compile_context` flow (roundtrip test):**
- Full `CompileContextRequest` payload (prompt text, session_id, repo_path — never truncated)
- Full `CompileContextResponse` payload (status, reason_code, additional_context in its entirety, health markers, scopes_considered, graph_version, latency_ms)
- Per-stage latency breakdown: embed prompt → Qdrant search (project) → Qdrant search (global) → PG CTE traversal → EQ3 scoring → MMR dedup → RRF fusion → rescue → template compilation → total
- Per-skill detail: skill_id, name, scope, semantic_score, lexical_score, prior, community_boost, eq3_score, rank before MMR, rank after MMR, rank after RRF, selected (boolean), subunits_highlighted, rescue_attached_from
- Suppression state: was_session_previously_compiled, suppression_written, suppression_reason
- Cache behavior: cache_hit (boolean), cache_key, graph_version_at_cache_time
- Scope resolution output: project_scope_paths (list), global_scope_paths (list), scope_resolution_latency_ms, resolution_errors (per scope)

**For `extract_session` flow (extraction test):**
- Full `ExtractSessionRequest` payload (transcript_ref, transcript_inline if provided, session_id, repo_path)
- Full `ExtractSessionResponse` payload (status, job_id, provider)
- Transcript content used (full JSONL — never summarized or truncated)
- Extraction provider used (claude/ollama) with model version
- Full `ExtractionResult` from provider: source_session_id, provider, every `ExtractedSkillCandidate` with name, description, tags, procedures, conventions, assets, confidence
- `.pending` file path, full `.pending` file content (YAML frontmatter + markdown body)
- Lifecycle events emitted: event_type, timestamp, correlation_id, payload
- Background job timing: enqueue_time, start_time, completion_time, total_duration_ms, provider_latency_ms, io_latency_ms

**For watcher/rebuild flow (roundtrip + churn tests):**
- Full `SkillFileChange` list: file_path, change_kind (Created/Modified/Deleted/ApprovedRename), scope_id, content_hash, idempotency_key
- Watcher snapshot before and after: HashMap<PathBuf, (content_hash, mtime, file_size)> — both snapshots in full
- Reconciliation scan output: detected changes, source (watcher/reconciliation), idempotency check results
- Rebuild orchestrator: skills_count, subunits_count, communities_count, graph_version, duration_ms
- Subunit extraction details per skill: rule_extraction_count, ollama_fallback_count, extraction_errors, content_hashes
- HDBSCAN output: cluster_labels (per skill), noise_skills (label -1), cluster_sizes, min_cluster_size, epsilon
- Embedding batch: texts_sent, vectors_received, ollama_batch_latency_ms, any retries
- Outbox relay: events_queued, events_published, events_failed, events_retried, relay_duration_ms
- Qdrant payload per point: point_id, content_hash, vector_dim, scope, node_type, tags
- PG mutation snapshot: skills table (all rows after rebuild), skill_subunits table (all rows), community_skills table (all rows), outbox_events table (all rows for this rebuild correlation_id)
- Audit log entries: all `audit_log` rows from this rebuild with before_snapshot and after_snapshot JSONB

**For degraded/recovery flow:**
- Dependency state timeline: timestamp → dependency_name → status (healthy/degraded/unreachable)
- For each dependency outage: outage_start, outage_end, recovery_duration_ms
- For each `compile_context` call during degraded period: full request, full response (status MUST be "degraded"), reason_code, health markers (per-dependency status), whether partial context was returned
- For each recovery point: first healthy compile_context response after recovery, latency comparison (degraded → healthy), any retries needed
- Circuit breaker state transitions: dependency → open/closed/half_open → timestamp → consecutive_failures

**For concurrency stress flow:**
- Per-request detail: request_id, session_id, prompt, start_time, end_time, latency_ms, status, reason_code, skills_returned_count
- Aggregate statistics: total_requests, ok_count, no_match_count, degraded_count, duplicate_suppressed_count, error_count, p50_latency_ms, p95_latency_ms, p99_latency_ms, max_latency_ms, min_latency_ms
- Contention evidence: semaphore_wait_times, pool_acquire_wait_times, any timeouts
- Resource usage: peak_memory_mb (if measurable), Ollama queue_depth_samples, PG active_connections_samples, Redis stream_length_samples
- No silent failures: every request that didn't return `ok` must have a reason_code. Report must assert zero requests with empty/missing reason_code on non-ok status.

**For cross-test environment consistency:**
- Full `EnvironmentSnapshot` captured before test execution and after test teardown
- Container health status per service: container_name, health_status, uptime_seconds
- Ollama model verification: model_name (`nomic-embed-text`), model_digest, embedding_dim (must be 768)
- Qdrant collection state: collection_name, vectors_count, indexed_vectors_count, segments_count, disk_usage_bytes
- PG schema state: migration_version, table_row_counts (all tables), index_list, constraint_list
- Redis state: connected_clients, used_memory_bytes, stream_names, stream_lengths
- Filesystem state: scope_directories (paths), file_count_per_scope, total_skill_files, pending_count, retired_count

### Report Output Location and Format

- Each test writes its report to `tests/e2e/reports/{test_name}__{timestamp}.json`
- The E2E runner script aggregates all reports from a run into `tests/e2e/reports/run__{timestamp}.json` — a JSON array of all individual `E2EReport` objects
- Reports are human-readable JSON (pretty-printed, `serde_json::to_string_pretty`). No binary formats, no compression.
- The summary report includes a top-level `run_summary` object: `{ total_tests, passed, failed, degraded_passed, total_duration_ms, start_time, end_time, container_versions }`
- Failed assertion detail includes the exact comparator: `assert_eq!(expected, actual, "assertion description")` materialized with both values inline
- Every `serde_json::Value` field in the report carries the full payload — never truncated, never summarized. If a prompt is 4KB, the report contains the full 4KB.

### Judge Contract

The report format is the contract between this test suite and any downstream judge (human or automated). A judge must be able to answer these questions from the report alone, without reading test source code:

1. Did every `compile_context` call return one of the four legal statuses (`ok`, `no_match`, `degraded`, `duplicate_suppressed`)?
2. Did any `degraded` call produce `duplicate_suppressed` on a subsequent healthy retry?
3. Did `graph.rebuilt` emit only after outbox drain (verified by `pg_outbox_pending_count == 0` AND `qdrant_point_count == pg_skill_count`)?
4. Did every non-`ok` status carry a non-empty `reason_code`?
5. Did any extraction produce a `.pending` file without emitting both `extraction_requested` AND `extraction.completed`?
6. Did any test observe a `graph_version` mismatch between what `compile_context` reported and what the PG `rebuild_locks` table shows?
7. Is the invalidation ordering preserved: PG commit → outbox drain → graph_version bump → `graph.rebuilt` → cache miss on next `compile_context`?
8. Did every concurrency stress request complete within the 500ms p50 / 800ms p95 budget?
9. Did any watcher churn event get silently dropped (present in filesystem snapshot, absent from `SkillFileChange` list)?
10. Did environment snapshots before and after differ in row counts, vector counts, or file counts beyond expected test-induced mutations?

These 10 questions form the judge's minimum evaluation checklist. Any report that cannot answer all 10 conclusively is a failing report regardless of test assertion status.

### Implementation Constraint

The `ReportSection` / `ReportedAction` / `SideEffect` collection must be embedded directly in test execution, not retrofitted from logs. Each test function must push actions to a `ReportBuilder` as they execute — the report IS the test's execution trace. No post-hoc log scraping. If a test assertion fails, the report must contain the full state at the point of failure so a judge can determine whether the failure is a system bug or a test environment issue.

## Acceptance Criteria

- A roundtrip E2E test validates: skill filesystem change -> watcher/rebuild -> outbox drain -> retrieval visibility in `compile_context`.
- A session extraction E2E test validates inline/live extraction output goes to `.pending` and lifecycle events are emitted with no silent failure path.
- A degraded/recovery E2E test validates explicit reason-coded `degraded` results during dependency loss (for example Ollama or Qdrant unavailable) and healthy recovery after dependency restore.
- A watcher churn/reconciliation test validates rename/delete storms remain idempotent and converge to correct graph state.
- A bounded concurrency stress test validates parallel `compile_context` calls during rebuild/extraction activity with recorded latency/error evidence and no silent failure modes.
- A helper runner script executes realistic E2E suites in one command and can optionally execute ignored dream-state contract tests.
- `build_live_server()` factory exists in `crates/mcp-server/src/lib.rs` alongside `build_seeded_server()`, wiring real `OllamaEmbeddingService`, `PostgresGraphWriteCoordinator`, `QdrantVectorStore`, `RedisStreamPublisher`, `PostgresAdapter`, and live scope resolvers.
- `build_live_server()` accepts a `mode` parameter (`Live` or `Deterministic`). Live tests marked `#[ignore = "requires live containers"]` run only in the E2E script. Seeded tests remain fast for `cargo test`.
- `build_live_server()` setup is idempotent — PG migrations and Qdrant collection creation safe to call repeatedly.
- `build_live_server().teardown()` truncates tables, deletes Qdrant points, flushes Redis streams so tests don't leak state.
- `docker-compose.test.yml` includes service definitions for mcp-server and graph-builder as test containers, with a `live-e2e-check` gate that verifies the full topology before tests run.
- Test fixture volumes (`test-global-skills`, `test-transcripts`) are pre-seeded with known content for deterministic live assertions.
- `./scripts/run-e2e-tests.sh` builds test images, starts the test topology, runs live E2E tests, and tears down.
- Every E2E test writes a complete, machine-parseable JSON report to `tests/e2e/reports/{test_name}__{timestamp}.json` containing full inputs, full outputs, all assertions, all side effects, all latency samples, all degradation events, and environment snapshots — nothing truncated, nothing summarized.
- The aggregated run report at `tests/e2e/reports/run__{timestamp}.json` contains all individual test reports plus a `run_summary` with totals, pass/fail counts, and timing.
- A judge consuming the reports can answer all 10 minimum evaluation questions (result status legality, outbox ordering, reason-code completeness, extraction event completeness, graph_version consistency, invalidation ordering, latency budgets, watcher completeness, state leak detection) without reading test source code.
- Report fields use `serde_json::Value` for all payloads — never `String` or truncated representations. Full prompt text, full compiled context markdown, full `.pending` file content, full JSONL transcript content, full PG row snapshots, full Qdrant payloads, full event envelopes.
- Failed assertions include expected value, actual value, and assertion description inline in the report.
- `tests/e2e/report.rs` contains the `E2EReport` and all sub-struct types (`ReportSection`, `ReportedAction`, `AssertionResult`, `SideEffect`, `EnvironmentSnapshot`, `ContractAssertion`, `DegradationEvent`, `LatencySample`) with `#[derive(Serialize)]`.
- Reports are collected by direct embedding in test execution (a `ReportBuilder` passed through each test's action pipeline), not by post-hoc log scraping.

## Shared / Global Notes

- This ticket validates frozen contracts across crates; it does not redefine ownership or interfaces.
- The invalidation order (`PG commit -> outbox drain -> graph_version visibility -> graph.rebuilt`) remains canonical and must be asserted by test evidence.
- Degraded vs healthy-empty output remains a non-negotiable contract during failure tests.

## Local Context

WHY link: the autonomous loop is only trustworthy if the complete live path behaves correctly under realistic dependency conditions, not only in seeded or partially mocked slices.

This ticket intentionally couples black-box flow and stress coverage because both answer the same operational question: can the system sustain real runtime pressure without violating data integrity or response semantics? Keep focus on:

- live dependency containers and fixture lifecycles,
- cross-service event and invalidation sequencing,
- explicit failure and recovery semantics visible at MCP boundaries.

This ticket also owns the `build_live_server()` harness factory because no other ticket creates the bridge between live infrastructure adapters (built in T02, hardened in T07/T11) and the test surface (seeded tests built in T03-T06). Without `build_live_server()`, live E2E tests must manually wire 6+ adapters per test file — a DRY violation that invites drift between test behavior and runtime behavior. The factory lives in `mcp-server/src/lib.rs` alongside `build_seeded_server()` so both harnesses share the same `McpServerApp` interface and a test can switch between seeded and live mode by swapping one constructor call.

Unknowns: stress workload sizes may need tuning per CI capacity, but coverage targets and contract assertions are fixed. Docker build time for the test image may require caching the cargo-chef dependency layer in CI.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.1, 2.2, 2.4, 3.1, 3.2`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`, `## Context Tiers`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#canonical-v11-contracts`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Full roundtrip E2E and stress scenarios stay together because both validate the same data-plane correctness boundary under different load/failure profiles.
- Splitting these surfaces into separate ownership would risk passing happy-path E2E while missing pressure-induced contract violations.
