# Capability Catalog

Reference for all MCP tools, events, lifecycle states, and degraded reason codes in the Dynamic Agent Skill Layer V1.1.

Deep architecture reference: [`docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`](../architecture/2026-05-21-skill-layer-v1-1-architecture.md)

## MCP Tool Contracts

### `compile_context`

**Purpose:** Compile task-relevant skill context at session start.

**Request:**
```json
{
  "prompt": "how do I read a file in rust",
  "session_id": "uuid-v7",
  "repo_path": "/absolute/path/to/repo"
}
```

**Response statuses (canonical):**

| Status | Meaning | When |
|--------|---------|------|
| `ok` | Context compiled successfully | Skills found, retrieval healthy |
| `no_match` | No relevant skills found | Healthy retrieval, empty result |
| `degraded` | Partial or failed retrieval | Infrastructure dependency unavailable |
| `duplicate_suppressed` | Already compiled this session | Session suppression active for current graph version |

**Response fields:**
- `status`: one of the four statuses above
- `reason_code`: machine-readable reason (e.g., `no_relevant_skills`, `embedding_provider_unavailable`)
- `additional_context`: compiled markdown (present for `ok` and `degraded` with partial results)
- `health`: per-dependency status map (`ollama`, `qdrant`, `postgres`, `redis`, `filesystem_index`)
- `scopes_considered`: list of scope IDs searched
- `graph_version`: current graph version at time of request
- `latency_ms`: end-to-end latency in milliseconds
- `source`: origin of the response — `"retrieval"` (fresh), `"cache"` (cached hit), or `"suppression"` (duplicate suppressed)

**Latency target:** <500ms p95 (verified by `cargo bench --bench compile_context_bench`)

### `find_skill`

**Purpose:** Search for a specific skill by name or semantic similarity.

**Request:**
```json
{
  "query": "retrieval scoring",
  "scope_filter": "project"
}
```

**Response:** Ranked list of matching skills with scores and scope information.

### `extract_session`

**Purpose:** Schedule asynchronous skill extraction from a session transcript.

**Request:**
```json
{
  "transcript_ref": "2026-05-21-session-001.jsonl",
  "transcript_inline": null,
  "session_id": "session-001",
  "repo_path": "/path/to/repo"
}
```

**Transcript ingress contract:** `transcript_ref` must be rooted under the mounted `CLAUDE_TRANSCRIPT_ROOT` directory. Raw host paths are not part of the V1.1 trust boundary. See [`transcript-ingress.md`](transcript-ingress.md) for full details.

**Response:**
- `status`: `processing` or `failed`
- `reason_code`: failure reason if `failed`
- `job_id`: UUIDv7 tracking identifier
- `provider`: `claude` or `ollama`

### Admin Tools

| Tool | Purpose | Authority |
|------|---------|-----------|
| `rebuild_graph` | Trigger full graph rebuild | Trigger-only |
| `inspect_skill` | Read skill + subunits by ID | Read-only |
| `list_communities` | List HDBSCAN communities | Read-only |
| `get_pending_extractions` | List pending extraction jobs | Read-only |

Admin tools are unauthenticated in V1.1 and MUST only be exposed on localhost or private network surfaces (constitution §Deferred-risk guard).

## Event Catalog

Canonical event set for V1.1:

| Event | Emitted By | Payload Summary |
|-------|-----------|-----------------|
| `skill.file_changed` | graph-builder | `{file_path, change_type, idempotency_key}` |
| `skill.extraction_requested` | session-extractor | `{job_id, provider, session_id, transcript_ref}` |
| `extraction.completed` | session-extractor | `{job_id, provider, source_session_id, draft_count, draft_paths}` |
| `extraction.failed` | session-extractor | `{job_id, provider, error}` |
| `graph.rebuilt` | graph-builder | `{graph_version, skills_count, communities_count}` |
| `graph.rebuild_failed` | graph-builder | `{error, scope_id}` |
| `skill.retired` | maintenance-worker | `{skill_id, reason, retirement_score}` |
| `skill.merged` | maintenance-worker | `{source_skill_ids, target_skill_id, merge_confidence}` |

**Note:** There is no `skill.approved` event in V1.1. Approval is a filesystem operation (renaming `.pending` to `.md`).

## Lifecycle States

### Skill Status

| Status | Meaning |
|--------|---------|
| `Draft` | New skill, not yet in graph |
| `Ready` | Active in graph, available for retrieval |
| `Deprecated` | Still in graph, lower priority |
| `Retired` | Removed from graph, `.retired` marker exists |

### Lifecycle Status

| Status | Meaning |
|--------|---------|
| `Draft` | Pending human approval |
| `Active` | Currently serving |
| `Retired` | Gracefully removed |
| `Rejected` | Explicitly rejected by human |
| `Deleted` | Hard deletion (rare) |

### File Extensions

| Extension | State |
|-----------|-------|
| `.md` | Active skill |
| `.pending` | Proposed skill awaiting approval |
| `.retired` | Retired skill marker |
| `.rejected` | Explicitly rejected |

## Degraded Reason Codes

| Reason Code | Meaning | Recovery |
|-------------|---------|----------|
| `embedding_provider_unavailable` | Ollama unreachable | Check Ollama container health |
| `embedding_timeout` | Embedding call exceeded timeout | Check Ollama load / model loaded |
| `embedding_invalid_input` | Prompt too long or malformed | Reduce prompt length |
| `embedding_unexpected` | Unknown embedding error | Check logs, restart Ollama |
| `retrieval_degraded` | Generic retrieval failure | Check scope resolver, graph state |
| `scope_resolution_failed` | Could not resolve project/global scope | Check `GRAPH_BUILDER_PROJECT_ROOT`, git root |
| `invalid_transcript_root` | Transcript path outside mount | Verify `CLAUDE_TRANSCRIPT_ROOT` mount |
| `invalid_transcript_ref` | Transcript file not found | Check transcript file exists under mount |
| `transcript_read_failed` | IO error reading transcript | Check filesystem permissions |
| `invalid_transcript_payload` | Transcript JSONL malformed | Validate transcript format |
| `extraction_failed` | LLM extraction call failed | Check provider health, retry |
| `invalid_repo_path` | `repo_path` does not exist | Verify path in request |
| `scope_resolution_failed` | Cannot resolve output scope | Check `SKILL_GLOBAL_PATHS` env var |
| `pending_draft_write_failed` | Cannot write `.pending` file | Check directory permissions |
| `event_publication_failed` | Redis Streams publish failed | Check Redis health |
| `rebuild_blocked_by_circuit_breaker` | Too many rebuild failures | Wait for circuit breaker cooldown |

## Scope Configuration

| Variable | Purpose | Default |
|----------|---------|---------|
| `GRAPH_BUILDER_PROJECT_ROOT` | Host path to git repository root | `.` |
| `GRAPH_BUILDER_GLOBAL_ROOT` | Host path to global skills | `./docs` |
| `SKILL_GLOBAL_HOST_PATH` | Host path mounted into container | `./docs` |
| `SKILL_GLOBAL_PATHS` | Container path for global skills | `/skills/global` |
| `SKILL_GLOBAL_ALLOWED_ROOTS` | Absolute allowlist for path validation | `/skills/project,/skills/global` |
| `CLAUDE_TRANSCRIPT_ROOT` | Host path to transcript directory | `./tests/fixtures` |

## Redis Event Envelope Schema

```json
{
  "event_id": "uuid-v7",
  "event_type": "skill.file_changed",
  "correlation_id": "uuid-v7",
  "idempotency_key": "file_path:mtime_hash",
  "schema_version": 1,
  "timestamp": "2026-05-21T12:00:00Z",
  "payload": { ... }
}
```

Idempotency is tracked via Redis `SETEX` with 24h TTL (not in-memory).
