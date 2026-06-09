---
unit: "T04-C1: Qdrant adapter — hybrid collection, sparse upsert, Query-API fusion"
unit_number: 3
unit_kind: expansion
serves: "Adapter foundation for the experimental qdrant_hybrid arm (owner-approved full sparse+dense)."
status: completed
attempt_count: 1
domains: [infrastructure, qdrant]
session_id: work-2026-06-09-T04
---

## What Was Implemented (qdrant.rs, additive only)
- `SparseVector {indices, values}`, `HybridHit {point_id, score, payload}` public types.
- `model_keyed_hybrid_collection_name(model) -> Result` => `skills__<slug>__hybrid` (distinct collection; charset-safe; existing dense path untouched because named vs unnamed vector schemas can't share a name).
- `ensure_hybrid_collection(name, dense_size)`: PUT named `{vectors:{dense:{size,Cosine}}, sparse_vectors:{sparse:{modifier:idf}}}`; 200/409 = success.
- `upsert_hybrid_point(name, id, dense, sparse, payload)`: named-vector point body, `?wait=true`.
- `query_hybrid(name, dense, sparse, limit)`: POST `/points/query` with 2-arm prefetch (`using: dense` + `using: sparse`) + `fusion: rrf`; parses `result.points` -> `Vec<HybridHit>`.

## Scope fence honored
- `ensure_collection` + `upsert_vector` (dense durability path used by ALL backends) UNTOUCHED.
- No graph-builder/retrieve/mcp-server wiring (C2/C3).

## Test Results (REAL Qdrant)
- `cargo test -p infrastructure vector::qdrant`: 31 passed, 3 ignored(live); lib clippy `-D warnings` clean; fmt clean.
- LIVE @ http://127.0.0.1:16333 (host port; NOTE 16333 not 6333): 3 ignored tests PASS —
  - ensure: GET shows sparse_vectors.sparse config present.
  - upsert: point round-trips dense `[0.18,0.36,0.54,0.73]` + sparse `{indices:[5,42],values:[0.8,0.3]}`.
  - query: RRF-fused order `[pt2 score1.0, pt3 0.333, pt1 0.25]` as designed.

## TDD Evidence
- Red: new test names matched 0 tests at baseline (stash).
- Green: 31 unit + 3 live pass.
- Post-Refactor Green: fmt + lib clippy clean; existing qdrant tests unchanged.

## Key facts for C2/C3/D
- Live Qdrant host port = **16333** (not 6333). Use `QDRANT_URL=http://127.0.0.1:16333`.
- Hybrid collection name = `skills__<model-slug>__hybrid` (separate from dense `skills__<model-slug>`).
- Sparse design: store per-doc term sparse vectors; collection `modifier: idf` => Qdrant applies IDF; query sends query-term sparse vector; fusion=rrf server-side.
