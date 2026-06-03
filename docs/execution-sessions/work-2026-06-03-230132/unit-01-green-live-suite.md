---
unit: "T10 Unit 1 — Unblock & green the live E2E suite"
unit_number: 1
unit_kind: hardening
serves: "SC-V1.5-E (live suite GREEN under default config) + SC-V1.5-F (strict clippy/fmt residue)"
status: completed
attempt_count: 1
domains: [testing, mcp-server, infrastructure, e2e]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10-green-live-suite-and-ci-gate.md
session_id: work-2026-06-03-230132
---

## What Was Implemented
1. **Teardown deadlock fix (the unblocker).** `spawn_usage_writer` now returns `UsageWriterHandle { sender, join_handle }`; `with_usage_writer` returns `(Self, Option<JoinHandle>)`; `LiveServerComponents` stores `usage_writer_join_handle` (test-cfg). `teardown()` now drains: `close_usage_sender()` (drop sender → EOF) → `abort()` → `await` the writer task → THEN `truncate_all_tables`. Production hot-path write behavior unchanged (still async try_send, observable via `health["usage_write"]`). Result: live roundtrip green under DEFAULT config (no `MCP_USAGE_LOGGING=off`).
2. **DS-003 rewrite to Option-A CQRS contract.** `dependency_chaos_matrix`: Qdrant down ⇒ `compile_context` returns `Ok|NoMatch` (read path = in-memory snapshot) AND `qdrant_write_side` health marker unhealthy; Ollama down ⇒ `Degraded` (embedding IS a read dep). No eager per-request Qdrant check. NOT `#[ignore]`. Bounded readiness-polling replaces fixed sleeps.
3. **Concurrency burst rebalance.** Added irrelevant prompts from `retrieval_corpus.json` negatives so `no_match_count > 0` deterministically; fixed root cause (build fresh server AFTER seeding so the in-memory snapshot contains seeded skills); teardown both servers.
4. **protocol.rs clippy.** Collapsed 3 `collapsible_if` (`:333/:369/:379`) into `if let … && let` chains.
5. **Health env-var flake.** `USAGE_LOGGING_ENV_LOCK: Mutex<()>` serializes the two env-mutating health tests.
6. **degraded_and_recovery_cycle (roundtrip).** Qdrant-stop phase asserts Option-A CQRS (read unaffected); Ollama-restart uses bounded polling (~30s cap); removed stale `assert_ne!(reason_qdrant, reason_ollama)`.
7. **Live extraction wait** extended to 120s for `granite4:3b` CPU inference (~17s/call).

## Files Changed
- `crates/mcp-server/src/usage_writer.rs` — `UsageWriterHandle`; `spawn_usage_writer` return type
- `crates/mcp-server/src/lib.rs` — teardown drain seam; `with_usage_writer` tuple; `close_usage_sender`; join-handle field
- `crates/mcp-server/src/protocol.rs` — 3 collapsible_if collapses
- `crates/infrastructure/src/health.rs` — test env-var mutex
- `tests/e2e/test_dream_state_contract.rs` — DS-003 Option-A rewrite + polling
- `tests/e2e/test_concurrency_stress.rs` — burst rebalance + fresh-server-post-seed
- `tests/e2e/test_live_data_plane_roundtrip.rs` — Option-A Qdrant-stop phase + polling + extraction wait

## Problems Encountered
### Problem 1: burst returned all NoMatch
- **Root cause:** server built BEFORE seeding → empty in-memory snapshot.
- **Fix:** build fresh server after seeding (roundtrip pattern).
### Problem 2: degraded_and_recovery Phase 4 expected retrieval restored on Qdrant restart
- **Root cause:** under Option A, Qdrant restart does nothing for the read path; only Ollama matters.
- **Fix:** restructured phases — Phase 5 restores Ollama and asserts recovery.
### Problem 3: extraction timed out at 3s
- **Root cause:** granite4:3b ~17s/inference on CPU host.
- **Fix:** 120s bounded wait.

## Patterns Discovered
- `teardown` must close the usage-writer sender (EOF) before abort+await; the writer holds `RowExclusive` locks until it exits, racing `TRUNCATE … CASCADE`.
- `from_environment` snapshots PG at boot; skills seeded after boot are invisible until a new boot or a Redis `graph.rebuilt` refresh. Tests seeding+retrieving in one instance need a second boot.
- `cargo test` accepts only ONE positional TESTNAME filter before `--`; multiple dream tests must run individually or via a regex after `--`.
- granite4:3b on CPU/WSL2 ≈ 17s/inference — live extraction wait budgets must reflect this.

## TDD Evidence
### Red
- `cargo clippy -p mcp-server --all-targets -- -D warnings` → FAIL (3 collapsible_if at protocol.rs:333/369/379).
- Live roundtrip under default config (pre-drain) → HANG in teardown (did not terminate in bounded time; only green under `MCP_USAGE_LOGGING=off`).
- Burst (pre-fix) → FAIL `at least one Ok response required` / stale `no_match_count > 0`.
### Green
- `test_live_data_plane_roundtrip` (default config) → PASS 5/5 in 91.50s (no hang).
- Dream DS-003…007 each PASS (dependency_chaos_matrix 5.28s, outbox_backlog_replays 0.60s, qdrant_pg_drift 0.34s, sustained_watcher 0.77s, high_qps 1.37s).
- Concurrency burst PASS (2.78s); watcher churn PASS (0.52s).
### Post-Refactor Green
- `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check && cargo test --workspace` → PASS (all clean).

## Orchestrator independent verification
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (clean).
- `cargo fmt --check` → exit 0.
- Scope fence: NO changes to `scripts/run-e2e-tests.sh`, `.github/workflows/`, `tests/e2e/report.rs`; all 5 `remove-after-v1.5-green` flags still present.
- Re-ran `dependency_chaos_matrix` live under DEFAULT config → `ok` 5.12s (not ignored).

## Test Results
- Command: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check && cargo test --workspace`
- Result: PASS
- Attempts: 1
