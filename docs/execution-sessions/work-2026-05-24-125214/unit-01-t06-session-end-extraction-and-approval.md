---
unit: "T06 session-end extraction and approval"
unit_number: 1
unit_kind: expansion
serves: "SC-3 session transcript extraction with human-gated pending drafts"
status: completed
attempt_count: 2
domains: [backend, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/06-session-end-extraction-and-approval.md
session_id: work-2026-05-24-125214
---

## What Was Implemented

Added a `session-extractor` crate and a thin MCP `extract_session` tool route. Implemented transcript reference trust-boundary checks under `CLAUDE_TRANSCRIPT_ROOT`, Claude/Ollama provider routing using infrastructure adapters, asynchronous extraction orchestration with immediate processing response, `.pending` draft writing with metadata/tags, and extraction lifecycle event capture.

## Files Changed

- `Cargo.toml` -- added `crates/session-extractor` workspace member
- `Cargo.lock` -- lockfile refresh
- `crates/session-extractor/Cargo.toml` -- new crate manifest
- `crates/session-extractor/src/lib.rs` -- extraction coordinator + async job lifecycle
- `crates/session-extractor/src/transcripts.rs` -- transcript_ref validation and JSONL parsing
- `crates/session-extractor/src/providers/claude.rs` -- Claude adapter builder
- `crates/session-extractor/src/providers/ollama.rs` -- Ollama adapter builder
- `crates/session-extractor/src/writer.rs` -- `.pending` draft writer
- `crates/mcp-server/Cargo.toml` -- added dependency/test target wiring
- `crates/mcp-server/src/lib.rs` -- registered extract_session tool
- `crates/mcp-server/src/protocol.rs` -- JSON-RPC tools/list + tools/call integration
- `crates/mcp-server/src/tools/extract_session.rs` -- thin MCP transport tool adapter
- `tests/integration/test_extract_session.rs` -- extraction integration coverage
- `tests/fixtures/sample-transcript.jsonl` -- sample transcript fixture
- `tests/integration/test_compile_context.rs` -- updated tool-registration assertions
- `tests/integration/env_guard.rs` -- env lock race hardening for integration tests

## Problems Encountered

### Problem 1: integration test instability
- **Error:** degraded-vs-ok assertion mismatch while running full workspace suite
- **Root cause:** initialization side effects and shared process env mutation race in tests
- **Fix:** lazy initialization in extract-session tool and lock-scoped env guard setup/restore

## Patterns Discovered

- MCP tool handlers should stay transport-thin and delegate to feature-home services.
- Environment mutation helpers in integration tests must hold a lock across setup and restore.

## TDD Evidence

- **Red**
  - Command: `cargo test -p mcp-server --test test_extract_session -- --nocapture`
  - Result: FAIL
  - Evidence: test failed with assertion error: expected extraction to write `.pending` draft and emit extraction lifecycle event, but output file and event were missing. This proves the missing behavior before implementation.
- **Green**
  - Command: `cargo test -p mcp-server --test test_compile_context --test test_dual_scope --test test_extract_session`
  - Result: PASS
  - Evidence: extraction path and existing MCP behavior passed together in the same surface
- **Post-Refactor Green**
  - Command: `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
  - Result: PASS
  - Evidence: workspace and topology rerun stayed green after stabilization changes

## Test Results

- Command: `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 2
