BEGIN;

-- Track which embedding model produced the current graph's Qdrant vectors.
--
-- Design rationale:
--   V1.7 introduces multiple embedding arms (nomic-embed-text / qwen3-embedding:4b).
--   Each arm writes to its own model-keyed Qdrant collection (skills__<slug>).
--   This table records which model+dimension+collection is in use after each
--   graph rebuild so:
--     - The retrieval measurement reports (retrieval_quality_live.py) can
--       attribute MRR/nDCG numbers to the exact model that produced the vectors.
--     - Graph-builder and mcp-server can assert model consistency at boot.
--     - Human operators can audit which model is active without reading code.
--
-- One row per active rebuild is sufficient; the table is replaced (not appended)
-- on each rebuild via UPSERT on the single sentinel key 'active'.
--
-- Fields:
--   key           : sentinel string, always 'active'.  Primary key.
--   model_name    : Ollama model identifier (e.g. 'nomic-embed-text', 'qwen3-embedding:4b').
--   dimension     : vector size returned by the live model (from discover_dimension probe).
--   collection    : Qdrant collection name (model_keyed_collection_name(model_name)).
--   model_digest  : optional Ollama model digest / SHA256 for exact-version traceability.
--                   NULL when Ollama does not expose the digest via the embed response.
--   updated_at    : wall-clock timestamp of the last rebuild that wrote this row.
--
-- Compatibility:
--   CREATE TABLE IF NOT EXISTS is idempotent; safe to replay.
--   The table is tiny (one active row) and never grows unboundedly.
--
-- Human gate: APPROVED 2026-06-09 (V1.7 embedding model metadata, migration 008,
--   follows 007_skill_generality.sql).
--
-- Rollback (down):
--   DROP TABLE IF EXISTS embedding_model_metadata;

CREATE TABLE IF NOT EXISTS embedding_model_metadata (
    key           TEXT        PRIMARY KEY CHECK (key = 'active'),
    model_name    TEXT        NOT NULL,
    dimension     INTEGER     NOT NULL CHECK (dimension > 0),
    collection    TEXT        NOT NULL,
    model_digest  TEXT,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMIT;
