# Transcript Ingress Reference

How session transcripts enter the system for skill extraction.

## Production path: the durable ingest queue (todo 103)

In v1.5 the **session lifecycle hooks no longer ship a transcript path** to the
`extract_session` MCP tool. The shipped hook passed `transcript_ref: "{{transcript_path}}"`,
but `{{transcript_path}}` is an **absolute host path**: `validate_ref` rejects absolute paths,
and even if it accepted them the container cannot resolve a host path. Every real `SessionEnd`
therefore returned `failed` (fire-and-forget, so silently) and the self-growth loop never ran.

The production ingress is now a **durable Postgres queue**:

```
SessionEnd / PreCompact  (Claude Code command hook)
  → config/claude-code/capture-transcript.sh
       reads {{transcript_path}} on the HOST (where the path is valid)
       POSTs {session_id, repo_path, source, content} to
       http://127.0.0.1:3001/ingest/transcript   (X-Ingest-Secret header)
  → mcp-server inserts a row into transcript_ingest_queue   (dedup on blake3 content_hash)
  → maintenance worker drains pending rows (FOR UPDATE SKIP LOCKED)
       feeds content to the extractor via transcript_inline  ← path validator NEVER runs
       writes .pending drafts, marks the row processed
```

Because the drain uses `transcript_inline`, the path-based `validate_ref` boundary documented
below is **bypassed for the production hook flow**. The `transcript_ref` contract still applies
to direct `extract_session` MCP calls (tests, manual invocation, future harnesses).

### Execution model — nothing blocks the user

Every stage is asynchronous with respect to the session:

1. **The hook returns immediately.** `capture-transcript.sh` reads the hook payload from stdin
   (the only thing it must do synchronously) and then hands the transcript read + POST to a
   **detached** worker (`setsid … &`). The session never waits on transcript IO or the network —
   the hook returns in single-digit milliseconds. It always exits `0` and swallows every error,
   so a capture problem can never block or fail the harness.
2. **The ingest endpoint only enqueues.** `POST /ingest/transcript` does a single `INSERT` and
   returns; it does **not** run extraction.
3. **Extraction runs in the background.** The slow, fallible LLM extraction happens in the
   maintenance worker's queue drain (startup + cron loop), fully decoupled from any session.
   Failures are logged and retried/parked — they never propagate to a user interaction.

This **folds T07** (crash-safe transcript reconciliation): the queue row is both the work item
and the dedup marker, and the drain replaces T07's filesystem-scan reconcile. Capture runs at
two points (`PreCompact` + `SessionEnd`) to narrow the crash window; a session that fires
neither hook is the accepted residual gap (no host-filesystem scan is reintroduced).

See the *Transcript Ingest Queue* section of [`capability-catalog.md`](capability-catalog.md)
for the endpoint, queue states, reason codes, and the `TRANSCRIPT_INGEST_SECRET` /
`MAINTENANCE_TRANSCRIPT_DRAIN` env vars.

## Trust Boundary

V1.1 uses a **mounted read-only transcript root** as the trust boundary. The session-extractor never accepts raw host filesystem paths directly. All transcript references must be relative paths under the mounted `CLAUDE_TRANSCRIPT_ROOT` directory.

## Transcript Format

Transcripts are **JSONL files** (one JSON object per line) with this structure:

```jsonl
{"speaker": "user", "content": "How do I set up logging in Rust?"}
{"speaker": "assistant", "content": "Use tracing-subscriber with JSON formatting..."}
{"speaker": "user", "content": "Can you show me an example?"}
```

Each line is a `TranscriptEntry`:
- `speaker`: `"user"` or `"assistant"`
- `content`: The message text

## Mount Contract

### Docker Compose Configuration

```yaml
mcp-server:
  volumes:
    - ${CLAUDE_TRANSCRIPT_ROOT:-./tests/fixtures}:/transcripts:ro
```

### Environment Variables

| Variable | Host Path | Container Path | Access |
|----------|-----------|----------------|--------|
| `CLAUDE_TRANSCRIPT_ROOT` | `./tests/fixtures` (default) | `/transcripts` | Read-only |

### Runtime Resolution

When `extract_session` receives a request:

1. `transcript_ref` is validated to be a relative path (no `..` segments, no absolute paths)
2. The resolved path is `CLAUDE_TRANSCRIPT_ROOT / transcript_ref`
3. If the file does not exist or is outside the mount, `invalid_transcript_ref` is returned

## Request Contract

### With `transcript_ref` (production)

```json
{
  "transcript_ref": "2026-05-21-session-001.jsonl",
  "transcript_inline": null,
  "session_id": "session-001",
  "repo_path": "/path/to/repo"
}
```

**Validation rules:**
- `transcript_ref` must not contain `..` or start with `/`
- Resolved path must exist under `CLAUDE_TRANSCRIPT_ROOT`
- File must be readable JSONL

### With `transcript_inline` (tests / future harnesses)

```json
{
  "transcript_ref": "inline-test",
  "transcript_inline": "{\"speaker\":\"user\",\"content\":\"test\"}\n",
  "session_id": "test-session",
  "repo_path": "/tmp/test-repo"
}
```

When `transcript_inline` is provided, `transcript_ref` validation is skipped and the inline content is parsed directly.

## Error Codes

| Code | Cause | Fix |
|------|-------|-----|
| `invalid_transcript_root` | `CLAUDE_TRANSCRIPT_ROOT` not configured | Set env var or volume mount |
| `invalid_transcript_ref` | Path traversal attempt or file not found | Use relative path under mount |
| `transcript_read_failed` | IO error reading file | Check permissions, disk space |
| `invalid_transcript_payload` | JSONL parse error | Validate transcript format |

## Security Notes

- The transcript mount is **read-only** (`:ro`) in Docker Compose
- Raw host paths are never accepted — only relative refs under the mount
- Path traversal (`../`) is rejected at validation time
- The session-extractor runs inside the container and cannot access host paths outside the mount

## Example: Adding a New Transcript

1. Place the JSONL file in your host transcript directory:
   ```bash
   cp my-session.jsonl ~/.claude/transcripts/
   ```

2. Ensure `CLAUDE_TRANSCRIPT_ROOT` points to that directory in `.env`:
   ```bash
   CLAUDE_TRANSCRIPT_ROOT=~/.claude/transcripts
   ```

3. Call `extract_session` with the filename:
   ```json
   {
     "transcript_ref": "my-session.jsonl",
     "session_id": "my-session",
     "repo_path": "/home/user/my-project"
   }
   ```

## Future Harness Support

V1.1 supports Claude Code transcripts natively. Other harnesses can use `transcript_inline` for testing. V2 may add native support for additional harness transcript formats via the same `transcript_ref` contract with format detection.
