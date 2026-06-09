---
unit: "T05 — Reliable real extraction (worker-pool correctness + provider disposition)"
unit_number: 1
unit_kind: hardening
serves: "SC-V1.5-C (real extraction is reliable under a ≥32-job burst, exactly one terminal event per accepted job) + SC-V1.5-F (no production stub paths remain)"
status: completed
attempt_count: 1
domains: [rust, concurrency, extraction, llm-providers, security]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/05-reliable-extraction-worker-pool-provider.md
session_id: work-2026-06-01-142356
---

## What Was Implemented

Resume-and-finish of T05: a prior run had left a complete, compiling implementation in the
working tree but had not validated, formatted, or reported it. This unit assessed every AC
against the working tree, fixed the one gap (rustfmt violations across the extraction files),
produced Ralph TDD evidence, and ran the regression guard.

Behaviors delivered (all verified against the working tree):
- **MPMC worker pool:** `Arc<Mutex<mpsc::Receiver>>` (held across `recv().await` — the "0/32" cause)
  replaced with `async_channel::bounded`; workers clone the receiver and pull concurrently; the now-
  redundant `Semaphore` removed.
- **Single terminal-event ownership:** `execute_job` returns a typed `ExtractionOutcome` and publishes
  nothing; the worker loop and the no-pool spawn path own all three events (completed/failed/timeout).
  No-pool path gained a `tokio::time::timeout` arm (no silent stall).
- **Unified retry:** dead `_retry_policy` param removed; hardcoded `RetryPolicy{...}` in
  `extract_with_retry` replaced with the single config-sourced `&self.retry_policy`.
- **Provider default = Ollama:** unset/empty `EXTRACT_SESSION_PROVIDER` ⇒ Ollama (no longer silently
  Claude); `OllamaExtractionConfig::default()` model = `granite4:3b`, inner `timeout_ms = 120_000`
  (outer pool 180s = 1.5×), documented UNMEASURED-conservative pending live p50/p95.
- **Claude = real Anthropic Messages API:** POSTs to `{ANTHROPIC_BASE_URL}/v1/messages` (default
  `https://api.anthropic.com`), forced `emit_candidates` tool_use, `x-api-key`, model via
  `EXTRACT_SESSION_MODEL` (default `claude-haiku-4-5`), static system block `cache_control: ephemeral`.
  The `:8080/extract` stub default is DELETED. `provider=claude` without `ANTHROPIC_API_KEY` is a loud
  construct-time error.
- **Security P1:** `scope_relative_draft_paths` strips absolute host paths from the
  `extraction.completed` Redis event; test asserts no absolute path in the payload.
- **Docs/compose:** capability catalog documents all 6 env vars + Ollama-default/Claude-opt-in;
  `docker-compose.yml` sets `EXTRACT_SESSION_PROVIDER=${EXTRACT_SESSION_PROVIDER:-ollama}`.
- **todo-103 coupling preserved:** `extract_blocking` still emits `extraction.completed` (via
  `publish_terminal_event`) so the durable transcript-ingest queue drain observes completion before
  acking a row.

## Files Changed
- `crates/session-extractor/src/worker_pool.rs` — MPMC receiver, terminal-event dispatch, timeout arm
- `crates/session-extractor/src/lib.rs` — provider default, unified retry, scope-relative draft paths, no-pool dispatch
- `crates/session-extractor/src/writer.rs` — rustfmt only
- `crates/session-extractor/src/providers/claude.rs` — provider wiring
- `crates/session-extractor/Cargo.toml` — `async-channel` dependency
- `crates/infrastructure/src/extraction/claude.rs` — Anthropic Messages API, forced tool_use, construct-time key check
- `crates/infrastructure/src/extraction/ollama.rs` — granite4:3b default + calibrated timeout
- `crates/infrastructure/src/extraction/http.rs`, `mod.rs`, `prompt_contract.rs` — supporting transport + prompt contract
- `docker-compose.yml` — explicit `EXTRACT_SESSION_PROVIDER`
- `docs/reference/capability-catalog.md` — opt-in provider contract
- `Cargo.lock` — async-channel

## Problems Encountered
### Problem 1: rustfmt violations left by prior run
- **Error:** `cargo fmt --check` exit 1 across `claude.rs`, `http.rs`, `ollama.rs`, `prompt_contract.rs`, `worker_pool.rs`, `writer.rs`
- **Root cause:** prior agent implemented correct logic but did not run `cargo fmt` before stopping
- **Fix:** ran `cargo fmt -p session-extractor -p infrastructure`; tests + clippy unchanged-green afterward

## Patterns Discovered
- `async_channel::bounded` cloneable receiver is the idiomatic MPMC replacement for `Arc<Mutex<Receiver>>` — each worker holds its own clone, the channel coordinates internally.
- Honest Red for a resumed/already-implemented unit: `git stash` to baseline, show the keystone tests did not exist (18 → 24), `stash pop` for Green.
- Project convention (commit a2c2271): tests needing live infra (Ollama/Redis/Qdrant/Anthropic) are `#[ignore]`-gated; the live ≥32-job burst e2e lives in `mcp-server` and is gated. The deterministic offline burst (fake provider) is the in-CI keystone unit test.

## TDD Evidence
- **Red**
  - Command: `git stash && cargo test -p session-extractor` (baseline HEAD)
  - Result: 18 passed — the 6 keystone behavior tests did not exist; baseline had the `Arc<Mutex<Receiver>>` bug, Claude-as-empty-default, no no-pool timeout arm, hardcoded retry policy
  - Evidence: genuine Red for the new behaviors — the tests proving them were absent at baseline (closest honest Red obtainable for resumed code)
- **Green**
  - Command: `cargo test -p session-extractor` (working tree restored)
  - Result: 24 passed incl. `parallel_burst_emits_exactly_one_terminal_event_per_job` (AC1), `completed_event_draft_paths_are_scope_relative` (Security P1), `provider_unset_and_empty_default_to_ollama`, `missing_api_key_fails_loudly_at_construct_time` (infrastructure)
- **Post-Refactor Green**
  - Command: `cargo test -p session-extractor -p infrastructure` (after `cargo fmt`)
  - Result: 79 passed (55 infrastructure + 24 session-extractor); the only refactor was the format fix; no behavior changed

## Test Results
- Command: `cargo test -p session-extractor` (+ orchestrator regression: `-p mcp-server -p infrastructure`)
- Result: PASS — session-extractor 24, infrastructure 55, mcp-server 55+11 (+ smaller suites), 0 failed
- Attempts: 1 (after format fix)

## Orchestrator Verification
- Regression guard on coupled crates (`mcp-server` — todo-103 `extract_blocking` path): PASS, no regressions.
- `cargo fmt --check -p session-extractor -p infrastructure`: clean (exit 0). NB: an unrelated pre-existing fmt drift exists in `crates/graph-builder/src/graph/rebuild.rs` — out of T05 scope, left untouched.

## Follow-ups
- **Measure `gemma4:e4b` p50/p95** on the target host and replace the UNMEASURED-conservative 120s inner timeout with a measured value (surfaced in code comments + capability catalog). (Model updated from `granite4:3b` to `gemma4:e4b` post-T05; retro-approved 2026-06-02 — see `docs/execution-sessions/retro-2026-06-02-model-healthcheck-approval/retro-approval.md`.)
- Live ≥32-job burst e2e against real Ollama is `#[ignore]`-gated; run it in an environment with live infra (belongs to T10's green-live-suite gate).
