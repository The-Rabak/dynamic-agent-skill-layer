BEGIN;

-- Add per-skill SKILL.md provenance so the retrieval read path can gate scope
-- matching on true source-file paths rather than the configured scope root.
--
-- Design rationale:
--   The `retrieval` crate's `seeded_skill_matches_scope` checks whether every
--   skill source path is under the queried scope root. Without this column the
--   boot adapter (build_graph_from_pg) substitutes the configured scope root as
--   the source path for every skill — a safe stand-in but not real provenance.
--   This column lets the write path record the actual SKILL.md file location and
--   the read path use it, eliminating the stand-in for post-migration rows.
--
-- Compatibility:
--   `ADD COLUMN … NOT NULL DEFAULT '{}'` is non-rewriting on Postgres 11+.
--   Pre-migration rows get an empty array; the boot adapter falls back to the
--   scope-root stand-in for rows where source_paths IS empty — documented in
--   build_graph_from_pg in crates/mcp-server/src/lib.rs.
--
-- Human gate: APPROVED 2026-06-03 (graph schema migration, migration 005,
--   follows 004_session_logs_status_check.sql).
--
-- Rollback (down):
--   ALTER TABLE skills DROP COLUMN IF EXISTS source_paths;

ALTER TABLE skills
    ADD COLUMN IF NOT EXISTS source_paths TEXT[] NOT NULL DEFAULT '{}';

COMMIT;
