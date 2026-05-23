---
status: complete
priority: p1
issue_id: "011"
tags: [code-review, architecture, mcp-server, t03]
dependencies: []
---

# T03 tools are not callable through MCP transport

## Problem Statement

T03 requires `compile_context` and `find_skill` to be registered and callable through the MCP server. Current `mcp-server` runtime only exposes in-process methods and prints registered tool names; it does not host protocol-callable MCP handlers.

## Findings

- T03 acceptance criteria require MCP-callable tools: `docs/tickets/2026-05-21-skill-layer-v1-1/03-single-scope-compile-context.md:59`.
- `crates/mcp-server/src/main.rs` builds app, prints tools, and blocks on Ctrl+C (`main.rs:12-19`) with no protocol serving path.
- `crates/mcp-server/src/lib.rs` exposes direct methods (`compile_context`, `find_skill`) instead of transport handlers (`lib.rs:42-48`).
- Reviewer consensus flagged this as merge-blocking for user-story delivery.

## Proposed Solutions

### Option 1: Implement real MCP transport handlers in `mcp-server` (Recommended)

**Approach:** Wire protocol runtime and register `compile_context` / `find_skill` handlers that delegate to existing orchestration logic.

**Pros:**
- Meets T03 acceptance criteria directly.
- Preserves current retrieval/compiler implementation investment.

**Cons:**
- Requires transport wiring and protocol-level test coverage.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Explicitly redefine T03 as in-process tracer bullet only

**Approach:** Amend ticket scope/AC to remove MCP-callable requirement for this slice.

**Pros:**
- Minimal code change now.

**Cons:**
- Alters promised outcome and architecture handoff.
- Introduces deliberate scope drift.

**Effort:** Small

**Risk:** High

## Recommended Action


## Technical Details

**Affected files:**
- `crates/mcp-server/src/main.rs`
- `crates/mcp-server/src/lib.rs`
- `docs/tickets/2026-05-21-skill-layer-v1-1/03-single-scope-compile-context.md`

**Related components:**
- MCP protocol boundary
- Tool registration contract
- T03 user-visible outcome

**Database changes (if any):**
- No

## Resources

- Ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/03-single-scope-compile-context.md`
- Architecture artifact: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`

## Acceptance Criteria

- [x] `compile_context` is callable via MCP protocol.
- [x] `find_skill` is callable via MCP protocol.
- [x] Protocol-level integration coverage exists for both tools.
- [x] Ticket AC for MCP callability is satisfied without waivers.

## Work Log

### 2026-05-23 - Review synthesis (full working tree)

**By:** Copilot CLI

**Actions:**
- Re-ran review agents against tracked + untracked T03 files.
- Verified T03 AC vs current `mcp-server` runtime behavior.
- Recorded MCP callability gap as blocking finding.

**Learnings:**
- In-process method exposure is not equivalent to MCP-callable contract delivery.

### 2026-05-23 - Implementation complete

**By:** Copilot CLI

**Actions:**
- Added MCP JSON-RPC protocol handling (`tools/list`, `tools/call`) in `crates/mcp-server/src/protocol.rs`.
- Exposed protocol request handling via `McpServerApp::handle_json_rpc`.
- Added HTTP serving entrypoint (`POST /mcp`) and switched `mcp-server` runtime to serve requests on `MCP_SERVER_ADDR` (default `127.0.0.1:3001`).
- Added integration test `json_rpc_tools_list_and_call_compile_context` proving MCP-callable flow.

**Learnings:**
- A thin protocol adapter preserves feature-home boundaries while meeting callability requirements.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY
- Severity rationale: without callable transport, the promised zero-touch MCP path is not delivered.
