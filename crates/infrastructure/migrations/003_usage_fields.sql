BEGIN;

-- Add typed scalar columns to session_logs and skill_usage (T06, V1.5).
--
-- These are non-rewriting ADD COLUMN operations (all nullable) so the
-- migration is safe to apply against a live database with existing rows.
--
-- session_logs: typed prompt_hash, latency_ms, and status columns so the usage
-- write does not need to serialise these into the JSONB metadata blob.
-- skill_usage:  typed relevance_score column per the append-log model (one
-- immutable row per selected skill, no UPSERT, no UNIQUE on skill_id).
--
-- RATIFIED 2026-06-01: maintainer approved to stage and apply to skill_layer_test.
-- Human gate: the retirement automation never auto-applies SQL; a human must run
-- this migration before the usage-write path is live.
--
-- Rollback (down):
--   ALTER TABLE session_logs DROP COLUMN IF EXISTS prompt_hash;
--   ALTER TABLE session_logs DROP COLUMN IF EXISTS latency_ms;
--   ALTER TABLE session_logs DROP COLUMN IF EXISTS status;
--   ALTER TABLE skill_usage  DROP COLUMN IF EXISTS relevance_score;

ALTER TABLE session_logs
    ADD COLUMN IF NOT EXISTS prompt_hash TEXT,
    ADD COLUMN IF NOT EXISTS latency_ms  BIGINT,
    ADD COLUMN IF NOT EXISTS status      TEXT;

ALTER TABLE skill_usage
    ADD COLUMN IF NOT EXISTS relevance_score REAL;

COMMIT;
