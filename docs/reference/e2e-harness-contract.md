# E2E Harness Contract — drive the REAL running app, observe REAL infra, log every stage

**Status:** foundational. ALL existing and future e2e tests MUST use this harness. Authored 2026-06-04 after the
user's directive: *"real infra and live logic paths. no in-memory simulations, no stubs and no fakes. it's not e2e
if we're not actually using the app END TO END … build the actual proper e2e test harness first which will use the
real bloody app and all existing and future e2e tests will use … it shouldn't be a completely black box … I want
detailed file logs for every test run covering all inputs and outputs for all stages."*

## Non-negotiable principles
1. **Real app, real transport.** Tests drive the **running containerized `mcp-server`** over HTTP `127.0.0.1:3001`
   (MCP JSON-RPC `POST /mcp`) and the **running `graph-builder` container** — NOT `McpServerApp::from_environment`
   in-process, NOT an in-test axum router on an ephemeral port. The app under test is the compiled container image.
2. **No stubs / fakes / synthetic data / in-memory simulation.** Embeddings are real (Ollama `nomic-embed-text`,
   768-dim). Skills enter through the **real ingest loop**. "Crash" = real `docker kill` of a real container. Drift =
   a real divergence produced by a real interruption — never hand-injected synthetic rows/points/vectors.
3. **Not pure black box — observe real infra.** White-box assertions/logging read the REAL stores read-only
   (PG / Qdrant / Redis / `.pending` files / `graph_state`) at each stage. Observation never mutates app state.
4. **Detailed per-run, per-stage file logs.** Every test run writes, for every pipeline stage, the full **inputs and
   outputs** (prompts, file contents, request/response bodies, observed store snapshots) to durable files.
5. **Guardrails.** Local-first (`cloud_calls: none`, Ollama default). Human-gate: NEVER edit
   `docker-compose.test.yml`/Dockerfile/`.env`/migrations — bringing the stack up and `kill/stop/start` of services
   are COMMANDS, allowed. Secrets (`TRANSCRIPT_INGEST_SECRET`) travel as the `X-Ingest-Secret` header, never in
   process args. Honor T08 port-ownership comments in the compose file. Do not regress topology.

## Real app surface (ground truth — cited)
- **Router** `crates/mcp-server/src/protocol.rs:637` — `POST /mcp`, `GET /health`, `POST /ingest/transcript`; body limit 4 MiB.
- **MCP call:** `POST /mcp` JSON-RPC 2.0: `{"jsonrpc":"2.0","id":<any>,"method":"tools/call","params":{"name":"compile_context"|"extract_session"|"find_skill","arguments":{...}}}`. `tools/list` for discovery.
  - `compile_context` args (`tools/compile_context.rs:44`): `{prompt, session_id (^[A-Za-z0-9_-]+$, no "::"), repo_path, trigger?}`.
    Response (`:56`): `{status:"ok"|"no_match"|"degraded"|"duplicate_suppressed", reason_code?, additional_context?, health:{}, scopes_considered:[], graph_version:i64, latency_ms, source}`.
  - `extract_session` args (`tools/extract_session.rs:11`): `{transcript_ref, transcript_inline?(≤4MiB), session_id, repo_path?}`. Response `{status:"processing"|"failed", reason_code?, job_id?, provider?}`.
- **NO stdio transport exists** — `main.rs` always `serve_http`. (Plan's "stdio + HTTP" for DS-002 ⇒ HTTP only; record the deferral.)
- **Health:** `GET /health` → 200 healthy / 503 unhealthy.
- **Ingest:** `POST /ingest/transcript` body `{session_id, repo_path?, source:"session_end"|"pre_compact"|"reconcile", content}` + header `X-Ingest-Secret: $TRANSCRIPT_INGEST_SECRET`. 202 enqueued / 200 duplicate / 400 / 413 / 401 / 503.

## Real ingest loop (the END-TO-END path tests must drive)
**Path A — skill files (the canonical loop):**
1. Sidecar-write `<slug>/SKILL.md.pending` into the named volume `test-global-skills` (→ `/skills/global`) or
   `test-project-skills` (→ `/skills/project`). Volumes are `:ro` in the app containers, so write via a sidecar:
   `docker run --rm -v test-global-skills:/skills alpine sh -c '…write file…'` (graph-builder, `:ro`, still sees it).
2. **Human gate = rename** `SKILL.md.pending` → `SKILL.md` (`crates/domain/src/lifecycle_files.rs:4`) via the same sidecar `mv`.
3. Real **graph-builder** container (polls `GRAPH_BUILDER_GLOBAL_ROOT=/skills/global`, `GRAPH_BUILDER_PROJECT_ROOT=/skills/project`
   every `GRAPH_BUILDER_POLL_INTERVAL_MS=5000`) runs `rebuild_from_changes` → writes PG (`skills/subunits/communities/outbox_events`),
   **real 768-dim embeddings** (`build_graph_from_pg`→`embed_batch`, Ollama), bumps `graph_state.graph_version`,
   `XADD graph.rebuilt` to Redis stream `skill-layer-events`.
4. mcp-server `graph_refresh_subscriber` (`XREADGROUP GROUP skill-layer worker-1`) → `reload_and_swap` → new snapshot, version bump.
5. Retrieve: `compile_context` over HTTP `:3001` reflects the new `graph_version` and returns `ok` for a matching prompt.

**Path B — transcript ingest (session capture):** `POST /ingest/transcript` → `transcript_ingest_queue` → maintenance
`TranscriptQueueDrain::drain_once` → `SessionExtractor` → `.pending` drafts → human rename → Path A.

## Real infra observation points (host-mapped ports)
- **Postgres** `postgres://skill_layer:skill_layer@localhost:15432/skill_layer_test`. `SELECT graph_version FROM graph_state` (singleton).
  Tables: `skills, subunits, communities, skill_subunits, community_skills, outbox_events, session_logs, skill_usage, transcript_ingest_queue, rebuild_locks, graph_state`.
- **Qdrant** REST `http://localhost:16333`, collection `skills` (768-dim). Count: `GET /collections/skills` → `.result.points_count`; list: `POST /collections/skills/points/scroll`.
- **Redis** `redis://localhost:16379`. Stream `skill-layer-events`, group `skill-layer`, consumer `worker-1`. Observe `XLEN`, `XPENDING`, `XREAD`.
- **Ollama** `http://localhost:11444` (`/api/embeddings` nomic-embed-text 768; extraction via `OLLAMA_EXTRACTION_ENDPOINT=/api/generate`).

## Harness module layout (`tests/e2e/harness/`)
- `stack.rs` — bring the FULL stack up (incl. `mcp-server` + `graph-builder` images, reusing the `scripts/run-e2e-tests.sh`
  bring-up: `docker compose build` then `up -d`, wait on BOTH `/health`s and `live-e2e-check`); `kill(svc)`, `stop(svc)`,
  `start(&[svc])`, `pause`/`unpause` for faults; teardown. Services: `mcp-server, graph-builder, postgres, redis, qdrant, ollama`.
- `app.rs` — `McpClient` over `reqwest` to `http://127.0.0.1:3001`: `call_tool(name, args) -> JsonRpcResponse`,
  `compile_context(req) -> CompileContextResponse`, `extract_session(req)`, `health() -> (status_code, body)`,
  `ingest_transcript(body, secret)`.
- `seed.rs` — sidecar volume writer/approver: `write_pending(scope, slug, skill_md)`, `approve(scope, slug)` (rename),
  `remove(scope, slug)` — all via `docker run --rm -v <vol>:/skills …`. Plus `seed_and_approve(...)` convenience.
- `observe.rs` — read-only `PgObserver` (graph_version, table counts, row fetch), `QdrantObserver` (points_count, scroll),
  `RedisObserver` (xlen, xpending, xread). NEVER mutate.
- `poll.rs` — `poll_until(pred, timeout, interval)`; `wait_for_rebuild(prev_version, timeout)` (PG graph_version bump
  AND `compile_context` over HTTP reports `graph_version > prev`); `wait_for_health(svc, timeout)`.
- `stagelog.rs` — per-run logger. Creates `tests/e2e/reports/<run_id>/<scenario>/`. For each stage call
  `log_stage(name, input_json, output_json, infra_snapshot_json)` → writes `NN-<name>.json` (full input+output+observed
  store snapshot + RFC3339 ts) AND appends a human-readable section to `<scenario>.md`. At end, emit an `E2EReport`
  JSON (existing `report.rs` schema — keep aggregator/`generate-e2e-summary.py` compatible) at
  `tests/e2e/reports/<scenario>__<YYYYMMDDHHMMSS>.json`, plus the rich per-stage tree under `<run_id>/`.
- `mod.rs` — re-exports; include via `#[path = "harness/mod.rs"] mod harness;`.

### Stage taxonomy to log (capture ALL inputs + outputs)
`ingest_input` (file/transcript content), `queue_state` (transcript_ingest_queue rows), `extraction_output` (.pending
content + provider), `approval` (rename event), `rebuild_event` (Redis graph.rebuilt payload), `snapshot_swap`
(graph_version before/after, PG + Qdrant counts), `retrieval_request` (prompt/session/repo), `retrieval_response`
(full CompileContextResponse), `store_snapshot` (PG/Qdrant/Redis at the moment), `fault_injection` (svc, action),
`recovery` (latency, recovered signal).

## Tracer-bullet acceptance (the first build must prove this LIVE)
Golden path, fully through the real app, with complete stage logs:
seed a SKILL.md into a scope → approve (rename) → `wait_for_rebuild` → `compile_context` over HTTP `:3001` with a
matching prompt → assert `status == "ok"` and the seeded skill is reflected (graph_version advanced, skill present in
PG + Qdrant count grew) → per-stage JSON+MD logs written under `tests/e2e/reports/<run_id>/golden-path/`.
RED (fail-able by construction): a non-matching prompt ⇒ `no_match`; app/container down ⇒ assertion fails.

## Migration / rollout (subsequent units, not this build)
1. Rework DS-003..007 onto the harness (faults = real `docker kill`; drift = real interruption; retrieval over HTTP).
2. Migrate the existing in-process "live" suite (`test_live_data_plane_roundtrip`, boot, watcher, transcript ingest,
   `test_project_scope_container`) to drive the real container; reclassify anything that stays in-process as
   "integration", not "e2e".
3. Promote DS-002 (HTTP transport roundtrip; stdio deferred — doesn't exist), DS-008 (multi-repo isolation), hostile-input.
