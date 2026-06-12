---
unit: "T20 Unit 1 — promote instrument + --gate + Rust shim + retire stale ruler + erratum (offline)"
unit_number: 1
unit_kind: infra-packet
serves: "The validated T11 ruler becomes the automated gate; T12/T14/T18 import the shared measurement lib; the falsified stale ruler is deleted."
status: completed
attempt_count: 1
domains: [measurement, scripts, e2e, docs]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/20-institutionalize-262-instrument-e2e-gate.md
session_id: work-2026-06-12-075627-T20
---

## Delegated to execution-agent (sonnet); orchestrator-verified
Offline scope (no live server). Commit 20a498e.

## What was implemented
- **Promote:** `git mv scripts/t11_metrics.py → retrieval_metrics.py`, `t11_sweep.py → retrieval_sweep.py`; import + live doc refs updated (assessment, ticket); historical session logs left immutable; `--self-test` kept.
- **`--gate` mode** (`retrieval_sweep.py`): 2 arms (dense_views_on + alpha0_control) via reboot_mcp + /health-200, anchor-only (deterministic, no LLM judge), asserts `GATE_THRESHOLDS` floors (MRR@3/MRR@10/nDCG@3/cand-recall@50/no_match) + the α=0 crater canary; emits gate+latency JSON; exit codes; fail-loud on infra.
- **`GATE_THRESHOLDS` + `gate_decision()`** (`retrieval_metrics.py`): floors set BELOW the T11 single-view-dense numbers with recorded inline-cited margins (0.64/0.64/0.64/0.68/0.88) → robust to the dense_views flag state, no threshold gaming; candidate-recall is an explicit floor (the lever); fails loud on missing metric; α=0 non-crater = void gate.
- **`--self-test`** extended with 4 gate-decision/crater cases → 38/38.
- **Thin Rust `#[ignore]` shim** (`tests/e2e/test_retrieval_quality_gate.rs`): shells to `--gate`, asserts exit 0 + `GATE: PASS`, fail-loud with stderr. Orchestrator FIXED a CWD bug (anchored the script path on `CARGO_MANIFEST_DIR/../..` since cargo runs tests with CWD = crate dir, not repo root).
- **Retire stale ruler loudly:** deleted `tests/fixtures/retrieval_quality_labeled.json` + `retrieval_quality_234_corpus_labeled.json`; deleted the superseded synthetic-seed Rust instrument (`test_retrieval_quality.rs`, `quality/labeled_corpus.rs`, `quality/metrics.rs`; Cargo.toml `[[test]]` re-pointed to the shim); tombstone `tests/fixtures/RETIRED_FIXTURES.md`.
- **T11 report erratum** appended (dense_views gold-in-pool 109/137 not 108; instrument-location + latency-artifact pointers). Originals not edited.

## Orchestrator verification
- self-test 38/38; `--gate` parses; both clippy forms (bare + `--features test-utils`) + fmt exit 0 after the deletions (T21's green state preserved); gate logic + thresholds reviewed (no gaming, fail-loud).

## Test Results
- Command: `python3 scripts/retrieval_metrics.py --self-test && cargo clippy --workspace --all-targets --features test-utils -- -D warnings && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
- Result: PASS (all exit 0)
- Attempts: 1
