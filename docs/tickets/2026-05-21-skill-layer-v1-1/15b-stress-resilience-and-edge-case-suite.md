---
ticket_id: T15b
title: Stress, resilience, and edge-case suite
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.1, 3.2"
feature_home: tests/
depends_on:
  - T15a
dependency_type: hard
serves:
  - SC-1: full runtime context injection flow validated under realistic load and failure
  - SC-4: PG-to-Qdrant durability and replay validated as one data-plane flow under stress
  - SC-7: degraded semantics stay explicit under dependency loss and recovery
files:
  - tests/e2e/test_live_data_plane_roundtrip.rs
  - tests/e2e/test_concurrency_stress.rs
  - tests/e2e/test_watcher_churn_reconciliation.rs
  - tests/e2e/test_dream_state_contract.rs
  - tests/e2e/report.rs
  - scripts/run-e2e-tests.sh
test_command: ./scripts/run-e2e-tests.sh --include-dream
tdd_mode: inherit
---

# Stress, resilience, and edge-case suite

## Serves

- SC-1 by proving `compile_context` operates correctly under concurrent load and during active rebuild/extraction activity.
- SC-4 by verifying graph mutations stay durable and replayable under watcher churn and reconciliation pressure.
- SC-7 by verifying dependency outages produce explicit `degraded` behavior and clean recovery semantics.

## Scope

Using the `build_live_server()` harness from T15a, add extraction E2E tests, degraded/recovery E2E tests, watcher churn/reconciliation tests, concurrency stress tests, report aggregation, and judge contract validation. Also evolve existing dream-state contract stubs toward live implementations where the harness enables them.

## Scope Fence

- Do not modify `build_live_server()` or Docker Compose topology — those are frozen by T15a.
- Do not re-implement existing unit or integration assertions; these tests validate full-flow behavior under realistic conditions.
- Do not introduce distributed transactions or alternate persistence paths; validate the existing outbox and reconciliation contract.
- Keep stress scenarios deterministic and bounded so CI remains repeatable.
- Dream-state contracts that depend on capabilities not yet built (shadow deployment, policy-native governance, counterfactual explanations, etc.) remain `#[ignore]` stubs — only promote those that the T15a harness makes testable.

## What to Build — Extraction Live E2E Test

Add to `tests/e2e/test_live_data_plane_roundtrip.rs`:

### `extract_session_live_inline_payload_writes_pending_and_emits_completion_events`

- Builds live server via `build_live_server()`
- Sends `ExtractSessionRequest` with inline JSONL transcript
- Asserts `status == "processing"` and `provider` is set
- Waits for `extraction.completed` lifecycle events
- Verifies `.pending` file written to global skills directory
- Asserts frontmatter contains `origin: session_extraction`
- Produces full report capturing: request payload, response payload, transcript content, `.pending` file content, lifecycle events, job timing

### `extract_session_live_ref_payload_loads_from_transcript_volume`

- Builds live server with transcript volume mounted
- Sends `ExtractSessionRequest` with `transcript_ref` pointing to pre-seeded `sample-transcript.jsonl`
- Asserts transcript loaded from volume and extraction produces a `.pending` draft
- Verifies no silent failure path — every non-`processing` status has a reason code

## What to Build — Degraded/Recovery E2E Test

Add to `tests/e2e/test_live_data_plane_roundtrip.rs`:

### `degraded_and_recovery_cycle_preserves_reason_codes_and_recovers_cleanly`

- Builds live server via `build_live_server()`
- Calls `compile_context` in healthy state to establish baseline (status `Ok` or `NoMatch`)
- Stops one dependency container (e.g., Qdrant via `docker stop`)
- Calls `compile_context` → asserts status `Degraded` with non-empty `reason_code`
- Calls again with another dependency down (Ollama) → asserts `Degraded` with different reason code
- Restores each dependency (via `docker start`)
- Calls `compile_context` → asserts recovered to `Ok` or `NoMatch`
- Reports capture full dependency state timeline: timestamps, dependency name, status (healthy/degraded/unreachable), outage duration, recovery duration, compile_context request/response per phase
- Circuit breaker state transitions recorded: dependency → open/closed/half_open → timestamp → consecutive_failures if observable
- Verifies no `DuplicateSuppressed` during degraded period (session suppression must not mask degradation)

## What to Build — Watcher Churn/Reconciliation Test

Evolve `tests/e2e/test_watcher_churn_reconciliation.rs` to use live PG + Qdrant (currently uses `InMemoryDurableGraphState`):

### `watcher_churn_and_reconciliation_converges_to_correct_graph_state_under_live_pg_qdrant`

- Sets up live PG adapter and Qdrant adapter from env vars
- Creates a sandbox scope directory with fixture skills
- Creates 20+ skills via pending→approve rename cycles
- Modifies every other skill
- Deletes 8 skills
- Runs reconciliation scan twice → second run produces empty results (idempotent)
- Rebuilds through `GraphRebuildOrchestrator` with `PostgresDurableGraphState` (real PG + Qdrant)
- Verifies:
  - `graph_version` increments
  - PG skills table contains only active SKILL.md files
  - Qdrant point count matches PG skill count
  - `graph.rebuilt` event emitted
  - Audit trail captures ApprovedRename and Deleted change types
  - Outbox drain sequence: PG commit → outbox drain → graph_version bump → `graph.rebuilt`
- Produces full report with: watcher snapshots before/after, SkillFileChange list, reconciliation output, PG row snapshots, Qdrant point count, audit log entries

## What to Build — Concurrency Stress Test

Evolve `tests/e2e/test_concurrency_stress.rs` to use live infrastructure (currently seeded):

### `compile_context_parallel_burst_under_live_infra_stays_within_contract_statuses`

- Builds live server via `build_live_server()`
- Seeds the test project scope with known SKILL.md files
- Triggers a graph rebuild and waits for Qdrant consistency
- Fires 96 parallel `compile_context` calls (24 sessions × 4 calls each)
- Asserts all responses return one of: `Ok`, `NoMatch`, `DuplicateSuppressed`
- Asserts at least one `Ok` and at least one `NoMatch`
- Follow-up calls for each session assert `DuplicateSuppressed`
- Records per-request latency (p50/p95/p99/max/min)
- Records aggregate statistics: total, ok_count, no_match_count, degraded_count, duplicate_suppressed_count, error_count
- Verifies zero requests with empty/missing reason_code on non-ok status

### `compile_context_and_rebuild_concurrent_activity_stays_consistent`

- Builds live server
- Spawns background rebuild activity (skill create → watcher → rebuild every ~500ms)
- Concurrently fires 48 `compile_context` calls
- Asserts no responses with missing reason codes
- Asserts `graph_version` in responses is monotonic (never decreases across calls)
- Asserts no cache-hit on stale graph_version

### `extract_session_parallel_burst_all_jobs_complete_and_drafts_persist`

- Builds live server with real extraction capabilities
- Fires 32 parallel `extract_session` requests
- Asserts all return `status == "processing"` with unique job IDs
- Waits for all `extraction.completed` events
- Verifies all `.pending` files written with canonical file name
- Verifies zero `extraction.failed` events
- Records job timing: enqueue_time, start_time, completion_time, provider_latency_ms, io_latency_ms

## What to Build — Dream-State Contract Implementation

Promote dream-state contract stubs in `tests/e2e/test_dream_state_contract.rs` that the T15a harness enables:

### Promote to live (remove `#[ignore]` and implement):
- **DS-003** (`dependency_chaos_matrix_preserves_degraded_semantics_and_fast_recovery`): Implement the full dependency chaos matrix using controlled Docker container stop/start
- **DS-004** (`outbox_backlog_replays_without_data_loss_after_multi_restart_sequence`): Implement crash/restart cycles with non-empty outbox backlogs
- **DS-005** (`qdrant_pg_drift_detection_and_reconciliation_closes_all_gaps`): Implement drift injection and repair cycle with the reconciliation worker
- **DS-006** (`sustained_watcher_and_extraction_saturation_keeps_eventual_consistency`): Implement continuous churn + extraction burst convergence test
- **DS-007** (`high_qps_compile_context_load_meets_p95_and_error_budget_targets`): Implement sustained mixed-query load with latency budget enforcement (p50 < 500ms, p95 < 800ms)

### Keep as stubs (capabilities not yet built):
- DS-001, DS-002, DS-008 through DS-024 remain `#[ignore]`

## What to Build — Report Aggregation and Judge Contract

Extend `tests/e2e/report.rs` (from T15a) and `scripts/run-e2e-tests.sh`:

### Report Aggregation

- After all tests complete, aggregate individual `E2EReport` JSON files into `tests/e2e/reports/run__{timestamp}.json` — a JSON array of all reports
- Include a top-level `run_summary` object: `{ total_tests, passed, failed, degraded_passed, total_duration_ms, start_time, end_time, container_versions }`

### Judge Contract Validation (10 Questions)

At the end of the E2E runner, validate that the aggregated reports can conclusively answer all 10 judge questions:

1. Did every `compile_context` call return one of the four legal statuses?
2. Did any `degraded` call produce `duplicate_suppressed` on a subsequent healthy retry?
3. Did `graph.rebuilt` emit only after outbox drain (verified by `pg_outbox_pending_count == 0` AND `qdrant_point_count == pg_skill_count`)?
4. Did every non-`ok` status carry a non-empty `reason_code`?
5. Did any extraction produce a `.pending` file without emitting both `extraction_requested` AND `extraction.completed`?
6. Did any test observe a `graph_version` mismatch between `compile_context` reported version and PG `rebuild_locks` table?
7. Is the invalidation ordering preserved: PG commit → outbox drain → graph_version bump → `graph.rebuilt` → cache miss on next `compile_context`?
8. Did every concurrency stress request complete within the 500ms p50 / 800ms p95 budget?
9. Did any watcher churn event get silently dropped (present in filesystem snapshot, absent from `SkillFileChange` list)?
10. Did environment snapshots before and after differ in row counts, vector counts, or file counts beyond expected test-induced mutations?

Reports that cannot answer all 10 are failing regardless of individual test assertion status. Output a `judge_evaluation.json` alongside the run report with pass/fail per question and supporting evidence citations.

## Acceptance Criteria

- A live extraction E2E test validates: inline transcript → `.pending` → `extraction.completed` event with no silent failure path.
- A degraded/recovery E2E test validates: explicit reason-coded `degraded` during dependency loss, healthy recovery after dependency restore, no session suppression masking during degraded period.
- A watcher churn/reconciliation test validates: rename/delete storms converge to correct graph state under live PG + Qdrant, idempotent reconciliation, correct outbox ordering.
- Bounded concurrency stress tests validate: parallel `compile_context` stays within contract statuses, concurrent rebuild + retrieval stays consistent, parallel extraction completes all jobs.
- Dream-state contracts DS-003 through DS-007 are implemented and pass (not ignored).
- `./scripts/run-e2e-tests.sh --include-dream` executes the full live suite, aggregates reports, runs judge validation, and tears down.
- Aggregated run report at `tests/e2e/reports/run__{timestamp}.json` contains all individual reports plus `run_summary`.
- `judge_evaluation.json` confirms all 10 judge questions are conclusively answerable from the reports.
- Every E2E test writes a complete JSON report with full inputs, outputs, assertions, side effects, latency samples, degradation events, and environment snapshots — nothing truncated.
- Reports use `serde_json::Value` for all payloads, never `String` or truncated representations.
- Failed assertions include expected value, actual value, and assertion description inline.
- Reports are collected by direct embedding in test execution (via `ReportBuilder` from T15a), not post-hoc log scraping.

## Shared / Global Notes

- This ticket validates frozen contracts across crates; it does not redefine ownership or interfaces.
- The invalidation order remains canonical and must be asserted by all test evidence.
- Degraded vs healthy-empty output remains a non-negotiable contract during failure tests.
- The report format is the contract between this test suite and any downstream judge (human or automated).

## Local Context

WHY: the autonomous loop is only trustworthy if the complete live path behaves correctly under realistic dependency conditions, load, and failure — not only in happy-path or partially mocked slices.

This ticket consumes the harness, topology, fixtures, and report infrastructure delivered by T15a. It focuses on coverage multiplication: extraction, degradation, churn, concurrency, and dream-state contracts that each validate a different dimension of the data-plane correctness boundary.

The original coupling concern ("splitting risks passing happy-path while missing pressure-induced violations") is addressed because:
1. T15a established the report infrastructure as the cross-cutting correctness boundary — contract assertions, reason-code completeness, and outbox ordering are validated uniformly by the shared report system.
2. T15b owns ALL stress/resilience tests as one cohesive batch — the split is harness-vs-tests, not happy-path-vs-stress.
3. The 10-question judge contract provides automated cross-test validation regardless of test file boundaries.

Unknowns: stress workload sizes (96 parallel calls, 32 extraction bursts) may need tuning per CI capacity, but coverage targets and contract assertions are fixed.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 3.1, 3.2`
- Original parent ticket (split from): `docs/tickets/2026-05-21-skill-layer-v1-1/15-live-data-plane-e2e-and-stress-suite.md`
- Harness ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/15a-live-harness-factory-and-roundtrip-validation.md`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#canonical-v11-contracts`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`
- T07: `docs/tickets/2026-05-21-skill-layer-v1-1/07-outbox-relay-and-reconciliation.md`

## Coupling Notes

- Extraction, degradation, churn, concurrency, and dream-state tests stay together in one ticket because they share the same data-plane correctness boundary and collectively answer "does the system hold up under realistic conditions?" Splitting further would risk shipping individual coverage bands without cross-test validation.
- The judge contract (10 questions) couples to the report infrastructure from T15a and aggregates evidence across all tests — this is the validation gate that prevents T15a-only shipping from leaving correctness blind spots.
- Dream-state contracts DS-003 through DS-007 are promoted here because T15a's live harness makes them testable for the first time. Remaining dream-state contracts stay stubbed.