---
unit: "T10 Unit 2 — Human-readable live-suite run summary"
unit_number: 2
unit_kind: hardening
serves: "SC-V1.5-E (adoption/legibility AC — readable proof report)"
status: completed
attempt_count: 1
domains: [tooling, e2e, observability]
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10-green-live-suite-and-ci-gate.md
session_id: work-2026-06-03-230132
---

## What Was Implemented
- `scripts/generate-e2e-summary.py` — standalone Python 3 stdlib-only post-run generator. Reads the newest `tests/e2e/reports/run__*.json` aggregate (`--input`/`--output` overrides), renders `tests/e2e/reports/latest-summary.md` with 8 sections: Overall Status (green/red + per-test table), Latency (overall + by-stage p50/p95/p99, nearest-rank, sample counts noted), Graph Version Progression, Extraction Attempts/Completions, Pending Draft Count, Degraded/Recovery Events, Ignored Dream Contracts (scraped `#[ignore = "…"]` reasons from `tests/e2e/*.rs`, bare `#[ignore]` flagged as gap), Environment. Deterministic (sorted), idempotent (byte-identical re-render), honest `n/a` for absent data, exits 1 when no aggregate exists.
- `tests/e2e/fixtures/sample_run_aggregate.json` — representative aggregate fixture (3 tests covering all sections).
- `tests/e2e/fixtures/test_summary_generator.py` — pytest-free fixture-driven acceptance harness (8 section headers + 19 content fragments + idempotency).

## Files Changed
- `scripts/generate-e2e-summary.py` — created
- `tests/e2e/fixtures/sample_run_aggregate.json` — created
- `tests/e2e/fixtures/test_summary_generator.py` — created

## Problems Encountered
- REPO_ROOT off-by-one (`parent×3` → `×4`) when run from repo root — fixed.
- Bare `#[ignore]` in `//!` doc-comment prose triggered false gap flags — added comment-line guard (skip `//`,`*`,`/*`).
- Test asserted literal `graph_version`/`DS-001` strings vs generator's correct rendered `v5` + function names — assertions aligned to actual output.

## Patterns Discovered
- Repo uses two ignore forms: `#[ignore = "requires live containers"]` (promoted tests that DO run in the gate via `-- --ignored`) and `#[ignore = "Dream-state contract: <reason>"]` (V2-deferred). The reason column distinguishes them.
- Aggregate `run__*.json` (`{run_summary, reports:[E2EReport]}`) is the canonical post-run artifact (same input the judge step reads); the generator is a pure consumer.

## Orchestrator independent verification
- Ran generator against the fixture → `latest-summary.md` with all 8 sections; re-ran → byte-identical (idempotent).
- Scope fence: only NEW files (`scripts/generate-e2e-summary.py`, `tests/e2e/fixtures/`); NO edits to `run-e2e-tests.sh`, `.github/`, `report.rs`, or any flag.

## Review note (carry to /workflows:review)
- The "Ignored Dream Contracts" table lists 38 entries mixing "requires live containers" (run in the gate) with genuinely-deferred V2 contracts. Honest (reason column distinguishes), but a reviewer may prefer splitting into "run-in-gate" vs "deferred" subsections for legibility. Non-blocking.

## TDD Evidence
- Red: `python3 tests/e2e/fixtures/test_summary_generator.py` → FAIL (`generator not found`).
- Green: same → PASS (all 8 sections + 19 fragments + idempotency).
- Post-Refactor Green: same → PASS after moving `import math` to top + removing a one-liner inner fn.

## Test Results
- Command: `python3 tests/e2e/fixtures/test_summary_generator.py`
- Result: PASS
- Attempts: 1
