BEGIN;

-- Add advisory scope-generality hint columns to the `skills` table.
--
-- These columns are populated from extraction candidates via the `.pending`
-- frontmatter and the maintenance promotion pass (#179).  Existing rows receive
-- NULL, which is treated as "uncertain" by all consumers.
--
-- Design rationale:
--   Extraction captures a cheap generality signal ("project", "general",
--   "uncertain") while the full transcript is in hand.  The maintenance worker
--   reads this hint when deciding whether a skill qualifies for cross-project
--   promotion (intrinsic path).  This is advisory data — it never changes where
--   extraction writes pending drafts (project-local always when repo_path set).
--   See docs/architecture/2026-06-05-scope-promotion-design.md and todo #178.
--
-- Enum domain: exactly "project", "general", "uncertain" (matches the CHECK on
--   `scope IN ('project', 'global', 'team')` precedent).  NULL ≡ uncertain for
--   all consumers (pre-migration rows).
--
-- Compatibility:
--   ADD COLUMN … NULL (no DEFAULT clause) is a metadata-only operation on
--   Postgres 11+ — no table rewrite.  The IF NOT EXISTS guard makes the
--   migration idempotent (safe to replay).
--
-- Rollback (down):
--   ALTER TABLE skills DROP COLUMN IF EXISTS generality;
--   ALTER TABLE skills DROP COLUMN IF EXISTS generality_rationale;
--
-- WRITE-AHEAD SCHEMA: generality/generality_rationale columns are populated via
--   .pending SKILL.md frontmatter (session-extractor/src/writer.rs) + the LLM
--   verifier, NOT via the skills row today.  No production code SELECTs these
--   columns yet.  Ratified alongside 008 on 2026-06-09 (owner triage #233):
--   dormant, additive, nullable, idempotent (pg_attribute guards).

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'generality'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills
            ADD COLUMN generality TEXT
                CHECK (generality IS NULL OR generality IN ('project', 'general', 'uncertain'));
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'generality_rationale'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills
            ADD COLUMN generality_rationale TEXT;
    END IF;
END
$$;

COMMIT;
