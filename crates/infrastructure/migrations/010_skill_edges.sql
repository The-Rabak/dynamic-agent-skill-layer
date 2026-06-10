BEGIN;

-- Typed inter-skill graph edges.
--
-- Design rationale:
--   V1.7 Design Decision #4: graph structure is separate evidence, NOT a scalar
--   rank multiplier.  Edges returned by graph search are offered to the agent as
--   neighbours (expand) or conflicts (prune/do-not-co-select), never folded into
--   the SkillRAE ranking score.  This table is the durable substrate for T05/T06.
--
-- Edge types:
--   depends_on   : skill A requires skill B as a prerequisite (backbone, walkable).
--   specializes  : skill A is a specialisation of skill B (backbone, walkable).
--   composes_with: skill A and skill B compose naturally together (walkable).
--   similar_to   : skill A is semantically similar to skill B (walkable).
--   conflicts_with: skill A and skill B should not be co-selected (NOT walkable —
--                  returned only as a one-hop prune signal).
--
-- Walkable set: depends_on, specializes, composes_with, similar_to.
-- Non-walkable : conflicts_with (stored but never traversed as a positive edge).
--
-- Directed-acyclicity constraint: enforced at the application layer for backbone
-- edge types (depends_on, specializes) via the acyclicity check in the edge
-- construction module.  The database stores what the application commits; it does
-- not re-validate topology on INSERT (topological validation is O(V+E) and requires
-- in-memory graph state the DB does not hold).
--
-- Origin values (edge_origin TEXT):
--   cold_start_deterministic : deterministic proposal from structured skill fields
--                              (requires/produces/artifacts/tools); auto-committed at
--                              high confidence (≥ 0.9).
--   cold_start_proposal      : same source, below auto-commit confidence threshold;
--                              staged for review, not yet a trusted walkable edge.
--   manual                   : operator-authored edge.
--   agent_derived            : agent-classified edge (requires evidence, not yet
--                              implemented in T05 — reserved for T06+).
--
-- Fields:
--   id              : UUIDv7 primary key (monotone, time-ordered).
--   source_skill_id : UUID of the skill that is the ORIGIN of the directed edge.
--                     References skills.id with ON DELETE CASCADE so edge rows are
--                     automatically cleaned up when a skill is removed.
--   target_skill_id : UUID of the skill that is the DESTINATION.
--                     Also cascades on skill delete.
--   edge_type       : one of the five type strings above.  CHECK constraint enforces
--                     the closed vocabulary at the DB level.
--   edge_origin     : how the edge was produced (see origin values above).
--   confidence      : real [0,1] confidence assigned at edge construction time.
--                     0.0 when unspecified (manual edges may omit this).
--   reason          : human-readable rationale for the edge (e.g. "B.produces
--                     matches A.requires: ['compiled binary']").  NOT NULL but can be
--                     empty string when no rationale is captured.
--   evidence        : JSONB blob carrying the source field values that justify the
--                     edge (e.g. {"source_requires":["compiled binary"],
--                     "target_produces":["compiled binary"]}).  NULL when no
--                     structured evidence is available (manual edges).
--   created_at      : row creation timestamp.
--   updated_at      : last update timestamp (changes when confidence is revised or
--                     edge is promoted from proposal to trusted).
--
-- Uniqueness:
--   A (source_skill_id, target_skill_id, edge_type) triple must be unique — the same
--   typed directed edge cannot be stored twice.  The UNIQUE constraint enforces this;
--   callers use ON CONFLICT DO NOTHING for idempotent inserts.
--
-- Indexes:
--   idx_skill_edges_source : fast lookup of all outgoing edges for a given skill.
--   idx_skill_edges_target : fast lookup of all incoming edges for a given skill.
--   idx_skill_edges_type   : fast scan by edge type (e.g. get all depends_on edges).
--
-- Compatibility:
--   CREATE TABLE IF NOT EXISTS is idempotent; safe to replay.
--
-- Human gate: APPROVED 2026-06-10 (V1.7 T05 typed graph edge storage).
--
-- Rollback (down):
--   DROP TABLE IF EXISTS skill_edges;

CREATE TABLE IF NOT EXISTS skill_edges (
    id              UUID        PRIMARY KEY,
    source_skill_id UUID        NOT NULL
                                REFERENCES skills (id) ON DELETE CASCADE,
    target_skill_id UUID        NOT NULL
                                REFERENCES skills (id) ON DELETE CASCADE,
    edge_type       TEXT        NOT NULL
                                CHECK (edge_type IN (
                                    'depends_on',
                                    'specializes',
                                    'composes_with',
                                    'similar_to',
                                    'conflicts_with'
                                )),
    edge_origin     TEXT        NOT NULL
                                CHECK (edge_origin IN (
                                    'cold_start_deterministic',
                                    'cold_start_proposal',
                                    'manual',
                                    'agent_derived'
                                )),
    confidence      REAL        NOT NULL DEFAULT 0.0
                                CHECK (confidence >= 0.0 AND confidence <= 1.0),
    reason          TEXT        NOT NULL DEFAULT '',
    evidence        JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT skill_edges_directed_unique
        UNIQUE (source_skill_id, target_skill_id, edge_type)
);

CREATE INDEX IF NOT EXISTS idx_skill_edges_source ON skill_edges (source_skill_id);
CREATE INDEX IF NOT EXISTS idx_skill_edges_target ON skill_edges (target_skill_id);
CREATE INDEX IF NOT EXISTS idx_skill_edges_type   ON skill_edges (edge_type);

COMMIT;
