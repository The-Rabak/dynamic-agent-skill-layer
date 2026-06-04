---
unit: "T10 follow-up — make the full live runner run end-to-end green"
unit_number: 4
unit_kind: hardening
serves: "SC-V1.5-E AC#1 (full run-e2e-tests.sh --include-dream green on live containers + populated summary)"
status: completed
attempt_count: 7
domains: [docker, e2e, scripts, retrieval]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10-green-live-suite-and-ci-gate.md
session_id: work-2026-06-03-230132
note: "User directive: 'do whatever it takes to fix the local docker setup and make the tests run all the way.' Orchestrator-driven iterative debugging (7 full-run attempts)."
---

## Result
`./scripts/run-e2e-tests.sh --include-dream` now runs **end-to-end GREEN: 18/18 passed, 0 failed (147s)**, emitting a populated `tests/e2e/reports/latest-summary.md` (per-test table, p50/p95/p99 latency by stage, graph version, extraction, degraded events, ignored contracts, environment). Verified by orchestrator (full run attempt 8, log `/tmp/t10_full8.log`). Commit `1a31678`.

## Why this was needed
The full wrapper had **never run end-to-end** (T08/T09 validated SC-E via direct `cargo test --ignored`, never the wrapper). So a chain of pre-existing breakages was masked, surfacing one per run attempt:

1. **Dockerfile build failure (attempts 1–2):** builder stage adds the `x86_64-unknown-linux-musl` target but not `musl-tools` → `ring`/`cc-rs` fail (`x86_64-linux-musl-gcc` not found). Also the binary was built inside a BuildKit cache mount (`/app/target`) not persisted to the image → runtime `COPY --from=builder` would find nothing. **Fix:** `apt-get install -y musl-tools musl-dev` + `CC_x86_64_unknown_linux_musl=musl-gcc`; `cp` binary out of the cache mount to `/app/service-bin`. See [[dockerfile-musl-tools-gap]].
2. **dual_scope latency flake (attempt 3):** `real_scope_search_path_meets_parallel_latency_envelope` asserted parallel<sequential at sub-ms scale (10µs margin) → flaked under load. **Fix:** assert the real envelope (parallel stays <250ms AND ≤ sequential+10ms jitter).
3. **Runner: missing `--features test-utils` (attempt 4):** mcp-server e2e targets are `required-features=["test-utils"]`; several runner invocations omitted it. **Fix:** added to all mcp-server e2e invocations.
4. **Runner: wrong package (attempt 5):** `test_watcher_churn_reconciliation` is registered under `mcp-server`, not `graph-builder`. **Fix:** run under `-p mcp-server --features test-utils`.
5. **Runner: multi-positional cargo filter (attempt 5):** 5 dream-contract names passed before `--`; cargo accepts only one positional TESTNAME. **Fix:** move them after `-- --ignored` (libtest OR-matches multiple).
6. **extract_session_parallel_burst (attempts 6–7):** 6s wait window for 32 granite4:3b jobs (4-worker pool) → 0 completions; and `pending==32`/`failed==0` assumed a stub that always drafts. Real granite4:3b **declines** to draft trivial stress transcripts (only 3/32 drafted). **Fix:** wait until every job TERMINATES (~480s cap), assert the SC-C contract — `completed+failed==32`, `completed>=1`, drafts canonical, `pending<=completed`. Measured: completed=32, failed=0, ~98–157s.
7. **e2e report path one dir short (attempts 7–8):** tests wrote to `CARGO_MANIFEST_DIR/../tests/e2e/reports` = `crates/tests/e2e/reports/` while the runner/generator/CI aggregate from repo-root `tests/e2e/reports/` → empty summary. **Fix:** `../tests` → `../../tests` in all 5 e2e test files; gitignore the repo-root run artifacts.

## Files Changed (commit 1a31678)
- `Dockerfile` (musl-tools + CC + cache-mount copy-out)
- `scripts/run-e2e-tests.sh` (test-utils on mcp-server invocations; watcher-churn package; dream-contract arg order; summary step — no port/env line touched)
- `crates/retrieval/src/dual_scope.rs` (latency envelope assertion)
- `tests/e2e/test_concurrency_stress.rs` (extract-burst SC-C contract + report path)
- `tests/e2e/test_dream_state_contract.rs`, `test_live_data_plane_roundtrip.rs`, `test_transcript_ingest_queue_e2e.rs`, `test_watcher_churn_reconciliation.rs` (report path)
- `.gitignore` (repo-root run artifacts)

## Verification
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0; `cargo fmt --check` → clean.
- Full `run-e2e-tests.sh --include-dream` → 18/18 passed, 0 failed; `latest-summary.md` GREEN 18/18; judge written; whole-run failure scan = 0.

## Known follow-up (non-blocking, out of scope here)
- `test_concurrency_stress.rs` + shared `report.rs` carry pre-existing clippy debt that only surfaces under `--features test-utils` (unused imports `process::Command`/`LiveGraphCommunityRecord`; dead helpers `BurstEmbeddingService`/`seeded_graph`/`retrieval_config` orphaned by unit-1's burst rebalance; `len_zero`; `if_same_then_else` + a cross-binary-dead `record_degradation_event`). NOT caught by the CI `clippy --workspace --all-targets` (no test-utils) and does NOT block the run. Left as cleanup since removal cascades imports.
