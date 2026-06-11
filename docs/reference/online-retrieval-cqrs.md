# Online Retrieval — CQRS Read Model

> Decision ratified in ADR-0001. This note explains the read/write split to developers
> who are reading health output, writing tests, or planning V2 work.

> **V1.7 note (2026-06-11):** the CQRS split below describes the **default** backend
> (`RETRIEVAL_BACKEND=snapshot_dense`), which is unchanged: Qdrant is write-side only and
> the in-memory `RetrievalSnapshot` is the sole query target. V1.7 added two **experimental,
> opt-in** backends (T04): `snapshot_hybrid` still queries only the in-memory snapshot (dense +
> in-memory BM25 — CQRS intact), but **`qdrant_hybrid` reads Qdrant at query time** via the
> Query API, which **deliberately breaks the Option-A read/write split** for that arm only. It is
> NOT the default, measured **no uplift** in T04, and is gated behind the env flag. If `qdrant_hybrid`
> is ever promoted to default, this document's "Qdrant is NOT read at query time" guarantee and the
> §Health-Reporting markers below must change, and the change must be ratified in a new ADR (Qdrant
> hot-path promotion is approval-sensitive — it alters the DS-003 resilience contract). Collections
> are model-keyed (`skills__<slug>`, charset-guarded, `Result`-returning derivation), not `"skills"`.

## The Split

The skill layer uses a **CQRS pattern** (Command Query Responsibility Segregation) to
separate how skill data is written from how it is read at query time.

| Side        | Store             | Role                                                     |
|-------------|-------------------|----------------------------------------------------------|
| Write side  | Qdrant + Postgres | Durable storage; graph-builder drains outbox into Qdrant |
| Read side   | `RetrievalSnapshot` | In-memory snapshot; the live query target             |

The `RetrievalSnapshot` is the **read model**. It is rebuilt from Postgres on every
`graph.rebuilt` Redis event and atomically swapped into the `mcp-server` without
restarting. Queries (`compile_context`) hit the in-memory snapshot exclusively.

## What Qdrant Is and Is Not

Qdrant is the **durable write-side vector store**. The graph-builder writes embedding
vectors there as an authoritative, persistent record that survives restarts. During a
graph rebuild, those vectors are reloaded from Postgres (which holds the canonical skill
graph) back into a fresh `RetrievalSnapshot`. Qdrant is NOT read at query time.

The function `search_qdrant` in `crates/retrieval/src/qdrant_search.rs` performs
**pure in-memory cosine similarity** against the snapshot's embedding vectors. It makes
no network call. Its name reflects the historical origin of the embeddings (built by
the graph-builder and stored in Qdrant), not a live interaction with the Qdrant service.

## Health Reporting

Because Qdrant and Postgres are write-side concerns, the read-path health markers
(`healthy_markers()` / `degraded_marker()` in `crates/retrieval/src/orchestrator.rs`)
do NOT include them. Including them would falsely imply that Qdrant or Postgres
downtime degrades retrieval — it does not under Option A (ADR-0001).

Read-path health markers report:

| Key                   | Meaning                                                      |
|-----------------------|--------------------------------------------------------------|
| `ollama`              | Embedding provider reachable; needed to vectorize the prompt |
| `skill_snapshot_sync` | CQRS read model status; reflects last successful rebuild     |
| `filesystem_index`    | Lexical graph derived from the snapshot                      |

The **infrastructure health checker** (`/health` endpoint) probes Qdrant under the key
`qdrant_write_side` to make its write-side-only role unambiguous in monitoring output.

## DS-003 Resilience Contract

Stopping Qdrant must NOT degrade `compile_context`:

- `compile_context` returns `Ok` (with results) or `NoMatch` regardless of Qdrant
  availability.
- Only the `qdrant_write_side` marker in `/health` changes to `degraded` / unreachable.
- The next graph rebuild will fail to drain the outbox (write path only), and
  `qdrant_write_side` will remain degraded until Qdrant is restored.

This is a positive CQRS-resilience guarantee, not a soft degradation. DS-003 tests
must assert it without `#[ignore]` or Qdrant-liveness preconditions. An eager
per-request Qdrant liveness check on the read path is explicitly rejected: it would
re-couple the read path to Qdrant availability and breach the <500ms latency budget.

## V2 Migration Path (Option B)

When the skill graph exceeds ~5000 skills or team-scope retrieval is required, migrate
to live Qdrant ANN queries (Option B). The migration is additive:

1. Implement `QdrantScopedRetriever` satisfying the unchanged `SkillRetriever` trait.
2. Wire it in place of `RetrievalOrchestrator` in `McpServerApp::from_environment`.
3. Update health markers to reflect the new live dependency.
4. No callers of `compile_context` change.

See ADR-0001 for the full migration trigger criteria.
