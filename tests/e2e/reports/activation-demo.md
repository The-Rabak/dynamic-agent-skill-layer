# Activation Demo Report

_Generated: 2026-06-04T06:10:59.345493Z_
_Elapsed: 2m47s (target: <10 min excluding model download)_

## Stack Health

| Check | Result |
|-------|--------|
| MCP server /health | `ok` |
| graph_version | `2` |
| cloud_calls | `none` (derived from /health extraction_provider) |

## Seeded Skills

- `rust-tokio-async-file-io`
- `docker-compose-service-health`
- `git-conventional-commit-workflow`
- `rust-unit-testing-patterns`
- `security-secrets-management`

Total seeded: **5** skills (from `tests/fixtures/retrieval_corpus.json`)

Embedding model: `nomic-embed-text` (local Ollama)

## compile_context Status

| Field | Value |
|-------|-------|
| Prompt | `how to read files in rust with tokio async` |
| Status | `degraded` |
| graph_version | `2` |
| cloud_calls | `none` |

### Live Why These Skills (from actual compile_context response)

The section below is extracted directly from the `compile_context` response
— it is NOT a corpus annotation. An `ok` status with `graph_version > 0`
confirms a real graph rebuild completed before this call.

```
### Why These Skills
- rust-tokio-async-file-io: scope=global | bucket=low | semantic=0.807 | lexical=0.444
- git-conventional-commit-workflow: scope=global | bucket=low | semantic=0.363 | lexical=0.111
- rust-unit-testing-patterns: scope=global | bucket=low | semantic=0.506 | lexical=0.111
```

## Transcript Ingest (Shipped Hook Path)

The shipped command-hook path was exercised:
`capture-transcript.sh` (SessionEnd hook) → `POST /ingest/transcript` → `transcript_ingest_queue` (Postgres)

| Step | Result |
|------|--------|
| Hook source | `session_end` |
| Queue row status | `pending` |
| Queue lookup method | `session_id (synchronous POST)` |
| Ingest check | `ok` |

## Queue Drain and .pending Drafts

The maintenance binary was run with `MAINTENANCE_RUN_ONCE=1` — the same
code path the production maintenance worker executes continuously.
Discovery scoped to `target/demo-sandbox-*` only (not the full `target/`).

| Step | Result |
|------|--------|
| Drain status | `ok` |
| Draft count | `2` |
| Extraction model | `granite4:3b` (local Ollama) |

- `target/demo-sandbox-1780553292/.skills/handle-file-write-failure/SKILL.md.pending`
- `target/demo-sandbox-1780553292/.skills/mkdir-parents-if-not-exists/SKILL.md.pending`

**Human gate:** `.pending` files require manual rename to `.md` before they
take effect. No auto-approval occurs. This is the constitution-required human gate.

## Warnings

- compile_context returned `degraded` (reason: project_scope_resolution_failed). This is expected when calling the containerized mcp-server: the musl static binary has no git binary, so project scope resolution always fails for any repo_path. The global-scope retrieval DID succeed — `### Why These Skills` section is LIVE (graph_version=2). To get `ok`, run compile_context in-process (as the live e2e roundtrip test does) where git is available on the host.

## Time-to-Wow

Elapsed from script start to completion: **2m47s**
Target: under 10 minutes excluding model download.

The live E2E suite (18/18 green, 147s) demonstrates the full path under load.
See `tests/e2e/reports/latest-summary.md` for the reference run report.
