---
unit: "T04-A: RetrievalBackend enum + fail-loud from_env + wiring"
unit_number: 1
unit_kind: infra-packet
serves: "Backend-selection substrate so the live sweep can measure snapshot_hybrid/qdrant_hybrid vs snapshot_dense baseline through the real server (#243 fail-loud)."
status: completed
attempt_count: 1
domains: [retrieval, config]
session_id: work-2026-06-09-T04
---

## What Was Implemented
- `RetrievalBackend { SnapshotDense (default), SnapshotHybrid, QdrantHybrid }` enum + `FromStr` mirroring `CommunityBoostMode` (orchestrator.rs). Accepts primary names + aliases (dense/hybrid/qdrant); unknown => `Err`.
- `RetrievalConfig.backend` field; `Default` = SnapshotDense; `from_env()` parses `RETRIEVAL_BACKEND` via the `env_or` fail-loud helper (panics on present-but-unparseable — #243 item 2).
- Explicit per-variant dispatch seam (no catch-all `_`) at the `search_scopes_concurrently` call in `retrieve()`, so B/C filling arms is a compile-time obligation. All three variants currently route to the existing dense path — behavior byte-identical to today.
- 2 new unit tests (parse + default guard).

## Files Changed
- `crates/retrieval/src/orchestrator.rs` — enum + FromStr, config field, Default, from_env, dispatch seam, 2 tests.

## Test Results
- `cargo test -p retrieval`: PASS (37/37)
- `cargo clippy -p retrieval --all-targets -- -D warnings`: PASS
- `cargo fmt --check`: PASS
- Orchestrator verification: `cargo test --workspace --all-targets --features test-utils --no-run`: PASS (all manual `RetrievalConfig {…}` test literals use `..default()` spread, so the new field is absorbed — no construction-site regressions).

## TDD Evidence
- Red: new parse test failed to compile (E0433 undeclared `RetrievalBackend`, E0609 no field `backend`).
- Green: enum + FromStr + config wiring → tests pass.
- Post-Refactor Green: fmt + clippy clean; 35 pre-existing retrieval tests unchanged.

## Notes / Caveats
- The `retrieval` crate has NO `test-utils` feature — drop `--features test-utils` for retrieval-scoped commands (ticket's generic validation command assumed it).
- Pre-existing flaky tests in retrieval (`relevance_threshold_*` env race; `*_parallel_latency_envelope` timing) are NOT introduced by this unit (confirmed via baseline stash run 35/35). The 2 new tests mutate no env.
