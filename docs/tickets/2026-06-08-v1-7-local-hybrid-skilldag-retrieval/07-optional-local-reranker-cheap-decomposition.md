---
ticket_id: T07
title: Optional local reranker and cheap query decomposition
kind: expansion
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 7"
feature_home: crates/retrieval
depends_on:
  - T04
dependency_type: hard
serves:
  - Optional P@1/MRR improvement when hybrid retrieval still falls short
files:
  - crates/retrieval/src/
  - crates/infrastructure/src/embeddings/
  - crates/mcp-server/src/lib.rs
test_command: "cargo test -p retrieval && RETRIEVAL_RERANKER=local RETRIEVAL_QUERY_DECOMPOSITION=cheap python3 scripts/retrieval_quality_live.py --split held_out --config-label local-reranker-cheap-decomposition --limit 5 --out tests/e2e/reports/v17-reranker-cheap-decomposition__held_out.json --gate --regression-floor 0.60"
tdd_mode: ralph
---

# Optional local reranker and cheap query decomposition

## Serves

Measure whether a local reranker or bounded cheap decomposition can close remaining ranking gaps without violating retrieval latency constraints.

## Scope

- Add opt-in local reranking over top-N candidates if a local model path is selected.
- Add cheap decomposition only: caller-provided subqueries, deterministic term extraction, or bounded extra dense queries that pass p95.
- Report quality and latency delta versus reranker/decomposition off.

## Scope Fence

- Do not call external paid APIs.
- Do not add default LLM query decomposition on the hot path.
- Do not enable reranking by default unless the real-server sweep proves both quality gain and acceptable p95.

## Acceptance Criteria

- Reranker/decomposition flags are off by default.
- On/off sweep reports MRR, nDCG, P@1 if available, no-match precision, and p95 latency.
- Query decomposition is limited to deterministic or caller-provided forms unless an explicit non-default agent tool path spends the time.
- If latency/quality does not justify the feature, docs say it remains experimental.
- Live held-out retrieval report proves the on/off delta; unit tests alone are not sufficient.

## Shared / Global Notes

This ticket is optional by design. It should be skipped or kept experimental if T04 already reaches the target or if local reranking is too slow for practical use.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: test the reranker bet without making retrieval expensive for normal users.
- User explicitly wants retrieval to stay quick; query decomposition is allowed only when runtime cost is low.
- Important unknown: suitable local reranker availability and hardware cost must be measured, not assumed.

## Inherited Changes — V1.7 batch 1-2 triage (todos 228-244)

These landed on `feat/v-1-7` during the 228-243 triage swarm (2026-06-09) and bind this ticket:

- **If this ticket introduces a rerank/decomposition Qdrant collection or reads the embedder model**, the same API rules as T04 apply: `model_keyed_collection_name` now returns `Result<String, QdrantError>` (#234); `QdrantAdapter::new` charset-validates collection names `^[A-Za-z0-9_-]+$` fail-loud (#241); never rely on `QdrantConfig::default().collection_name` (now the `UNCONFIGURED__…` sentinel, #236) — set it explicitly; resolve the embedder via `infrastructure::embedding_model_from_env()` / `resolve_embedding_model(Option<&str>)` (#232).
- **Harness honesty gates apply to the on/off sweep** (#229/#230): `--require-dimension` / `--gate` fail loud on a null dimension; the `dimension` field + per-arm latency are now in the T01 report shape, so the on/off latency-delta evidence inherits them.
- **`reboot_arm` (from T04, routed via #237/#243)** is the prerequisite for any reranker arm that changes the embedder/collection — reuse it, don't reboot mcp-server alone.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md`
- `docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md`
- https://qwenlm.github.io/blog/qwen3-embedding/

## Coupling Notes

Shares retrieval config and scoring surfaces with T04, so keep sequential. T08 should document the final default/experimental status.
