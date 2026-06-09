---
ticket_id: T04
title: Hybrid dense/BM25 candidate generation
kind: expansion
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 4"
feature_home: crates/retrieval
depends_on:
  - T01
  - T02
  - T03
dependency_type: hard
serves:
  - Exact-term recall for tools, APIs, files, invariants, and repo-specific language
files:
  - crates/retrieval/src/
  - crates/infrastructure/src/vector/qdrant.rs
  - crates/mcp-server/src/lib.rs
  - crates/infrastructure/migrations/
test_command: "export RETRIEVAL_CANDIDATE_BACKEND=${RETRIEVAL_CANDIDATE_BACKEND:-qdrant_hybrid} && cargo test -p retrieval && python3 scripts/retrieval_quality_live.py --split held_out --config-label ${RETRIEVAL_CANDIDATE_BACKEND} --limit 5 --out tests/e2e/reports/v17-${RETRIEVAL_CANDIDATE_BACKEND}__held_out.json --gate --regression-floor 0.60"
tdd_mode: ralph
---

# Hybrid dense/BM25 candidate generation

## Serves

Add a local hybrid candidate-generation arm so exact terms and dense semantics both contribute to retrieval.

## Scope

- Add a configurable candidate backend such as `snapshot_dense`, `snapshot_hybrid`, or `qdrant_hybrid`.
- Build bounded lexical documents from high-signal skill fields.
- Fuse dense and sparse candidate pools with measured RRF or equivalent.
- Keep hybrid behind config until live quality and latency earn default status.

## Scope Fence

- Do not quietly make Qdrant a required hot-path dependency.
- Do not preserve the old "Qdrant down cannot affect compile_context" claim when hybrid mode is active.
- Do not let BM25 exact matches bypass the calibrated no-match/relevance gate.

## Acceptance Criteria

- Hybrid arm runs through the real server and appears in T01 reports.
- Held-out MRR/nDCG/no-match precision do not regress versus current default.
- p95 latency stays under the configured budget.
- Health markers make Qdrant dependency semantics explicit when the Qdrant backend is active.
- Completion evidence records the selected backend path (`qdrant_hybrid` or `snapshot_hybrid`), Qdrant version support, and read-path health semantics.
- `RETRIEVAL_BACKEND` MUST be wired on BOTH sides simultaneously: the harness already reads/labels it, and `RetrievalConfig::from_env()` (`crates/retrieval/src/orchestrator.rs`) MUST parse it and FAIL LOUD on an unrecognized backend value — never silently default to `snapshot_dense` (which would mislabel the arm). (Source: #243 item 2.)
- qwen+hybrid p95 latency MUST be verified < 500 ms (the constitution's `compile_context` budget) before any qwen-based candidate becomes a session-start default; measured qwen p95 ≈ 409 ms is close. A flag-gated or `find_skill`-only budget needs explicit documentation if the 500 ms bound cannot be guaranteed. (Source: #243 item 3 / performance-oracle.)
- Sweep prerequisite — implement `reboot_arm(overrides)` that restarts graph-builder AND mcp-server with the arm env, polls graph-builder for ≥ 1 completed rebuild into the arm's model-keyed collection, and warms up mcp-server before measuring; add a measure-time guard that fails loud if the target collection point-count is 0. (Source: #237 deferred full-fix.)

## Shared / Global Notes

This is the central architecture migration risk. If Qdrant becomes default read path, update ADR/docs in T08. If not, document snapshot-hybrid as the default and leave Qdrant hybrid experimental.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: close dense-only recall/ranking gaps for exact tools, files, APIs, and invariants.
- Current read path is in-memory `RetrievalSnapshot`; Qdrant is write-side only.
- Important unknown: Qdrant version and sparse/BM25 Rust adapter support may force an in-memory BM25 fallback. If so, record `snapshot_hybrid` as the selected backend and keep Qdrant hybrid experimental.
- Evidence command is backend-selectable: execution may run the default `qdrant_hybrid` arm or set `RETRIEVAL_CANDIDATE_BACKEND=snapshot_hybrid` when Qdrant sparse/BM25 support is not viable.

## Inherited Changes — V1.7 batch 1-2 triage (todos 228-244)

These landed on `feat/v-1-7` during the 228-243 triage swarm (2026-06-09) and bind this ticket (the three AC bullets above sourced from #243/#237 are the hard prerequisites; the below are API/behavior changes every new Qdrant/embedder caller must adopt):

- **`model_keyed_collection_name(model)` now returns `Result<String, QdrantError>`** (was `String`, #234). Any new BM25/sparse/hybrid collection-name derivation must `?`/handle the Result.
- **`QdrantAdapter::new` now charset-validates the collection name** against `^[A-Za-z0-9_-]+$` and FAILS LOUD (#241). Any new collection this ticket introduces (sparse/BM25/hybrid) MUST use that charset or construction fails loud.
- **`QdrantConfig::default().collection_name` is now the sentinel `UNCONFIGURED__use_model_keyed_collection_name`** (was `"skills"`, #236). Never rely on the default — set `collection_name` explicitly (the sentinel is charset-valid but points at a nonexistent collection, so it fails loud at runtime if used).
- **Embedder-model resolution is centralized** in `infrastructure::embedding_model_from_env()` / `resolve_embedding_model(Option<&str>)`, default once as `DEFAULT_EMBEDDING_MODEL` (#232). Do NOT re-read `OLLAMA_EMBED_MODEL` directly.
- **DimensionMismatch guard is "fatal only when Qdrant reachable at boot"** (#235), not unconditionally — the offline window is observable (loud warn + first-relay-write error) but NOT fully closed (relay re-`ensure_collection` on reconnect remains future work). Switching to a new arm REQUIRES graph-builder to rebuild the write side into the arm's model-keyed collection before reads are valid (this is exactly what the `reboot_arm` AC enforces).
- **Harness honesty gates exist** (#229/#230): `_probe_ollama_dimension(model)` is the dimension seam; `--require-dimension` / `--gate` fail loud on a null dimension for a live arm. A gated arm with no real dimension fails loud — design new arms accordingly.
- **`embedding_model_metadata` is written per rebuild + `/health` exposes `embedding_arm`** (#228/#239) — use these for honest arm attribution in reports instead of re-discovering.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/reference/online-retrieval-cqrs.md`
- `docs/reference/retrieval-contract.md`
- https://qdrant.tech/documentation/search/text-search/
- https://qdrant.tech/documentation/manage-data/vectors/

## Coupling Notes

T06 depends on this to expose dense/lexical scores. T08 must reconcile docs with whatever backend becomes default.
