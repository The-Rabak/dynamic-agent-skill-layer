# Activation Demo Report

_Generated: 2026-06-04T05:10:32.700192Z_
_Elapsed: 2m31s (target: <10 min excluding model download)_

## Stack Health

| Check | Result |
|-------|--------|
| MCP server /health | `ok` |
| graph_version | `0` |
| cloud_calls | `none` (default path: Ollama only — no cloud calls) |

## Seeded Skills

- `rust-tokio-async-file-io`: Highly lexical and semantic overlap: rust + tokio + async + file + read all appear in both skill and prompt
- `docker-compose-service-health`: Strong lexical overlap on docker, compose, healthcheck, postgres, redis
- `git-conventional-commit-workflow`: Direct lexical match on git, commit, conventional, branch
- `rust-unit-testing-patterns`: Semantic overlap on rust + testing + async + tokio
- `security-secrets-management`: Semantic and lexical match on secrets, credentials, security, env

Total seeded: **5** skills (from `tests/fixtures/retrieval_corpus.json`)

Embedding model: `nomic-embed-text` (local Ollama)

## compile_context Status

| Prompt | `how to read files in rust with tokio async` |
| Status | `degraded` |
| cloud_calls | `none` |

## Transcript Ingest (Shipped Hook Path)

The shipped command-hook path was exercised:
`capture-transcript.sh` (SessionEnd hook) → `POST /ingest/transcript` → `transcript_ingest_queue` (Postgres)

| Step | Result |
|------|--------|
| Hook source | `session_end` |
| Queue row status | `pending` |
| Ingest check | `ok` |

## Queue Drain and .pending Drafts

The maintenance binary was run with `MAINTENANCE_RUN_ONCE=1` — the same
code path the production maintenance worker executes continuously.

| Step | Result |
|------|--------|
| Drain status | `ok` |
| Draft count | `5` |
| Extraction model | `granite4:3b` (local Ollama) |

- `/home/rabak/projects/dynamic-agent-skill-layer/target/tmp-live-extract-stress-1780544627698437312/.skills/run-tests/SKILL.md.pending`
- `/home/rabak/projects/dynamic-agent-skill-layer/target/tmp-live-extract-stress-1780544627698437312/.skills/reproduce-bug-from-logs/SKILL.md.pending`
- `/home/rabak/projects/dynamic-agent-skill-layer/target/tmp-live-extract-stress-1780544627698437312/.skills/none/SKILL.md.pending`
- `/home/rabak/projects/dynamic-agent-skill-layer/target/demo-sandbox-1780549218/.skills/handle-write-failure/SKILL.md.pending`
- `/home/rabak/projects/dynamic-agent-skill-layer/target/demo-sandbox-1780549218/.skills/mkdir-parents-before-file-write/SKILL.md.pending`

**Human gate:** `.pending` files require manual rename to `.md` before they
take effect. No auto-approval occurs. This is the constitution-required human gate.

## Warnings

None

## Time-to-Wow

Elapsed from script start to completion: **2m31s**
Target: under 10 minutes excluding model download.

The live E2E suite (18/18 green, 147s) demonstrates the full path under load.
See `tests/e2e/reports/latest-summary.md` for the reference run report.
