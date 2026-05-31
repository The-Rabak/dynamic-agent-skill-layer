---
ticket_id: T09
title: Retrieval quality — real/seeded skills actually match
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: ready # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.2: Retrieval quality — real/seeded skills actually match"
feature_home: crates/retrieval
depends_on: [T01, T02, T03]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-E (live suite is GREEN)
  - SC-V1.5-A (loop closes in the body)
files:
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/dual_scope.rs
  - tests/fixtures/retrieval_corpus.json
  - tests/e2e/test_live_data_plane_roundtrip.rs
test_command: cargo test -p mcp-server --features test-utils -- --ignored test_live_data_plane_roundtrip
tdd_mode: inherit
---

# Retrieval quality — real/seeded skills actually match

## Serves
- **SC-V1.5-E** and **SC-V1.5-A** — the `NoMatch != Ok` roundtrip failure is gone: a semantically-relevant prompt returns `ok` with the seeded skill, while cold-start stays honestly `no_match`.
- Plan SC-2.

## Scope
Root-cause the `NoMatch`: confirm seeded skills load into the live retriever (T01/T02), then tune `relevance_threshold`/`candidate_limit`/embedding alignment so relevant prompts clear the bar, and provide a realistic shared fixture corpus the live tests reuse.

- **Owns:** threshold tuning + retrieval fixture corpus + diagnosis of the snapshot-vs-match cause.
- **Non-goals:** new scoring features, learned weights (V2).

## Scope Fence
Deterministic threshold tuning only; no model swap. Keep cold-start `no_match` honest (not degraded, not forced filler).

## Acceptance Criteria
- [ ] Roundtrip and concurrency-burst tests get `ok` for relevant prompts (e.g. seed "Rust file I/O … async tokio" + prompt "how to read files in rust with tokio async" → `ok` containing the seeded skill).
- [ ] Cold-start still returns `no_match` (not degraded, not forced filler).
- [ ] Suggested starting values to validate (not hardcode blindly): `relevance_threshold: 0.20`, `candidate_limit: 50`, `max_results: 3`, `rescue_threshold: 0.15`, `scope_timeout_ms: 400` (tight with two scopes — validate).
- [ ] Provide a shared fixture corpus where ≥3 project-scope skills clear 0.20 on the roundtrip prompt; keep a named negative fixture proving cold-start `no_match`.
- [ ] `mmr_select` uses `total_cmp` (NaN-safe).

## Shared / Global Notes
- Shared fixture corpus under `tests/fixtures/` is reused by the live tests — T10 consumes it but does not redefine it; this ticket owns its creation.
- Shares `tests/e2e/test_live_data_plane_roundtrip.rs` with T08 and T10 → kept in separate sequential batches.
- **T03 dependency (Batch 2) edits the same `orchestrator.rs`/`dual_scope.rs`:** T03 relabels the health markers there before this ticket tunes thresholds. Read T03's diff before editing those files so threshold tuning doesn't conflict with or duplicate T03's health-marker changes.
- Human-gate: none.

## Local Context
**WHY:** The roundtrip test fails `NoMatch != Ok` at `test_live_data_plane_roundtrip.rs:489` even after the test's post-seed server rebuild — the seeded skill is not retrieved. After Phase 1 makes seeded skills actually load, the remaining cause is threshold/candidate/embedding tuning plus a fixture corpus whose skills semantically overlap the prompts (current ad-hoc generic seeds don't).

**Open question to surface:** confirm whether the residual `NoMatch` is a load issue (Phase 1) or a threshold issue (this ticket) by first asserting the seeded skill is present in the snapshot; if still absent after T01/T02, escalate to those tickets rather than over-tuning the threshold.

## Parent Refs
- Plan → Slice 3.2; Architecture artifact.
- Source packet: `## Execution Slices > Slice 3.2`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §3.2 (threshold values; shared fixture corpus; `total_cmp`).
- Plan §Firsthand test evidence (the `NoMatch` panic location).

## Coupling Notes
One unit because the threshold tuning and the fixture corpus must be co-designed — tuning a threshold against a corpus whose skills don't overlap the prompts proves nothing. Hard-depends on T01/T02 (seeded skills must actually be loaded/visible before tuning means anything). Singleton batch: shares `orchestrator.rs`/`dual_scope.rs` with T02/T03/T06 and `tests/e2e/*` with T08/T10.
