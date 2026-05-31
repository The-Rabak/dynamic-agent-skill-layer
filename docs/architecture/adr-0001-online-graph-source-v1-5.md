---
adr: "0001"
date: 2026-05-31
status: accepted
deciders:
  - architecture-strategist
  - solo-developer
supersedes: []
---

# ADR-0001: Online Graph Source — Option A (In-Memory CQRS Read Model)

## Context

The V1.5 close-the-loop milestone requires the `mcp-server` to retrieve real skills
from the knowledge graph at query time. At the design-it-twice stage (architecture
artifact `docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`,
section "Design-It-Twice Options") two options were evaluated:

- **Option A:** keep Qdrant as a durable write-side store; serve the online read path
  from a refreshable in-memory `RetrievalSnapshot` (the CQRS read model), reloaded
  from Postgres on every `graph.rebuilt` event.
- **Option B:** query Qdrant live on every request (ANN query per scope).

The system has a **local-first, <500ms warm read-path** constraint and a practical
skill-set cap well under 5000 skills for the V1.5 deployment target.

## Decision

**Option A is adopted for V1.5.**

The online read path operates entirely on the in-memory `RetrievalSnapshot`. Qdrant is
the durable vector write-side store only: the graph-builder drains the outbox into
Qdrant as a persistent audit-and-rebuild record; it is NOT consulted at read time.

The `RetrievalSnapshot` is the CQRS read model. It is rebuilt from Postgres on
`graph.rebuilt` and atomically swapped under an `ArcSwap`/`RwLock` in the
`mcp-server` coordination layer without restarting the process.

The in-memory cosine path (function `search_qdrant` in `crates/retrieval/src/qdrant_search.rs`)
performs cosine similarity against the snapshot embeddings. The function name reflects
the historical source of those embeddings (graph-builder → Qdrant), NOT a live network
call. No network I/O occurs on the read path.

## Consequences

### Accepted

- **5000-skill in-memory cap.** At ≥5000 skills, heap pressure and cosine-scan latency
  may breach the 500ms budget. This is acceptable for local-first V1.5 workloads;
  it is the explicit V2 migration trigger (see below).
- **Qdrant unused at read time.** The read path has no network dependency on Qdrant.
  Qdrant downtime does NOT degrade `compile_context`; only the `qdrant_write_side`
  health marker changes.
- **Snapshot freshness lag.** The read model is at most one rebuild cycle behind the
  write model. This is acceptable: the lag is bounded by the polling interval and the
  `graph.rebuilt` event path.

### Positive

- Zero new inward dependency for `crates/retrieval`: it remains a pure-transformation
  crate with no `sqlx`/`redis`/`qdrant` imports.
- The unchanged `SkillRetriever` trait provides a clean V2 seam: Option B can replace
  the read source without touching any caller.
- The read path stays within the 500ms budget because there is no remote I/O per request.

### V2 Migration Trigger for Option B

Migrate to Option B (live Qdrant ANN query per request) when either condition is met:

1. **Scale trigger:** the deployed skill graph exceeds 5000 skills and latency measurements
   show the cosine-scan path approaching the 500ms budget.
2. **Scope trigger:** team-scope or multi-tenant retrieval is required (V2 architecture),
   where per-tenant Qdrant collections are the natural isolation boundary.

The migration is additive: a `QdrantScopedRetriever` implementing the unchanged
`SkillRetriever` trait replaces the `RetrievalOrchestrator` as the wired-in retriever.
No caller changes are required.

## Health Marker Semantics

Under Option A, honest health reporting requires:

- **Read-path markers** (`healthy_markers()` / `degraded_marker()` in
  `crates/retrieval/src/orchestrator.rs`) must NOT include `qdrant` or `postgres`.
  These are write-side stores; claiming them as read-path dependencies would imply
  Qdrant/Postgres down ⇒ retrieval degraded, which is false.
- Read-path markers report: `ollama` (embedding provider), `skill_snapshot_sync`
  (CQRS read model age), `filesystem_index` (lexical graph state).
- **Infrastructure health checker** (`crates/infrastructure/src/health.rs`)
  probes Qdrant under the key `qdrant_write_side` (not `qdrant`), making its
  write-side-only role explicit in the `/health` endpoint output.

## DS-003 Contract (to be honored by T10)

Under Option A, the following CQRS resilience contract must hold:

> Stopping Qdrant (write-side store) must NOT cause `compile_context` to return an
> error or degrade retrieval quality. `compile_context` returns `Ok` (with results)
> or `NoMatch`; only the `qdrant_write_side` health marker shows degraded.

This is the positive CQRS-resilience proof that DS-003 must be rewritten to assert
(replacing any `#[ignore]` or Qdrant-liveness-precondition tests). An eager per-request
Qdrant liveness check on the read path is explicitly rejected: it would re-couple
the read path to Qdrant availability and breach the <500ms budget.

T10 owns the DS-003 test rewrite. T03 (this unit) defines the contract and the
production code that satisfies it; T10 adds the live integration test that verifies it.

## References

- Architecture artifact: `docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md`
- CQRS read-model note: `docs/reference/online-retrieval-cqrs.md`
- Deletion guard test: `crates/retrieval/src/orchestrator.rs::tests::read_path_health_markers_do_not_claim_qdrant_or_postgres_as_live_dependencies`
