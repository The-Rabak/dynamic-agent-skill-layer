---
unit: "Slice 1.2 — Honest reporting: outcome derived from real assertions"
unit_number: 2
unit_kind: hardening
serves: "SC#1 (no fake passes) — prerequisite for every scenario slice"
status: completed
attempt_count: 1
domains: [rust, e2e, test-harness, reporting]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
`ReportBuilder::build()` now derives overall outcome from `contract_assertions` AND `sections`: any failed assertion ⇒
`Failed`; any failed section ⇒ `Failed`; **zero assertions AND zero sections ⇒ `Failed{reason:"no contract assertions
recorded — scenario proved nothing"}`** (reused existing variant — verified JSON-compatible with `generate-e2e-summary.py`,
which reads `outcome.status`/`outcome.reason`). New ergonomic API `assert_contract(name, passed: bool, expected, actual,
details) -> bool` pushes the derived `ContractAssertion` and returns the bool. De-hardcoded DS-003/004/005 to record
computed assertions; DS-006/007 marked `// TODO(2.x)` (their brutal rewrites are sibling slices). 5 new unit tests.

## Files Changed
- `tests/e2e/report.rs` — honest `build()`, `assert_contract()` API, 5 unit tests
- `tests/e2e/test_dream_state_contract.rs` — de-hardcoded DS-003/004/005 assertions; TODO(2.x) markers on DS-006/007

## Problems Encountered
- `cargo clippy --all-targets -D warnings` red on `scope.rs:37` ptr_arg — PRE-EXISTING, and scope.rs is sibling-owned;
  surfaced not fixed. (Sibling 1.1 fixed it independently.)

## Patterns Discovered
- `report.rs` shared via `#[path="report.rs"] mod report;`; its `#[cfg(test)]` runs inside the `test_dream_state_contract` binary.
- New assertion API is the canonical way for Phase-2 scenarios to record fail-able outcomes.

## TDD Evidence
- **Red:** simulated old `build()` + `git stash` proof — old logic returned `Passed` for zero-assertion and failing-assertion cases.
- **Green:** `cargo test -p mcp-server --features test-utils --test test_dream_state_contract -- --skip ignored` → 7 passed (6 new report tests).
- **Post-Refactor Green:** same command → 7 passed; fmt clean.

## Test Results
- `cargo test ... --test test_dream_state_contract -- --skip ignored` → 7 passed, 24 ignored. JSON consumer compatibility verified.
