---
ticket_id: T02
title: Live graph refreshes on graph.rebuilt without restart
kind: expansion # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed (commit 8a65bf0)
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.2: Live graph refreshes on graph.rebuilt"
feature_home: crates/mcp-server
depends_on: [T01]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-A (loop closes in the body)
files:
  - crates/graph-builder/src/main.rs
  - crates/mcp-server/src/graph_refresh_subscriber.rs
  - crates/mcp-server/src/lib.rs
  - crates/retrieval/Cargo.toml
  - crates/retrieval/src/orchestrator.rs
  - tests/e2e/test_live_data_plane_roundtrip.rs
test_command: cargo test -p mcp-server --features test-utils --test test_live_data_plane_roundtrip -- --ignored graph_rebuilt_event_refreshes_running_server_without_restart
tdd_mode: inherit
---

# Live graph refreshes on graph.rebuilt without restart

## Serves
- **SC-V1.5-A** — a skill approved **while the server is running** becomes retrievable **without restart**. This ticket delivers the online-refresh half of SC-A (T01 delivered the boot half).
- Plan SC-4/SC-5 (self-growing visible online).

## Scope
Two coupled changes: (1) **make graph-builder actually publish `graph.rebuilt` to Redis** (confirmed blocker — it currently pushes to an un-drained in-memory `Vec`), and (2) subscribe the online server, reload the snapshot from PG on receipt, and atomically swap the retriever's graph handle + version via `ArcSwap`.

- **Owns:** graph-builder Redis publication of the existing event + the online subscription + atomic graph swap + version update.
- **Non-goals:** changing the event schema; live Qdrant query (T03); ranking changes.

## Scope Fence
No new event types — reuse the frozen `graph.rebuilt` from the 8-event catalog. No schema change. No change to suppression semantics. `retrieval` must not gain a `redis`/`sqlx`/`qdrant` import — it exposes only `swap_graph`; the subscriber + bounded PG reload live in `mcp-server`.

## Acceptance Criteria
- [ ] **PREREQUISITE (CONFIRMED BLOCKER, R-2):** graph-builder publishes `graph.rebuilt` to the shared Redis stream. Today `rebuild.rs:117` pushes the envelope into an in-memory `Vec<EventEnvelope>` (`main.rs:192`) the rebuild loop (`main.rs:208–220`) never drains. **Fix:** `XADD` each published envelope via the existing `infrastructure::RedisStreamsAdapter` (graph-builder already depends on `infrastructure`; wire `REDIS_URL`). Reuse the frozen event — no new type.
- [ ] `retrieval` exposes only `RetrievalOrchestrator::swap_graph(&self, snapshot: RetrievalSnapshot)`; the Redis subscriber + bounded PG reload live in `crates/mcp-server/src/lib.rs` (subscriber suggested at `crates/mcp-server/src/graph_refresh_subscriber.rs`). No `redis`/`sqlx`/`qdrant` import enters `retrieval`.
- [ ] Graph + version live in **one struct** (`GraphSnapshot { graph, version }`) under an `ArcSwap` (lock-free reads on the hot path — NOT `RwLock<Arc<…>>`); readers `load()` once so graph and version can never skew.
- [ ] Subscriber coalesces a burst of `graph.rebuilt` (reload once for the newest version) and reloads idempotently (re-applying the same version is a no-op).
- [ ] Redis consumer reuses `RedisStreamsAdapter::read_group` (XREADGROUP; pending-replay `"0"` + new `">"`; ACK only after a successful reload+swap). Wrap in an exponential-backoff reconnect loop; never panic, never block the HTTP server.
- [ ] Approving a skill while the server runs makes it retrievable without restart.
- [ ] No torn reads / race under concurrent `compile_context` during a swap (covered by a concurrency test).
- [ ] `graph_version` in responses advances after rebuild.
- [ ] The approval→retrievable latency window (bounded by graph-builder poll interval, ~15s) is documented in the hook-contract docs so SC-A's "no restart" is not misread as "instant".
- [ ] `MCP_GRAPH_REFRESH=off` rollback flag carries `// TODO(remove-after-v1.5-green)` + removal criterion.

## Shared / Global Notes
- **Cross-feature-home work:** the publication fix lives in `crates/graph-builder/` (a different feature home than `mcp-server`). Declared explicitly — this ticket spans graph-builder (publish) + mcp-server (subscribe) because the loop is only observable when both halves exist. Reuses the shared `infrastructure::RedisStreamsAdapter`; do not fork a second Redis client.
- **Frozen event catalog** is a global guardrail — if a new event seems required, STOP and request approval.
- Human-gate: none (no infra-config/schema change; `REDIS_URL` is already in the compose env).

## Local Context
**WHY:** `RetrievalOrchestrator` holds `graph: Arc<RetrievalSnapshot>` (`orchestrator.rs:136`) loaded once at boot and never refreshed; the roundtrip test even builds a second server post-seed and still gets `NoMatch`. R-2 confirms graph-builder never reaches Redis, so SC-A is unreachable until publication exists.

**In-flight safety invariant (document it):** a `compile_context` holding the old `Arc` completes against the old graph — correct. A version bump that makes suppression "not suppressed" forces a fresh recompute — intended.

**Open question to surface:** confirm the rebuild loop's envelope-drain point in `main.rs:208–220` before wiring `XADD`; if the loop shape differs from R-2's reading, flag it rather than guessing.

## Parent Refs
- Plan → Slice 1.2; Architecture artifact.
- Source packet: `## Execution Slices > Slice 1.2`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §1.2 (one-struct-under-swap; Redis consumer shape; subscriber placement; in-flight safety).
- Plan WHY Reassessment R-2 (graph.rebuilt never reaches Redis); Open Question 1 (answered: No).

## Coupling Notes
Publish + subscribe + atomic swap are one unit because none is independently demonstrable: publishing without a subscriber changes nothing observable, and subscribing without publication can never fire. Hard-depends on T01 (the `RetrievalSnapshot` rename and production constructor must exist before `ArcSwap` wraps the type). Parallel-safe with T05 in Batch 3 (disjoint files: orchestrator/lib/events/graph-builder vs session-extractor/extraction).
