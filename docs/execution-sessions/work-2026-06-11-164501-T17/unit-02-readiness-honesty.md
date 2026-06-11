---
unit: "Readiness honesty — snapshot-ready signal + fast tool warming"
unit_number: 2
unit_kind: hardening
serves: "T17 AC1 (no healthy-while-warming window; tools return explicit warming fast, no hang) + AC5 (T11 can gate on the honest /health signal)"
status: completed
attempt_count: 1
domains: [mcp-server, infrastructure, health, retrieval]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/17-mcp-server-boot-readiness-honesty.md
session_id: work-2026-06-11-164501-T17
---

## What Was Implemented
A shared snapshot-readiness signal that makes `/health` honest during a build/reload window and short-circuits tool calls to an explicit warming status BEFORE the query embed (killing the embed-semaphore hang).

- **`ReadinessHandle`** (crates/infrastructure/src/health.rs): Arc<RwLock<ReadinessState>> with states Warming/Ready/Failed(msg); `warming()`/`ready()`/`set_warming()`/`set_ready()`/`set_failed(msg)`/`is_ready()`/`health_component()`. Exported from the infrastructure crate.
- **/health integration**: `InfrastructureHealthChecker::with_readiness(Arc<ReadinessHandle>)` stores the handle; `check()` appends a `readiness` component — Ready→healthy/"ready", Warming→unhealthy/"warming: snapshot build/reload in flight", Failed→unhealthy/"failed: <msg>". Since overall `healthy = all components healthy`, /health returns 503 while warming/failed → no healthy-while-warming window.
- **Boot wiring** (crates/mcp-server/src/lib.rs build_live_server): handle created Warming at boot start, `set_ready()` after the initial snapshot/orchestrator is built (before listener binds), threaded onto McpServerApp (`with_readiness_handle`) and PostgresGraphReloader (via spawn_graph_refresh_subscriber), and exposed on LiveServerComponents.readiness_handle. main.rs wires it into the health checker via `.with_readiness(live.readiness_handle.clone())`.
- **Reload flip** (PostgresGraphReloader::reload_and_swap): `set_warming()` before build_graph_from_pg; `set_ready()` after a successful swap; `set_failed(err)` if build errors (observable on /health, never stuck "warming forever"). Event replay/ACK contract unchanged (failed reload still returns Err → replays).
- **Tool short-circuit** (McpServerApp::compile_context/find_skill/search_skill_graph): at the TOP, before any retrieval/embed, `if !self.readiness.is_ready()` returns an explicit warming response — find_skill/search_skill_graph `status:"warming"`, compile_context new `CompileContextStatus::Warming` (serializes "warming") + reason_code "snapshot_warming", empty results, fast. Non-live/test constructors default `ReadinessHandle::ready()` so existing tests don't hit the guard.

## Files Changed
- `crates/infrastructure/src/health.rs` — ReadinessHandle + with_readiness + check() readiness component + 10 unit tests
- `crates/infrastructure/src/lib.rs` — export ReadinessHandle
- `crates/mcp-server/src/lib.rs` — readiness field/builder/accessor on McpServerApp; 3 tool short-circuits; boot create-Warming/set-Ready; PostgresGraphReloader flip; LiveServerComponents.readiness_handle; spawn_graph_refresh_subscriber param; readiness_short_circuit_tests (4)
- `crates/mcp-server/src/tools/compile_context.rs` — CompileContextStatus::Warming variant
- `crates/mcp-server/src/main.rs` — wire handle into health checker

## Problems Encountered
### Problem 1: non-exhaustive match on CompileContextStatus (orchestrator-caught)
- **Error:** `E0004: CompileContextStatus::Warming not covered` at lib.rs:2118 (`build_session_usage_record`).
- **Root cause:** new Warming variant; one real `match` over the enum (only reached when a live retrieval ran, so never Warming, but must compile).
- **Fix:** added `CompileContextStatus::Warming => "warming"` arm; confirmed no other match sites.

## Patterns Discovered
- The hang was the query-embed waiting on the shared Ollama semaphore (saturated by bulk re-embed during a background graph.rebuilt reload), NOT a snapshot lock (snapshot is lock-free ArcSwap). The fix must short-circuit BEFORE the embed — which the readiness guard does.
- `InfrastructureHealthChecker` needed a runtime-mutable slot (Option<Arc<ReadinessHandle>>) distinct from the static `with_static_component` path, since static components are fixed at builder time.
- LiveServerComponents is the surface for handing live boot artifacts to main.rs (mirrors embedding_model_info).

## Test Results
- `cargo build -p infrastructure -p mcp-server --features test-utils` → OK
- `cargo test -p infrastructure --lib readiness` → **10 passed**
- `cargo test -p mcp-server --lib readiness_short_circuit` → **4 passed** (warming returned, embedder NOT called while warming; normal after set_ready)
- `cargo test -p mcp-server --lib --features test-utils` → **43 passed; 0 failed** (no regression)
- `cargo test -p infrastructure --lib --features test-utils` → **215 passed; 0 failed; 10 ignored** (no regression)
- `cargo clippy -p infrastructure -p mcp-server --lib --features test-utils` → **clean**

## TDD Evidence
- **Red:** `cargo test -p infrastructure --lib readiness` and `cargo test -p mcp-server --lib readiness_short_circuit` before impl — FAIL (ReadinessHandle/CompileContextStatus::Warming/with_readiness_handle absent → compile error). Plus the orchestrator build surfaced the missing match arm. Proves the readiness signal + warming short-circuit were genuinely missing.
- **Green:** 10 + 4 readiness tests PASS; warming response returned without an embedder call; /health readiness component flips the report.
- **Post-Refactor Green:** after the match-arm fix, re-ran both full lib suites — 43 + 215 PASS, clippy lib-clean. Cleanup preserved behavior; no existing test regressed (non-live default Ready).

## AC5 note
AC5 (T11 can gate on the honest readiness signal): `/health` now returns 503 while the snapshot is warming/failed and 200 only when ready — T11's sweep scripts can poll `/health` for 200 and trust it means snapshot-ready, removing the interim probe-query workaround. To be verified live in Unit 3.
