---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/01-production-server-retrieves-live-graph.md
source_packet_ref: "## Execution Slices > Slice 1.1: Production server retrieves from the live graph"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-31T09:17:12Z
status: in_progress
execution_shape: vertical-slices
batch: "2-3 (continuing same /workflows:work run; batch 1 = T01 completed)"
current_unit: 2
total_units: 4
batch_1_completed: 2026-05-31T09:17:12Z
session_id: work-2026-05-31-121712
review_mode: bulk
---

## WHY Context

### Problem Narrative
V1.1 scored 82% — "the loop closes on the bench, not yet in the body." The two halves of
the system are not connected in the shipped binaries: `docker compose up` gives a context
layer that returns `no_match` forever because the deployed MCP server serves an empty
in-memory graph. These are missing *wiring and proof*, not missing features. T01 is the
tracer bullet that proves the core promise on `docker compose up`.

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need the deployed
online server to actually retrieve the skills my graph contains, so that every session starts
with real compiled context — without restarting a process or hand-triggering a tool.

### Architectural Context
9-crate workspace, no new crates. T01 touches `crates/mcp-server` (production bootstrap +
live wiring), `crates/retrieval` (graph source type rename), `crates/domain` (ScopeRoot home),
`crates/infrastructure` (dependency factory), and `crates/graph-builder` (ScopeRoot alias).
Online graph source = Option A (in-memory snapshot, production-constructible). Qdrant is the
durable write-side CQRS store. Read path is the in-memory snapshot.

### Success Criteria
- SC-V1.5-A (Loop closes in the body): boot-time half — a clean deployment retrieves a skill
  that exists in the graph. (Refresh-without-restart half is T02.)
- SC-V1.5-F (No production stub paths remain): the empty-graph production path is removed.

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: failing test first → minimal implementation → refactor → post-refactor rerun
- Required evidence: unit (`cargo test --workspace` Red→Green→post-refactor Green) + e2e
  (narrow containerized seed-and-retrieve smoke; NOT the full `test_live_data_plane_roundtrip`,
  whose NoMatch is fixed by T02+T09 — see WHY Reassessment R-3)
- Exceptions: none

### Constitution Context
Constitution v1.0.0 (active, no waivers). Relevant: Principle 1 Local-first (all wiring stays
Docker-Compose/local — no cloud on T01's path); "No stubs" (Agent Execution Rules) directly
motivates SC-F (the empty-graph production path is a current no-stubs violation). T01 introduces
no infra-config or schema change → **no human-gate checkpoint** in this ticket.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Feature homes: primary `crates/mcp-server` (production wiring); rename surface spans
  `crates/retrieval` (`SeededGraph`→`RetrievalSnapshot`) + all callers; `crates/domain` (new
  `ScopeRoot` home); `crates/graph-builder` (transitional `ScopeRoot` alias re-export).
- Shared / global decisions: `crates/infrastructure/src/dependency_factory.rs` is a shared
  adapter (PG/Qdrant/Redis/Ollama wiring) — keep retrieval business logic OUT of the factory.
- Naming precedence: trust `RetrievalSnapshot` (NOT `SkillSnapshot` — collides with 3 existing
  types, WHY Reassessment R-4). Architecture doc interface examples were corrected to
  `RetrievalSnapshot` on 2026-05-31; the ticket naming wins over any stale on-demand doc.
- Forward-looking concurrency: rename target is `RetrievalSnapshot`; T02 wraps it as
  `GraphSnapshot { graph, version }` under `ArcSwap`. Do NOT introduce `ArcSwap` here.
- Constructor contract: exactly two public constructors after T01 — prod `from_environment`
  (renamed `build_live_server`) + an explicit-graph test constructor. Delete `build_seeded_server`.
- Deletion test: `build_seeded_server` is deleted, not deprecated; cold-start reports the true
  `graph_version` from `graph_state` even with an empty graph.
- Review guidance (for later /workflows:review): verify no third constructor introduced, no
  `ArcSwap`/refresh path leaked in from T02, rename completeness (`grep -rn 'SeededGraph'` = 0),
  and that `dependency_factory.rs` did not absorb retrieval logic.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T01 Production server retrieves from the live graph | tracer-bullet | SC-V1.5-A (boot-time half), SC-V1.5-F | completed | 4 | unit-01-production-server-retrieves-live-graph.md |
| 2 | T03 Resolve Qdrant online role (Option A + CQRS + honest health) | hardening | SC-V1.5-F | completed (batch 2, ‖ T04) | 1 | unit-02-t03-resolve-qdrant-online-role.md |
| 3 | T04 Wire full Claude Code session lifecycle | expansion | SC-V1.5-B | completed (batch 2, ‖ T03) | 1 | unit-03-t04-wire-claude-code-lifecycle.md |
| 4 | T02 Live graph refreshes on graph.rebuilt | expansion | SC-V1.5-A (online-refresh half) | in_progress (batch 3, after T03) | -- | -- |

## Learnings Brief
- **[retrieval/boot]** `seeded_skill_matches_scope` (`retrieval/src/dual_scope.rs:123`) requires a skill's `source_paths` to fall within the scope's configured paths; an empty `source_paths` silently drops the skill before scoring. Any PG-loaded graph MUST populate `source_paths` with a scope-matching value. **This is the real reason deployed retrieval returned `no_match` even with a populated graph** — not a threshold issue. T09 ranking tuning depends on this load fix.
- **[retrieval/schema]** The persisted `skills` table has no source-path column → only scope-root provenance is reconstructable at boot. If T09 needs finer per-file path matching, that gap must be addressed there.
- **[mcp-server/constructors]** Public graph-assembly constructors are now exactly two: `McpServerApp::from_environment` (live) + `McpServerApp::with_explicit_graph` (test). `build_live_server` is a private helper; `build_seeded_server` deleted. `McpServerApp::new`/`new_with_admin` inject an already-built retriever (different abstraction level) and are not part of the "two constructors" contract.
- **[infrastructure/factory]** The shared dependency factory is `DependencyFactory` in `crates/infrastructure/src/health.rs` (not a `dependency_factory.rs`). Keep retrieval/live-wiring logic in `mcp-server`, not the factory.
- **[type naming]** Rename target is `RetrievalSnapshot` (NOT `SkillSnapshot` — collides with 3 types, R-4). T02 will wrap it as `GraphSnapshot { graph, version }` under `ArcSwap`; T01 left it `ArcSwap`-free.
- **[env/containers]** Test containers: PG 15432, Qdrant HTTP 16333 / gRPC 16334, Redis 16379, Ollama 11444; live DB `skill_layer_test`. The connectivity/preflight check must use the **HTTP** Qdrant port (16333), not gRPC (16334) — the gRPC-port preflight was a known defect (T08 owns the `run-e2e-tests.sh` env fix).
- **[repo/fmt]** Pre-existing `rustfmt` drift in `crates/graph-builder/src/graph/rebuild.rs` + several `infrastructure` files (qdrant.rs, extraction/*, health.rs lines 82/218/264/274) — unrelated to V1.5 edits; agents left them untouched per scope fence. Surface to a cleanup pass or T10's CI-purity gate.
- **[retrieval/health — T03]** Read-path health markers now report `skill_snapshot_sync` and DROP `qdrant`/`postgres`/`redis` (none are read-path deps under Option A). The Qdrant infra probe is labelled `qdrant_write_side`. The named test `read_path_health_markers_do_not_claim_qdrant_or_postgres_as_live_dependencies` is the deletion guard. **DS-003 contract is defined in `docs/architecture/adr-0001-online-graph-source-v1-5.md`** — T10 must rewrite the dream-state test to it (Qdrant down ⇒ compile_context still Ok/NoMatch; only `qdrant_write_side` degrades).
- **[retrieval — naming]** The in-memory cosine search fn is `search_qdrant` in `crates/retrieval/src/qdrant_search.rs` (name kept for call-site stability; doc comment clarifies it is in-memory only, NOT an online Qdrant query).
- **[mcp-server/compile_context — T04]** `CompileContextRequest` now has optional `trigger: Option<String>` (`#[serde(default)]`); `trigger=="compact"` bypasses suppression for that single call (read-only, does not mutate suppression state — T08 owns global semantics). The MCP `inputSchema` in `protocol.rs` carries it. **Gotcha:** `CompileContextRequest` is constructed in 30+ sites; adding fields forces edits across 8 test/bench files — consider `Default`/`#[non_exhaustive]` later.
- **[hooks/config — T04]** `config/claude-code/hooks.example.json` now wires SessionStart + PreCompact(trigger=compact) + UserPromptSubmit + SessionEnd. Hook facts: SessionStart/SessionEnd cannot block; UserPromptSubmit/PreCompact can; inject via `hookSpecificOutput.additionalContext` ≤~10,000 chars; SessionEnd doesn't fire on crash (T07 reconcile is the backstop). Contract documented in `capability-catalog.md` + `degraded-state.md`.
- **[testing — T04]** Live e2e `extract_session_live_ref_payload` lives in the `test_live_data_plane_roundtrip` binary; the ticket's filter-only command needs `--test test_live_data_plane_roundtrip` to actually run it. T10/CI must use the explicit `--test`.
