BEGIN;

-- Add optional multi-view structured fields to the `skills` table.
--
-- Design rationale:
--   V1.7 Design Decision 5: structured extraction must grow before graph quality
--   can grow.  These columns hold the source data for T04 (multi-view dense/BM25
--   matching) and T05 (typed graph-edge proposals).  They are populated via the
--   SKILL.md YAML frontmatter during each graph rebuild (session-extractor writes
--   them; graph-builder reads them; rebuild.rs INSERTs them).
--
--   Field semantics:
--     use_when    : short list of task triggers (situations where this skill applies)
--     avoid_when  : short list of negative triggers (when NOT to apply)
--     artifacts   : file types, protocols, config names, repo objects
--     tools       : commands, libraries, frameworks, services, models, APIs
--     invariants  : verifier-critical constraints that must hold
--     requires    : prerequisites the skill assumes are already in place
--     produces    : outcome or artifact produced by following the skill
--
--   All columns are nullable TEXT[].  NULL is stored when the frontmatter field is
--   absent or empty; the application layer treats NULL and an empty array
--   identically.  No CHECK constraint is needed — these are free-form lists.
--
-- Compatibility:
--   ADD COLUMN … NULL (no DEFAULT clause) is a metadata-only operation on
--   Postgres 11+ — no table rewrite.  The IF NOT EXISTS guard (via pg_attribute)
--   makes the migration idempotent (safe to replay).
--
-- Rollback (down):
--   ALTER TABLE skills DROP COLUMN IF EXISTS use_when;
--   ALTER TABLE skills DROP COLUMN IF EXISTS avoid_when;
--   ALTER TABLE skills DROP COLUMN IF EXISTS artifacts;
--   ALTER TABLE skills DROP COLUMN IF EXISTS tools;
--   ALTER TABLE skills DROP COLUMN IF EXISTS invariants;
--   ALTER TABLE skills DROP COLUMN IF EXISTS requires;
--   ALTER TABLE skills DROP COLUMN IF EXISTS produces;
--
-- WRITE-AHEAD SCHEMA: these columns are populated via .pending SKILL.md frontmatter
--   (session-extractor/src/writer.rs) + the graph rebuild INSERT
--   (infrastructure/src/persistence/rebuild.rs).  No production code SELECTs these
--   columns yet.  T04 (multi-view embeddings) and T05 (typed edges) will add the
--   readers.  This follows the same write-ahead pattern as 007_skill_generality.sql
--   (ratified alongside that migration on 2026-06-09).

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'use_when'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN use_when TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'avoid_when'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN avoid_when TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'artifacts'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN artifacts TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'tools'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN tools TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'invariants'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN invariants TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'requires'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN requires TEXT[];
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'skills'::regclass
          AND attname = 'produces'
          AND NOT attisdropped
    ) THEN
        ALTER TABLE skills ADD COLUMN produces TEXT[];
    END IF;
END
$$;

COMMIT;
