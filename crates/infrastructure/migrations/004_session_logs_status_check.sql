BEGIN;

-- Add CHECK constraint on session_logs.status to match skill_usage.context_status.
--
-- Retention-policy note (owner decision, 2026-06-02):
--   session_logs and skill_usage are append-only event logs. Their full history
--   feeds the retirement-scoring window (RetirementConfig.scoring_window_days,
--   default 90 days). No pruning is planned at V1.5 — all rows are retained so
--   the scorer always has access to the complete usage signal. This decision is
--   intentional; revisit at scale when the tables grow large enough to warrant
--   a time-partitioned retention policy.
--
-- schema_logs.status valid values mirror skill_usage.context_status:
--   'ok'                   — context compiled successfully
--   'no_match'             — no skills matched the prompt
--   'degraded'             — skills returned but below confidence threshold
--   'duplicate_suppressed' — session suppressed as a duplicate
--
-- RATIFIED 2026-06-02: owner decision (todo #129 P3 data-hygiene batch).
-- Human gate: the retirement automation never auto-applies SQL; a human must
-- run this migration before it takes effect.
--
-- T09 (2026-06-03): wired into MIGRATIONS array + converted to idempotent DO block.
-- #130 (2026-06-03): NOT VALID + NULL-allowed CHECK (status IS NULL OR ...) + schema-scoped probe
--   (table_schema = current_schema() in the existence query).
-- Human gate: APPROVED 2026-06-03.
--
-- Rollback (down):
--   ALTER TABLE session_logs DROP CONSTRAINT IF EXISTS chk_session_logs_status;

-- Use DO block for idempotent constraint addition (Postgres does not support
-- ADD CONSTRAINT IF NOT EXISTS before PG 17; this works on all supported versions).
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.table_constraints
        WHERE table_name = 'session_logs'
          AND constraint_name = 'chk_session_logs_status'
          AND table_schema = current_schema()
    ) THEN
        ALTER TABLE session_logs
            ADD CONSTRAINT chk_session_logs_status
            CHECK (status IS NULL OR status IN ('ok', 'no_match', 'degraded', 'duplicate_suppressed'))
            NOT VALID;
    END IF;
END
$$;

COMMIT;
