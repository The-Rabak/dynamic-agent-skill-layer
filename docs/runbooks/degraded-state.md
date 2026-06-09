# Degraded State Runbook

What degraded means, how to detect it, and how to recover.

## What Is "Degraded"?

In the Dynamic Agent Skill Layer, **degraded** means the system is running but one or more infrastructure dependencies is unavailable or unhealthy. The system continues to serve requests with explicit degraded markers rather than failing silently or crashing.

This is distinct from:
- **Healthy empty (`no_match`):** All dependencies are up, but no skills matched the prompt.
- **Duplicate suppressed:** The session already compiled context successfully for the current graph version.

## Detecting Degraded State

### 1. MCP Server Response

When `compile_context` returns `status: "degraded"`, check the response fields:

```json
{
  "status": "degraded",
  "reason_code": "embedding_provider_unavailable",
  "additional_context": "...",
  "health": {
    "ollama": "degraded",
    "skill_snapshot_sync": "ok",
    "filesystem_index": "ok",
    "reason": "embedding_provider_unavailable"
  }
}
```

The `health` map covers the compile_context **read path** only: `ollama` (embedding provider), `skill_snapshot_sync` (freshness of the in-memory snapshot), and `filesystem_index`. Qdrant, Postgres, and Redis are not read-path markers and do not appear here.

### 2. Health Endpoints

```bash
# MCP server health
curl http://127.0.0.1:3001/health

# Graph builder health
curl http://127.0.0.1:8080/health
```

**MCP server** (`/health`) returns `HealthReport`:
```json
{
  "healthy": false,
  "checked_at": "2026-05-31T12:00:00Z",
  "components": [
    {"name": "postgres", "healthy": true, "detail": "ok"},
    {"name": "redis", "healthy": true, "detail": "ok"},
    {"name": "qdrant_write_side", "healthy": false, "detail": "connection refused"}
  ]
}
```

**Graph builder** (`/health`) returns its own circuit-breaker-aware shape:
```json
{
  "healthy": false,
  "detail": "degraded (graph_builder_runtime)",
  "circuit_state": "Open",
  "last_rebuild_error": "embedding_provider_unavailable",
  "dependencies": {
    "postgres": "ok",
    "redis": "ok",
    "qdrant": "ok",
    "ollama": "degraded"
  }
}
```

### 3. Structured Logs

All services emit JSON logs to stdout. Filter for degraded events:

```bash
docker compose logs mcp-server | jq 'select(.status == "degraded")'
docker compose logs graph-builder | jq 'select(.circuit_state == "Open")'
```

## Degraded Reason Codes and Recovery

### Embedding Provider Unavailable (`embedding_provider_unavailable`)

**Symptom:** Ollama container is down or model not loaded.

**Check:**
```bash
curl http://localhost:11444/api/tags  # host-mapped Ollama port
docker compose ps ollama
```

**Recovery:**
```bash
docker compose restart ollama
# Wait for health check to pass (model may need loading)
```

### Embedding Timeout (`embedding_timeout`)

**Symptom:** Ollama is running but slow to respond.

**Check:**
```bash
docker compose logs ollama | tail -20
```

**Recovery:**
- Check `OLLAMA_NUM_PARALLEL` setting (default: 2)
- Verify GPU acceleration is available if expected
- Reduce concurrent requests if under heavy load

### Scope Resolution Failed (`scope_resolution_failed`)

**Symptom:** Graph builder cannot resolve project or global scope.

**Check:**
```bash
docker compose exec graph-builder ls /skills/project
docker compose exec graph-builder ls /skills/global
```

**Recovery:**
- Verify volume mounts in `docker-compose.yml`
- Check `GRAPH_BUILDER_PROJECT_ROOT` points to a git repository root
- Check `GRAPH_BUILDER_GLOBAL_ROOT` exists

### Rebuild Blocked by Circuit Breaker (`rebuild_blocked_by_circuit_breaker`)

**Symptom:** Graph builder has failed 3 consecutive rebuilds and opened the circuit breaker.

**Check:**
```bash
curl http://127.0.0.1:8080/health | jq '.circuit_state'
```

**Recovery:**
- Wait 10 seconds (circuit breaker cooldown)
- Fix underlying cause (check `last_rebuild_error` in health response)
- Circuit closes automatically after successful rebuild

### Redis Streams Publication Failed (`event_publication_failed`)

**Symptom:** Events cannot be published to Redis Streams.

**Check:**
```bash
docker compose exec redis redis-cli ping
docker compose logs redis | tail -20
```

**Recovery:**
```bash
docker compose restart redis
```

## Test Isolation

### Unexpected `DuplicateSuppressed` in Live Tests

If live tests fail with unexpected `DuplicateSuppressed` responses (stale session-suppression state left from a prior run), reset Redis: `redis-cli -p 16379 FLUSHDB ASYNC`

## Circuit Breaker Behavior

The graph builder uses a circuit breaker with these settings:

- **Failure threshold:** 3 consecutive failures
- **Cooldown:** 10 seconds
- **State transitions:** Closed → Open (on threshold) → Half-Open (after cooldown) → Closed (on success)

When the circuit is **Open**:
- Rebuild cycles are skipped
- Health endpoint returns `503 Service Unavailable`
- `last_rebuild_error` shows the reason

## Graceful Degrade Guarantees

1. **Never fake healthy:** A degraded response is always explicit — never disguised as `ok` or `no_match`.
2. **Partial context preserved:** If some scopes are healthy, partial context may still be returned with degraded markers.
3. **Session suppression respected:** Degraded results are NOT written to session suppression state. Only `ok` and `no_match` trigger suppression.
4. **Retry is bounded:** All retry policies have max attempts and backoff ceilings.

## Session Lifecycle Degraded States

### SessionEnd Does Not Fire on Crash (`session_end_skipped_on_crash`)

**Symptom:** A session ends abruptly (crash, SIGKILL, or OOM) and no `.pending` files are produced for it.

**Why:** Claude Code only fires `SessionEnd` on clean termination (`clear`, `resume`, `logout`, `prompt_input_exit`, `other`). Crash paths bypass the hook entirely.

**Recovery:** The durable ingest queue (shipped in V1.5) acts as the reconciliation path: the queue row survives crashes and the maintenance worker drains it on next startup, replacing the T07 filesystem-scan approach. **If the ingest queue row was also lost (e.g., the host was killed before any capture point), manual re-triggering is required** using the shipped `POST /ingest/transcript` path:

```bash
# Re-trigger extraction by POSTing transcript content directly to the ingest endpoint.
# Reads the transcript file on the host (where the path is valid) and enqueues it.
curl -s -X POST http://127.0.0.1:3001/ingest/transcript \
  -H 'Content-Type: application/json' \
  -H 'X-Ingest-Secret: <your-TRANSCRIPT_INGEST_SECRET>' \
  -d '{
    "session_id": "<session-id>",
    "repo_path": "/path/to/repo",
    "source": "reconcile",
    "content": '"$(cat /path/to/<session-transcript>.jsonl | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))')"'
  }'
# Alternatively, run capture-transcript.sh manually against the transcript file:
#   SKILL_LAYER_INGEST_URL=http://127.0.0.1:3001/ingest/transcript \
#   SKILL_LAYER_INGEST_SECRET=<secret> \
#   config/claude-code/capture-transcript.sh session_end < <(echo '{"transcript_path":"/path/to/<session>.jsonl","session_id":"<id>","repo_path":"/path/to/repo"}')
```

**Note on the V1.4 path:** The previous wiring called the `extract_session` MCP tool with `transcript_ref: "{{transcript_path}}"`. That path is broken — `validate_ref` rejects absolute host paths and the container cannot resolve them. Use `POST /ingest/transcript` with inline `content` as shown above. See `docs/reference/transcript-ingress.md` for the full ingress contract.

**Important:** `SessionEnd` extraction produces only `.pending` files. No auto-approval occurs. Human rename (`.pending` → `.md`) is required.

### Compaction Re-Inject Suppressed (`compact_reinject_suppressed`)

**Symptom:** Context disappears after conversation compaction and is not re-injected.

**Why:** Without `trigger: "compact"` in the `PreCompact` hook arguments, the server treats the re-inject as a duplicate and returns `DuplicateSuppressed`. Claude Code's `ignore_on: ["duplicate_suppressed"]` policy discards the response silently.

**Check:** Confirm the `PreCompact` hook in your `~/.claude/settings.json` includes `"trigger": "compact"` in its arguments:

```json
"PreCompact": [{
  "type": "mcp_tool",
  "tool_name": "compile_context",
  "arguments": {
    "trigger": "compact",
    ...
  }
}]
```

**Recovery:** Copy `config/claude-code/hooks.example.json` to `~/.claude/settings.json` and restart the session.

## First-Run Failure Modes (run-demo.sh / doctor.sh)

These failure modes are surfaced by `scripts/doctor.sh` and may block `scripts/run-demo.sh`.

**`run-demo.sh` execution model:** The script runs the maintenance binary **natively on the host** (via `cargo build -p maintenance` and a direct binary exec), not inside a container. It sets `SKILL_GLOBAL_PATHS=$SANDBOX_DIR` and host-mapped ports (e.g., Postgres on `127.0.0.1:15432`, Redis on `127.0.0.1:16379`) for that native run. This is a developer shortcut that avoids the full containerized production setup — the production worker runs inside the `maintenance` container with internal Docker network addresses and volume-mounted skill paths.

### Missing Ollama Model

**Symptom:** `doctor.sh` reports `warn  Ollama model 'nomic-embed-text' not found`.
`run-demo.sh` compile_context call returns `status: degraded` or times out.

**Check:**
```bash
curl -s http://127.0.0.1:11444/api/tags | python3 -m json.tool | grep name
```

**Fix:**
```bash
# Pull the embedding model (docker-compose.test.yml exposes Ollama on port 11444)
curl -X POST http://127.0.0.1:11444/api/pull -d '{"name":"nomic-embed-text"}'
# Wait for the pull to complete, then rerun:
scripts/run-demo.sh
```

### Wrong Qdrant Port

**Symptom:** `doctor.sh` reports `FAIL  Qdrant REST not reachable on http://127.0.0.1:16333`.

**Why:** The canonical REST port is **16333**. The gRPC port is 16334. A common mistake is
using 16334 for REST calls — this produces `hyper::Parse(Version)` errors.

**Check:**
```bash
curl http://127.0.0.1:16333/collections   # REST — should return 200 OK
# (gRPC on 16334 is binary-framed; a plain curl there is expected to return garbage)
```

**Fix:**
```bash
# Ensure docker-compose.test.yml maps 16333 → 6333 for REST
docker compose -f docker-compose.test.yml up -d qdrant
# Verify the port mapping:
docker compose -f docker-compose.test.yml port qdrant 6333
```

### No Ingest Secret (`TRANSCRIPT_INGEST_SECRET` not set)

**Symptom:** `doctor.sh` warns `TRANSCRIPT_INGEST_SECRET not set`. `run-demo.sh` transcript
ingest probe receives a non-401 response or the POST is rejected silently.

**Why:** When `TRANSCRIPT_INGEST_SECRET` is set, the `/ingest/transcript` endpoint requires a
matching `X-Ingest-Secret` header. Without it, the endpoint relies on the loopback binding
alone (`127.0.0.1`) and logs a warning. An unset secret is acceptable for **local developer
use on loopback only** — the loopback binding prevents off-host access in that case. However,
any deployment that exposes the MCP server beyond localhost (e.g., in a shared environment or
behind a reverse proxy) **MUST** set `TRANSCRIPT_INGEST_SECRET` to prevent unauthenticated
transcript ingestion. The doctor flags the unset state so you are aware of the exposure.

**Fix:**
```bash
# Set a real shared secret in your shell or .env:
export TRANSCRIPT_INGEST_SECRET="my-local-ingest-secret"
# Update hooks.example.json → ~/.claude/settings.json with the same value.
```

### MCP Server Down

**Symptom:** `doctor.sh` reports `FAIL  MCP server not reachable on http://127.0.0.1:3001`.
`run-demo.sh` cannot call `compile_context` and exits early.

**Check:**
```bash
curl http://127.0.0.1:3001/health
docker compose -f docker-compose.test.yml ps mcp-server
docker compose -f docker-compose.test.yml logs mcp-server | tail -30
```

**Fix:**
```bash
# Build and start the MCP server (requires postgres, redis, qdrant, ollama to be healthy first):
docker compose -f docker-compose.test.yml build mcp-server
docker compose -f docker-compose.test.yml up -d mcp-server
# Wait for health check (~30s first boot):
until curl -sf http://127.0.0.1:3001/health >/dev/null; do sleep 2; done
echo "MCP server ready"
```

### No Matching Skills (Empty Graph)

**Symptom:** `compile_context` returns `status: no_match` — no skills matched the prompt.
`run-demo.sh` reports `compile_context: no_match`.

**Why:** The graph is empty (no skills have been seeded or the graph-builder has not rebuilt
yet). `run-demo.sh` seeds skills into a sandbox directory, but the MCP server container uses
its own mounted `/skills/global` volume. If you are using a persistent stack (not the demo
stack), you may need to add skills manually.

**Check:**
```bash
# With the demo stack (run-demo.sh): the sandbox is written to target/demo-sandbox-*/
# With a persistent stack:
curl -s -X POST http://127.0.0.1:3001/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"find_skill","arguments":{"prompt":"rust async file io"}}}'
```

**Fix for demo stack:**
```bash
# Re-run the demo which seeds the corpus into a fresh sandbox:
scripts/run-demo.sh
```

**Fix for persistent stack:**
```bash
# Copy a skill from the fixture corpus into the global skills volume:
mkdir -p /path/to/your/global-skills/rust-tokio-async-file-io
# Write a SKILL.md with the skill content, then trigger a rebuild:
curl -X POST http://127.0.0.1:3001/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rebuild_graph","arguments":{}}}'
```

## Monitoring Checklist

- [ ] `docker compose ps` shows all services healthy
- [ ] `curl http://127.0.0.1:3001/health` returns `200 OK`
- [ ] `curl http://127.0.0.1:8080/health` returns `200 OK`
- [ ] Ollama responds to `api/tags` within 5 seconds
- [ ] Redis responds to `PING`
- [ ] PostgreSQL responds to `pg_isready`
- [ ] Qdrant responds to `/collections` on port 16333 (REST, not gRPC 16334)
- [ ] No `circuit_state: "Open"` in graph-builder health
- [ ] `SessionEnd` hook configured in `~/.claude/settings.json`
- [ ] `PreCompact` hook includes `"trigger": "compact"` argument
- [ ] `scripts/doctor.sh` exits 0
