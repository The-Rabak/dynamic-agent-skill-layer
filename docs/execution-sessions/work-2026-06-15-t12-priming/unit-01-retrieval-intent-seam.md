---
unit: "Typed RetrievalIntent seam + SessionStart trigger→Priming"
unit_number: 1
unit_kind: tracer-bullet
serves: "the typed intent distinction the whole T12 ticket rests on; Task path byte-identical"
status: completed
attempt_count: 1
domains: [rust, retrieval, mcp-server, hooks]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
session_id: work-2026-06-15-t12-priming
---

## What Was Implemented
Pure seam: `RetrievalIntent {Task(default), Priming}` in `crates/retrieval`, threaded through
`SkillRetriever::retrieve(prompt, repo_path, intent)` and the orchestrator impl (no branch yet — Priming
runs the identical path). `TriggerKind::SessionStart` (serde `session_start`) added; `compile_context`
maps `SessionStart→Priming`, everything else (None/Compact/Other) → `Task`. `find_skill` → `Task`.
SessionStart hook configs now pass `"trigger":"session_start"` (production prime activation wiring).

## Files Changed
- crates/retrieval/src/orchestrator.rs (enum, trait sig, impl, 8 test calls, 2 new tests)
- crates/retrieval/src/lib.rs (re-export)
- crates/mcp-server/src/tools/compile_context.rs (SessionStart variant, intent derivation, 7 new tests)
- crates/mcp-server/src/tools/find_skill.rs (Task call + TwoSkillStub sig)
- crates/mcp-server/src/lib.rs (EmbedCountingRetriever sig)
- crates/mcp-server/src/protocol.rs (trigger description)
- crates/mcp-server/tests/test_admin_tools.rs (EmptyRetriever sig)
- scripts/settings-efficacy-on.json + config/claude-code/hooks.example.json (session_start trigger)

## TDD Evidence
- **Red**: `cargo test -p retrieval --lib` → `E0433 undeclared type RetrievalIntent` + `E0061 method takes 2 args but 3 supplied` (tests referenced not-yet-existing type/arity).
- **Green**: `cargo test -p retrieval --lib && cargo test -p mcp-server --lib` → 80 + 50 pass, incl. new tests: `retrieval_intent_default_is_task`, `priming_intent_produces_identical_outcome_to_task_intent`, `trigger_kind_session_start_deserializes`, `session_start_trigger_routes_to_priming_intent`, `{no,compact,other}_trigger_routes_to_task_intent`.
- **Post-Refactor Green**: `cargo fmt && <tests>` → 80 + 50 pass; `cargo fmt --check` clean; `cargo clippy -p retrieval -p compiler -p mcp-server --all-targets` clean (no `#[allow]` — `_intent` prefix handles the seam param).

## Test Results
- `cargo test -p retrieval --lib`: 80 passed. `cargo test -p mcp-server --lib`: 50 passed.
- build/clippy/fmt clean. Attempts: 1.

## Patterns Discovered (for later units)
- `SkillRetriever` = 3 methods (retrieve, current_graph_version, configured_scopes); real impl `RetrievalOrchestrator<E>`; test impls all `#[cfg(test)]`.
- orchestrator.rs test scaffolding: `versioned_snapshot(n)`, `qdrant_hybrid_snapshot()`, `ConstantEmbeddingService` → `[1.0,0.0,0.0,0.0]`.
- `TriggerKind::Other` is `#[serde(other)]` and must stay last.
- compile_context derives intent in `invoke_and_capture_outcome` before `retrieve`.

## Byte-identical Task proof
All pre-existing retrieval (80) + mcp-server (50) tests unchanged-green; the only net change for non-session_start callers is a label (`Task`) over the identical code path.
