---
unit: "T08 — Fix Qdrant port handling + suppression test isolation + compose alignment"
unit_number: 1
unit_kind: hardening
serves: "SC-V1.5-E (live suite GREEN) — preflight connects + suppression no longer leaks; unblocks T09/T10"
status: completed
attempt_count: 1
domains: [infrastructure, mcp-server, testing, e2e, security]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/08-fix-qdrant-ports-and-suppression-isolation.md
session_id: work-2026-06-03-001046
---

## What Was Implemented
1. **Qdrant REST/gRPC port alignment.** `run-e2e-tests.sh:76` live-section `QDRANT_URL` export switched from `${QDRANT_GRPC_PORT}` (16334) to `${QDRANT_HTTP_PORT}` (16333) so the REST `/collections` preflight connects. `docker-compose.test.yml:110,144` `http://qdrant:6334` → `:6333` for both mcp-server and graph-builder service env blocks. `QdrantConfig` documented with the REST/gRPC invariant + a deletion-guard test asserting the default endpoint uses the REST port.
2. **Suppression local `clear_session` bug fixed.** `session_prefix` (`suppression:{sid}`) → `local_session_prefix` (`{sid}::`) so the local DashMap `retain` actually matches `{sid}::{repo}` keys; Redis SCAN pattern corrected `suppression:{sid}:*` → `suppression:{sid}::*`.
3. **DashMap-first / Redis-fallback lookup inversion** in `is_suppressed`, `graph_version`, `scopes_considered` — warm sessions skip the Redis RTT (per AC; explicit performance trade-off).
4. **DefaultBodyLimit (Security P1).** `MCP_BODY_LIMIT_BYTES = 4 MiB` constant + `DefaultBodyLimit::max(..)` layer on the axum MCP router; test asserts an oversized body → 413.
5. **Localhost safety-posture preflight.** `default_bind_address_is_loopback` test asserts the default bind is `127.0.0.1`.
6. **E2E coverage** added: two-server suppression isolation, cleared-session reuse, oversized-payload rejection.

## Files Changed
- `crates/mcp-server/src/suppression_state.rs` — clear_session prefix fix, DashMap-first lookup, 3 new unit tests
- `crates/mcp-server/src/protocol.rs` — DefaultBodyLimit constant + layer, 2 protocol tests
- `crates/mcp-server/src/lib.rs` — `suppression_state_for_tests` re-export (test-utils)
- `crates/infrastructure/src/vector/qdrant.rs` — QdrantConfig doc + REST-port deletion-guard test
- `crates/infrastructure/src/health.rs` — formatting-only (cargo fmt side effect; no behavioral change)
- `tests/e2e/test_live_data_plane_roundtrip.rs` — 3 new unit-level tests
- `scripts/run-e2e-tests.sh` — HUMAN-GATE: REST port fix (line 76)
- `docker-compose.test.yml` — HUMAN-GATE: REST port (lines 110, 144)

## Verification (orchestrator, independent)
- `cargo test -p mcp-server --features test-utils` — PASS (all suites; 9 targeted T08 tests green)
- `cargo test -p infrastructure --lib` — 91 pass / 1 fail = `build_health_checker_injects_usage_write_disabled_when_flag_is_off`; **passes in isolation** → pre-existing env-var-contamination flake (health.rs only reformatted by T08). Belongs to T10 suite-reliability scope.
- `cargo clippy -p mcp-server -p infrastructure --features test-utils --lib --bins -- -D warnings` — CLEAN
- `cargo fmt -p mcp-server -p infrastructure -- --check` — only `extraction/claude_code.rs` flagged = **pre-existing T05 file, NOT touched by T08**; outside scope fence.
- `cargo build --workspace` — GREEN
- **Live E2E** (containers up: PG 15432 / Redis 16379 / Qdrant 16333-4 / Ollama 11444):
  - `MCP_USAGE_LOGGING=off`: `test_live_data_plane_roundtrip` **PASS** — preflight connects (no more `hyper Parse(Version)`), compile_context returns `Ok` w/ seeded skill `roundtrip-rust-file-io`, duplicate → `DuplicateSuppressed`, teardown succeeds. **Proves every T08 functional AC.**
  - default (`MCP_USAGE_LOGGING=on`): FAILS at teardown line 554 — `pg truncate failed: ... deadlock detected`. See blocker below.

## TDD Evidence
- **Red**
  - suppression clear: 2 new tests FAILED before fix (`retain` prefix never matched `{sid}::{repo}` keys).
  - DefaultBodyLimit: `E0425 cannot find value 'MCP_BODY_LIMIT_BYTES'` before the constant/layer existed.
  - port: live preflight originally `GET http://localhost:16334/collections → hyper Parse(Version)` (gRPC port for REST).
- **Green**
  - suppression clear + body-limit unit tests PASS; live preflight connects on 16333 and the roundtrip body passes all assertions (Ok + DuplicateSuppressed) — green end-to-end with usage-logging off.
- **Post-Refactor Green**
  - `cargo test -p mcp-server --features test-utils` + `cargo test -p infrastructure --lib vector::qdrant::tests` PASS; lib/bins clippy clean; workspace build green. No T08-owned regressions.

## Blocker surfaced (NOT T08 scope) — teardown TRUNCATE deadlock under default usage logging
- **Symptom:** `test_live_data_plane_roundtrip` fails in `LiveServerComponents::teardown` → `truncate_all_tables` → `TRUNCATE ... session_logs, skill_usage CASCADE` → `deadlock detected`, only with `MCP_USAGE_LOGGING=on` (default).
- **Root cause:** T06's fire-and-forget async usage-writer transactions (spawned by `compile_context`) are still in flight, holding RowExclusive locks on `session_logs`/`skill_usage`, when teardown issues TRUNCATE (ACCESS EXCLUSIVE). Two live server instances (`components` + `components2`) are both alive, compounding the contention.
- **Why newly visible:** the Qdrant preflight previously blocked every live test before the body ran; T08's port fix unblocked it, surfacing this latent T06 race.
- **Correct owner:** the T06/teardown seam — drain/await background usage writers (or close the app pool / abort the writer tasks) before `truncate_all_tables`, or make the test-only truncate deadlock-resilient. Files: `crates/infrastructure/src/persistence/postgres.rs` (truncate), `crates/mcp-server/src/lib.rs` (`teardown`/usage-writer drain) — **outside T08's declared file set**. This is an integration-gate (T10) / T06-follow-up concern, not a port/suppression defect.

## Pre-existing issues noted for T10 (integration gate) — not introduced by T08
1. `extraction/claude_code.rs` (T05) fails `cargo fmt --check`.
2. Shared e2e helpers `tests/e2e/report.rs` (`if_same_then_else`) and `tests/e2e/test_concurrency_stress.rs` (`len_zero`) fail strict `clippy -D warnings --all-targets`.
3. `health::tests::build_health_checker_injects_usage_write_disabled_when_flag_is_off` env-var-contamination flake (passes isolated).
4. Teardown TRUNCATE-vs-async-usage-writer deadlock (above) — highest priority for T10 green suite.

## Patterns Discovered
- Two parallel suppression key namespaces: local DashMap `{sid}::{repo}` (no prefix) vs Redis `suppression:{sid}::{repo}`. Any clear/scan must use the matching prefix form per store.
- `suppression_state_for_tests` test-utils re-export mirrors the existing `OutboxVectorStore` pattern for exposing internals to E2E without polluting the production API.
- Fixing the Qdrant preflight unmasks downstream latent harness races (T06 truncate deadlock) — the expected "fix one defect, reach the next" progression the plan predicted.
