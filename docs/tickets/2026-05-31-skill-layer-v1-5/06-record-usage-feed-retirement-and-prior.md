---
ticket_id: T06
title: Record usage; feed retirement + a deterministic ranking prior
kind: expansion # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: ready # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.3: Record usage; feed retirement + a deterministic ranking prior"
feature_home: crates/mcp-server
depends_on: [T01]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-D (usage data exists and feeds retirement + ranking)
files:
  - crates/mcp-server/src/lib.rs
  - crates/infrastructure/src/persistence/mod.rs
  - crates/infrastructure/src/persistence/postgres.rs
  - crates/maintenance/src/runtime.rs
  - crates/retrieval/src/scoring.rs
  - crates/retrieval/src/orchestrator.rs
  - crates/infrastructure/migrations/002_usage_fields.sql
test_command: cargo test -p maintenance --test test_maintenance_e2e
tdd_mode: inherit
---

# Record usage; feed retirement + a deterministic ranking prior

## Serves
- **SC-V1.5-D** — each `compile_context` writes `session_logs` + `skill_usage`; retirement scoring consumes real usage; ranking includes a deterministic recency/frequency prior replacing the hardcoded constant.
- Plan SC-4/SC-2; V2 fence (no learned/adaptive tuning).

## Scope
Persist usage asynchronously on each `compile_context`, wire maintenance retirement to read real usage, and replace `prior: 0.1` with a deterministic fixed-coefficient `usage_prior`. Includes the human-gated `002_usage_fields.sql` migration (typed columns).

- **Owns:** usage writes + retirement input + deterministic prior.
- **Non-goals:** adaptive threshold tuning, outcome/acceptance learning, SkillLens utility scoring (all V2).

## Scope Fence
The prior formula is **fixed and documented, not learned**. No `skill_prior_overrides` write-back. The write site is `McpServerApp::compile_context` in `lib.rs` — NOT `tools/compile_context.rs` (which stays a pure query-compile unit, owned by T04).

## Acceptance Criteria
- [ ] Every `compile_context` writes `session_logs` + `skill_usage` (verified against live PG).
- [ ] Retirement scoring uses real usage; a never-used skill is eligible, a recently-used one is not. (Replace the empty slice `propose(&skills, &[], now)` at `runtime.rs:178` with a real `UsageSampleStore` read; create `UsagePersistencePort` write + `UsageSampleStore` read in `infrastructure` — neither exists yet.)
- [ ] Ranking prior is deterministic and documented; latency stays <500ms warm.
- [ ] **Observability seam:** a simulated usage-write failure emits a `warn` log AND sets `health["usage_write"]="failed"`; it never propagates to the caller or affects latency. Holds regardless of `MCP_USAGE_LOGGING` state. Use a dedicated background writer fed by a bounded `mpsc` (~128; `try_send` → drop+health on full) — not raw `tokio::spawn` (panics vanish).
- [ ] **Usage model = append-log:** one immutable `skill_usage` row per selected skill (`usage_count=1`, per-selection `relevance_score`); reads use `count(*)`/`max(used_at)`. No `UNIQUE`, no UPSERT.
- [ ] **Atomicity:** the one `session_logs` row + N `skill_usage` rows for a single `compile_context` are wrapped in one transaction. `age_days` derives from DB `now() - used_at`, not app clock.
- [ ] **Schema (RATIFIED): typed columns via approved migration.** `002_usage_fields.sql` adds `skill_usage.relevance_score REAL`, `session_logs.{prompt_hash TEXT, latency_ms BIGINT, status TEXT}` — all nullable (non-rewriting `ADD COLUMN`). Keep `session_logs.metadata JSONB` as the overflow tail. No new indexes yet. **HUMAN-GATED.**
- [ ] `truncate_all_tables` (`postgres.rs:98–104`) includes `session_logs` + `skill_usage` (fixes E2E row leakage).
- [ ] Prior = `usage_prior(usage_count, age_days) = min(ln(1+usage_count) · e^(−age_days/30), 0.15)`; pure `#[inline] fn` in `scoring.rs`; populate `RetrievalSnapshot.prior` at load/refresh time from one batched usage query; `f32::total_cmp` for NaN-safe sort; `usage_count=0 ⇒ 0.0`. Replaces `prior:0.1`/`community_boost:0.2` at `lib.rs:429–430` and `dual_scope.rs` constants.
- [ ] A bench baseline for `compile_context` exists so "p95 unchanged" is measurable.

## Shared / Global Notes
- **Graph schema migration — HUMAN GATE:** `002_usage_fields.sql` is staged and requires explicit approval before applying. No `.sqlx` cache exists, so INSERTs need no `cargo sqlx prepare`.
- **CI purity guardrail:** `retrieval` must not gain `sqlx`/`redis`/`qdrant` — the prior is a pure fn; the batched usage query that populates `RetrievalSnapshot.prior` runs in `mcp-server`/`infrastructure`, and the value is handed to `retrieval`.
- Cross-feature-home: write (`mcp-server`), consume (`maintenance`), prior (`retrieval`), persistence ports (`infrastructure`). Declared explicitly — the usage signal spans all four.

## Local Context
**WHY:** `skill_usage`/`session_logs` exist + are indexed but have zero INSERTs; ranking priors are hardcoded; retirement (`runtime.rs:178`) passes an empty `&[]` so SC-4 has no data to score against. The existing `idx_skill_usage_skill_used_at (skill_id, used_at DESC)` already serves `max(used_at)` + windowed counts — no new index.

**Open question to surface:** confirm `compile_context` selected-skill list is available at `lib.rs:99–100` post-tool-return before wiring the writer; if the return shape differs, flag rather than guess.

## Parent Refs
- Plan → Slice 2.3; Architecture artifact.
- Source packet: `## Execution Slices > Slice 2.3`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §2.3 (write site, bounded-mpsc writer, transaction, append-log rationale + forward-guards, prior formula, truncate fix).
- Plan WHY Reassessment R-6 (typed-columns migration); Open Questions 2 & 3 (answered).

## Coupling Notes
One unit because the write, the migration that makes the write typed, the retirement read, and the prior all consume the same `skill_usage` shape — splitting the prior from the write would leave the prior with no data and a retirement pass still scoring an empty slice. Hard-depends on T01 (real retrieval to log). Singleton batch: touches `mcp-server/lib.rs`, `retrieval/orchestrator.rs`, and `maintenance/runtime.rs` — each overlaps another ticket's files, so it cannot safely parallelize.
