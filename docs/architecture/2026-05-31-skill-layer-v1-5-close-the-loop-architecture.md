---
date: 2026-05-31
topic: skill-layer-v1-5-close-the-loop
status: complete
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
v1_1_architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
reviewers:
  - architecture-strategist
  - uncle-bob
handoff:
  deepen_plan: true
  work: true
  review: true
---

# Skill Layer V1.5 — Close the Loop: Architecture Improvement

> References used: `.github/skills/workflows-architecture/references/architecture-improvement-prompt.md` ("# Architecture Improvement Artifact Contract") and `.github/skills/workflows-architecture/references/vertical-slice-architecture.md` ("# Vertical Slice Architecture Contract"). Design heuristics from the `agent-native-architecture` skill. Mandatory reviewers `architecture-strategist` and `uncle-bob` were run; their findings are folded in below (conflicts resolved explicitly).

## Purpose Linkage

- **Problem Narrative:** V1.1 is 82% — "the loop closes on the bench, not yet in the body." The deployed `mcp-server` serves an empty in-memory graph, the self-growing loop has no automatic trigger, real extraction is unproven, usage is never recorded, and the live suite is RED and blocked by a Qdrant port mismatch. V1.5 is wiring + reliability + proof, not new product surface.
- **User Story:** As a developer who runs `docker compose up`, I need the deployed server to retrieve the skills my graph contains (including just-approved ones), sessions to auto-extract skills and re-inject context across compaction, extraction to be reliable, and the live data plane proven green — without restarting a process or hand-triggering a tool.
- **Success Criteria protected by this artifact:** SC-V1.5-A (loop closes in the body), -B (self-growing trigger), -C (reliable real extraction), -D (usage → retirement + deterministic prior), -E (green live suite), -F (no production stubs).
- **Architectural Context:** existing 9-crate workspace; no new crates. Dependency direction is strongly enforced today (`domain` pure; `retrieval` has zero infra deps; online/offline split clean). The gap is entirely *connection*, not *structure*.

## Problem Framing and Constraints

- **Local-first, <500ms warm, no V2 encroachment.** The hot read path (`compile_context`) must stay sub-500ms and must not gain blocking I/O. No SkillLens/SkillOpt/team-scope/LLM-compiler/self-healing/learning loops.
- **Frozen contracts stay frozen.** Reuse the 8-event catalog (esp. `graph.rebuilt`); no new Redis events; no schema migration unless approved (the `skill_usage`/`session_logs` tables already exist and are indexed).
- **Prized invariant:** `domain` purity and `retrieval`-as-pure-transformation. New seams must not leak `sqlx`/`redis`/`qdrant` into `domain` or `retrieval`.

## Feature Homes and Ownership

- **Feature home: `crates/mcp-server/`** — online bootstrap + coordination.
  - Owns: production server construction & live/seeded selection (`build_server_from_environment`), the `graph.rebuilt` refresh subscriber, the coordination-layer trigger of the async usage write, suppression isolation.
  - Crosses into: `infrastructure` (adapters/ports), `retrieval` (graph swap method).
  - Notes: this is where the empty-graph defect lives and where the loop is physically closed.

- **Feature home: `crates/retrieval/`** — pure scoring/fusion + the in-memory graph handle.
  > Naming correction (2026-05-31, per plan WHY Reassessment R-4): the rename target is **`RetrievalSnapshot`**, not `SkillSnapshot` (the latter collides with 3 existing types). All `SkillSnapshot` references in this artifact have been corrected to `RetrievalSnapshot`.
  - Owns: the renamed `RetrievalSnapshot` (was `SeededGraph`), the `swap_graph(&self, RetrievalSnapshot)` method, the deterministic recency/frequency prior **function** (pure math), the relevance-threshold/fixtures tuning.
  - Crosses into: nothing infra. May take `arc-swap` (pure concurrency primitive) — acceptable, no I/O.
  - Notes: subscription/reload logic must NOT live here; retrieval only exposes a swap method.

- **Feature home: `crates/session-extractor/`** — extraction reliability.
  - Owns: worker-pool concurrency fix, terminal-event ownership consolidation, retry-policy unification, provider default.

- **Feature home: `crates/infrastructure/`** — the new ports + adapters.
  - Owns: `UsagePersistencePort` (write `session_logs`/`skill_usage`), `UsageSampleStore` (read usage for retirement and for prior-at-load), the Qdrant adapter's REST/gRPC port derivation.

- **Feature home: `crates/maintenance/`** — retirement consumes real usage via the shared `UsageSampleStore` port (must NOT import `mcp-server` types).

- **Feature home: `config/claude-code/` + `docs/reference/`** — the Claude Code lifecycle hook contract (config + docs only).

- **Feature home: `scripts/` + `tests/e2e/` + `tests/fixtures/`** — green live suite, port/env alignment, retrieval fixtures.

## Shared / Global Decisions

| Candidate | Decision | Why |
|---|---|---|
| `UsagePersistencePort` (write) | **Shared in `infrastructure`** | Cross-feature contract; both the write caller (mcp-server) and the read consumer (maintenance, build-time prior) depend on the same boundary. Keeps `compile_context` from owning persistence. |
| `UsageSampleStore` (read) | **Shared in `infrastructure`** | Retirement (maintenance) and prior-at-load (mcp-server) both read usage. A single port prevents two divergent queries and stops `maintenance` importing `mcp-server`. |
| Deterministic prior `fn usage_prior(usage_count, age_days) -> f32` | **Feature-local in `retrieval`** | It is pure feature math, not infrastructure. Sealing it as a named pure fn with fixed constants is the V2 fence. |
| Graph swap handle (`ArcSwap`/`RwLock<GraphSnapshot>`) | **Feature-local in `retrieval`** (handle), driven from `mcp-server` (subscriber) | Concurrency primitive is pure; the I/O (Redis subscribe, PG reload) is infrastructure-adjacent and belongs to the mcp-server coordination layer. |
| `graph_builder::ScopeRoot` used by `mcp-server`/`maintenance` | **Move to `domain` or `infrastructure`** (deepening candidate, see Deletion Test) | A data type leaking out of an offline binary crate couples online + maintenance to the offline build pipeline; hurts the V2 offline/team-scope divergence. |
| Lifecycle hook config | **Global context artifact** (`config/claude-code` + docs) | Not business logic; it's the harness integration contract. |

## Capability Map and Parity Gaps

- **Online read parity:** today the deployed agent path returns `no_match` (empty graph). After 1.1/1.2 the deployed server reads the real, refreshing graph — parity with the test harness.
- **Self-growth parity:** today extraction is a manually-invokable tool with no hook. After 2.1 the `SessionEnd` hook gives the agent the same self-growth the architecture always promised; `SessionStart`/compaction give context-injection parity across the whole session, not just first prompt.
- **Observability parity gap to close:** `healthy_markers()` claims `qdrant: "ok"` for a store the online path never queries — a false capability assertion. Fix it (see Interfaces/Drift).

## Deepening Candidates

- **Online graph source (highest leverage).** Pick and document Option A vs B (see Design-It-Twice). This is the spine of SC-A.
- **Production constructor consolidation.** Collapse `build_seeded_server` / `build_live_server` / proposed `build_server_from_environment` into a non-duplicative set (see Deletion Test + Interfaces).
- **Usage concept ownership.** One owner (the `UsagePersistencePort`/`UsageSampleStore` in `infrastructure`), triggered at the `McpServerApp` coordination site, read at graph-load time and by retirement.
- **Worker-pool concurrency + terminal-event contract.** The serialized `recv` is the actual cause of "0/32 completed"; the terminal-event ownership is split across `execute_job` and the dispatch layer.
- **`ScopeRoot` relocation** to break `maintenance`/`mcp-server` → `graph-builder` coupling before V2 diverges the offline builder.

## Deletion Test

| Candidate | Keep / Delete / Delay | Why |
|---|---|---|
| `build_seeded_server` (empty-graph prod path) | **Delete** | It only exists to satisfy `main.rs` with an empty graph — the exact defect. Replace with one env-driven production constructor. Keep an explicit-graph constructor for tests. |
| Third constructor `build_server_from_environment` *alongside* `build_live_server` | **Delete the duplication** | Two functions with identical "read env, wire live infra" semantics drift. Keep exactly two public constructors: `from_environment` (prod) and an explicit-graph one (tests). |
| Live Qdrant online query (Option B) | **Delay to V2** | Adds an inward `retrieval → infrastructure/qdrant` dependency for scale we don't need at local-first sizes (≤5000). The V2 team-scope migration is the right time. |
| New Redis event for refresh | **Delete (don't add)** | `graph.rebuilt` already exists in the frozen catalog; subscribe to it. |
| `_retry_policy` param + the second hardcoded `RetryPolicy` in `extract_with_retry` | **Delete one, unify** | Two retry sites for one operation silently override operator config. One policy, one source. |
| Claude extraction provider as default → `:8080/extract` | **Delete as default** | Points at a non-existent service; every default deployment fails. Make Ollama the default; keep Claude as a documented transport stub. |
| `ScopeRoot` in `graph-builder` public surface | **Delay/relocate** | Move to `domain`/`infrastructure`. Not strictly required for SC-A..F, but cheap and prevents calcifying coupling; defer only if it expands the slice. |
| Env rollback flags (`MCP_RETRIEVAL_MODE`, `MCP_GRAPH_REFRESH`, `MCP_USAGE_LOGGING`) | **Keep with expiry** | Permissible as deployment-day safety valves ONLY with a removal criterion ("after first green CI on main"). Otherwise they ship two permanent half-paths and re-introduce the empty-graph bug behind a flag. |

## Interfaces as Test Surfaces

- **Interface: production server construction.** `McpServerApp::from_environment(config) -> Result<McpServerApp>` (prod) and an explicit-graph constructor (tests).
  - Callers/tests rely on: a server that, in live mode, retrieves real skills.
  - Must not leak: `LiveServerComponents` internals (pg/qdrant/coordinator) into production callers — return only the app (+ the swap handle for the subscriber).
  - Evidence: containerized `compile_context` returns `ok` for a seeded skill.

- **Interface: graph refresh.** `RetrievalOrchestrator::swap_graph(&self, snapshot: RetrievalSnapshot)` + an atomic `GraphSnapshot { graph, version }` read.
  - Callers rely on: after a `graph.rebuilt`, the next `compile_context` sees new skills; `graph_version` advances atomically with the graph (no skew).
  - Must not leak: Redis/PG into `retrieval`. The subscriber lives in `mcp-server`.
  - Evidence: approve-while-running test → retrievable, no restart; concurrency test during swap (no torn read).

- **Interface: usage persistence.** `UsagePersistencePort { write_session_log, write_skill_usage }` and `UsageSampleStore { recent_usage(skill_ids) }` in `infrastructure`.
  - Callers rely on: each `compile_context` (via `McpServerApp` coordination) records usage; retirement reads real counts; prior-at-load reads counts.
  - Must not leak: persistence into `CompileContextTool` (stays a pure query-compile unit) or into `RetrievalOrchestrator::retrieve()` (stays stateless).
  - Evidence: usage rows present in live PG; never-used skill retire-eligible; recently-used not.

- **Interface: extraction terminal event.** Every accepted job emits exactly one of `extraction.completed | extraction.failed`.
  - Callers/tests rely on: terminal event count == accepted job count; no silent stalls.
  - Evidence: 32-job burst → 32 terminal events + deterministic `.pending`.

- **Interface: deterministic prior.** `fn usage_prior(usage_count: u32, age_days: u32) -> f32` (fixed coefficients, doc-marked "V1.5 fixed formula, no adaptive tuning").
  - Must not leak: into a learned/written-back path. The fence is the absence of any `skill_prior_overrides` write.

## Seams, Adapters, and Contracts

- **Seam: online graph source.** Adapter: the `RetrievalSnapshot` loaded by `build_graph_from_pg`. Contract: `SkillRetriever` trait stays the only thing `mcp-server` depends on — a future `QdrantScopedRetriever` (V2) satisfies the same trait with no interface change.
- **Seam: graph freshness.** Adapter: `mcp-server` Redis `graph.rebuilt` subscriber → `swap_graph`. Contract: graph + version swap atomically; cache (version-keyed) invalidates the one-cycle skew naturally.
- **Seam: usage persistence.** Adapter: `PostgresUsageWriter`/`PostgresUsageSampleStore` impl the ports. Contract: write is async/off-the-response-path AND its failure is observable (warn log + `health["usage_write"]="failed"`), never silently swallowed.
- **Seam: extraction provider.** Adapter: `OllamaExtractor` (default, owns prompt) / `ClaudeExtractor` (documented transport stub). Contract: provider switch never changes the candidate output shape (existing `prompt_contract`).
- **Seam: extraction dispatch.** Contract: the dispatch layer (worker pool path AND no-pool path) owns terminal-event emission for all three outcomes; `execute_job` returns an outcome rather than publishing `completed` itself (removes split ownership).
- **Seam: Qdrant transport.** Adapter: the Qdrant adapter derives REST (`:6333`/host `:16333`) and gRPC (`:6334`/host `:16334`) from one configured base so preflight and operational client never disagree. Contract: `run-e2e-tests.sh` exports the base the adapter expects.

## Design-It-Twice Options (online graph source — the one high-leverage boundary)

- **Option A — Refresh-on-rebuild in-memory snapshot (CHOSEN for V1.5).** Keep `RetrievalSnapshot` in `retrieval`, make it production-constructible from PG, atomically swap on `graph.rebuilt`. Qdrant is the durable write-side store (CQRS read model); the online read side is the PG-loaded snapshot. Pros: zero new inward dependency for `retrieval`; cheapest path to SC-A; clean V2 seam (same `SkillRetriever` trait). Cons: 5000-skill in-memory cap; Qdrant unused at read time (must be *documented*, and `healthy_markers` must stop claiming Qdrant health).
- **Option B — Live Qdrant query per request.** `retrieval` issues concurrent dual-scope Qdrant queries. Pros: scales past the cap; uses the vector store as the paper intends; closer to V2 team-scope. Cons: new `retrieval → infrastructure/qdrant` dependency (breaks current purity), heavier, higher risk for a <500ms budget, real V2 surface.
- **Chosen for now:** **A.** It closes the loop with the least structural risk and leaves B as a clean, additive V2 move behind the unchanged `SkillRetriever` trait. The non-negotiable outcome (approved skill retrievable without restart) is satisfied by A.

## Context Tiers

- **Global context:** the 5 constitution principles (esp. "No stubs", human-gate, <500ms, local-first); the frozen 8-event catalog; `domain` purity + `retrieval` purity invariants; the V2 fence list.
- **On-demand context:** this artifact; `vertical-slice-architecture.md`; the V1.1 architecture; the 2026-05-31 assessment; the firsthand test-failure evidence in the plan.
- **Ticket-local context (per slice):** the named feature home, the exact files, the one interface/seam it touches, its scope fence, its evidence command, and the single WHY line. (Each slice in the plan already carries these; deepen-plan should keep them this small.)

## Recommendations for `/deepen-plan`

1. **Reframe Slice 3.3 as a "Phase 3 integration gate," not a standalone vertical slice.** It depends on seven upstream slices and has no independent demo — model it as the final verification/CI-gating pass so it isn't resourced as ordinary work that sits blocked.
2. **Rewrite Slice 2.3's home statement:** owner = `UsagePersistencePort`/`UsageSampleStore` in `infrastructure`; **write is triggered at the `McpServerApp` coordination layer after `CompileContextTool` returns** (NOT inside the tool, NOT inside `RetrievalOrchestrator`). Add an acceptance criterion: "usage-write failure is observable (warn + health key)."
3. **Make Slice 1.1 a rename+delete:** rename `SeededGraph` → `RetrievalSnapshot`; delete `build_seeded_server`; land exactly two constructors. Do the rename here (before `ArcSwap` spreads the type).
4. **Split Slice 1.2 responsibilities explicitly:** Redis subscription + PG reload live in `mcp-server`; `retrieval` exposes only `swap_graph`. Store graph+version in one struct under the lock.
5. **Slice 1.3 must include the health-map fix** (drop/relabel `qdrant` in `healthy_markers()`) and a one-paragraph CQRS read-model doc — as acceptance criteria, not prose.
6. **Slice 2.2 must name two retry sites** (the dead `_retry_policy` param AND the hardcoded `RetryPolicy` in `extract_with_retry`) and the serialized-`recv` mutex as the primary throughput bug; plus flip the default provider to Ollama.
7. **Add the `ScopeRoot` relocation as an explicit (small) deepening item** with a "delay only if it grows the slice" note.
8. **Attach an expiry to every env rollback flag** ("remove after first green CI on main"; `TODO(remove-after-v1.5-green)`), and rename any genuinely-permanent knob to reflect optionality (e.g., `DISABLE_USAGE_LOGGING`).

## Recommendations for `/workflows-work`

- Preserve `domain` and `retrieval` purity: no `sqlx`/`redis`/`qdrant` imports in them. The refresh subscriber and usage writer live in `mcp-server`/`infrastructure`.
- Keep `CompileContextTool` a pure query-compile unit; coordinate side effects (usage write) one layer up.
- The graph+version swap must be atomic (single struct under the lock / `ArcSwap`).
- Async usage writes must log on failure and reflect in the health map — no silent swallow.
- Honor human-gate: hook/compose/script edits and any schema change pause for approval; reuse the frozen event catalog.
- Every terminal extraction outcome publishes exactly one event from the dispatch layer.

## Recommendations for `/workflows-review`

- Verify SC-A with a containerized approve-while-running test (no restart), not a unit test.
- Verify the live suite is GREEN (`run-e2e-tests.sh --include-dream`), not merely compiling; confirm any still-`#[ignore]` dream contract has a logged reason (no silent truncation).
- Confirm no third/duplicate server constructor survived; confirm `SeededGraph` rename is complete.
- Confirm `healthy_markers()` no longer claims Qdrant health on the in-memory path.
- Confirm the deterministic prior is a sealed fixed-coefficient fn (V2 fence intact — no learned tuning, no `skill_prior_overrides`).
- Confirm usage-write failures surface; confirm retirement scoring reads real usage.
- Confirm `retrieval`/`domain` purity unchanged (`cargo tree -p domain --depth 1`, `-p retrieval`).

## Drift Checks

- **Feature-home drift:** usage-persistence logic appearing inside `CompileContextTool` or `RetrievalOrchestrator::retrieve()` (should be the `infrastructure` port, triggered at `McpServerApp`).
- **Boundary drift:** `retrieval` or `domain` gaining `sqlx`/`redis`/`qdrant` deps; the Redis subscriber implemented inside `RetrievalOrchestrator`.
- **Duplication drift:** more than two server constructors; two retry policies; a new Redis event instead of reusing `graph.rebuilt`.
- **Naming drift:** `SeededGraph` still named so once it serves production retrieval.
- **Honesty drift:** `healthy_markers()` claiming `qdrant: "ok"` on the in-memory path; an env flag without an expiry criterion; a still-ignored dream test without a logged reason.
- **Coupling drift:** `maintenance` importing `mcp-server`; `maintenance`/`mcp-server` deepening their reliance on `graph-builder` internal modules.
- **Scope drift (V2 fence):** the deterministic prior gaining runtime-tuned/written-back coefficients; live Qdrant query sneaking in; any team/remote scope surface.

## Open Questions

- **Option A vs B ratification:** artifact recommends A; confirm during `/deepen-plan` that ≤5000-skill cap + write-only-Qdrant (documented) is acceptable for V1.5, or pull B forward if the slice budget allows.
- **`ScopeRoot` relocation in V1.5 vs V2:** include now (cheap, prevents calcification) or defer? Decide in deepen-plan based on actual diff size.
- **Claude provider:** demote-and-document (recommended) vs implement a minimal headless bridge in V1.5 — depends on whether a real bridge is small.
