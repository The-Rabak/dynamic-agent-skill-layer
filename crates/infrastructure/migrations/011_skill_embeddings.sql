BEGIN;

-- Persisted embedding cache for the skill-graph snapshot builder.
--
-- Design rationale:
--   V1.7 T17 (mcp-server boot readiness honesty): on the qwen3-embedding:4b
--   model the cold-boot re-embed of the full 262-skill corpus takes ~7 minutes.
--   During that time /health claims healthy and find_skill hangs, corrupting
--   T11's efficacy sweeps and causing the e2e gate's find_skill 60s timeout.
--
--   This table persists the four embedding views for each skill:
--     e_summary : name + description + tags (the primary L1 embedding)
--     e_task    : use_when + subunit procedure titles (dense multi-view, T09)
--     e_needs   : requires + invariants (dense multi-view, T09)
--     e_negative: avoid_when (dense multi-view, T09)
--     subunit:{n}: title + content for position-n subunit (0-indexed)
--
--   At boot / reload, build_graph_from_pg loads the full cached set for the
--   active (model_name, dimension) pair and reuses exact-match rows (same
--   content_hash, same model, same dimension).  Only misses (new or changed
--   skills) are sent to the embedding provider.  On an unchanged corpus the
--   four embed batches collapse to ~zero calls, dropping boot time from ~7
--   minutes to seconds.
--
-- Cache invalidation semantics:
--   A row is a HIT if its (content_hash, model_name, dimension) all match the
--   active values.  A cached row with dimension != requested dimension is a hard
--   fail-loud error (DimensionMismatch) — consistent with #235 semantics.
--   A cached row for a different model_name is simply absent from the load
--   (filtered by model_name in the SELECT) and treated as a cache miss.
--
-- Vector encoding:
--   `vector` stores the f32 embedding as little-endian IEEE-754 bytes (BYTEA).
--   Each f32 occupies exactly 4 bytes; for a 2560-dim qwen3 embedding the BYTEA
--   is 10240 bytes.  This format is compact, exact-roundtrip (no float string
--   serialization loss), and efficient for a single `COPY`/bulk-load at boot.
--
-- Table layout:
--   PRIMARY KEY (skill_id, view_kind, model_name) — one row per (skill, view,
--   model) triple.  A rebuild with a NEW model overwrites nothing from the old
--   model; the old rows remain until explicitly pruned (no auto-cleanup here —
--   operators may switch back to the previous model without re-embedding).
--
--   `dimension` is stored alongside model_name so a mismatched re-deploy
--   (same model name, different server that returns a different dimension) is
--   detected at load time, not silently.
--
-- Blank-view invariant:
--   Blank view texts (empty e_task / e_needs / e_negative due to absent
--   frontmatter) are NEVER cached here; the load path returns an empty Vec<f32>
--   for any (skill, view) not present in this table, preserving the
--   embed_dense_view_skipping_blank semantics from T09.
--
-- Indexes:
--   idx_skill_embeddings_model: supports the primary access pattern —
--   `SELECT … WHERE model_name = $1` — used by load_for_model to bulk-load
--   the full embedding set for the active model at boot/reload.
--
-- Compatibility:
--   CREATE TABLE IF NOT EXISTS is idempotent; safe to replay.
--   The table is purely additive — no existing column is altered.
--
-- Human gate: APPROVED 2026-06-11 (V1.7 T17 embedding cache persistence,
--   migration 011, follows 010_skill_edges.sql).
--
-- Rollback (down):
--   DROP TABLE IF EXISTS skill_embeddings;

CREATE TABLE IF NOT EXISTS skill_embeddings (
    skill_id     TEXT        NOT NULL,
    view_kind    TEXT        NOT NULL,
    model_name   TEXT        NOT NULL,
    dimension    INTEGER     NOT NULL CHECK (dimension > 0),
    content_hash TEXT        NOT NULL,
    vector       BYTEA       NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (skill_id, view_kind, model_name)
);

CREATE INDEX IF NOT EXISTS idx_skill_embeddings_model
    ON skill_embeddings (model_name);

COMMIT;
