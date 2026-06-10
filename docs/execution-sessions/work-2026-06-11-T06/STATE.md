---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/06-skilldag-style-agent-retrieval-tools.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "plan ## Execution Slices > Slice 6"
brainstorm_ref: none
started: 2026-06-11
status: in_progress
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-11-T06
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: Expose retrieval as an agent-callable typed graph surface (matches / neighbors / conflicts + show-body), with honest relevance scores and provenance, instead of opaque ranked context injection.
- Success-criteria focus: T06 ACs — matches/neighbors/conflicts separation; show_skill full bodies; score/rationale detail; live-server MCP proof; retrieval_context provenance (#243); 7 multi-view fields readable via inspect_skill (#255); find_skill rationale (#255 P1-B); /health backend (#255 P2-C/D); relevance-meaningful score (#260).

### TDD Contract
- Effective mode: Ralph-driven TDD (ticket tdd_mode: ralph)
- Effective loop: failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: unit (cargo test -p retrieval; cargo test -p mcp-server --lib) + real-server MCP/HTTP test (the `--ignored test_skill_graph_tools` integration test drives a live graph). Live-server proof is an AC.
- Exceptions: none

### Constitution Context
- docs/constitution.md present. Approval-sensitive areas relevant here: embedding model changes and schema migrations (NOT touched by T06 — no new storage; multi-view fields already persisted by migration 009). Qdrant hot-path promotion (NOT touched by T06). No constitution waivers needed.
- Standing machine rule: NO stubs/fakes/placeholders in production paths or non-unit tests — fail loud. The live-server MCP test must drive the real app, not an in-process rig.

### Architecture Handoff
- Artifact: plan-derived handoff (parent plan ## Architectural Context / ## Proposed V1.7 Architecture). No separate architecture artifact.
- Feature homes: crates/mcp-server/src/tools and crates/retrieval (primary). crates/admin/src/tools.rs (inspect_skill read model) and crates/mcp-server/src/lib.rs (/health). crates/compiler/src ONLY for an explicitly-measured compile_context change — otherwise untouched.
- Shared / global decisions: user-facing MCP protocol — preserve find_skill backward compatibility; do not blindly inject graph neighbors into compile_context; do not bloat session-start context; do not hide conflict signals inside positive match scores.
- Deepening candidates to preserve: neighbors/conflicts come from T05's typed skill_edges (depends_on/composes_with/similar_to/conflicts_with). retrieval_context source already exists server-side (embedding_model_metadata row + /health embedding_arm; model_keyed_collection_name -> Result).
- Deletion test: keep find_skill concrete + compatible.
- Interfaces as test surfaces: the MCP tool JSON contracts (find_skill, search_skill_graph, show_skill/inspect_skill, /health) are the behavioral contracts under test.
- Review guidance: /workflows:review must verify no compiler context-injection regression, score-semantics correctness (#260), provenance correctness (#243), and live-server proof honesty (no fakes).

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T06 SkillDAG-style agent retrieval tools | expansion | Agent-callable typed graph surface (matches/neighbors/conflicts/show-body) + honest scores/provenance; unblocks T11 | in_progress | -- | unit-01-skilldag-agent-retrieval-tools.md |

## Learnings Brief
- [retrieval/qwen3] Live mcp-server serves the T10 262-skill qwen3 corpus (skill_layer_test / skills__qwen3-embedding-4b, dim 2560). The committed 234-corpus eval fixture is 0/30 aligned with it — relevant for any live retrieval assertions (assert on structural/contract behavior, not on specific 234-corpus skill IDs).
- [retrieval/#260] orchestrator.rs:989 fusion overwrites FusedCandidate.score with the RRF rank artifact (~0.016, ~2 quantized values), surfaced verbatim at find_skill.rs:57 as `score`. The eq.3 relevance/semantic is the meaningful number to expose.
- [retrieval/#255] orchestrator already computes per-skill rationale (orchestrator.rs:1122 / ~1024-1027 region); SkillMatch currently drops it.
- [provenance/#243] retrieval_context data already exists: embedding_model + collection in embedding_model_metadata (key='active'), surfaced via /health embedding_arm; model_keyed_collection_name returns Result<String,QdrantError>. Reuse, don't re-discover.
- [tests] crates/mcp-server/tests/ is currently empty — test_skill_graph_tools.rs must be created as the live-server (--ignored) integration test.
- [safety] WSL2: single serial agent only; no parallel/background cargo, no concurrent heavy actions. Live PG for --ignored tests = 127.0.0.1:15432; mcp-server HTTP = 127.0.0.1:3001.
