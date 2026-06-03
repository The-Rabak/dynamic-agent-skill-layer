---
ticket_id: T08
title: Fix Qdrant port handling + suppression test isolation + compose alignment
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.1: Fix Qdrant port handling + suppression test isolation + compose alignment"
feature_home: crates/infrastructure
depends_on: [T01, T02, T03]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-E (live suite is GREEN)
files:
  - crates/infrastructure/src/vector/qdrant.rs
  - crates/infrastructure/src/health.rs
  - crates/mcp-server/src/suppression_state.rs
  - crates/mcp-server/src/protocol.rs
  - scripts/run-e2e-tests.sh
  - docker-compose.test.yml
  - tests/e2e/test_live_data_plane_roundtrip.rs
test_command: cargo test -p mcp-server --features test-utils -- --ignored test_live_data_plane_roundtrip
tdd_mode: inherit
---

# Fix Qdrant port handling + suppression test isolation + compose alignment

## Serves
- **SC-V1.5-E** — the live suite preflight connects on the first try; two live servers on one Redis stop leaking suppression. This unblocks the harness so T09/T10 can reach real behavior.
- Plan SC-7.

## Scope
Make the Qdrant adapter own the REST↔gRPC port relationship from one configured base; fix `run-e2e-tests.sh` + `docker-compose.test.yml` to use the REST port for the REST preflight; add suppression test isolation; fix the local `clear_session` prefix bug.

- **Owns:** port derivation, suppression isolation, compose/runner alignment, and the local-only safety posture preflight that lets a new user trust the stack before running the live suite.
- **Non-goals:** retrieval quality (T09); un-RED-ing tests (T10).

## Scope Fence
No behavior change to production suppression semantics beyond namespacing. **This ticket owns the port/env lines in `run-e2e-tests.sh`; T10 must NOT touch them (it only adds the CI gate stanza)** — avoids a same-file collision.

## Acceptance Criteria
- [x] Live preflight connects with the runner's defaults (no manual port override). **Root cause:** `run-e2e-tests.sh:76` exports `QDRANT_URL=…${QDRANT_GRPC_PORT}` (16334, gRPC) for the live section while the REST `/collections` preflight needs 16333 — change line 76 to `${QDRANT_HTTP_PORT}`. Also fix `docker-compose.test.yml:107,141` (`http://qdrant:6334` → `:6333`). Adapter derives REST/gRPC from one base so they can't disagree. — **Done** (`run-e2e-tests.sh:76` → `${QDRANT_HTTP_PORT}`; compose lines 110,144 → `:6333`; `QdrantConfig` documented + REST-port deletion-guard test). **Verified live (2026-06-03):** roundtrip preflight connects on 16333, no `hyper Parse(Version)`.
- [x] Two live servers on one Redis do not leak suppression (UUID-namespaced session ids and/or namespace prefix; documented "clear Redis between boots" / `FLUSHDB ASYNC` path). — **Done** (unique session ids + namespace prefix; `two_server_instances_on_shared_redis_do_not_leak_suppression…` test; live roundtrip's two-instance compile returns `Ok` then `DuplicateSuppressed`).
- [x] **Fix the local suppression `clear_session` bug:** `suppression_state.rs` `retain` uses prefix `suppression:{session_id}` but local DashMap keys are `{session_id}::{repo_path}`, so local state is never cleared. — **Done** (`local_session_prefix` → `{sid}::`; Redis SCAN `suppression:{sid}::*`; 2 Red→Green tests).
- [x] (Performance) Invert the suppression lookup to DashMap-first / Redis-fallback so warm sessions don't pay a Redis RTT on the hot path. — **Done** (`is_suppressed`/`graph_version`/`scopes_considered` check DashMap first; `is_suppressed_returns_dashmap_value_when_redis_unavailable` test).
- [x] **Security P1 — add `DefaultBodyLimit` to the axum MCP router** (`crates/mcp-server/src/protocol.rs:428` `Router::new()`). — **Done** (`MCP_BODY_LIMIT_BYTES = 4 MiB` + `DefaultBodyLimit::max` layer at `protocol.rs` `router()`; `mcp_router_rejects_oversized_body_with_413` test).
- [x] **Localhost safety posture preflight:** verify the MCP server binds to `127.0.0.1` by default. — **Done** (`default_bind_address_is_loopback` test; adoption/trust check only, no auth surface added).
- [x] Oversized payload coverage includes both MCP JSON-RPC and `/ingest/transcript` where applicable. — **Done for MCP JSON-RPC** (router body limit + `mcp_http_router_rejects_oversized_payload_with_payload_too_large`). Transcript-queue content cap is covered by the durable transcript-ingest queue (todo 103); explicit 413/reason-code assertion left to T10's full-suite pass.
- [x] **Scope-line ownership:** `run-e2e-tests.sh` changes restricted to the port/env lines; no CI stanza added (T10 owns that). — **Honored** (diff touches only the `QDRANT_URL` export at line 76).
- [x] **Human-gate enforced:** compose/script/env edits staged for explicit approval before commit. — **Done** (maintainer approved both edits 2026-06-03 before commit).

> **Integration-gate handoff to T10 (NOT a T08 defect):** with the preflight fixed, `test_live_data_plane_roundtrip` now runs its full body and passes all assertions (compile `Ok` w/ seeded skill, duplicate `DuplicateSuppressed`); it is **green under `MCP_USAGE_LOGGING=off`**. Under the default (`on`) it deadlocks in teardown (`TRUNCATE … session_logs, skill_usage` racing T06's in-flight async usage-writer transactions while two live server instances are alive). Root cause + fix owner is the T06/teardown seam (drain async usage writers before truncate, or deadlock-resilient truncate) in `persistence/postgres.rs` + `mcp-server/lib.rs::teardown` — **outside T08's declared file set.** Logged as a T10 blocker in `index.md`.

## Shared / Global Notes
- **Infrastructure configuration change — HUMAN GATE:** `docker-compose.test.yml`, `scripts/run-e2e-tests.sh` env edits require approval.
- Shared adapter `vector/qdrant.rs` is touched by T03 (role/labelling) and T08 (port derivation) — T03 runs in an earlier batch, so T08 picks up its changes; no concurrent edit.
- The `run-e2e-tests.sh` ownership fence with T10 is a deliberate same-file conflict-avoidance: T08 = port/env lines, T10 = CI gate stanza.
- The local-only safety posture check intentionally stays in T08 because this ticket already owns port/env alignment and the body-limit security fix. Do not expand it into auth or public-network support; constitution §Deferred-risk guard still says admin/MCP surfaces are localhost/private-network only for this phase.

## Local Context
**WHY:** Every live test in `test_live_data_plane_roundtrip` failed identically at the preflight: `GET http://localhost:16334/collections` → `hyper Parse(Version)` (16334 is gRPC; REST `/collections` belongs on 16333). Suppression `suppression:<session_id>::<repo_path>` has no per-instance namespace, so a second `build_live_server()` reusing a session id returns `DuplicateSuppressed` instead of `Ok`.

**Adoption WHY (2026-06-02 assessment):** Users will only trust a local-first agent skill layer if the first preflight proves the stack is loopback-bound, payload-bounded, and not accidentally exposing code/transcripts on broad host interfaces. This adds a small trust signal without changing the V1.5 product surface.

**Open question to surface:** confirm current line numbers (`run-e2e-tests.sh:76`, `docker-compose.test.yml:107,141`) before editing — if drifted, locate via `semble search "QDRANT_URL grpc port preflight"`.

## Parent Refs
- Plan → Slice 3.1; Architecture artifact.
- Source packet: `## Execution Slices > Slice 3.1`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §3.1 (one base derives REST 6333/16333 + gRPC 6334/16334; UUID-namespaced ids + `FLUSHDB ASYNC`).
- Plan §Firsthand test evidence (2026-05-31).

## Coupling Notes
One unit because the port fix and the suppression-isolation fix are the two harness defects that block the same preflight-to-assertion path; fixing one without the other still leaves the live suite RED before assertions. Hard-depends on Phase 1 (T01–T03) so the corrected preflight reaches real behavior, and on T03 specifically for the settled `vector/qdrant.rs` role labelling. Singleton batch: shares `tests/e2e/*` with T09/T10 and `run-e2e-tests.sh` with T10.
