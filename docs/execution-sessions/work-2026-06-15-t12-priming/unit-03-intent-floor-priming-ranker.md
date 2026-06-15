---
unit: "Intent-conditional floor + priming ranker (recurrence + freshness via created_at)"
unit_number: 3
unit_kind: expansion
serves: "raise set-coverage@3 toward ≥0.17 via Priming-scoped floor + recurrence-baseline rerank + bounded freshness slot"
status: completed
attempt_count: 2
domains: [rust, retrieval, infrastructure, mcp-server]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
session_id: work-2026-06-15-t12-priming
---

## What Was Implemented
1. **Priming-scoped config** (RetrievalConfig + from_env, fail-loud): `priming_relevance_threshold`=0.30,
   `priming_max_results`=5, `priming_recurrence_weight`=0.10, `priming_freshness_slots`=1,
   `priming_freshness_window_days`=30. Each env-overridable (`RETRIEVAL_PRIMING_*`).
2. **Freshness data on snapshot (no SeededSkill churn)**: `RetrievalSnapshot.skill_age_days: HashMap<String,u32>`
   (default empty; builder `with_skill_age_days`, mirrors `community_centroids`). Infra read query
   `rebuild.rs::list_skills` now SELECTs `skills.created_at` (added to tuple + GROUP BY + destructure +
   `PersistedGraphSkillRecord.created_at: DateTime<Utc>`). `build_graph_from_pg` computes
   `age_days = (Utc::now()-created_at).num_days().max(0)` per skill and attaches the map.
3. **Priming ranker** `crates/retrieval/src/priming_rank.rs::select_priming_prime` (pure): reranks by
   `score + recurrence_weight*prior`, reserves `freshness_slots` for the highest-reranked FRESH
   (age≤window) candidate that would fall outside top-N (injection over existing pool, no new source).
   Degenerate (weight 0, slots 0) = plain top-N. Orchestrator Priming arm uses a `priming_search_config`
   (floor 0.30, N=5) for the passes + fusion, then `select_priming_prime` for selection. **Task path
   untouched** (same self.config, same `.take(max_results)`).

## Files Changed
- crates/retrieval/src/orchestrator.rs (5 config fields + from_env; snapshot field + builder; intent-conditional fusion_limit + selection; imports; 6 new tests)
- crates/retrieval/src/priming_rank.rs — NEW (select_priming_prime + PrimingRankConfig + 10 unit tests)
- crates/retrieval/src/lib.rs (pub mod priming_rank)
- crates/infrastructure/src/persistence/rebuild.rs (created_at: SELECT/tuple/GROUP BY/destructure/record field)
- crates/mcp-server/src/lib.rs (Utc import; skill_age_days map; with_skill_age_days attach)
- crates/mcp-server/Cargo.toml (chrono dev→prod dependency, since build_graph_from_pg now calls Utc::now())

## TDD Evidence
- **Red**: `E0560 struct has no field 'priming_relevance_threshold'`; floor-divergence test red until Priming arm used priming_search_config.
- **Green**: `cargo test -p retrieval --lib` → 110 pass (incl. 5 config, 1 snapshot round-trip, 1 floor-divergence, 10 priming_rank).
- **Post-Refactor Green**: retrieval 110 + mcp-server 50 pass; clippy 0 warnings; fmt clean. Infra builds (sqlx created_at query compiles). Attempts: 2 (fix: `seeded.prior` not `seeded.skill.prior`).

## Key tests
- `priming_lower_floor_surfaces_skill_below_task_threshold`: eq3≈0.45 skill DROPPED by Task (floor 0.48) but SURFACED by Priming (floor 0.30) — proves intent-scoped floor divergence.
- `priming_intent_produces_identical_outcome_to_task_intent`: retargeted to empty snapshot (documented) — empty==empty guard; real divergence is intended + tested separately.
- priming_rank: degenerate=top-N; recurrence promotion; freshness-slot injection; unknown-age never fresh; bound respected.

## Flag for Unit 4 (measured honesty)
Corpus was rebuilt together (~2026-06-10) → wall-clock `created_at` is near-uniform → the freshness slot
may measure as INERT on THIS corpus (everything equally "fresh" or nothing distinguishes). That is an
honest measured outcome to record (freshness likely DROPPED on this corpus, kept for evolving corpora),
NOT a bug. The live levers this session are: Priming floor (0.30) + query-side multi-view + recurrence.
