---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/04-hybrid-dense-bm25-candidate-generation.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "## Execution Slices > Slice 4"
brainstorm_ref: null
started: 2026-06-09
status: completed
execution_shape: vertical-slices
current_unit: 6
total_units: 6
session_id: work-2026-06-09-T04
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: add a local hybrid candidate-generation arm so exact terms (tools, APIs, files, invariants, crate names) AND dense semantics both contribute to retrieval — closing dense-only recall gaps.
- Success-criteria focus: hybrid arm runs through the REAL server and appears in T01 reports; held-out MRR/nDCG/no-match precision do not regress vs current default; p95 < 500ms; Qdrant dependency semantics explicit in health when qdrant backend active.

### Owner Decision (2026-06-09)
- Backend scope: **BOTH** — `snapshot_hybrid` (in-memory BM25 + RRF) as the measured DEFAULT arm, AND `qdrant_hybrid` wired EXPERIMENTAL (flag-only, not default). `snapshot_dense` remains the current default until the live sweep earns a change.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan overrides; mode=ralph, unit + e2e required, no exceptions).
- Required evidence: unit (backend enum/from_env fail-loud; BM25 scoring; RRF fusion ordering; qdrant query/sparse JSON shape via mock REST) + e2e (REAL-server live sweep snapshot_hybrid vs snapshot_dense baseline; p95 < 500ms; gate regression-floor 0.60).
- STANDING RULE (memory): ALL retrieval/quality measurement drives the REAL running mcp-server over HTTP. NO in-process reconstruction. Sweeps = env-tunable real-server config + restart the real server.

### Constitution Context
- v2.1.0. APPROVAL-SENSITIVE: Qdrant hot-path promotion (changes CQRS resilience). qdrant_hybrid is EXPERIMENTAL/flag-only this ticket — NOT promoted to default read path; the CQRS contract change + ADR is deferred to T08. snapshot_hybrid keeps Qdrant write-only (resilience intact).
- No external paid APIs. Local-first. zero-touch <500ms compile_context budget.

### Architecture Handoff (from Explore map 2026-06-09)
- `RetrievalConfig` (orchestrator.rs:178-196), `from_env()` via `env_or` fail-loud helper (331-347); mirror `CommunityBoostMode` FromStr (53-77) for new `RetrievalBackend` enum.
- Dense pool: `rank_by_cosine` (cosine_rank.rs:22) called in `perform_scope_search` (dual_scope.rs:227); candidate = `FusedCandidate` (fusion.rs:6-19) with `lexical_score` field already present (currently weak token-overlap from graph_search.rs).
- `fusion.rs`: `weighted_reciprocal_rank_fusion(&[ScopeRanking], k, max_results)` (scope-level RRF). Reuse/extend for dense+sparse POOL RRF inside dual_scope.
- `RetrievalSnapshot` (orchestrator.rs:79-106) {graph_version, skills: Vec<SeededSkill>, community_centroids}. Built in `build_graph_from_pg` (mcp-server/src/lib.rs:890, assembly ~1074-1205). BM25 index built after seeded_skills, stored on snapshot.
- T03 fields NOT on read path yet: `PersistedGraphSkillRecord` (rebuild.rs:129-142) + `list_skills` SQL (180-205) do NOT SELECT migration-009 columns. Sub-unit B adds this reader (the T03 WRITE-AHEAD consumer) to feed the BM25 corpus.
- Qdrant adapter (qdrant.rs): raw reqwest REST, WRITE-ONLY (no search/query method). `ensure_collection` (227, body 290) dense-only. qdrant_hybrid must ADD a query method (`/collections/{name}/points/query`) + named dense+sparse vectors + IDF. Inherited #234/#236/#241 API: `model_keyed_collection_name -> Result`, charset guard `^[A-Za-z0-9_-]+$`, default collection_name is fail-loud sentinel.
- Health: `health.rs` `qdrant_write_side` probe (233) is deliberately write-side-only; read markers in orchestrator.rs (524-544) EXCLUDE qdrant (guarded test ~911). qdrant_hybrid active => surface qdrant as a read-path dependency (conditional marker + probe).
- Harness: `retrieval_quality_live.py` measurement-only (reads RETRIEVAL_BACKEND ~171); `retrieval_quality_sweep.py` has `reboot_mcp` (209), `wait_ready` (233), `assert_collection_nonempty` (148) but NO `reboot_arm`. Sub-unit D adds `reboot_arm` + uncomments hybrid arm (112-113).
- Live PG :15432; Qdrant REST :6333; full docker stack RUNNING. Qdrant v1.18.0 (sparse/BM25+IDF supported).
- Inherited: embedder resolution centralized `infrastructure::embedding_model_from_env()` / `resolve_embedding_model` — do NOT re-read OLLAMA_EMBED_MODEL directly.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | A: RetrievalBackend enum + fail-loud from_env + orchestrator wiring | infra-packet | backend selection substrate; #243 fail-loud AC | completed | 1 | unit-01-backend-config.md |
| 2 | B: snapshot_hybrid — read T03 fields, in-memory BM25, dense+sparse RRF | expansion | exact-term recall (default measured arm) | completed | 3 | unit-02-snapshot-hybrid-bm25.md |
| 3 | C1: Qdrant adapter — named dense+sparse(idf) collection, sparse upsert, Query-API dense+sparse fusion read method | expansion | Qdrant hybrid capability (adapter) | completed | 1 | unit-03-qdrant-adapter.md |
| 4 | C2: write-side sparse — graph-builder computes per-skill BM25 sparse vector through outbox/relay/upsert; reconciler sparse-aware | expansion | Qdrant collection populated dense+sparse | completed | 1 | unit-04-qdrant-writeside-sparse.md |
| 5 | C3: read-side qdrant_hybrid arm + health markers (Qdrant as read-path dep when active) | expansion | experimental qdrant_hybrid arm end-to-end | completed | 1 | unit-05-qdrant-read-arm-health.md |
| 6 | D: reboot_arm + live real-server sweep (snapshot_dense vs snapshot_hybrid vs qdrant_hybrid; p95<500ms; gate 0.60) | hardening | e2e measurement proof | completed | 1 | unit-06-reboot-arm-live-sweep.md |

## OUTCOME: all 6 sub-units complete. T04 ACs MET (hybrid arms real + measured + p95<500ms + no regression). Honest finding: no quality uplift from hybrid/sparse on the 234 corpus (all arms MRR 0.767); snapshot_dense stays default; 0.80 unmet (embedding/scoring ceiling, not architecture). qdrant_hybrid end-to-end real (2 prod bugs caught by the live run). Hands T08/#205/#218 a measured substrate.

OWNER DECISION (2026-06-09): qdrant_hybrid = FULL Qdrant sparse+dense hybrid (real end-to-end), not dense-only. Largest lift; breaks CQRS resilience when active (ADR deferred to T08); approval-sensitive — owner approved.

WRITE-PATH anchors: outbox events published in graph-builder rebuild.rs; `parse_vector_upsert_request(payload)` {content_hash, vector, payload}; OutboxRelay reads outbox_events → `upsert_vector`; `outbox_reconciler.rs` re-derives expected points via `qdrant_point_id_from_content_hash`. Sparse must thread through ALL of these. Adapter: ensure_collection (qdrant.rs ~227, dense-only body ~290), upsert_vector (~353), no query method. ensure_collection at boot in mcp-server/lib.rs ~699 with `vector_size`.
SPARSE design: store per-doc term-frequency sparse vectors (term->u32 index via stable hash; value=tf), collection sparse config `modifier: idf` so Qdrant applies IDF; query sends query-term sparse vector; fuse dense+sparse via Qdrant Query-API prefetch + fusion (rrf/dbsf). Reuse B's tokenizer for write/read consistency.

## Learnings Brief
- [T03 carryover] WRITE-AHEAD 009 columns exist; sub-unit B is their first reader.
- [gate] Pre-existing workspace `--all-targets -D warnings` clippy RED from e2e harness dead-code (not ours). Verify T04-owned crates clippy-clean; don't attribute harness errors to T04.
