---
unit: "T06 SkillDAG-style agent retrieval tools (owns folded #255, #260)"
unit_number: 1
unit_kind: expansion
serves: "Agent-callable typed graph surface (matches/neighbors/conflicts/show-body) + honest relevance scores and provenance; unblocks T11"
status: completed
attempt_count: 2
domains: [mcp-server, retrieval, admin, protocol, health]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/06-skilldag-style-agent-retrieval-tools.md
session_id: work-2026-06-11-T06
---

## What Was Implemented

All seven T06 sub-deliverables, delivered by a sonnet execution-agent + one orchestrator-driven fix cycle + an orchestrator-added inspect assertion.

1. **#255 P1-B — find_skill rationale.** `SkillMatch` (`crates/mcp-server/src/tools/find_skill.rs`) gains `rationale: Vec<String>` mapped from `retrieved.scored_skill.rationale` (the `rrf=/semantic=/subunit_evidence=/lexical=` components the orchestrator already computes).
2. **#260 — relevance-meaningful score.** `score` now exposes the eq.3 relevance, not the RRF rank artifact. Implemented robustly (after fix cycle): a new `semantic_score: f32` field threaded onto `domain::ScoredSkill`, populated from `candidate.semantic_score` in `orchestrator.rs`, read directly in find_skill (`format!("{:.3}", scored_skill.semantic_score)`). RRF artifact preserved separately as `fusion_rank_score` for ordering provenance. (First pass string-parsed `semantic=` from rationale — replaced with the threaded field to remove the fail-silent-to-0.000 risk.)
3. **#255 P1-A/P2-E — 7 multi-view fields via inspect_skill.** `use_when/avoid_when/artifacts/tools/invariants/requires/produces` projected from migration-009 columns (`PersistedGraphSkillRecord`) into `SkillSnapshot` + `InspectedSkill` + the `PostgresGraphSnapshotReader::list_skills()` projection (`crates/admin/src/tools.rs`). inspect_skill tool description updated. Proven readable end-to-end by a new assertion in `tests/integration/test_admin_tools.rs` (JSON-RPC `/skill/use_when/0` etc. round-trip).
4. **#255 P2-C/D — /health backend.** `crates/mcp-server/src/main.rs` registers a static `retrieval_backend` health component (`backend=snapshot_dense|snapshot_hybrid|qdrant_hybrid`) from the real `RetrievalConfig.backend` at boot.
5. **#243 — retrieval_context provenance.** `find_skill`/`search_skill_graph` carry optional `retrieval_context { embedding_model, collection, graph_version }`, wired in `build_live_server` from `embedding_model_info.model_name` + `qdrant_adapter.config.collection_name` (reuses the persisted embedding_model_metadata source, not re-discovered).
6. **Graph surface — matches/neighbors/conflicts + show bodies.** New `crates/mcp-server/src/tools/search_skill_graph.rs` tool, registered in `protocol.rs`. Returns separate `matches`, `neighbors` (depends_on/composes_with/similar_to from T05 `skill_edges`), `conflicts` (conflicts_with — never folded into neighbors), plus `retrieval_context` and `latency_ms`. After fix cycle: edges are FILTERED to those incident on matched skills (skill_id threaded onto SkillMatch), with correct outbound/inbound direction; a real edge-store `Err` returns a `degraded` response (`graph_edge_read_failed`) — no silent empty-edge fallback. Full bodies remain available via inspect_skill.
7. **Live-server proof.** New `tests/e2e/test_skill_graph_tools.rs` (`#[ignore]`-gated) drives the real MCP HTTP endpoint (127.0.0.1:3001/mcp) via `McpClient`: structural contract on find_skill (rationale/fusion_rank_score/retrieval_context), three-section search_skill_graph (conflicts-never-in-neighbors, latency), and /health embedding_arm + retrieval_backend. Asserts contract/structure (not 234-corpus IDs, which are 0/30 on the live 262 corpus).

## Files Changed
- `crates/domain/src/types.rs` — `ScoredSkill.semantic_score: f32` (fix cycle)
- `crates/retrieval/src/orchestrator.rs` — populate `semantic_score` from `candidate.semantic_score`
- `crates/compiler/src/lib.rs` — test literal updated for the new field
- `crates/mcp-server/src/tools/find_skill.rs` — rationale, score=#260 relevance, fusion_rank_score, skill_id, retrieval_context, with_provenance
- `crates/mcp-server/src/tools/search_skill_graph.rs` — created (matches/neighbors/conflicts, matched-skill edge filtering, fail-loud edge read)
- `crates/mcp-server/src/lib.rs` — register search_skill_graph, with_find_skill_provenance wiring in build_live_server
- `crates/mcp-server/src/protocol.rs` — search_skill_graph registration + /health backend unit test + inspect description
- `crates/mcp-server/src/main.rs` — retrieval_backend static health component (real boot wiring)
- `crates/admin/src/tools.rs` — 7 multi-view fields on SkillSnapshot/InspectedSkill + list_skill_edges (trait default + Postgres impl)
- `crates/mcp-server/Cargo.toml` — [[test]] test_skill_graph_tools
- `tests/integration/test_admin_tools.rs` — multi-view literals + #255 readable assertion (orchestrator)
- `tests/e2e/test_skill_graph_tools.rs` — created (live-server contract test)

## Orchestrator Review Findings (fix cycle, all resolved)
- **P1** search_skill_graph returned the ENTIRE graph topology (not neighbors of matches) → fixed: filter edges by matched skill_ids + correct direction.
- **P2** `list_skill_edges` Err silently swallowed → fixed: fail-loud `degraded` (`graph_edge_read_failed`).
- **P2** #260 score recovered by fragile cross-crate string-parse (fail-silent to 0.000) → fixed: threaded `semantic_score` field.
- **P2** dead/contradictory test setup in search_skill_graph (invoke built then discarded) → fixed: clean synchronous classifier tests + direction/filter coverage.
- **Orchestrator add:** #255 P1-A had no direct assertion → added inspect_skill multi-view round-trip assertion.

## Test Results
- `cargo test -p retrieval` → 78/0
- `cargo test -p mcp-server --lib` → 40/0
- `cargo test -p mcp-server --features test-utils --test test_admin_tools` → 6/0 (incl. new multi-view assertion)
- `cargo test -p domain -p compiler` → green
- `cargo test -p mcp-server --features test-utils --test test_skill_graph_tools --no-run` → compiles
- `cargo fmt -p domain -p retrieval -p mcp-server -p compiler -p admin --check` → clean
- Live-server `--ignored` test_skill_graph_tools → run by orchestrator after container rebuild (see below).

## TDD Evidence (Ralph)
- **Red:** new behavioral tests (semantic_score distinctness #260, edge filtering/direction, conflicts-separation, /health backend, multi-view readable) did not exist / could not pass before the impl; adding `semantic_score` to ScoredSkill broke existing literals (compile-red).
- **Green:** 78 retrieval + 40 mcp-server lib + 6 admin-integration pass; e2e binary compiles.
- **Post-Refactor Green:** after fix cycle + `cargo fmt`, all suites re-run green.

## Live-Server Result
_(filled in by orchestrator after container rebuild — see STATE.md / index.)_
