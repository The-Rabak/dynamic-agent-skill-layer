---
unit: "T21 fix-run — drain the --features test-utils Final-Gate clippy form (AC#3)"
unit_number: 1
unit_kind: hygiene-fix
serves: "AC#3 (e2e harness/support dead-code wired-or-deleted/justified) + T20's green-tree premise under the Final-Gate clippy form."
status: completed
attempt_count: 2
domains: [build, workspace-hygiene, rust, e2e-harness]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/21-workspace-gates-green.md
session_id: work-2026-06-12-012651-T21
---

## Why this 2nd pass
The 1st pass closed only the BARE clippy form. AC#3 targets the `tests/e2e/harness+support`
dead-code, which compiles ONLY under `--features test-utils` — and that form was still RED (exit 101,
~60 errors across 12 e2e binaries + 2 genuine lints). Owner directed "extend T21 now" rather than
deferring. The `test_command` in the ticket frontmatter (bare form) under-specified AC#3; the
Final-Gate form is the form that actually exercises the targeted code.

## What Was Implemented (zero production crate; all under tests/)

**Bucket A — 2 genuine lints, real behavior-preserving fixes (NOT silenced):**
1. `tests/e2e/report.rs` `push_action` — `clippy::if_same_then_else`: two `if/else if` arms with the
   identical body `existing.status = action_outcome;` merged into one `if A || (B && C)` guard.
   Truth-table-equivalent (Failed always overwrites; a Passed section downgrades to Skipped; else no-op).
2. `tests/e2e/test_dream_state_contract.rs:3012` — `clippy::absurd_extreme_comparisons`:
   `degraded_count <= DEGRADED_BUDGET` (usize, const 0) → `== DEGRADED_BUDGET`. Intent "zero degraded
   errors allowed" preserved exactly; the human-readable expectation string aligned `<=`→`==`.
   Budget value and assertion strength unchanged. Also removed a now-redundant outer
   `#[allow(dead_code, unused_imports)]` on `mod harness` (it conflicted with the new inner allow →
   `duplicated_attributes`).

**Bucket B — shared-harness dead-code class (~60 errors), justified module-root allows:**
Root cause = FEATURE DICHOTOMY: each e2e `test_*.rs` `#[path]`-includes the whole harness/support/
report module tree but exercises only part, so `dead_code` fires per-binary for genuinely
cross-binary-shared helpers. Fixed with ONE inner `#![allow(dead_code)]` (each with a one-line
rationale) at four shared module roots: `tests/e2e/harness/mod.rs`, `tests/e2e/support/mod.rs`,
`tests/e2e/report.rs`, `tests/integration/env_guard.rs`. An inner `#![allow]` in a `mod.rs` covers the
whole submodule tree, so no per-submodule/per-item allows were needed. NO blanket crate-level allow,
NO per-item paper-overs. True-orphan check: every suppressed symbol has ≥1 referencing e2e binary →
zero deletions.

## Files Changed
- `tests/e2e/report.rs` — if-merge (Bucket A#1) + module-root `#![allow(dead_code)]` (Bucket B)
- `tests/e2e/test_dream_state_contract.rs` — `<=`→`==` (Bucket A#2) + removed redundant outer allow
- `tests/e2e/harness/mod.rs` — module-root `#![allow(dead_code)]` + rationale
- `tests/e2e/support/mod.rs` — module-root `#![allow(dead_code)]` + rationale
- `tests/integration/env_guard.rs` — module-root `#![allow(dead_code)]` + rationale

## TDD Evidence
- **Red:** `cargo clippy --workspace --all-targets --features test-utils -- -D warnings` → exit 101, ~60 errors (orchestrator-captured + agent-reproduced).
- **Green:** all three of `… --features test-utils -- -D warnings`, bare `… -- -D warnings`, `cargo fmt --check` → exit 0 (orchestrator-verified independently).
- **Post-Refactor Green:** final clean re-run of all three after removing the redundant outer allow → exit 0.

## Patterns Discovered
- A shared `#[path]`-included module carrying its own inner `#![allow(dead_code)]` makes any OUTER
  `#[allow(dead_code)]` on the `mod xyz;` decl in an including binary a `duplicated_attributes` error.
  Prefer the inner attribute in the shared file; drop per-binary outer allows.
- One inner `#![allow]` at a `mod.rs` root covers the full submodule tree — smallest justified scope
  for the cross-binary shared-harness dead-code class.

## Test Results
- Command: `cargo clippy --workspace --all-targets --features test-utils -- -D warnings && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
- Result: PASS (all exit 0)
- Attempts: 2
