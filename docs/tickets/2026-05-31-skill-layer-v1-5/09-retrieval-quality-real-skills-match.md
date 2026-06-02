---
ticket_id: T09
title: Retrieval quality — real/seeded skills actually match
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: ready # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.2: Retrieval quality — real/seeded skills actually match"
feature_home: crates/retrieval
depends_on: [T01, T02, T03]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-E (live suite is GREEN)
  - SC-V1.5-A (loop closes in the body)
files:
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/dual_scope.rs
  - crates/compiler/src/template.rs
  - crates/infrastructure/migrations/004_skill_source_paths.sql
  - crates/infrastructure/src/persistence/rebuild.rs
  - crates/mcp-server/src/lib.rs
  - tests/fixtures/retrieval_corpus.json
  - tests/e2e/test_live_data_plane_roundtrip.rs
test_command: cargo test -p mcp-server --features test-utils -- --ignored test_live_data_plane_roundtrip
tdd_mode: inherit
human_gate: schema-migration # 004_skill_source_paths.sql — stage and confirm before apply/commit
---

# Retrieval quality — real/seeded skills actually match

## Serves
- **SC-V1.5-E** and **SC-V1.5-A** — the `NoMatch != Ok` roundtrip failure is gone: a semantically-relevant prompt returns `ok` with the seeded skill, while cold-start stays honestly `no_match`.
- Plan SC-2.

## Scope
Root-cause the `NoMatch`: confirm seeded skills load into the live retriever (T01/T02), then tune `relevance_threshold`/`candidate_limit`/embedding alignment so relevant prompts clear the bar, provide a realistic shared fixture corpus the live tests reuse, and surface a tiny deterministic "why this matched" explanation in compiled context.

**Persist real per-skill source paths (folded in 2026-05-31 per maintainer direction).** The `skills` table has **no source-path column**, so T01 had to populate `RetrievalSnapshot` `source_paths` with the *configured scope root* as a coarse stand-in to stop scope-matching from dropping every live skill. T09 replaces that workaround with the real thing: add a `skills.source_paths` column, populate it on write from each skill's actual SKILL.md file path(s), and read it at boot so `seeded_skill_matches_scope` matches on true provenance instead of a scope-root approximation.

- **Owns:** threshold tuning + retrieval fixture corpus + diagnosis of the snapshot-vs-match cause + the `skills.source_paths` column (migration, write-population, boot read replacing T01's scope-root stand-in) + minimal deterministic match-reason output.
- **Non-goals:** new scoring features, learned weights (V2).

## Scope Fence
Deterministic threshold tuning only; no model swap. Keep cold-start `no_match` honest (not degraded, not forced filler). The `source_paths` column is a **non-rewriting `ADD COLUMN`** with a safe default — do not alter existing columns or backfill-rewrite the table. Do not change the SKILL.md format or the extraction path that discovers source files. The "why this matched" section must be deterministic and compact — no LLM-generated explanation, no counterfactual engine, no V2 explainability surface.

## Acceptance Criteria
- [ ] Roundtrip and concurrency-burst tests get `ok` for relevant prompts (e.g. seed "Rust file I/O … async tokio" + prompt "how to read files in rust with tokio async" → `ok` containing the seeded skill).
- [ ] Cold-start still returns `no_match` (not degraded, not forced filler).
- [ ] Suggested starting values to validate (not hardcode blindly): `relevance_threshold: 0.20`, `candidate_limit: 50`, `max_results: 3`, `rescue_threshold: 0.15`, `scope_timeout_ms: 400` (tight with two scopes — validate).
- [ ] Provide a shared fixture corpus where ≥3 project-scope skills clear 0.20 on the roundtrip prompt; keep a named negative fixture proving cold-start `no_match`.
- [ ] **Fixture corpus doubles as demo corpus:** `tests/fixtures/retrieval_corpus.json` contains realistic agent-work skills across Rust, Docker Compose, git, testing, and security. The corpus is suitable for `scripts/run-demo.sh` / T10b reuse, not only artificial threshold tuning. Include at least one negative prompt fixture proving honest `no_match`.
- [ ] `mmr_select` uses `total_cmp` (NaN-safe).
- [ ] **`skills.source_paths` column added** via human-gated `004_skill_source_paths.sql` — `ALTER TABLE skills ADD COLUMN source_paths TEXT[] NOT NULL DEFAULT '{}'` (nullable-safe, non-rewriting). Staged and approved before apply.
- [ ] **Write path populates it:** the skill INSERT in `crates/infrastructure/src/persistence/rebuild.rs` writes `source_paths`, and `LiveGraphSkillRecord` carries the real source path(s) supplied by the graph-builder rebuild that reads the SKILL.md files. (Confirm the upstream constructor of `LiveGraphSkillRecord` during execution and thread the file path through.)
- [ ] **Boot read uses the column, replacing T01's workaround:** `build_graph_from_pg` (`crates/mcp-server/src/lib.rs`) reads `source_paths` from the column into `RetrievalSnapshot` instead of substituting the configured scope root. If a row has empty `source_paths` (pre-migration data), fall back to the scope-root behavior so old graphs still match — document the fallback.
- [ ] A named test proves a skill loaded from PG carries its real `source_paths` and that scope-matching uses it (not the scope-root stand-in).
- [ ] **Why-this-matched lite:** compiled `ok` context includes a compact deterministic section (e.g. `### Why These Skills`) listing each selected skill's scope, graph_version, score bucket, top matched terms/subunit title when available, and source path provenance. This is a trust affordance, not full explainability; keep it short enough to preserve context budget.
- [ ] E2E asserts the seeded roundtrip skill's compiled context contains both the skill name and a deterministic match reason (scope + source path or score bucket), so users can see why context was injected.

## Shared / Global Notes
- Shared fixture corpus under `tests/fixtures/` is reused by the live tests and T10b's activation demo — T10/T10b consume it but do not redefine it; this ticket owns its creation.
- Shares `tests/e2e/test_live_data_plane_roundtrip.rs` with T08 and T10 → kept in separate sequential batches.
- **T03 dependency (Batch 2) edits the same `orchestrator.rs`/`dual_scope.rs`:** T03 relabels the health markers there before this ticket tunes thresholds. Read T03's diff before editing those files so threshold tuning doesn't conflict with or duplicate T03's health-marker changes.
- **Shared persistence file `crates/infrastructure/src/persistence/rebuild.rs`:** also touched by T01 (boot load) and T02 (graph-builder publish reads nothing here, but the snapshot read does). T09 runs after both, so its `LiveGraphSkillRecord`/INSERT change builds on their committed state — read their diffs first.
- **Human-gate: YES — schema migration.** `004_skill_source_paths.sql` is a graph-schema change; stage it and obtain explicit human approval before applying or committing (constitution: graph schema migration). It is ordered after T06's `002` and T07's `003`.

## Local Context
**WHY:** The roundtrip test fails `NoMatch != Ok` at `test_live_data_plane_roundtrip.rs:489` even after the test's post-seed server rebuild — the seeded skill is not retrieved. After Phase 1 makes seeded skills actually load, the remaining cause is threshold/candidate/embedding tuning plus a fixture corpus whose skills semantically overlap the prompts (current ad-hoc generic seeds don't).

**Adoption WHY (2026-06-02 assessment):** A correct retrieved skill still feels like magic unless the user can see why it appeared. A deterministic match-reason block makes the first successful context injection legible without pulling V2 counterfactual explainability into V1.5.

**Open question to surface:** confirm whether the residual `NoMatch` is a load issue (Phase 1) or a threshold issue (this ticket) by first asserting the seeded skill is present in the snapshot; if still absent after T01/T02, escalate to those tickets rather than over-tuning the threshold.

**T01 handoff (the `source_paths` stand-in):** T01 discovered the real loop-blocker — `seeded_skill_matches_scope` (`crates/retrieval/src/dual_scope.rs:123`) drops any skill whose `source_paths` is empty against a scope that has configured paths, so PG-loaded skills never reached scoring. T01 shipped a coarse fix: populate `source_paths` with the configured scope root at boot. This ticket replaces that stand-in with a real `skills.source_paths` column so matching uses true per-file provenance. Until the migration lands, the scope-root fallback must remain so existing graphs keep matching. Verify the T01 behavior in `build_graph_from_pg` before changing it (see session `work-2026-05-31-121712`, unit-01).

## Parent Refs
- Plan → Slice 3.2; Architecture artifact.
- Source packet: `## Execution Slices > Slice 3.2`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §3.2 (threshold values; shared fixture corpus; `total_cmp`).
- Plan §Firsthand test evidence (the `NoMatch` panic location).

## Coupling Notes
One unit because the threshold tuning and the fixture corpus must be co-designed — tuning a threshold against a corpus whose skills don't overlap the prompts proves nothing. Hard-depends on T01/T02 (seeded skills must actually be loaded/visible before tuning means anything). Singleton batch: shares `orchestrator.rs`/`dual_scope.rs` with T02/T03/T06 and `tests/e2e/*` with T08/T10.
