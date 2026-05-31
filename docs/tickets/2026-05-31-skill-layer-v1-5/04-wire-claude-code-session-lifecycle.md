---
ticket_id: T04
title: Wire the full Claude Code session lifecycle (inject + extract triggers)
kind: expansion # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed (hooks.example.json human-gate approved 2026-05-31)
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.1: Wire the full Claude Code session lifecycle"
feature_home: config/claude-code
depends_on: [T01]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-B (self-growing trigger exists)
files:
  - config/claude-code/hooks.example.json
  - docs/reference/capability-catalog.md
  - docs/runbooks/degraded-state.md
  - crates/mcp-server/src/tools/compile_context.rs
test_command: cargo test -p mcp-server --features test-utils -- --ignored extract_session_live_ref_payload
tdd_mode: inherit
---

# Wire the full Claude Code session lifecycle (inject + extract triggers)

## Serves
- **SC-V1.5-B** — `SessionEnd` triggers `extract_session`; `SessionStart` injects compiled context; context is re-injected after compaction. The self-growing loop finally has a trigger.
- Plan SC-1/SC-3; constitution Principle 2 (zero-touch) & 3 (human-gate).

## Scope
Extend the example hook config + contract docs to wire `SessionStart` (inject), compaction re-injection (`PreCompact`/`SessionStart` source `compact`), and `SessionEnd → extract_session`, alongside the mandated `UserPromptSubmit → compile_context`. Add a minimal `trigger`/`source` hint to `CompileContextRequest` so compaction re-injection bypasses suppression.

- **Owns:** lifecycle hook contract + docs + the minimal `trigger` hint plumbing.
- **Non-goals:** multi-harness hook configs (OpenCode/Copilot/Codex = V2); changing extraction internals (T05); building the crash backstop (T07).

## Scope Fence
Claude Code only; no new MCP tools. Do not change extraction worker internals. The crash safety-net is built in T07 — this ticket documents the caveat and points to T07; it must NOT claim a backstop that does not yet exist.

## Acceptance Criteria
- [ ] Example config wires SessionStart (inject), compaction re-inject, and SessionEnd (extract) in addition to UserPromptSubmit.
- [ ] Docs explain injection/blocking/fire-and-forget semantics and the crash caveat. **Verified hook facts:** `SessionStart`/`SessionEnd` **cannot block** (inject/observe only); `UserPromptSubmit` (30s timeout) and `PreCompact` **can block**; context injects via `hookSpecificOutput.additionalContext` (≤~10,000 chars); `SessionEnd` matchers include `clear|resume|logout|prompt_input_exit|other`; `SessionEnd` does **not** fire on crash/SIGKILL.
- [ ] **Compaction re-injection actually works:** add an optional `trigger: Option<String>` to `CompileContextRequest` + the MCP input schema; a `compact`-triggered call **bypasses/clears suppression** so it returns `Ok` with fresh context instead of `DuplicateSuppressed`.
- [ ] **Crash safety-net referenced, not claimed:** document the `SessionEnd` crash caveat AND point to T07 (level-triggered reconcile) as the backstop — no "safety net that doesn't exist" claim.
- [ ] **Human-gate enforced as an AC:** changes to `config/claude-code/hooks.example.json` are staged and presented for explicit human approval before commit.
- [ ] SessionEnd-triggered extraction produces only `.pending` files (no auto-approval/auto-rename).
- [ ] A live extraction is demonstrably triggerable by the SessionEnd contract (E2E simulating the SessionEnd payload).

## Shared / Global Notes
- **Infrastructure configuration change — HUMAN GATE:** editing `config/claude-code/hooks.example.json` requires explicit approval (constitution: infra-config change). Stage and confirm before commit.
- The `trigger` field touches `crates/mcp-server/src/tools/compile_context.rs` (the pure query-compile tool) — NOT `lib.rs`'s `McpServerApp::compile_context` (that is T06's write site). Keep these two `compile_context` surfaces distinct.
- Suppression bypass here is a **request-driven, per-call** clear — it must not alter the production suppression semantics owned by T08.

## Local Context
**WHY:** `config/claude-code/hooks.example.json` wires only `UserPromptSubmit → compile_context`. There is no `SessionEnd` hook, so `extract_session` is never auto-invoked — the self-growing loop has no trigger. Compaction re-injection is blocked by suppression unless a `trigger` hint clears it (otherwise the re-inject hook is a silent no-op).

**Validate during execution:** exact hook names/payload fields against current Claude Code docs; anchor on the well-established `UserPromptSubmit`/`SessionStart`/`PreCompact`/`SessionEnd` set; treat any additional events as optional. The `result_policy`/`inject_additional_context_on` shape in the example config is the correct MCP tool-result form; raw shell hooks use `hookSpecificOutput.additionalContext`.

## Parent Refs
- Plan → Slice 2.1; Architecture artifact.
- Source packet: `## Execution Slices > Slice 2.1`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §2.1 (hook facts validated against current docs).
- Plan §References & Research → External (Claude Code hooks).

## Coupling Notes
One unit because the hook config, the docs that explain its semantics, and the `trigger` field that makes compaction re-injection non-trivial are the same behavior expressed across config + a tiny code surface; splitting would ship a re-injection hook that silently no-ops. Hard-depends on T01 (context must be real to be worth injecting). Parallel-safe with T03 in Batch 2 (disjoint code files; different doc files).
