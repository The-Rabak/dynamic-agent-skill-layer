---
unit: "T04-C2: write-side sparse — populate Qdrant hybrid collection on rebuild"
unit_number: 4
unit_kind: expansion
serves: "Data-plane half of the approved Qdrant hybrid migration (real BM25 sparse stored in Qdrant)."
status: completed
attempt_count: 1
domains: [retrieval, infrastructure, graph-builder, mcp-server]
session_id: work-2026-06-09-T04
---

## What Was Implemented
- **`crates/retrieval/src/sparse.rs`** (new, shared write+read): `term_to_sparse_index` (FNV-1a 32-bit, deterministic), `build_skill_sparse_vectors` (per-doc BM25 tf-saturation weights, NO idf — Qdrant `modifier:idf` applies idf at query), `query_sparse_vector` (values 1.0/term, deduped — consumed by C3). Reuses Bm25Index corpus stats (new `avg_doc_length()`, `doc_lengths_map()` accessors). 6 unit tests.
- **outbox.rs**: `SparseVectorPayload{indices,values}`; `VectorUpsertRequest.sparse: Option<...>` (serde default => backward-compatible); `parse_vector_upsert_request` parses it, rejects empty/length-mismatch; `OutboxVectorStore::upsert_hybrid` (default fail-loud, QdrantAdapter overrides → `upsert_hybrid_point`); `OutboxRelay::with_hybrid_collection` routes to hybrid upsert ONLY when hybrid_collection set AND event carries sparse. 4 new unit tests.
- **graph-builder** rebuild.rs: `PostgresDurableGraphState::with_hybrid_collection`; `persist_graph_mutation` builds real per-skill sparse (same lexical doc format as mcp-server build) when hybrid; relay wired in `mark_outbox_drained`. main.rs: resolves `RETRIEVAL_BACKEND` at boot, ensures hybrid collection, wires hybrid relay. Cargo: `retrieval` promoted dev-dep → real dep.
- **mcp-server** lib.rs: ensure_hybrid_collection at boot when qdrant_hybrid (non-fatal, Option-A resilience).
- **tests/e2e/test_real_infrastructure_e2e.rs**: `#[ignore]` live test driving the REAL rebuild→outbox→relay→Qdrant path.

## Scope fence honored
Non-hybrid backends byte-identical (sparse defaults None; dense collection + upsert_vector untouched). Lexical doc format matches B's read path exactly (avoid_when excluded). Reconciler left dense-scoped (repair re-enqueues original payload intact → relay routes correctly; documented).

## Test Results
- `cargo test -p retrieval`(51) / `-p graph-builder`(19+2) / `-p mcp-server --lib`(33): PASS. infrastructure: 183 pass, 2 PRE-EXISTING scope failures (unrelated), 7 ignored.
- Lib clippy (4 crates) `-D warnings`: CLEAN. fmt: CLEAN. workspace `--all-targets --no-run`: compiles.
- **LIVE @ :16333**: rebuild under qdrant_hybrid → `skills__nomic-embed-text__hybrid` got 4 points, EACH 768-dim dense + non-empty BM25 sparse (term counts 27/33/354; weights 1.6028 for tf=1 = correct k1=1.2,b=0.75). Pasted scroll output in agent report.

## TDD Evidence
- Red: `sparse` module absent (compile fail); live test fails (no sparse upserted).
- Green: unit tests pass; live collection populated dense+sparse.
- Post-Refactor Green: fmt + lib clippy clean; non-hybrid unchanged; workspace compiles.

## Key facts for C3
- Query sparse builder ready: `retrieval::sparse::query_sparse_vector(query)`.
- Hybrid collection name: `model_keyed_hybrid_collection_name(model)` (infrastructure export).
- retrieval does NOT dep infrastructure — C3's query path needs the QdrantAdapter injected (likely via mcp-server wiring), since the retrieval orchestrator has no infra adapter today.
