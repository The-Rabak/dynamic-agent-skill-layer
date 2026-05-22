BEGIN;

-- UUID values are supplied by application code. UUIDv7 is the canonical contract.

CREATE TABLE IF NOT EXISTS skills (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('project', 'global', 'team')),
    merged_from_scopes TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL CHECK (status IN ('draft', 'ready', 'deprecated', 'retired')),
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('proposed', 'active', 'deprecated', 'retired')),
    tags TEXT[] NOT NULL DEFAULT '{}',
    graph_version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS subunits (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('procedure', 'convention', 'asset', 'evidence', 'summary')),
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('proposed', 'active', 'deprecated', 'retired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS communities (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('project', 'global', 'team')),
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('proposed', 'active', 'deprecated', 'retired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS skill_subunits (
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    subunit_id UUID NOT NULL REFERENCES subunits(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (skill_id, subunit_id)
);

CREATE TABLE IF NOT EXISTS community_skills (
    community_id UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, skill_id)
);

CREATE TABLE IF NOT EXISTS session_logs (
    id UUID PRIMARY KEY,
    session_id TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('project', 'global', 'team')),
    transcript_ref TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS skill_usage (
    id UUID PRIMARY KEY,
    session_id TEXT NOT NULL,
    skill_id UUID REFERENCES skills(id) ON DELETE SET NULL,
    usage_count INTEGER NOT NULL DEFAULT 1 CHECK (usage_count > 0),
    context_status TEXT NOT NULL CHECK (context_status IN ('ok', 'no_match', 'degraded', 'duplicate_suppressed')),
    used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id UUID,
    action TEXT NOT NULL,
    actor TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    happened_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS outbox_events (
    event_id UUID PRIMARY KEY,
    event_type TEXT NOT NULL,
    correlation_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    payload JSONB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'processing', 'published', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    stream_id TEXT,
    last_error TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS rebuild_locks (
    lock_name TEXT PRIMARY KEY,
    owner_id UUID NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS graph_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    graph_version BIGINT NOT NULL DEFAULT 0,
    rebuilt_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO graph_state (singleton, graph_version)
VALUES (TRUE, 0)
ON CONFLICT (singleton) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_skills_scope_status
    ON skills (scope, status, lifecycle);
CREATE INDEX IF NOT EXISTS idx_skills_name_trgm
    ON skills (name);
CREATE INDEX IF NOT EXISTS idx_subunits_kind
    ON subunits (kind);
CREATE INDEX IF NOT EXISTS idx_skill_subunits_subunit
    ON skill_subunits (subunit_id, skill_id);
CREATE INDEX IF NOT EXISTS idx_community_skills_skill
    ON community_skills (skill_id, community_id);
CREATE INDEX IF NOT EXISTS idx_session_logs_session_id_created
    ON session_logs (session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_skill_usage_skill_used_at
    ON skill_usage (skill_id, used_at DESC);
CREATE INDEX IF NOT EXISTS idx_skill_usage_session_used_at
    ON skill_usage (session_id, used_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_entity_happened
    ON audit_log (entity_type, entity_id, happened_at DESC);
CREATE INDEX IF NOT EXISTS idx_outbox_status_available
    ON outbox_events (status, available_at);
CREATE INDEX IF NOT EXISTS idx_outbox_claim_pending
    ON outbox_events (status, available_at, occurred_at, event_id);
CREATE INDEX IF NOT EXISTS idx_outbox_event_type_occurred
    ON outbox_events (event_type, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_rebuild_locks_expires_at
    ON rebuild_locks (expires_at);

CREATE OR REPLACE FUNCTION set_updated_at_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_skills_set_updated_at ON skills;
CREATE TRIGGER trg_skills_set_updated_at
BEFORE UPDATE ON skills
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

DROP TRIGGER IF EXISTS trg_subunits_set_updated_at ON subunits;
CREATE TRIGGER trg_subunits_set_updated_at
BEFORE UPDATE ON subunits
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

DROP TRIGGER IF EXISTS trg_communities_set_updated_at ON communities;
CREATE TRIGGER trg_communities_set_updated_at
BEFORE UPDATE ON communities
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

DROP TRIGGER IF EXISTS trg_outbox_events_set_updated_at ON outbox_events;
CREATE TRIGGER trg_outbox_events_set_updated_at
BEFORE UPDATE ON outbox_events
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

DROP TRIGGER IF EXISTS trg_rebuild_locks_set_updated_at ON rebuild_locks;
CREATE TRIGGER trg_rebuild_locks_set_updated_at
BEFORE UPDATE ON rebuild_locks
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

DROP TRIGGER IF EXISTS trg_graph_state_set_updated_at ON graph_state;
CREATE TRIGGER trg_graph_state_set_updated_at
BEFORE UPDATE ON graph_state
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

COMMIT;
