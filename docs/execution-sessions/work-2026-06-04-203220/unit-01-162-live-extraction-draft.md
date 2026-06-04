---
unit: "#162 — live inline extraction produces a real .pending draft"
unit_number: 1
unit_kind: fix-item
serves: "extraction guarantee is real — the extraction→draft path is proven end-to-end on live infra"
status: completed
attempt_count: 2
domains: [extraction, e2e, ollama]
plan_file: "todos/162-pending-p1-live-inline-extraction-writes-no-pending-draft.md"
session_id: work-2026-06-04-203220
---

## What Was Implemented
Replaced the contentless 2-line `inline_transcript_jsonl()` fixture with a substantive single-turn Tokio
`WouldBlock` debugging transcript (concrete error, named commands, numbered steps, explicit project convention).
Pinned the test to `gemma4:e4b` with `temperature=0` for deterministic extraction, extended the poll window to
180s to match the worker-pool outer timeout, and added an explicit `candidate_count > 0` assertion that fails loud
with a clear reason. To support deterministic extraction, added a real, default-off `temperature` capability to the
production Ollama extraction config (env `OLLAMA_EXTRACTION_TEMPERATURE`).

## Files Changed
- `tests/e2e/test_live_data_plane_roundtrip.rs` — substantive transcript; gemma4:e4b + temperature=0 + 150s timeout;
  180s poll; explicit `candidate_count > 0` assertion; docstring corrected to match actual model/timeout.
- `crates/infrastructure/src/extraction/ollama.rs` — `temperature: Option<f32>` on `OllamaExtractionConfig`
  (default `None`); `OllamaGenerateOptions` + `options` on the request, both `skip_serializing_if = Option::is_none`
  so the production wire format is unchanged by default; unit test asserts the `None` default.
- `crates/session-extractor/src/providers/ollama.rs` — `OLLAMA_EXTRACTION_TEMPERATURE` env parse (fail-loud on bad value).

## Problems Encountered
### Problem 1: granite4:3b nondeterministically returns zero candidates
- **Root cause:** the 2.1GB granite4:3b stochastically emits `{"candidates": []}` even from concrete content.
- **Fix:** switched to the project-default gemma4:e4b with `temperature=0` (greedy). Required a real, default-off
  temperature capability in the production extraction path — not a test-only hack.
### Problem 2: dense multi-turn transcript timed out >180s on CPU
- **Fix:** reduced to a single focused Q&A exchange that still carries real procedures/conventions.

## Patterns Discovered
- granite4:3b is too small/flaky for reliable extraction; gemma4:e4b + temperature=0 is repeatable (identical
  candidate on consecutive runs, KV-cache speedup on the second).
- Ollama optional inference params: `options` with `skip_serializing_if = Option::is_none` keeps the wire format
  unchanged when unset.
- **Latent pre-existing test fragility (NOT this unit):** `infrastructure` `health::tests::build_health_checker_always_injects_usage_write_enabled`
  is a sync `#[test]` that builds a sqlx pool; it panics "this functionality requires a Tokio context" when the
  `infrastructure` and `session-extractor` lib test binaries run in parallel (`cargo test -p A -p B --lib`). Passes
  when each crate is run alone and on clean HEAD. Cross-binary parallel flake — candidate for a future todo.

## Test Results
- Command: `cargo test -p mcp-server --features test-utils --test test_live_data_plane_roundtrip extract_session_live_inline_payload_writes_pending_and_emits_completion_events -- --ignored --nocapture`
- Result: PASS (independently re-run by orchestrator)
- Attempts: 2

## TDD Evidence
- **Red:** original contentless fixture → `pending_written=false ... (no draft = extraction produced nothing)`,
  `FAILED. 0 passed; 1 failed` in 1.72s. Proves the empty-extraction gap was real.
- **Green:** substantive transcript + gemma4:e4b + temperature=0 → `1 passed` in 90.52s (cold). Real draft written,
  `candidate_count > 0`, `origin: session_extraction` present.
- **Post-Refactor Green:** after clippy fix + docstring correction → orchestrator-verified `1 passed` in 42.49s
  (warm). Deterministic real inference.

## Regression Guard
- `cargo test -p infrastructure --lib` (alone): 97 passed, 0 failed.
- `cargo test -p session-extractor --lib` (alone): 39 passed, 0 failed.
- Confirmed clean HEAD also passes these alone; the only failure was the cross-crate parallel flake above.
