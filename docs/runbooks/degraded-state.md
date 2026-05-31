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
    "qdrant": "ok",
    "postgres": "ok",
    "redis": "ok",
    "filesystem_index": "ok",
    "reason": "embedding_provider_unavailable"
  }
}
```

### 2. Health Endpoints

```bash
# MCP server health
curl http://127.0.0.1:3001/health

# Graph builder health
curl http://127.0.0.1:8080/health
```

Both return dependency-level status:
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

**Recovery:** T07 (level-triggered reconcile loop) reconciles sessions that lacked a `SessionEnd` event on the next startup by replaying extraction from the session transcript. **Until T07 is deployed, manual re-triggering is required:**

```bash
# Re-trigger extraction for a specific session via the MCP server
curl -s -X POST http://localhost:3001/mcp \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "extract_session",
      "arguments": {
        "transcript_ref": "<session-transcript-filename>.jsonl",
        "session_id": "<session-id>",
        "repo_path": "/path/to/repo"
      }
    }
  }'
```

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

## Monitoring Checklist

- [ ] `docker compose ps` shows all services healthy
- [ ] `curl http://127.0.0.1:3001/health` returns `200 OK`
- [ ] `curl http://127.0.0.1:8080/health` returns `200 OK`
- [ ] Ollama responds to `api/tags` within 5 seconds
- [ ] Redis responds to `PING`
- [ ] PostgreSQL responds to `pg_isready`
- [ ] Qdrant responds to `/collections`
- [ ] No `circuit_state: "Open"` in graph-builder health
- [ ] `SessionEnd` hook configured in `~/.claude/settings.json`
- [ ] `PreCompact` hook includes `"trigger": "compact"` argument
