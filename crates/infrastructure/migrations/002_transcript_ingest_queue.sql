BEGIN;

-- Durable transcript-ingest queue (todo 103, Option 4).
--
-- Folds the planned T07 `processed_transcripts` marker into a single
-- content-bearing queue: the row IS both the work item and the dedup marker.
-- Host `command` hooks (SessionEnd / PreCompact) read the transcript where the
-- path is natively valid and POST its CONTENT to the localhost ingest endpoint,
-- which inserts a row here. The maintenance worker drains `pending` rows by
-- feeding `content` to the extractor via `transcript_inline` (so the absolute
-- transcript-path validator is never exercised — the path bug is moot).
--
-- Dedup is keyed on `content_hash` (UNIQUE): a SessionEnd capture that repeats
-- an identical PreCompact tail is an idempotent no-op. `status` + `attempts`
-- subsume T07's marker/reconcile machinery — the queue is the level-triggered
-- work list.
--
-- UUID values (id) are supplied by application code; UUIDv7 is the canonical
-- contract, matching every other table in 001_initial_schema.sql.
CREATE TABLE IF NOT EXISTS transcript_ingest_queue (
    id UUID PRIMARY KEY,
    session_id TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE,
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('session_end', 'pre_compact', 'reconcile')),
    repo_path TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'processed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    error TEXT,
    enqueued_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Claim path: drain workers select the oldest `pending` rows under
-- FOR UPDATE SKIP LOCKED; this index keeps that ordered scan cheap.
CREATE INDEX IF NOT EXISTS idx_transcript_ingest_status_enqueued
    ON transcript_ingest_queue (status, enqueued_at);

-- Reuses the shared trigger function declared (CREATE OR REPLACE) in
-- 001_initial_schema.sql; 002 always runs after 001 so the function exists.
DROP TRIGGER IF EXISTS trg_transcript_ingest_set_updated_at ON transcript_ingest_queue;
CREATE TRIGGER trg_transcript_ingest_set_updated_at
BEFORE UPDATE ON transcript_ingest_queue
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

COMMIT;
