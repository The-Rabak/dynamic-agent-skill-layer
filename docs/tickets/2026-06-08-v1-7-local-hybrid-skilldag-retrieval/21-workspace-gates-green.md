---
ticket_id: T21
title: Workspace gates green — clippy -D warnings + fmt clean at HEAD (the honest-tree gate)
kind: hygiene
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "constitution: honest tree; V1.7 final gate requires cargo clippy --workspace --all-targets -D warnings and cargo fmt --check green"
source_packet_ref: "NEW 2026-06-12 — from the post-T11 follow-up assessment §131#5 re-grade: both gates RED at HEAD with V1.7-session-introduced offenders"
feature_home: "workspace-wide mechanical hygiene (tests/e2e/harness, crates/* fmt, no behavior changes)"
depends_on: []
dependency_type: none
serves:
  - The honest-tree property every measured claim rests on; unblocks meaningful full-suite runs for T20/T14
files:
  - crates/retrieval/src/cosine_rank.rs
  - crates/graph-builder/src/graph/
  - crates/infrastructure/src/persistence/embedding_cache.rs
  - crates/infrastructure/src/health.rs
  - crates/mcp-server/src/lib.rs
  - tests/e2e/harness/
  - tests/e2e/support/
test_command: "cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check (both exit 0)"
tdd_mode: ralph
---

# Workspace gates green — the honest-tree gate

## Serves

The midpoint assessment named the workspace gates a V1.7 final-gate blocker; the 2026-06-12
follow-up re-graded them and found the debt is no longer "pre-existing" — it is V1.7's own:

- **clippy RED:** `clippy::useless_vec` at `crates/retrieval/src/cosine_rank.rs:64` (test code),
  introduced by 8bd9b32 (2026-06-09 review-fix session). The compile aborts there, which MASKS
  whatever remains behind it — the documented e2e-harness dead-code blocker
  (QdrantObserver/RedisObserver/run_docker/ScopeEnvGuard…, see
  [[workspace-clippy-e2e-harness-deadcode-blocker]]) and the compile_context_bench SeededSkill item
  cannot even be enumerated until the first error clears.
- **fmt RED:** ~31 diffs at a clean HEAD, including files landed THIS week (T17's
  `embedding_cache.rs`, `health.rs`; `graph-builder/src/graph/{edges,rebuild}.rs`;
  `mcp-server/src/lib.rs`) — recent sessions are committing unformatted code.

Every measured claim in Phase B stands on "the tree is honest"; these are mechanical fixes that must
not wait for the final gate, and T20's full-suite verification run is only meaningful on a green tree.

## Scope

- Fix the `useless_vec` offender, then re-run clippy and drain EVERY remaining
  `--workspace --all-targets -D warnings` error to zero. For the e2e-harness dead-code class: wire it
  or delete it (fail-loud philosophy — no blanket `#[allow(dead_code)]` paper-overs; a targeted allow
  is acceptable only with a one-line justification for why the code must exist unused).
- `cargo fmt` the workspace; commit.
- Identify why recent sessions land unformatted (missing pre-commit habit?) and record the
  prevention in the session learnings (a hook/checklist item, not a new tool).

## Scope Fence

- ZERO behavior changes — hygiene only. Any fix that would change runtime behavior gets surfaced as
  its own todo instead of being slipped in here.
- No gate relaxation (no crate-level allow walls, no removing targets from the gate).

## Acceptance Criteria

- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo fmt --check` exits 0.
- [x] Dead code in tests/e2e/harness+support either wired into real use or deleted (decision per
      item recorded; targeted allows justified inline).
- [x] Prevention note recorded for the unformatted-commit pattern.

## Completion Evidence (session work-2026-06-12-012651-T21)

Both gates GREEN, orchestrator-verified independently (clippy exit 0, fmt exit 0). The clippy
`useless_vec` abort at `cosine_rank.rs:64` was masking three further error classes, all drained:

1. **`useless_vec`** (`crates/retrieval/src/cosine_rank.rs:64`) — `vec![vec![…]]` → array literal in test.
2. **`items_after_test_module`** (`crates/mcp-server/src/tools/search_skill_graph.rs`) — `classify_edges_for_matches`/`emit_edge` moved before the `#[cfg(test)]` block (pure relocation, byte-identical).
3. **Dead-code class** — the real offender was NOT the QdrantObserver/RedisObserver/run_docker class but `crates/mcp-server/tests/env_guard.rs` being auto-discovered by Cargo as an orphan standalone test binary. Fixed by relocating it to `tests/helpers/env_guard.rs` (byte-identical move; subdirectory files are not auto-discovered) and updating the three `#[path]` includes. No blanket allows used.
4. **Bench compile-fix** (`tests/bench/compile_context_bench.rs`) — added the three T09 `e_task/e_needs/e_negative` empty-vec fields the bench was missing (empty == absent per the fusion contract; zero behavior delta).

fmt sweep: 31 diffs across 7 files (graph-builder, infrastructure ×4, mcp-server). All formatting-only.

**Zero behavior change audited:** every non-fmt edit is test/bench/relocation; touched-test regression green (retrieval cosine + mcp-server search_skill_graph suites pass).

**Prevention note (unformatted-commit pattern):** root cause is `cargo fmt` not enforced at commit time. Habit: run `cargo fmt` before staging; optionally a `.git/hooks/pre-commit` running `cargo fmt --check`. No new tool added.

## Local Context

- First batch of the restructured sequence: cheapest ticket, unblocks honest full-suite runs for
  everything after it.
- The `golden_path_real_app` :3001 live-suite item noted in Batch 12 is a separate live-infra
  concern, NOT owned here — this ticket is static gates only.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
