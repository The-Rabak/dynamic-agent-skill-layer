---
unit: "T10 Unit 3 — CI gate + purity check + retire all V1.5 rollback flags"
unit_number: 3
unit_kind: hardening
serves: "SC-V1.5-E (CI-gated green suite + readable artifact) + SC-V1.5-F (no production half-paths)"
status: completed
attempt_count: 1
domains: [ci, mcp-server, maintenance, graph-builder, infrastructure, scripts]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10-green-live-suite-and-ci-gate.md
session_id: work-2026-06-03-230132
human_gate: "infra-config (CI workflow + runner append) + flag removal — APPROVED by repo owner 2026-06-04"
---

## What Was Implemented
**(A) Runner summary step** — `scripts/run-e2e-tests.sh` appends one step calling `generate-e2e-summary.py` after aggregation. Append-only; T08's port/env lines untouched (verified by diff).
**(B) CI workflow** — `.github/workflows/live-e2e.yml` (new): live-e2e job with cargo-tree purity gates (domain + retrieval fail on sqlx/redis/qdrant), fmt/clippy/unit-test gates, `docker volume create ollama_data` + `nomic-embed-text` pull (granite4:3b ~2GB documented as CI-omitted; extraction degrades gracefully), `run-e2e-tests.sh --include-dream`, artifact upload of `latest-summary.md` + JSON `if: always()`, 20-min timeout, cancel-in-progress.
**(C) Purity confirmed** — domain + retrieval depend only on async-trait/chrono/serde/thiserror/arc-swap/tokio/domain; no sqlx/redis/qdrant.
**(D) Flag removal (single removal point)** —
- `MCP_RETRIEVAL_MODE` (main.rs): empty-graph seeded fallback deleted; always `from_environment`.
- `MCP_GRAPH_REFRESH` (lib.rs): `spawn_graph_refresh_if_enabled`→`spawn_graph_refresh_subscriber`, always spawns.
- `MCP_USAGE_LOGGING` (usage_writer.rs): `spawn_usage_writer` returns `UsageWriterHandle` (not Option); always spawns. health.rs flag-off test + `USAGE_LOGGING_ENV_LOCK` removed; always-enabled. Teardown drain (Unit 1) kept.
- `MAINTENANCE_TRANSCRIPT_DRAIN` (maintenance/runtime.rs): off short-circuit removed; drain always runs.
- `graph_builder::watcher::ScopeRoot` alias removed; all internal + 3 external test callers migrated to `domain::ScopeRoot`.

## Files Changed
- `.github/workflows/live-e2e.yml` (new, HUMAN-GATE approved), `scripts/run-e2e-tests.sh` (append-only, HUMAN-GATE approved)
- `crates/mcp-server/src/{main.rs,lib.rs,usage_writer.rs}`, `crates/infrastructure/src/health.rs`, `crates/maintenance/src/runtime.rs`
- `crates/graph-builder/src/{watcher.rs,watcher_recovery.rs,lib.rs,graph/build.rs,graph/rebuild.rs}`
- `tests/integration/{test_compile_context.rs,test_pending_lifecycle.rs,test_admin_tools.rs}`, `tests/e2e/test_maintenance_e2e.rs`
- `.gitignore` (orchestrator fix: `.github/` was blanket-ignored, hiding the workflow; changed to `.github/*` + `!.github/workflows/` so CI workflows track while the local plugin dirs stay ignored)

## Problems Encountered
### Problem 1: ScopeRoot alias used internally too
- **Root cause:** internal graph-builder modules imported `crate::watcher::ScopeRoot`, not just external callers.
- **Fix:** `use domain::ScopeRoot` in each affected file.
### Problem 2: flag-off health test asserted a removed branch
- **Fix:** retarget the test to the always-on contract (`compile_context_omits_usage_write_health_key_when_writer_is_ok`).
### Problem 3 (orchestrator): `.github/` gitignored → workflow silently dropped from first commit
- **Root cause:** `.gitignore` blanket-ignores `.github/` (local plugin install); GitHub Actions workflows must live at `.github/workflows/`.
- **Fix:** scoped negation `.github/*` + `!.github/workflows/`; amended the commit to include the workflow + gitignore.

## Patterns Discovered
- `.github/` in this repo is a local plugin install (agents/skills/copilot configs) and is gitignored; real CI workflows need the scoped-negation gitignore to be tracked.
- Removing a flag's optionality cascades types (`Option<UsageWriterHandle>`→`UsageWriterHandle` through with_usage_writer → LiveServerComponents → teardown).

## Orchestrator independent verification
- `grep -rn "remove-after-v1.5-green" crates/ scripts/` → 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0; `cargo fmt --check` → exit 0.
- Purity: domain + retrieval PURE.
- Runner fence: `git diff scripts/run-e2e-tests.sh` touches no port/env line.
- CI YAML: `yaml.safe_load` OK.
- Affected unit tests: mcp-server lib 28 ok, maintenance lib 23 ok, graph-builder lib 7 ok, infra health 4 ok.
- Live roundtrip under DEFAULT config (flags gone): `test_live_data_plane_roundtrip ... ok`.

## TDD Evidence
- Red: `grep remove-after-v1.5-green` = 5; flag-off test asserting the half-path existed.
- Green: grep = 0; workspace compiles, clippy/fmt clean, `cargo test --workspace` green.
- Post-Refactor Green: full validation re-run after dead-code/import cleanup → green; live roundtrip green under default config (always-on usage + always-live retrieval proven). Full CI-on-GitHub runs on merge (can't run locally); local replacement evidence = YAML valid + runner valid (bash -n) + purity green + live test green post-removal.

## Test Results
- Command: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check && cargo test --workspace && grep -rn remove-after-v1.5-green crates/ scripts/ | wc -l`
- Result: PASS (grep → 0)
- Attempts: 1 (orchestrator added 1 amend for the gitignore/workflow tracking fix)
