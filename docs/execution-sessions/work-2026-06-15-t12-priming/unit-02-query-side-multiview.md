---
unit: "Verbose fix — query-side multi-view (max-over-segments)"
unit_number: 2
unit_kind: expansion
serves: "fixes T18 constraint #2 verbose dilution (set-coverage@3 0.027) via the query-side twin of T09 doc-side multi-view"
status: completed
attempt_count: 1
domains: [rust, retrieval]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
session_id: work-2026-06-15-t12-priming
---

## What Was Implemented
For `RetrievalIntent::Priming` on SnapshotDense/SnapshotHybrid: segment the prompt (pure string work,
no LLM) → embed ALL segments in ONE `embed_batch` call (latency fence) → run the existing per-scope
search once per segment → merge candidates by MAX score per (scope_id, skill_id). Task path and
QdrantHybrid (any intent) stay byte-identical (single embed_text, single pass). A short/single-paragraph
prompt yields 1 segment → merge is a no-op → numerically identical to Task.

## Design rationale (low-risk, byte-identical)
Did NOT thread multi-vectors through the concurrent search machinery (high churn/risk). Instead reuse
the existing single-vector pipeline once per segment and merge at the orchestrator. Embedding (the cost)
is paid once via embed_batch; in-memory cosine passes are sub-ms so K passes are cheap.

## Files Changed
- crates/retrieval/src/query_segments.rs — NEW: `segment_prompt(prompt, max_segments, max_segment_chars)` (blank-line paragraphs + sentence sub-split for long paras; cap; whitespace-only → [trimmed]); 8 unit tests.
- crates/retrieval/src/lib.rs — `pub mod query_segments;`
- crates/retrieval/src/dual_scope.rs — `merge_scope_results_max(passes, candidate_limit)` (max per (scope,skill), re-sort, truncate; single-pass = identity); 5 unit tests.
- crates/retrieval/src/orchestrator.rs — `retrieve` branches on intent; breaker allow_request hoisted once, each backend embeds + records; Priming loop+merge; 3 new tests incl. KeywordAwareEmbeddingService.

## TDD Evidence
- **Red**: `cargo test -p retrieval --lib -- priming` → `priming_multi_segment_surfaces_skill_matching_only_second_paragraph FAILED` (Priming still ran Task single-embed → only auth-skill, missed migration-skill).
- **Green**: same → 3 priming tests pass (segment 2 "database migration" → migration-skill surfaced).
- **Post-Refactor Green**: `cargo test -p retrieval --lib && cargo test -p mcp-server --lib` → 95 + 50 pass. clippy/fmt clean (fixed `empty_line_after_doc_comments` via `//!`).

## Test Results
- retrieval --lib: 95 passed. mcp-server --lib: 50 passed. build/clippy/fmt clean. Attempts: 1.
- Latency fence CONFIRMED: Priming makes exactly 1 embed_batch call for K segments (K=1 ≈ Task embed_text).

## Notes for Unit 3
- 0.48 floor still applied per-segment-pass inside perform_scope_search; max-merge means a skill surfaces if it clears the floor on ANY segment. Unit 3 adds the Priming-scoped intent-conditional floor + recurrence/freshness ranker.
- β (search_graph subunit evidence) still uses the per-pass segment embedding (consistent — each pass is a full search). candidate_limit truncation applied post-merge.
