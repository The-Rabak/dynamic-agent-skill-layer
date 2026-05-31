---
unit: "T02 — Live graph refreshes on graph.rebuilt without restart"
unit_number: 4
unit_kind: expansion
serves: "SC-V1.5-A (online-refresh half; T01 did the boot half) + plan SC-4/SC-5"
status: completed
attempt_count: 1
domains: [rust, retrieval, mcp-server, graph-builder, redis, concurrency, testing]
batch: 3
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/02-live-graph-refreshes-on-rebuild.md
session_id: work-2026-05-31-121712
commit: 8a65bf0
---

## What Was Implemented
- **graph-builder publish (R-2 fix):** `main.rs` adds `build_redis_streams_adapter()` (wired to `REDIS_URL`, same `skill-layer-events`/`skill-layer` stream+group the subscriber reads) and `drain_published_events()` that `XADD`s each envelope and removes it only on success (transient Redis failure retries next cycle, never silently drops). `rebuild.rs` still pushes to the in-memory `Vec` (transport-agnostic), preserving the existing `published_events.len()==1` unit test. Confirmed the R-2 drain point exactly: `main.rs:192` creates the Vec, `graph/rebuild.rs:117` pushes, loop never drained it.
- **retrieval atomic swap:** `GraphSnapshot { graph, version }` held in `ArcSwap`; readers `load_full()` once per `retrieve` so graph+version never skew; inherent `swap_graph(&self, snapshot) -> bool` (lock-free `store`, idempotent — no-op if incoming version ≤ current). `arc-swap = "1.7.1"` added. No `redis`/`sqlx`/`qdrant` import entered retrieval.
- **mcp-server subscriber:** new `graph_refresh_subscriber.rs` (`run_graph_refresh_loop`) reuses `RedisStreamsAdapter::read_group` (pending `"0"` then new `">"`), coalesces bursts into one reload, ACKs only after a successful reload+swap, exponential-backoff reconnect, never panics/blocks the HTTP server. `lib.rs` adds `PostgresGraphReloader` (reuses T01's `build_graph_from_pg`) behind a `GraphReloader` seam + `spawn_graph_refresh_if_enabled` (detached; `MCP_GRAPH_REFRESH=off` rollback with TODO).
- **docs:** appended the ~15s approval→retrievable latency-window note to `capability-catalog.md`.

## Design decisions (review-relevant)
- `swap_graph` is an **inherent** method on `RetrievalOrchestrator<E>`, NOT on the `SkillRetriever` trait — keeps `EmptyRetriever`/future Option-B backends free of a graph-swap obligation (seam discipline). mcp-server holds the same `Arc<RetrievalOrchestrator>` both concretely (for swaps) and as `Arc<dyn SkillRetriever>` (for tools), so swaps are visible to readers.
- Subscriber ACKs non-`graph.rebuilt` messages too (so unrelated shared-stream events don't accumulate as pending for this consumer), but only ACKs `graph.rebuilt` after a successful reload+swap.

## Files Changed
- `crates/retrieval/Cargo.toml` (arc-swap), `crates/retrieval/src/orchestrator.rs` (GraphSnapshot/ArcSwap/swap_graph + tests), `crates/retrieval/src/lib.rs` (re-export GraphSnapshot)
- `crates/graph-builder/src/main.rs` (Redis publish + drain-to-XADD)
- `crates/mcp-server/src/graph_refresh_subscriber.rs` (created), `crates/mcp-server/src/lib.rs` (reloader + spawn + rollback)
- `tests/e2e/test_live_data_plane_roundtrip.rs` (new live test), `docs/reference/capability-catalog.md` (latency note), `Cargo.lock`

## TDD Evidence
- **Red:** live `graph_rebuilt_event_refreshes_running_server_without_restart` with `MCP_GRAPH_REFRESH=off` → FAILED ("running server must retrieve the newly-available skill after graph.rebuilt without restart") — proves the swap (not incidental boot loading) delivers the behavior. Also a non-atomic two-step swap variant demonstrated the torn-read window the concurrency test guards.
- **Green:** subscriber enabled (default), clean DB → PASS (1 passed); `cargo test -p retrieval` 18 passed (concurrency + idempotency).
- **Post-Refactor Green:** re-ran live e2e + retrieval + subscriber unit + compile + clippy/fmt after cleanup → e2e 1 passed, retrieval 18 passed, subscriber 2 passed, clippy 0 warnings on owned code.

## Test Results (orchestrator-verified on live containers)
- New live refresh test: PASS (1.23s)
- Regressions: T01 boot smoke PASS (0.47s), T03 health deletion-guard PASS, T04 compact bypass PASS
- `cargo test --workspace --features test-utils --no-run`: 0 errors; retrieval 18/18; mcp-server subscriber 8/8
- Invariants: retrieval has no redis/sqlx/qdrant dep/import; no new event type; no fmt churn in unrelated files; `MCP_GRAPH_REFRESH` rollback present.

## Notes / handoffs
- Ticket file-path inaccuracies corrected: `rebuild.rs` is `crates/graph-builder/src/graph/rebuild.rs`; events live in `crates/infrastructure/src/streaming/redis.rs` (no `events/mod.rs`); no infrastructure change was needed (consumption-only).
- Test DB (`skill_layer_test`) is shared and not auto-reset; deliberate-RED live tests must be bracketed with manual cleanup (truncate tables + reset graph_version + delete Redis stream).
- T02's bounded reload inherits whatever `build_graph_from_pg` returns, so T09's source_paths column swap will flow through automatically.
