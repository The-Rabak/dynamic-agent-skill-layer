---
status: complete
priority: p1
issue_id: "015"
tags: [code-review, t04, architecture, mcp-server, retrieval, protects-user-story, constitution]
dependencies: []
---

# Use request repo_path for project scope resolution

## Problem Statement

`compile_context` receives `repo_path`, but retrieval project scope is currently resolved from server process startup CWD. This can return context for the wrong repository and breaks ticket T04 scope semantics.

## Findings

- `CompileContextRequest` carries `repo_path`, but retrieval API only takes `prompt`.
- `build_seeded_server` creates `GitRootProjectResolver` from `std::env::current_dir()` once at startup.
- Ticket/plan/architecture contracts expect project scope detection aligned with hook/session repo context.
- WHY impact: directly threatens SC-2 and can produce wrong first-prompt context for the user story.

## Proposed Solutions

### Option 1: Request-scoped resolver input

**Approach:** Extend retrieval request contract to include `repo_path`; build/resolve project scope from that per call.

**Pros:**
- Correct by construction for multi-repo sessions.
- Aligns with ticket/plan architecture contracts.

**Cons:**
- Requires API changes across mcp-server and retrieval boundaries.

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Resolver cache keyed by repo_path

**Approach:** Keep resolver abstraction but cache per-repo resolved roots and inject by request key.

**Pros:**
- Preserves seam and reduces repeated git calls.
- Better runtime performance under repeated calls.

**Cons:**
- Slightly higher complexity (cache invalidation strategy).

**Effort:** Medium

**Risk:** Medium

---

### Option 3: Document strict single-repo runtime and enforce

**Approach:** Explicitly reject requests where repo_path differs from startup repo and return deterministic degraded reason.

**Pros:**
- Smallest code delta.

**Cons:**
- Violates intended portability/zero-touch behavior for mixed sessions.
- Likely unacceptable for T04 goals.

**Effort:** Small

**Risk:** High

## Recommended Action

Implemented Option 1 (request-scoped resolver input) with minimal boundary-safe API changes:
- Added `repo_path` context propagation from `compile_context` into retrieval orchestration.
- Updated scope resolver contracts so project resolution uses request repo path per call.
- Kept MCP handlers transport-thin by only forwarding request data to the retriever boundary.

## Technical Details

**Affected files:**
- `crates/mcp-server/src/lib.rs`
- `crates/mcp-server/src/tools/compile_context.rs`
- `crates/retrieval/src/orchestrator.rs`
- `crates/retrieval/src/scope_resolution.rs`

## Resources

- Ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/04-dual-scope-retrieval-and-hooking.md`
- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Execution session: `docs/execution-sessions/work-2026-05-23-122414/STATE.md`

## Acceptance Criteria

- [x] Retrieval path consumes request repo context when resolving project scope.
- [x] Integration test proves different `repo_path` values isolate retrieval scope behavior.
- [x] No transport-thin boundary violation introduced in MCP handlers.
- [x] T04 SC-2 semantics remain satisfied.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Reviewed T04 uncommitted diff and reviewer outputs.
- Traced request and resolver flow.
- Identified contract drift vs ticket/plan/architecture expectations.

**Learnings:**
- Current suppression isolation uses `{session_id, repo_path}`, but retrieval scope resolution does not yet use request repo context.

### 2026-05-23 - Fix implemented and verified

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Extended `SkillRetriever::retrieve` to accept request-scoped `repo_path`.
- Forwarded `repo_path` from `CompileContextTool` to retrieval; kept `FindSkillTool` context-free (`None`).
- Updated `ScopeResolver` contract to accept optional request repo path and wired project/global resolution accordingly.
- Updated integration tests to use real repository paths and added
  `compile_context_uses_request_repo_path_for_scope_resolution`.

**Verification:**
- `cargo fmt`
- `cargo test -p infrastructure -p retrieval -p mcp-server --tests`
  - Includes passing integration coverage for repo-path scope isolation in `tests/integration/test_dual_scope.rs`.

## Notes

- WHY classification: 🎯 PROTECTS USER STORY, 🏛️ CONSTITUTION VIOLATION candidate (contract drift without waiver).
