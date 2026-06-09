---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/06-record-usage-feed-retirement-and-prior.md
source_packet_ref: "## Execution Slices > Slice 2.3: Record usage; feed retirement + a deterministic ranking prior"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-06-01T18:53:38Z
status: completed
completed: 2026-06-01T19:10:00Z
execution_shape: vertical-slices
batch: "5 (T06 — singleton)"
current_unit: 1
total_units: 1
session_id: work-2026-06-01-215338
review_mode: bulk
execution_model: sonnet # maintainer directive: execution agents are always sonnet
---

## WHY Context

### Problem Narrative
`skill_usage` / `session_logs` tables exist and are indexed but have ZERO INSERTs anywhere.
Ranking priors are hardcoded (`prior: 0.1`, `community_boost: 0.2` at `lib.rs:429`). Retirement
(`runtime.rs:178`) passes an empty `&[]` slice to `propose(...)`, so SC-4 ("retire skills used
< 1/month") has no data to score against. The usage signal the whole design depends on is never
recorded — the loop "closes on the bench, not in the body."

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need the system to
record which skills it actually compiles into context so that (a) unused skills become eligible
for human-gated retirement and (b) frequently/recently used skills get a deterministic ranking
nudge — without any learned/adaptive tuning and without ever slowing the <500ms warm hot path.

### Architectural Context
9-crate workspace, no new crates. Usage signal spans four feature homes:
- **write** → `crates/mcp-server` (`McpServerApp::compile_context` coordination layer, NOT the tool),
- **persistence ports** → `crates/infrastructure` (new `UsagePersistencePort` write + `UsageSampleStore` read),
- **consume** → `crates/maintenance` (retirement reads real usage),
- **prior** → `crates/retrieval` (pure `usage_prior` fn; populated into `RetrievalSnapshot.prior` at load/refresh).
PRIZED INVARIANT: `domain` + `retrieval` purity — NO `sqlx`/`redis`/`qdrant` may leak into them.
The batched usage query that fills `RetrievalSnapshot.prior` runs in `mcp-server`/`infrastructure`;
the value is handed to `retrieval`.

### Success Criteria
- SC-V1.5-D: each `compile_context` writes `session_logs` + `skill_usage`; retirement consumes real
  usage; ranking includes a deterministic recency/frequency prior replacing the hardcoded constant.
  No adaptive/learned tuning. (Serves plan SC-4/SC-2; V2 fence.)

### TDD Contract
- Effective mode: Ralph-driven TDD (tdd_mode: inherit → local default mode: ralph)
- Effective loop: failing tests first → minimal implementation → refactor → post-refactor rerun
- Required evidence: Unit (`cargo test --workspace` / per-crate) Red→Green→Post-Refactor Green +
  E2E (`cargo test -p maintenance --test test_maintenance_e2e`); a `cargo bench` p95-unchanged baseline.
- Exceptions: none.

### Constitution Context
- Constitution v2.0.0; `constitution_waivers: []`.
- Principle 1 (local-first): unaffected — usage writes are local PG. No cloud.
- Principle 2 (<500ms warm): usage write MUST be async/off-the-response-path (bounded mpsc background writer).
- Principle 3 (human gate): retirement still produces human-approved `.retired`; no auto-approval.
- "No stubs": replace the empty `&[]` retirement slice + hardcoded prior with real wiring.
- HUMAN GATE: `002_usage_fields.sql` schema migration — RATIFIED content; maintainer approved
  "stage + apply" (2026-06-01) since the SQL is nullable ADD COLUMN only (non-rewriting).

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Feature homes: write=mcp-server, ports=infrastructure, consume=maintenance, prior=retrieval.
- Shared / global: `UsagePersistencePort` (write) + `UsageSampleStore` (read) live in `infrastructure`
  as the single cross-feature contract (maintenance must NOT import mcp-server types).
- Feature-local: `usage_prior` pure fn sealed in `retrieval`; the graph-swap handle stays in retrieval.
- Deletion test: keep `session_logs.metadata JSONB` as overflow tail; typed columns for known scalars.
- Interfaces as test surfaces: `UsagePersistencePort { write_session_log, write_skill_usage }`,
  `UsageSampleStore { recent_usage(skill_ids) }`, `fn usage_prior(usage_count, age_days) -> f32`.
- Seam: usage persistence — write is async/off-path AND its failure is observable (warn +
  `health["usage_write"]="failed"`), never silently swallowed.
- Drift checks: persistence inside `CompileContextTool`/`retrieve()`; retrieval gaining infra deps;
  prior gaining runtime-tuned/written-back coefficients; >2 server constructors.
- Review guidance: confirm prior is a sealed fixed-coefficient fn (no `skill_prior_overrides` write);
  usage-write failure surfaces; retirement reads real usage; `cargo tree -p retrieval` / `-p domain` pure.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T06 — Record usage; feed retirement + deterministic prior | expansion | SC-V1.5-D (usage data → retirement + ranking prior) | completed | 1 | unit-01-t06-record-usage-prior.md |

## Learnings Brief
Carried forward from Batches 1–4 (relevant to T06's files):
- **[mcp-server/constructors]** Public graph-assembly constructors are exactly two: `McpServerApp::from_environment` (live) + `McpServerApp::with_explicit_graph` (test). Do NOT add a third.
- **[mcp-server/compile_context site]** `McpServerApp::compile_context` (`lib.rs:99–100`) is the coordination layer — the usage-write trigger site. `CompileContextTool` (`tools/compile_context.rs`) stays a pure query-compile unit (owned by T04). Confirm the selected-skill list is available post-tool-return before wiring the writer; if the return shape differs, flag rather than guess.
- **[mcp-server/request shape]** `CompileContextRequest` is constructed in 30+ sites; adding fields forces edits across ~8 test/bench files. Reuse existing fields where possible.
- **[retrieval/snapshot]** In-memory graph type is `RetrievalSnapshot` (renamed from `SeededGraph`), wrapped by T02 as `GraphSnapshot { graph, version }` under `ArcSwap`. The prior must be populated into `RetrievalSnapshot.prior` at graph load/refresh time — coordinate with the swap path T02 added.
- **[retrieval/scope match]** `seeded_skill_matches_scope` (`dual_scope.rs:123`) requires a skill's `source_paths` within scope paths; empty `source_paths` silently drops the skill before scoring. (T09 owns the source-path column; T06 only touches priors/scoring constants.)
- **[retrieval/health]** Read-path markers report `skill_snapshot_sync`; Qdrant probe is `qdrant_write_side`. Don't reintroduce read-path infra markers.
- **[infrastructure/factory]** Shared dependency factory is `DependencyFactory` in `crates/infrastructure/src/health.rs` (not a `dependency_factory.rs`).
- **[env/containers]** Test containers: PG 15432, Qdrant HTTP 16333 / gRPC 16334, Redis 16379, Ollama 11444; live DB `skill_layer_test`.
- **[testing]** Project convention (a2c2271): live-infra tests are `#[ignore]`-gated. Keep deterministic offline tests as the in-CI keystone; live PG assertions run under the ignore gate / e2e.
- **[repo/fmt]** Pre-existing rustfmt drift in `crates/graph-builder/src/graph/rebuild.rs` + some `infrastructure` files is unrelated to V1.5 — leave untouched per scope fence; run `cargo fmt` only on T06's touched files.
