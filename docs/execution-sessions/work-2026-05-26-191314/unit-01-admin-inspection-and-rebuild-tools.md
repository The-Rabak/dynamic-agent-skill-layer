---
unit: "T09: Admin inspection and rebuild tools"
unit_number: 1
unit_kind: expansion
serves: "SC-4 manual rebuild/inspection operations and SC-5 read-only graph visibility"
status: completed
attempt_count: 1
domains: [backend, testing]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/09-admin-inspection-and-rebuild-tools.md
session_id: work-2026-05-26-191314
---

## What Was Implemented

- Added a new `admin` crate as the dedicated feature home for online admin/debug tools.
- Implemented `rebuild_graph`, `inspect_skill`, and `list_communities` tool orchestration with explicit request/response contracts.
- Added rebuild trigger seams:
  - `FilesystemGraphRebuildTrigger` delegates to `graph-builder` orchestration.
  - `NoopGraphRebuildTrigger` fails closed when runtime wiring is unavailable.
- Composed admin tools into `mcp-server` registration and JSON-RPC dispatch without moving business logic into MCP transport handlers.
- Added integration tests for admin tool payload contracts and graph-builder-triggered rebuild behavior.

## Files Changed

- `Cargo.toml` -- workspace now includes `crates/admin`
- `Cargo.lock` -- lockfile updated for new crate/dependency graph
- `crates/admin/Cargo.toml` -- created admin crate manifest
- `crates/admin/src/lib.rs` -- created module export
- `crates/admin/src/tools.rs` -- created tool contracts, readers, triggers, and handlers
- `crates/mcp-server/Cargo.toml` -- added `admin` and `graph-builder` dependencies plus integration test target
- `crates/mcp-server/src/lib.rs` -- wired admin tools into `McpServerApp`, tool registry, and seeded graph reader
- `crates/mcp-server/src/protocol.rs` -- added JSON-RPC list/call handling for `rebuild_graph`, `inspect_skill`, `list_communities`
- `tests/integration/test_admin_tools.rs` -- added admin integration coverage
- `tests/integration/test_compile_context.rs` -- updated tool-list expectations to include admin tools

## Problems Encountered

### Problem 1: missing graph-builder dependency in mcp-server
- **Error:** `error[E0432]: unresolved import 'graph_builder'`
- **Root cause:** `mcp-server` started using `ScopeRoot` to compose rebuild triggering but lacked a direct dependency declaration.
- **Fix:** added `graph-builder = { path = "../graph-builder" }` to `crates/mcp-server/Cargo.toml`.

## Patterns Discovered

- MCP tool exposure is dual-surface: update both `registered_tools()` and JSON-RPC `tools/list` + `tools/call` dispatch.
- Feature-home split stays clean by placing admin contracts/logic in `crates/admin` and leaving `mcp-server` as composition/transport.

## TDD Evidence

- **Red**
  - Command: `cp tests/integration/test_compile_context.rs /home/rabak/projects/dasl-red/tests/integration/test_compile_context.rs && cd /home/rabak/projects/dasl-red && cargo test -q -p mcp-server --test test_compile_context registers_compile_context_find_skill_and_extract_session_tools`
  - Result: FAIL
  - Evidence: the registration contract test failed because baseline tools omitted `rebuild_graph`, `inspect_skill`, and `list_communities`, proving the admin behavior was missing (not setup noise).
- **Green**
  - Command: `cargo test -q -p mcp-server --test test_compile_context registers_compile_context_find_skill_and_extract_session_tools`
  - Result: PASS
  - Evidence: the same registration contract now passes with all three admin tools present.
- **Post-Refactor Green**
  - Command: `cargo test -q -p mcp-server --test test_compile_context registers_compile_context_find_skill_and_extract_session_tools`
  - Result: PASS
  - Evidence: rerun remained green with no additional cleanup, proving no regression in the admin tool registration behavior.

## Test Results

- Command: `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- Result: PASS
- Attempts: 1
