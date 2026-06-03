---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/09-retrieval-quality-real-skills-match.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
source_packet_ref: "## Execution Slices > Slice 3.2: Retrieval quality — real/seeded skills actually match"
brainstorm_ref: n/a
started: 2026-06-03T07:38:51Z
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-03-073851
---

## WHY Context

### Problem Narrative
The deployed loop returns `NoMatch` for semantically-relevant prompts: `test_live_data_plane_roundtrip` fails `NoMatch != Ok` at line 489 even after a post-seed rebuild. T01 found the real loop-blocker — `seeded_skill_matches_scope` (`dual_scope.rs:139`) drops any skill whose `source_paths` is empty against a scope with configured paths — and shipped a coarse stand-in: populate `source_paths` with the configured scope root at boot. T09 replaces that stand-in with real per-skill provenance, validates threshold/corpus alignment so relevant prompts clear the bar, keeps cold-start honestly `no_match`, and adds a deterministic "why this matched" trust affordance.

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need the deployed server to actually retrieve the skills my graph contains, so every session starts with real compiled context — and I can *see why* a skill was injected.

### Architectural Context
Option A (CHOSEN): in-memory `RetrievalSnapshot` loaded from PG at boot, swapped on `graph.rebuilt`. `retrieval`/`domain` stay pure (no sqlx/redis/qdrant). The deterministic prior is sealed feature-math in `retrieval`. Qdrant is the write-side CQRS store, unused at online read time. V2 fence: no learned weights, no counterfactual explainability.

### Success Criteria
- SC-V1.5-A: a skill that exists in the graph (incl. approved-while-running) is retrievable; `compile_context` returns `ok` <500ms warm.
- SC-V1.5-E: the live suite is GREEN (`run-e2e-tests.sh --include-dream`); T09 delivers the retrieval-quality half of that.
- Plan SC-2.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan `tdd.mode: ralph`; `tdd_mode: inherit` on ticket falls back to plan/local default).
- Effective loop: failing tests first → minimal implementation → refactor → post-refactor rerun.
- Required evidence: unit (`cargo test --workspace` / `-p retrieval` / `-p infrastructure`) + e2e (`cargo test -p mcp-server --features test-utils -- --ignored test_live_data_plane_roundtrip`, run under `MCP_USAGE_LOGGING=off` per human decision 2026-06-03).
- Exceptions: none to the TDD contract. E2E captured under `MCP_USAGE_LOGGING=off` because the default-on teardown deadlock is T10-owned (see Blockers in index), NOT a TDD weakening.

### Constitution Context
- Constitution v2.1.0 (active). Relevant: Local-first (retrieval stays local, no cloud added), Human gate for mutations (schema migration → approved 2026-06-03 as `005_skill_source_paths.sql`), No stubs (real provenance, not a stand-in), Quality Standards (clippy strict + rustfmt pass), Filesystem-observable (unchanged).
- Human gate: schema migration **APPROVED** 2026-06-03 — `005_skill_source_paths.sql` `ALTER TABLE skills ADD COLUMN source_paths TEXT[] NOT NULL DEFAULT '{}'` (non-rewriting ADD COLUMN, safe default). Renumbered from ticket's `004` because `004_session_logs_status_check.sql` already exists.
- Waivers: none.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md.
- Feature homes: `crates/retrieval` (owner — scoring/threshold/prior, pure), with provenance write crossing into `crates/infrastructure` (persistence) and boot read in `crates/mcp-server` (coordination). Migration in `crates/infrastructure/migrations`.
- Shared / global: `retrieval`/`domain` purity invariant (no sqlx/redis/qdrant). The fixture corpus under `tests/fixtures/` is shared (T10/T10b consume, T09 owns creation).
- Context tiers: ticket-local packet primary; plan §3.2 + arch Interfaces/Seams as deeper dive.
- Deletion test: keep the scope-root fallback ONLY as the empty-`source_paths` (pre-migration) fallback — document it; do not keep two permanent provenance paths.
- Interfaces as test surfaces: `SkillRetriever` trait unchanged; `seeded_skill_matches_scope` matches on true provenance; compiled `ok` context carries a deterministic match-reason section.
- Seams: online graph source seam (`RetrievalSnapshot` from `build_graph_from_pg`); provenance write seam (rebuild INSERT + `LiveGraphSkillRecord`).
- Review guidance: confirm V2 fence intact (no learned tuning), `retrieval`/`domain` purity unchanged, cold-start still honest `no_match`, fallback documented, migration human-gated.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T09 — Retrieval quality: real/seeded skills actually match | hardening | SC-V1.5-A (loop closes in body) + SC-V1.5-E (retrieval-quality half of green suite) | completed | 2 | unit-01-t09-retrieval-quality-real-skills-match.md |

## Learnings Brief
Carried forward from prior V1.5 sessions (filtered to T09's domain):
- [retrieval] `seeded_skill_matches_scope` (`dual_scope.rs:123-149`) gates by `scope_type` + `scope_id`, then: empty `scope.paths` → match; empty `seeded.source_paths` (with non-empty scope paths) → DROP; else `source_path.starts_with(scope_path)` for all. PG-loaded skills MUST carry scope-matching `source_paths` or they silently drop before scoring.
- [persistence] `skills` table had no source-path column → T01 used the configured scope root (`SKILL_GLOBAL_PATHS`/canonicalized cwd) as a coarse stand-in in `build_graph_from_pg` (`lib.rs:726-794`). T09 adds the real column.
- [persistence] write path: `rebuild.rs:399` `INSERT INTO skills (...)` has no `source_paths`; `LiveGraphSkillRecord` (`rebuild.rs:34`) has no `source_paths` field; two ctor sites — `graph-builder/src/graph/rebuild.rs:219` and `admin/src/tools.rs:262`. `list_skills` SELECT (`rebuild.rs:~99`) must add the column → `PersistedGraphSkillRecord`.
- [retrieval] `RetrievalConfig` defaults (`orchestrator.rs:143-159`) ALREADY equal the ticket's suggested values; threshold work is validate-against-corpus, not blind change.
- [retrieval] `mmr_select`/sorts ALREADY use `total_cmp` (`dual_scope.rs:297`, `fusion.rs:152-155`); verify all sort sites, don't assume churn needed.
- [compiler] `template.rs` emits `### Highlights` / `### Rescue cues`; "Why These Skills" is a NEW deterministic section.
- [testing] Live containers: PG 15432, Qdrant HTTP 16333 / gRPC 16334, Redis 16379, Ollama 11444; live DB is `skill_layer_test`. Roundtrip e2e green only under `MCP_USAGE_LOGGING=off` (T10 owns the default-on teardown deadlock).
- [testing] Roundtrip seed/prompt (`test_live_data_plane_roundtrip.rs`): seed "Rust file I/O … async tokio", prompt "how to read files in rust with tokio async" → expects `Ok` containing seeded skill (assertions ~486-503).
