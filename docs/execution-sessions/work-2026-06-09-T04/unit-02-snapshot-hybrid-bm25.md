---
unit: "T04-B: snapshot_hybrid — in-memory BM25 + dense/sparse fusion"
unit_number: 2
unit_kind: expansion
serves: "Exact-term recall for tools/APIs/files/crate-names/invariants (the measured default hybrid arm)."
status: completed
attempt_count: 3
domains: [retrieval, infrastructure, mcp-server]
session_id: work-2026-06-09-T04
---

## What Was Implemented
- **Real Okapi BM25** (`crates/retrieval/src/bm25.rs`): k1=1.2, b=0.75, smooth RSJ IDF (non-negative), TF + df + doc-length normalization + avg doc length. Dependency-free. Raw token split (preserves repetitions — NOT graph_search `tokenize()` which dedups). 6 unit tests (rare-term ranking, TF saturation, IDF discrimination, candidate filter, empty corpus/query).
- **T03 WRITE-AHEAD reader**: `PersistedGraphSkillRecord` (rebuild.rs) + `list_skills()` SQL now SELECT the 7 migration-009 multi-view columns (nullable TEXT[] => empty). First production reader of T03's columns.
- **BM25 corpus at snapshot build** (mcp-server/src/lib.rs `build_graph_from_pg`): bounded lexical doc per skill from name+description+tags+tools+artifacts+invariants+use_when+requires+produces+subunit text. `avoid_when` deliberately EXCLUDED (anti-pattern terms would surface a skill for exactly the queries it must not match). Index stored as `RetrievalSnapshot.bm25_index: Option<Arc<Bm25Index>>` (atomic swap), built unconditionally so switching RETRIEVAL_BACKEND needs no rebuild.
- **Hybrid fusion** (dual_scope.rs `perform_scope_search`, `expand_candidates_with_bm25`): when `backend==SnapshotHybrid`, union dense top-K with BM25 top-K (up to `limit` extra slots), dedup, then the EXISTING eq.3 → relevance_threshold floor → MMR pipeline. `lexical_score` populated with real BM25. `SnapshotDense` unchanged; `QdrantHybrid` routes to dense pending sub-unit C.

## Fusion design + how the floor stays authoritative
BM25 expands the candidate POOL (recall) only; the final SCORE and the no-match GATE remain eq.3 + `relevance_threshold`. A high-BM25 / low-semantic lexical hit is still floored out (unit-tested: `snapshot_hybrid_relevance_floor_gates_lexical_only_hit_with_low_eq3_score`). This is pool-union fusion ("RRF or equivalent"), NOT strict RRF rank-blending — chosen to keep the calibrated floor authoritative and avoid regression risk. **Open measurement question for sub-unit D:** does pool-union improve MRR/nDCG, or is rank-blending of the BM25 signal needed? D decides empirically.

## Files Changed
- created `crates/retrieval/src/bm25.rs`; `crates/retrieval/src/lib.rs` (mod/export); `crates/retrieval/src/orchestrator.rs` (snapshot field+builder); `crates/retrieval/src/dual_scope.rs` (fusion + 2 tests); `crates/infrastructure/src/persistence/rebuild.rs` (reader); `crates/mcp-server/src/lib.rs` (corpus build).

## Test Results
- `cargo test -p retrieval`: PASS (45) · `cargo test -p mcp-server --lib`: PASS (33) · `cargo test -p infrastructure`: PASS except 2 PRE-EXISTING env-dependent `scope::tests::fs_marker_resolver_*` failures (proven pre-existing: fail at committed HEAD 8cfa173 with T04 stashed).
- Lib-only clippy (`-p retrieval -p infrastructure -p mcp-server --lib -D warnings`): CLEAN (B added no warnings).
- Full workspace `cargo test --workspace --all-targets --features test-utils --no-run`: compiles.
- `--all-targets` clippy RED = pre-existing e2e-harness dead-code blocker (not B).

## TDD Evidence
- Red: hybrid test failed (no `with_bm25_index`; then lexical target absent from candidates). BM25 TF test failed (dedup tokenizer).
- Green: 45 retrieval tests pass incl. lexical-surfacing + floor-gate.
- Post-Refactor Green: fmt + lib clippy clean; dense/scope tests unchanged.
