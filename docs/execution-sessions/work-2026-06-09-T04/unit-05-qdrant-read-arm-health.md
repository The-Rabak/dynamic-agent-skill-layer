---
unit: "T04-C3: read-side qdrant_hybrid arm + health markers"
unit_number: 5
unit_kind: expansion
serves: "Experimental qdrant_hybrid arm end-to-end (request-time Qdrant dense+sparse fusion) + read-path health semantics."
status: completed
attempt_count: 1
domains: [retrieval, infrastructure, mcp-server]
session_id: work-2026-06-09-T04
---

## What Was Implemented
- **`crates/retrieval/src/hybrid.rs`** (new): `HybridCandidateSource` async trait + `HybridCandidate{skill_stable_id, score}` + `HybridQueryError` — retrieval-side seam so retrieval does NOT dep infrastructure.
- **orchestrator.rs**: `hybrid_candidate_source: Option<Arc<dyn HybridCandidateSource>>` field + `with_hybrid_candidate_source` builder; `search_scopes_qdrant_hybrid` async path; QdrantHybrid dispatch; backend-aware health (`healthy_markers_for_backend`/`degraded_marker_for_backend`) adding `qdrant_hybrid_read` ONLY under QdrantHybrid (snapshot invariant preserved). Fail-loud when source absent OR Qdrant errors (NO silent dense fallback — #243).
- **dual_scope.rs**: `search_scopes_with_qdrant_candidates` / `perform_scope_search_with_qdrant_candidates` — maps Qdrant hits (by `skill_stable_id`) to snapshot skills, scope-filters, runs EXISTING eq.3 → relevance floor → MMR (floor authoritative).
- **mcp-server lib.rs**: `QdrantHybridCandidateSource` wrapping `QdrantAdapter::query_hybrid` + `query_sparse_vector` + `model_keyed_hybrid_collection_name`; injected into orchestrator ONLY when backend==QdrantHybrid.
- **infrastructure health.rs**: `qdrant_read_path` HTTP probe registered ONLY when RETRIEVAL_BACKEND=qdrant_hybrid (kept `qdrant_write_side` as-is). 2 guard tests.

## Scope fence honored
snapshot_dense/snapshot_hybrid retrieve + health byte-identical; Qdrant read marker appears only under qdrant_hybrid (guard test). retrieval gained NO infrastructure dep (trait injection). Reused C1 query_hybrid + C2 query_sparse_vector.

## Test Results
- 5 QdrantHybrid arm tests PASS: hit→snapshot mapping+eq3 rank; floor gates low-eq3 hit; fail-loud on source-absent; fail-loud on Qdrant-down; health marker only under qdrant_hybrid. +2 health guard tests.
- retrieval 56 lib tests pass; lib clippy (3 crates) `-D warnings` clean; fmt clean; workspace `--all-targets --no-run` compiles.
- infrastructure: 2 PRE-EXISTING `scope::tests::fs_marker_resolver_*` failures (env-dependent; unrelated).

## TDD Evidence
- Red: trait/arm absent → compile fail.
- Green: 5 arm tests + 2 health guards pass.
- Post-Refactor Green: fmt + lib clippy clean; snapshot arms + their no-Qdrant-read invariant unchanged.

## Gap folded into D
C3 unit tests use a MOCK HybridCandidateSource. The LIVE end-to-end proof (real Qdrant query → real mapped skills through the running mcp-server) is exercised by sub-unit D's live sweep (RETRIEVAL_BACKEND=qdrant_hybrid). D MUST prove qdrant_hybrid returns real non-empty mapped results (fail loud if empty), not just measure.

## Note
- qdrant_hybrid intentionally BREAKS the CQRS "Qdrant down can't affect compile_context" contract (ADR deferred to T08). It fails loud when Qdrant unavailable — by design.
- Health test uses RFC 5737 non-routable probe (slow ~30s due to reqwest default timeout) — functionally correct.
