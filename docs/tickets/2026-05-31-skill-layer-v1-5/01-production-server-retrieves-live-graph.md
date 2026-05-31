---
ticket_id: T01
title: Production server retrieves from the live graph
kind: tracer-bullet # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: ready # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.1: Production server retrieves from the live graph"
feature_home: crates/mcp-server
depends_on: []
dependency_type: none # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-A (loop closes in the body)
  - SC-V1.5-F (no production stub paths remain)
files:
  - crates/mcp-server/src/main.rs
  - crates/mcp-server/src/lib.rs
  - crates/infrastructure/src/dependency_factory.rs
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/lib.rs
  - crates/retrieval/src/dual_scope.rs
  - crates/domain/src/lib.rs
  - crates/graph-builder/src/watcher.rs
test_command: cargo test -p mcp-server --features test-utils boot_time_live_retrieval
tdd_mode: inherit
---

# Production server retrieves from the live graph

## Serves
- **SC-V1.5-A** — a clean deployment retrieves a skill that exists in the graph (this ticket delivers the boot-time half: real graph at startup).
- **SC-V1.5-F** — removes the empty-graph production stub path.
- Plan SC-1/SC-8; constitution "No stubs".

This is the **tracer bullet**: it proves the product's core promise on `docker compose up` — the deployed `compile_context` stops returning `no_match` for a seeded skill.

## Scope
Make `main.rs` call the production live constructor instead of hard-coding an empty graph. Read the real `graph_version` from `graph_state`. Rename `SeededGraph` → `RetrievalSnapshot` across `retrieval` + all callers, **here**, before `ArcSwap` (T02) spreads the type. Collapse to exactly two public constructors. Also land the ratified `ScopeRoot` relocation into `domain` (plan decision #5) — folded here because it is the same cross-crate type-move surface as the rename.

- **Owns:** production retrieval wiring + env selection + the `SeededGraph` rename + the `ScopeRoot` relocation.
- **Non-goals:** refresh-on-rebuild (T02), Qdrant live query (T03), ranking changes.

## Scope Fence
Do not change the retrieval algorithm or schema here — only **where the graph comes from at boot**. Do not introduce a third server constructor. Do not add the `ArcSwap` swap path (that is T02).

## Acceptance Criteria
- [ ] `main.rs` no longer constructs an empty graph in live mode — it calls `McpServerApp::from_environment` (the renamed `build_live_server`), not `build_seeded_server`.
- [ ] Live construction compiles into the production binary without `test-utils`. *(Note: it already does — `build_live_server` is not cfg-gated; only `teardown()`/`OutboxVectorStore` are gated. The real work is the call-site in `main.rs`, not un-gating. See WHY Reassessment R-1.)*
- [ ] Exactly **two** public constructors remain: prod `from_environment` + an explicit-graph test constructor. `build_seeded_server` is deleted (no third/duplicate constructor).
- [ ] `SeededGraph` is renamed to **`RetrievalSnapshot`** (NOT `SkillSnapshot` — collides with 3 existing types; WHY Reassessment R-4) across `retrieval` + all callers (~14 refs in `orchestrator.rs:33`, `retrieval/src/lib.rs`, `mcp-server/src/{main.rs,lib.rs}`, `dual_scope.rs`, ≥10 test files).
- [ ] `build_graph_from_pg` reads the real `graph_version` from `graph_state` (do not hardcode `0`/`1`), even when the graph is empty (cold-start reports the true version with `no_match`).
- [ ] Do **not** hard-fail on >5000 skills: `warn!` + truncate instead of returning `Err` (which panics boot). (rabak-rust P3)
- [ ] Any rollback env flag (`MCP_RETRIEVAL_MODE=seeded|live`, default live) carries an inline `// TODO(remove-after-v1.5-green)` + removal criterion ("first green CI on main").
- [ ] **`ScopeRoot` relocated to `domain`** (it has only `domain::ScopeType` + `String`/`PathBuf` fields), with `graph_builder::ScopeRoot` re-exported as a transitional alias; update `maintenance` and `mcp-server` imports. **Defer only if the diff exceeds ~50 lines** — if deferred, record the deferral + reason in this ticket's completion note (do not silently drop it). (plan decision #5)
- [ ] Containerized `compile_context` returns `ok` for a pre-seeded skill — proven by a **narrow seed-and-retrieve smoke** (containerized, not a unit test, and **not** the full `test_live_data_plane_roundtrip`, whose `NoMatch` is fixed by T02+T09 — see WHY Reassessment R-3). The roundtrip stays Phase-3 evidence (T09/T10).

## Shared / Global Notes
- **Shared adapter touched:** `crates/infrastructure/src/dependency_factory.rs` (PG/Qdrant/Redis/Ollama wiring) — cross-feature infra; keep retrieval business logic in `retrieval`/`mcp-server`, not in the factory.
- The `SeededGraph` → `RetrievalSnapshot` rename is a cross-crate type change; it is intentionally done first so T02's `ArcSwap` does not race the rename. **This makes T01 a hard predecessor of every retrieval-touching ticket** — keep it a singleton batch.
- Human-gate: none here (no infra-config or schema change in this ticket).

## Local Context
**WHY:** Deployed `crates/mcp-server/src/main.rs:22` builds `SeededGraph::new(Vec::new(), 0)` → `build_seeded_server(...)`, so deployed `compile_context` always returns `no_match`. `build_live_server` (`lib.rs:239`) is already public/compilable; `main.rs` simply never calls it. `build_graph_from_pg` (`lib.rs:356`, capped at 5000) hardcodes version 0/1 instead of reading `graph_state` → version-keyed cache serves stale after first rebuild.

**Concurrency primitive (forward-looking):** the rename target is `RetrievalSnapshot`; T02 wraps it as `GraphSnapshot { graph, version }` under `ArcSwap`. Do NOT introduce `ArcSwap` here — just rename cleanly.

**Rename completeness check:** run `grep -rn 'SeededGraph' crates/ tests/` before marking done; expect **zero** hits (≥10 test files reference it). `cargo test --workspace` will also fail to compile on any residual ref — but check explicitly so T02's `ArcSwap` doesn't inherit a stray reference.

**Architecture-doc note:** the architecture artifact's interface examples were corrected from `SkillSnapshot` → `RetrievalSnapshot` (per R-4) on 2026-05-31; trust the ticket's `RetrievalSnapshot` naming if any other on-demand doc still says `SkillSnapshot`.

**Open question to surface, not guess:** if the explicit-graph test constructor signature must change for the rename, keep its public shape minimal (graph + version) and flag any caller that breaks. If the `ScopeRoot` relocation diff looks > ~50 lines, stop and confirm deferral rather than ballooning this ticket.

## Parent Refs
- Plan: `docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md` → Slice 1.1
- Architecture: `docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`
- Source packet: `## Execution Slices > Slice 1.1`

## Deeper-Dive Refs
- Plan §Deepening Research Insights §1.1 (production wiring; rename inventory; `graph_version`).
- Plan WHY Reassessment R-1 (stale cfg-gate claim corrected) and R-4 (rename collision).
- Plan Current-State Evidence #1, #2.

## Coupling Notes
One unit because the rename, the `main.rs` call-site swap, the constructor consolidation, and the `graph_version` read are a single atomic change to the production boot path — splitting them would leave a half-renamed type or a constructor count that violates the "exactly two" AC. It is a singleton batch because the cross-crate rename touches files every downstream retrieval ticket also edits.
