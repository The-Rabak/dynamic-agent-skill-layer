# Capability Catalog

Reference for all MCP tools, events, lifecycle states, and degraded reason codes in the Dynamic Agent Skill Layer V1.5.

Deep architecture reference: [`docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`](../architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md)

## Claude Code Session Lifecycle Hook Contract

The skill layer wires into four Claude Code session lifecycle events. The example config lives at `config/claude-code/hooks.example.json`.

### Hook semantics

| Hook | Can block Claude? | Inject context? | Payload available |
|------|-----------------|-----------------|-----------------------------|
| `SessionStart` | No (observe only) | Yes (via tool result) | `initial_prompt`, `session_id`, `repo_path` |
| `PreCompact` | Yes (30s timeout) | Yes (via tool result) | `summary`, `session_id`, `repo_path` |
| `UserPromptSubmit` | Yes (30s timeout) | Yes (via tool result) | `prompt`, `session_id`, `repo_path` |
| `SessionEnd` | No (fire-and-forget) | No | `transcript_path`, `session_id`, `repo_path` |

**`SessionEnd` matchers:** `clear`, `resume`, `logout`, `prompt_input_exit`, `other`.

**Crash caveat:** `SessionEnd` does NOT fire on crash or SIGKILL. To narrow that hole, transcript capture runs at **two** points — `PreCompact` (full pre-summarization snapshot) and `SessionEnd` — and both push into a **durable Postgres queue** (`transcript_ingest_queue`) that survives worker restarts. A session that fires *neither* hook (host killed before any capture point) is the residual gap; for V1.5 that gap is **accepted** (dual capture mitigates it) rather than re-introducing a host-filesystem scan. See *Transcript ingest queue (self-growth loop)* below.

**Context injection limit:** `hookSpecificOutput.additionalContext` is capped at approximately 10,000 characters. Compiled context that exceeds this limit is truncated by the harness.

### Lifecycle hook wiring

```
SessionStart → compile_context (inject)            [cold start or resume]
PreCompact   → compile_context (trigger=compact)   [survive summarization]
             + capture-transcript.sh (pre_compact) [snapshot transcript → ingest queue]
UserPromptSubmit → compile_context (inject)        [subsequent prompts; suppressed after first Ok]
SessionEnd   → capture-transcript.sh (session_end) [self-growing loop trigger → ingest queue]
```

> **SessionEnd changed in v1.5 (todo 103).** It previously called the `extract_session`
> MCP tool with `transcript_ref: "{{transcript_path}}"`. `{{transcript_path}}` is an
> **absolute host path** that `validate_ref` rejects and the container cannot resolve, so
> every real `SessionEnd` silently `failed` — the self-growth loop never ran. It is now a
> host `command` hook (`config/claude-code/capture-transcript.sh`) that reads the transcript
> where the path is valid and POSTs its **content** to the localhost ingest endpoint. The
> maintenance worker drains the queue through `transcript_inline`, so the path validator is
> never exercised.

### `result_policy` key semantics

Each hook entry in `config/claude-code/hooks.example.json` carries a `result_policy` object that controls how the Claude Code harness interprets the tool result. The harness — not Claude — enforces this policy.

| Policy key | Status values listed | Harness action |
|------------|---------------------|---------------|
| `inject_additional_context_on` | `ok`, `degraded` | Inject `additional_context` from the tool response into the conversation as additional context before Claude's next turn |
| `suppress_duplicate_on_healthy` | `ok`, `no_match` | Mark the session as already compiled; subsequent calls to the same hook within the same session return `duplicate_suppressed` without re-running retrieval |
| `retry_on` | `degraded` | Re-invoke the tool (once, immediately) when the listed status is returned — used on `UserPromptSubmit` to retry on transient degradation |
| `ignore_on` | `duplicate_suppressed`, `no_match`, `processing`, `failed`, `degraded` (hook-dependent) | Silently discard the tool result — no context injection, no retry, no error surfaced to Claude |

Canonical status values: `ok`, `no_match`, `degraded`, `duplicate_suppressed`, `processing`, `failed`.

### Compaction re-injection

When Claude Code compacts the conversation (summarizes history), context in the system prompt is lost. The `PreCompact` hook re-invokes `compile_context` with `trigger: "compact"` immediately before summarization so the summary includes fresh skill context. The `trigger` field bypasses session suppression for this single call — without it, suppression would return `DuplicateSuppressed` and the re-inject would be a silent no-op.

### SessionEnd extraction and human gate

`SessionEnd` (and `PreCompact`) capture the transcript into the durable ingest queue; the maintenance worker drains it and produces `.pending` files under `.skills/` — never `.md` files. A human must rename `.pending → .md` to approve a skill. There is no auto-approval path.

---

## Transcript Ingest Queue (self-growth loop)

The self-growth trigger (SC-V1.5-B) flows through a durable Postgres queue rather than
shipping a transcript path across the container boundary. This **folds T07** (crash-safe
reconciliation): the queue row is both the work item and the dedup marker, and the drain
replaces T07's filesystem-scan reconcile.

```
host command hook (SessionEnd / PreCompact)
  → capture-transcript.sh reads {{transcript_path}} (valid on host)
  → POST {session_id, repo_path, source, content} to 127.0.0.1:3001/ingest/transcript
  → server inserts a row into transcript_ingest_queue (dedup on content_hash)
  → maintenance worker claims pending rows (FOR UPDATE SKIP LOCKED)
  → feeds content via transcript_inline to the extractor (path validator never runs)
  → writes .pending drafts, marks the row processed
```

### Ingest endpoint — `POST /ingest/transcript`

Localhost-bound (`127.0.0.1`) HTTP endpoint on the MCP server (not an MCP tool).

**Request:**
```json
{ "session_id": "uuid", "repo_path": "/abs/repo", "source": "session_end", "content": "<jsonl>" }
```
- `source`: `session_end` | `pre_compact` | `reconcile`
- `content`: raw transcript JSONL (capped at 10 MiB; oversize → `413`)

**Auth:** shared-secret header `X-Ingest-Secret` matched against `TRANSCRIPT_INGEST_SECRET`
when that env var is set (coordinated with todo 099). When unset, the endpoint relies on the
loopback binding alone and logs a warning. A mismatched secret returns `401`.

**Responses:** `202 enqueued` (new row) · `200 duplicate` (same `content_hash`) · `400/413` (contract) · `401` (secret) · `503` (queue unconfigured or DB error).

### Queue states (`transcript_ingest_queue.status`)

| Status | Meaning |
|--------|---------|
| `pending` | Captured, awaiting drain |
| `processing` | Claimed by a drain sweep |
| `processed` | Drafts written (or extraction yielded zero candidates); terminal success |
| `failed` | Retried `MAX_TRANSCRIPT_DRAIN_ATTEMPTS` (3) times; parked with `error` |

Dedup is keyed on `content_hash` (blake3 of content), so a `SessionEnd` capture that repeats
a `PreCompact` tail is an idempotent no-op. The drain marks a row `processed` only after the
draft write returns, so a crash mid-drain leaves the row reclaimable.

---

## MCP Tool Contracts

### `compile_context`

**Purpose:** Compile task-relevant skill context for the current session.

**Request:**
```json
{
  "prompt": "how do I read a file in rust",
  "session_id": "uuid-v7",
  "repo_path": "/absolute/path/to/repo",
  "trigger": "compact"
}
```

The `trigger` field is optional. Omit it (or pass `null`) for all calls except post-compaction re-injection. Setting `trigger` to `"compact"` bypasses session suppression for that single call. Other values are logged and treated as ordinary calls.

**Request fields:**
- `prompt`: natural-language task description
- `session_id`: stable session identifier (UUIDv7 recommended)
- `repo_path`: absolute path to the current repository root
- `trigger` (optional): lifecycle event identifier; `"compact"` bypasses session suppression

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
- `health`: per-dependency status map for the compile_context read path — keys are `ollama`, `skill_snapshot_sync`, `filesystem_index`. Note: `qdrant_write_side` appears only on the infrastructure `/health` endpoint (it is the durable write-side store, not a read-path dependency).
- `scopes_considered`: list of scope IDs searched
- `graph_version`: current graph version at time of request
- `latency_ms`: end-to-end latency in milliseconds
- `source`: origin of the response — `"retrieval"` (fresh), `"cache"` (cached hit), or `"suppression"` (duplicate suppressed). Compaction-bypass calls always return `"retrieval"` regardless of prior suppression state.

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
- `provider`: `ollama` (default) or `claude`

#### Extraction provider selection

Extraction is provider-selectable (constitution v2.0.0). **Ollama is the default
local path and needs no cloud key.** Two Claude paths are available: a
subscription-based CLI path and an API-key path.

| `EXTRACT_SESSION_PROVIDER` value | Provider | Auth requirement |
|----------------------------------|----------|-----------------|
| unset / blank / `ollama` | Ollama (local default) | None |
| `claude` | Claude Code CLI (`ClaudeCodeExtractor`) — subscription-based | An already-logged-in `claude` CLI in the run environment; **no API key, no credential handling** |
| `claude-api` | Anthropic Messages API (`ClaudeExtractor`) — direct API call | `ANTHROPIC_API_KEY` |

**Environment constraint for `claude` (CLI path):** This provider does **not** read,
store, or pass any credentials. It simply shells out to the `claude` binary, which
uses whatever login already exists in its environment (`~/.claude`). The only
requirement is that the `claude` CLI is installed and already authenticated where the
extractor process runs. That holds on a host where you've run `claude` interactively,
but **not** in the stock compose container, which ships neither the CLI nor a login.
The compose default therefore remains `ollama`. For containerised deployments use
`claude-api` (API key) or leave unset for Ollama.

| Variable | Purpose | Default |
|----------|---------|---------|
| `EXTRACT_SESSION_PROVIDER` | Extraction provider: `ollama`, `claude` (CLI), or `claude-api` (API key). Unset or blank ⇒ `ollama`. Unknown ⇒ loud startup error. | `ollama` |
| `EXTRACT_SESSION_MODEL` | Model id for Claude CLI and API providers | `claude-sonnet-4-6` |
| `CLAUDE_CLI_PATH` | Path to the `claude` CLI binary (CLI path only) | `claude` (from `$PATH`) |
| `CLAUDE_CODE_EXTRACTION_TIMEOUT_MS` | Inner per-call timeout for the CLI path | `120000` |
| `ANTHROPIC_API_KEY` | Anthropic API key. **Required** when `EXTRACT_SESSION_PROVIDER=claude-api` — a missing key fails loudly at startup (no silent fallback). Read from the environment, never committed. | _(unset)_ |
| `ANTHROPIC_BASE_URL` | Anthropic API base URL (no `/v1/messages` suffix) | `https://api.anthropic.com` |
| `CLAUDE_EXTRACTION_TIMEOUT_MS` | Inner per-call timeout for the API path | `30000` |
| `OLLAMA_EXTRACTION_MODEL` | Local extraction model for the Ollama provider | `gemma4:e4b` |
| `OLLAMA_EXTRACTION_TIMEOUT_MS` | Inner per-call timeout for Ollama CPU inference. The default (`120000`) is an **unmeasured placeholder** — confirm single-job p50/p95 against the target host. The worker-pool (outer) timeout stays ≥ 1.5× this value. | `120000` |

**Opt into Claude CLI (subscription, host-only):**

```bash
EXTRACT_SESSION_PROVIDER=claude
# No API key needed — uses your Claude Code subscription via the local CLI.
EXTRACT_SESSION_MODEL=claude-sonnet-4-6  # optional override
CLAUDE_CLI_PATH=/home/user/.local/bin/claude  # optional: explicit path to claude binary
```

**Opt into Claude API (API key, containerised-compatible):**

```bash
EXTRACT_SESSION_PROVIDER=claude-api
ANTHROPIC_API_KEY=sk-ant-...           # required; never commit
EXTRACT_SESSION_MODEL=claude-sonnet-4-6  # optional override
ANTHROPIC_BASE_URL=https://api.anthropic.com  # optional override
```

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

### Approval-to-retrievable latency (online refresh)

A skill approved while the server is running becomes retrievable **without restarting any process** (SC-V1.5-A). This is **not instantaneous**: the path is

1. human renames `.pending → .md` (approval),
2. graph-builder detects the change on its **poll interval** (`GRAPH_BUILDER_POLL_INTERVAL_MS`, default **~15s**), rebuilds, and publishes `graph.rebuilt` to the shared Redis stream,
3. the running `mcp-server` consumes `graph.rebuilt`, reloads the snapshot from Postgres, and atomically swaps the in-memory read model.

The dominant term is the graph-builder poll interval, so the worst-case approval→retrievable window is bounded by roughly that interval (~15s by default) plus a small reload+swap delay. "No restart" therefore means "no restart, refreshed within one poll cycle" — not "instant". The subscriber coalesces bursts (multiple rebuilds collapse into one reload of the newest version) and can be disabled with the temporary rollback flag `MCP_GRAPH_REFRESH=off`, which falls back to boot-only graph loading.

### Graph-refresh subscriber liveness

There is no dedicated health field for the `graph_refresh_subscriber` component (the goroutine inside `mcp-server` that watches the Redis stream for `graph.rebuilt` events). Agents infer liveness indirectly: if `graph_version` in `compile_context` responses advances after a known `graph.rebuilt` event, the subscriber is alive. A stall — `graph_version` does not advance even after a confirmed rebuild — implies the subscriber is dead or in an exponential-backoff reconnect loop.

A dedicated `graph_refresh_subscriber` component in the `/health` response is a possible future enhancement (post-V1.5).

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
| `transcript_content_empty` | Ingest payload had blank content | Capture hook found an empty transcript; no-op |
| `transcript_content_too_large` | Ingest content exceeded 10 MiB cap | Returned `413`; transcript too large to enqueue |
| `transcript_ingest_invalid_contract` | Bad `source` or blank `session_id` | Returned `400`; check capture hook payload |
| `transcript_queue_persistence_failed` | DB error enqueuing/draining | Returned `503`; check Postgres health |
| `ingest_secret_mismatch` | `X-Ingest-Secret` did not match | Returned `401`; align hook + server secret |

## Scope Configuration

| Variable | Purpose | Default |
|----------|---------|---------|
| `GRAPH_BUILDER_PROJECT_ROOT` | Host path to git repository root | `.` |
| `GRAPH_BUILDER_GLOBAL_ROOT` | Host path to global skills | `./docs` |
| `SKILL_GLOBAL_HOST_PATH` | Host path mounted into container | `./docs` |
| `SKILL_GLOBAL_PATHS` | Container path for global skills | `/skills/global` |
| `SKILL_GLOBAL_ALLOWED_ROOTS` | Absolute allowlist for path validation | `/skills/project,/skills/global` |
| `CLAUDE_TRANSCRIPT_ROOT` | Host path to transcript directory | `./tests/fixtures` |
| `TRANSCRIPT_INGEST_SECRET` | Shared secret for `POST /ingest/transcript` (`X-Ingest-Secret`) | _(unset → loopback-only)_ |
| `MAINTENANCE_TRANSCRIPT_DRAIN` | Set `off` to disable the transcript queue drain (rollback) | _(on)_ |
| `OLLAMA_EXTRACTION_ENDPOINT` | Ollama `/api/generate` URL for skill extraction (distinct from `OLLAMA_URL`) | `http://127.0.0.1:11434/api/generate` |

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
