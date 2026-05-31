---
unit: "T04 — Wire the full Claude Code session lifecycle (inject + extract triggers)"
unit_number: 3
unit_kind: expansion
serves: "SC-V1.5-B (self-growing trigger exists)"
status: completed
attempt_count: 1
domains: [rust, mcp-server, config, claude-code, docs, testing]
batch: 2
human_gate: hooks.example.json — APPROVED by maintainer 2026-05-31
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/04-wire-claude-code-session-lifecycle.md
session_id: work-2026-05-31-121712
---

## What Was Implemented
- `config/claude-code/hooks.example.json`: extended from `UserPromptSubmit`-only to the full 4-event lifecycle — `SessionStart` (inject), `PreCompact` (re-inject with `trigger:"compact"`), `UserPromptSubmit` (unchanged), `SessionEnd` (`extract_session`, fire-and-forget). **HUMAN-GATE — approved before commit.**
- `crates/mcp-server/src/tools/compile_context.rs`: added optional `trigger: Option<String>` (`#[serde(default)]`) to `CompileContextRequest`; a `trigger=="compact"` call sets `compact_bypass` that skips the suppression/cache check (single-call, read-only — does NOT mutate suppression state; T08 owns global semantics).
- `crates/mcp-server/src/protocol.rs`: added `trigger` to the `compile_context` MCP `inputSchema` (optional).
- `docs/reference/capability-catalog.md`: full "Claude Code Session Lifecycle Hook Contract" section (blocking/inject/fire-and-forget semantics, ≤~10,000-char inject limit, SessionEnd crash caveat → points to T07, human gate on `.pending`).
- `docs/runbooks/degraded-state.md`: "Session Lifecycle Degraded States" (crash caveat + manual re-trigger; compaction re-inject suppressed scenario).
- 8 test/bench files: added `trigger: None` to existing `CompileContextRequest` literals.

## Files Changed
- `crates/mcp-server/src/tools/compile_context.rs` — `trigger` field + compact bypass
- `crates/mcp-server/src/protocol.rs` — inputSchema `trigger`
- `config/claude-code/hooks.example.json` — full lifecycle (human-gate, approved)
- `docs/reference/capability-catalog.md`, `docs/runbooks/degraded-state.md` — contract docs
- `tests/integration/{test_compile_context,test_session_persistence,test_dual_scope}.rs`, `tests/e2e/{test_live_data_plane_roundtrip,test_boot_time_live_retrieval,test_dream_state_contract,test_concurrency_stress}.rs`, `tests/bench/compile_context_bench.rs` — `trigger: None`

## TDD Evidence
- **Red:** `cargo test -p mcp-server -- compact_trigger_bypasses_suppression` → FAIL (E0560: `CompileContextRequest` has no field `trigger`) — behavior absent.
- **Green:** `cargo test -p mcp-server --test test_compile_context -- compact_trigger_bypasses_suppression` → PASS: first call Ok, second (no trigger) DuplicateSuppressed, third (trigger=compact) Ok with fresh context.
- **Post-Refactor Green:** `cargo test -p mcp-server --features test-utils --test test_compile_context` → 9 passed after `cargo fmt` + clippy `collapsible_if` fix (let-chain).
- **E2E:** `extract_session_live_ref_payload_loads_from_transcript_volume` (`--ignored`, live containers) → PASS (0.72s): SessionEnd contract triggers extraction.

## Test Results
- Command: `cargo test -p mcp-server --features test-utils -- --ignored extract_session_live_ref_payload`; Result: PASS; Attempts: 1. Re-verified by orchestrator in combined-tree run + boot-smoke regression (green).

## Patterns Discovered / Notes for Review
- `CompileContextRequest` literals exist in 30+ sites; a new non-optional field forced edits across 8 files. Consider `#[non_exhaustive]` or `Default` + `..Default::default()` to localize future additive fields.
- Validation command in the ticket (`cargo test ... -- --ignored extract_session_live_ref_payload`) is filter-only without `--test test_live_data_plane_roundtrip`; the live test lives in that binary. Flag for T10 when wiring CI.
- `PreCompact` `result_policy` intentionally has `inject_additional_context_on: ["ok","degraded"]` alongside `ignore_on:["degraded"]` — different axes (inject vs retry/block).
- Crash backstop only DOCUMENTED (points to T07), not implemented — per scope fence.
