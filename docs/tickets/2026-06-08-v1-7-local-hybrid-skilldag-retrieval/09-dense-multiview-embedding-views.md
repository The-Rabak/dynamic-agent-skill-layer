---
ticket_id: T09
title: Dense multi-view embedding views (e_task / e_needs / e_negative)
kind: expansion
status: completed
status_note: "Code delivered + committed (eb3d5df, 87c0e11). The measured real-server ON/OFF sweep AC is delegated to T11 (owner decision 2026-06-11): the only labeled eval fixture is 0/30 aligned with the live 262 qwen3 corpus, so an aligned-fixture sweep is T11's deliverable. Dense-views flag stays default-OFF until T11 measures a non-negative delta."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Proposed V1.7 Architecture > Embedding views (lines 243-251)"
source_packet_ref: "plan ## Data Model Additions > Embedding views; promoted from the plan-to-tickets gap surfaced 2026-06-09"
feature_home: "crates/graph-builder (build) and crates/mcp-server (snapshot) and crates/retrieval"
depends_on:
  - T03
dependency_type: hard
serves:
  - Let DENSE retrieval exploit the T03 multi-view fields, not just sparse/BM25
files:
  - crates/mcp-server/src/lib.rs
  - crates/graph-builder/src/graph/build.rs
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/dual_scope.rs
  - crates/infrastructure/src/embeddings/
test_command: "cargo test -p retrieval && cargo test -p mcp-server --lib && real-server retrieval sweep dense-views off vs on"
tdd_mode: ralph
---

# Dense multi-view embedding views (e_task / e_needs / e_negative)

## Serves

The plan's target pipeline specifies five embedding views (plan lines 243-251) but only two exist today: `e_summary` (name+description+tags) and `e_subunit`. The DENSE candidate path embeds ONLY `e_summary` (`build_graph_from_pg` → `format!("{} {} {}", name, description, tags)`); only the sparse/BM25 `skill_lexical_document` reads the 9 multi-view fields. So even on a multi-view-rich corpus, dense retrieval stays blind to `use_when`/`tools`/`artifacts`/`invariants`/`requires`/`produces`/`avoid_when`. This ticket builds the missing dense views so dense retrieval can exploit T03's structured fields.

## Scope

- Add the plan's multi-view dense views: `e_task` (use_when + procedures + artifacts + tools), `e_needs` (prerequisites + requires + failure modes), `e_negative` (avoid_when + anti-patterns). Keep `e_summary` and `e_subunit`.
- Embed each view with bounded text (no single unbounded body blob — honor the embedding-window discipline).
- Decide and document how views combine at scoring time (per-view cosine fused, or max-over-views) without breaking the eq.3 contract; measure before defaulting.
- Store/rebuild views on the snapshot; record which views exist in graph metadata.

## Scope Fence

- Do not concatenate full bodies into one giant embedding input (plan line 251).
- Do not change the eq.3 floor calibration without a real-server re-sweep.
- Do not make any view default-on until a measured sweep shows non-negative MRR/nDCG delta.
- Do not reintroduce community boost or graph-as-multiplier.

## Acceptance Criteria

- `e_task` / `e_needs` / `e_negative` are built from the T03 fields with bounded text and are observable in graph/snapshot metadata. ✅ delivered (eb3d5df, 87c0e11).
- A real-server sweep records the held-out MRR/nDCG/no-match delta of dense-multi-view ON vs OFF (drives the live mcp-server; no in-process rig). ⤳ **DELEGATED to T11** (owner decision 2026-06-11): the committed eval fixture is 0/30 aligned with the live 262 qwen3 corpus, so an honest sweep needs T11's corpus-aligned fixture. Harness arm is wired and runnable.
- Views that don't improve quality are left off by default, with the measured delta recorded — not shipped blindly. ✅ flag is default-OFF; the measured delta is recorded by T11.
- p95 stays within the constitutional budget with the new views enabled.
- `cargo test -p retrieval` green; no eq.3 regression on the existing held-out set.

## Local Context

- WHY source: plan `## Proposed V1.7 Architecture > Embedding views` and `## Indexing Additions > Dense`.
- This is the DENSE counterpart to T04's sparse/BM25 multi-view indexing. T11 (multi-view re-sweep) consumes this to measure the full multi-view story, not just the lexical half.
- Couples with T11 (re-sweep) and the [[hybrid-is-the-retrieval-bet]] decision: the hybrid bet is only fully testable once BOTH sparse and dense exploit the multi-view fields.

## Source

Promoted 2026-06-09 from the plan-to-tickets gap identified while triaging #259/#250: the plan mandates 5 embedding views; T04 built only the sparse multi-view document; the dense views were never ticketed. No prior todo number.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
