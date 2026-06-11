use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{ScopeType, SubunitType};
use serde_json::Value;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildLock {
    pub lock_name: String,
    pub owner_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum RebuildError {
    #[error("invalid rebuild contract: {0}")]
    InvalidContract(String),
    #[error("rebuild persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphSubunitRecord {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
}

/// Write DTO used by the durable write path during a graph rebuild.
///
/// This record owns the authoritative domain types at write time: `scope` is
/// [`ScopeType`] (the typed domain enum) and `source_paths` carries raw
/// absolute path strings exactly as discovered by the graph builder. The DB
/// stores `source_paths` as a `text[]` column; the retrieval read path
/// re-resolves them to `PathBuf` values at boot (see [`PersistedGraphSkillRecord`]).
///
/// An empty `source_paths` is valid for skills seeded programmatically
/// (e.g. E2E test fixtures); the retrieval boot adapter falls back to the
/// configured scope root for those rows so scope matching still works.
///
/// Multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) are WRITE-AHEAD source data from the
/// SKILL.md frontmatter.  They are persisted to nullable `skills` columns added
/// in migration 009.  No production reader SELECTs them yet (T04/T05 will).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphSkillRecord {
    pub stable_id: String,
    pub name: String,
    pub description: String,
    pub scope: ScopeType,
    pub tags: Vec<String>,
    /// Real SKILL.md file path(s) for this skill, as absolute path strings.
    /// Empty for skills that were seeded without a filesystem origin.
    pub source_paths: Vec<String>,
    pub subunits: Vec<LiveGraphSubunitRecord>,
    /// Task triggers where this skill applies. Empty when the skill has no frontmatter.
    pub use_when: Vec<String>,
    /// Situations where this skill should NOT be applied. Empty when absent.
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names the skill applies to. Empty when absent.
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, models, or APIs. Empty when absent.
    pub tools: Vec<String>,
    /// Verifier-critical constraints. Empty when absent.
    pub invariants: Vec<String>,
    /// Prerequisites assumed by this skill. Empty when absent.
    pub requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill. Empty when absent.
    pub produces: Vec<String>,
}

/// Write DTO for a single community membership source.
///
/// A skill can appear in multiple `LiveGraphCommunityRecord` values
/// (one per source) — this is how dual membership is expressed at the
/// persistence boundary.  The `source` field maps directly to the
/// `community_skills.source` column added in migration 006.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphCommunityRecord {
    pub stable_id: String,
    pub name: String,
    pub scope: ScopeType,
    /// Skills that belong to this community under `source`.
    pub member_skill_ids: Vec<String>,
    /// Membership origin: `"hdbscan"` for semantic clusters, `"tag"` for
    /// first-tag grouping.  Must match the DB CHECK constraint values.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphSnapshotMutation {
    pub rebuilt_at: DateTime<Utc>,
    pub skills: Vec<LiveGraphSkillRecord>,
    pub communities: Vec<LiveGraphCommunityRecord>,
}

/// Write DTO for a single typed skill edge (V1.7 T05).
///
/// `source_stable_id` / `target_stable_id` are the graph builder's stable skill ids
/// (blake3 hex of the source path); [`PostgresRebuildCoordinator::replace_skill_edges`]
/// maps them to the durable `skills.id` UUIDs via the same derivation skills use, so an
/// edge always references the row written for that skill in the same rebuild.
///
/// `edge_type` and `origin` carry the canonical DB labels produced by
/// [`domain::EdgeType::as_db_str`] / [`domain::EdgeOrigin::as_db_str`] and are enforced
/// by CHECK constraints in migration 010. `evidence` is the structured justification
/// stored as JSONB; `None` for edges with no structured evidence (e.g. manual edges).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveGraphEdgeRecord {
    pub source_stable_id: String,
    pub target_stable_id: String,
    pub edge_type: String,
    pub origin: String,
    pub confidence: f32,
    pub reason: String,
    pub evidence: Option<Value>,
}

/// Persisted subunit projection used by live graph read adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGraphSubunitRecord {
    pub subunit_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
}

/// Read projection returned by live graph read adapters from the database.
///
/// Unlike [`LiveGraphSkillRecord`] (the write DTO), this record uses raw DB
/// strings for fields that carry typed values at write time: `scope` is a
/// `String` (the raw DB value, e.g. `"project"` or `"global"`) rather than
/// [`ScopeType`], and `source_paths` holds the raw path strings exactly as
/// stored in the `skills.source_paths` `text[]` column. The retrieval boot
/// adapter resolves these strings to `PathBuf` values via `canonicalize` with
/// a raw-string fallback so scope prefix matching works even for paths that
/// no longer exist on the current host.
///
/// Rows written before migration 005 carry an empty `source_paths` array;
/// callers must fall back to the configured scope root for those rows.
///
/// `community_ids` carries ALL community memberships for this skill (across
/// all sources — `hdbscan` and `tag`).  Migration 006 introduced dual
/// membership; pre-migration rows with a single membership still return a
/// one-element vec.  Empty when the skill has no memberships.
///
/// Multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) are the migration-009 columns.
/// NULL in the DB maps to an empty `Vec<String>` here so callers can treat
/// absent and present-but-empty uniformly. These fields feed the BM25
/// lexical corpus (T04-B) and are intentionally NOT added to dense embedding
/// text (embedding stays name+description+tags per the T04-B scope fence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGraphSkillRecord {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub scope: String,
    pub tags: Vec<String>,
    /// Real SKILL.md source paths from `skills.source_paths`. Empty for
    /// pre-migration rows or skills seeded without a filesystem origin.
    pub source_paths: Vec<String>,
    /// All community IDs this skill belongs to (any source).  Empty when the
    /// skill has no community memberships.
    pub community_ids: Vec<String>,
    pub subunits: Vec<PersistedGraphSubunitRecord>,
    /// Task triggers where this skill applies (migration 009). Empty when NULL or absent.
    pub use_when: Vec<String>,
    /// Situations where this skill must NOT be applied (migration 009). Empty when NULL.
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names (migration 009). Empty when NULL.
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, APIs (migration 009). Empty when NULL.
    pub tools: Vec<String>,
    /// Verifier-critical constraints (migration 009). Empty when NULL.
    pub invariants: Vec<String>,
    /// Prerequisites assumed by this skill (migration 009). Empty when NULL.
    pub requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill (migration 009). Empty when NULL.
    pub produces: Vec<String>,
}

/// Persisted community projection used by live graph read adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGraphCommunityRecord {
    pub community_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_skill_ids: Vec<String>,
}

/// Read projection for a persisted typed skill edge (V1.7 T05).
///
/// `source_skill_id` / `target_skill_id` are the durable `skills.id` UUIDs as text.
/// `edge_type` and `origin` are raw DB labels; callers parse them back into the typed
/// [`domain::EdgeType`] / [`domain::EdgeOrigin`] via `from_db_str`. This projection makes
/// cold-start proposals observable (acceptance criterion) without re-deriving them.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedGraphEdgeRecord {
    pub source_skill_id: String,
    pub target_skill_id: String,
    pub edge_type: String,
    pub origin: String,
    pub confidence: f32,
    pub reason: String,
    pub evidence: Option<Value>,
}

/// Postgres reader for live graph projections consumed by admin read tools.
#[derive(Debug, Clone)]
pub struct PostgresGraphSnapshotStore {
    pool: PgPool,
}

impl PostgresGraphSnapshotStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_skills(&self) -> Result<Vec<PersistedGraphSkillRecord>, RebuildError> {
        // source_paths was added in migration 005; pre-migration rows return '{}'.
        // community_ids aggregates ALL community memberships across sources (migration 006).
        // Pre-migration rows with a single membership return a one-element array.
        // use_when…produces are the multi-view columns from migration 009; NULL in the
        // DB is returned as NULL from PostgreSQL and mapped to an empty Vec here so the
        // BM25 corpus builder can concatenate them without separate NULL checks.
        let skill_rows = sqlx::query_as::<
            _,
            (
                String,              // id
                String,              // name
                String,              // description
                Vec<String>,         // tags
                Vec<String>,         // source_paths
                Vec<String>,         // community_ids (aggregated)
                String,              // scope
                Option<Vec<String>>, // use_when (nullable TEXT[])
                Option<Vec<String>>, // avoid_when
                Option<Vec<String>>, // artifacts
                Option<Vec<String>>, // tools
                Option<Vec<String>>, // invariants
                Option<Vec<String>>, // requires
                Option<Vec<String>>, // produces
            ),
        >(
            r#"
            SELECT
                skills.id::TEXT,
                skills.name,
                skills.description,
                skills.tags,
                skills.source_paths,
                COALESCE(
                    ARRAY_AGG(DISTINCT communities.id::TEXT)
                    FILTER (WHERE communities.id IS NOT NULL),
                    '{}'::TEXT[]
                ),
                skills.scope,
                skills.use_when,
                skills.avoid_when,
                skills.artifacts,
                skills.tools,
                skills.invariants,
                skills.requires,
                skills.produces
            FROM skills
            LEFT JOIN community_skills
                ON community_skills.skill_id = skills.id
            LEFT JOIN communities
                ON communities.id = community_skills.community_id
            WHERE skills.lifecycle = 'active'
            GROUP BY skills.id, skills.name, skills.description,
                     skills.tags, skills.source_paths, skills.scope,
                     skills.use_when, skills.avoid_when, skills.artifacts,
                     skills.tools, skills.invariants, skills.requires, skills.produces
            ORDER BY skills.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let subunit_rows = sqlx::query_as::<_, (String, String, String, String, String)>(
            r#"
            SELECT
                skill_subunits.skill_id::TEXT,
                subunits.id::TEXT,
                subunits.kind,
                subunits.title,
                subunits.content
            FROM skill_subunits
            JOIN subunits
                ON subunits.id = skill_subunits.subunit_id
            ORDER BY skill_subunits.skill_id, skill_subunits.position
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut subunits_by_skill: std::collections::HashMap<
            String,
            Vec<PersistedGraphSubunitRecord>,
        > = std::collections::HashMap::new();
        for (skill_id, subunit_id, kind, title, content) in subunit_rows {
            subunits_by_skill
                .entry(skill_id)
                .or_default()
                .push(PersistedGraphSubunitRecord {
                    subunit_id,
                    kind,
                    title,
                    content,
                });
        }

        Ok(skill_rows
            .into_iter()
            .map(
                |(
                    skill_id,
                    name,
                    description,
                    tags,
                    source_paths,
                    community_ids,
                    scope,
                    use_when,
                    avoid_when,
                    artifacts,
                    tools,
                    invariants,
                    requires,
                    produces,
                )| {
                    PersistedGraphSkillRecord {
                        subunits: subunits_by_skill.remove(&skill_id).unwrap_or_default(),
                        skill_id,
                        name,
                        description,
                        scope,
                        tags,
                        source_paths,
                        community_ids,
                        // NULL in the DB (absent frontmatter fields) maps to empty Vec
                        // so BM25 corpus builders can concatenate without NULL guards.
                        use_when: use_when.unwrap_or_default(),
                        avoid_when: avoid_when.unwrap_or_default(),
                        artifacts: artifacts.unwrap_or_default(),
                        tools: tools.unwrap_or_default(),
                        invariants: invariants.unwrap_or_default(),
                        requires: requires.unwrap_or_default(),
                        produces: produces.unwrap_or_default(),
                    }
                },
            )
            .collect())
    }

    pub async fn list_communities(
        &self,
    ) -> Result<Vec<PersistedGraphCommunityRecord>, RebuildError> {
        let rows = sqlx::query_as::<_, (String, String, String, Vec<String>)>(
            r#"
            SELECT
                communities.id::TEXT,
                communities.name,
                communities.scope,
                COALESCE(
                    ARRAY_AGG(community_skills.skill_id::TEXT ORDER BY community_skills.skill_id)
                    FILTER (WHERE community_skills.skill_id IS NOT NULL),
                    '{}'::TEXT[]
                )
            FROM communities
            LEFT JOIN community_skills
                ON community_skills.community_id = communities.id
            WHERE communities.lifecycle = 'active'
            GROUP BY communities.id, communities.name, communities.scope
            ORDER BY communities.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(community_id, name, scope, member_skill_ids)| {
                Ok(PersistedGraphCommunityRecord {
                    community_id,
                    name,
                    scope: scope_from_db_value(&scope)?,
                    member_skill_ids,
                })
            })
            .collect()
    }

    /// Lists all persisted typed skill edges, ordered deterministically.
    ///
    /// Returns every edge regardless of origin so proposals (`cold_start_proposal`)
    /// are observable alongside trusted edges; callers filter by origin/walkability.
    pub async fn list_skill_edges(&self) -> Result<Vec<PersistedGraphEdgeRecord>, RebuildError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,                           // source_skill_id
                String,                           // target_skill_id
                String,                           // edge_type
                String,                           // edge_origin
                f32,                              // confidence
                String,                           // reason
                Option<sqlx::types::Json<Value>>, // evidence (nullable JSONB)
            ),
        >(
            r#"
            SELECT
                source_skill_id::TEXT,
                target_skill_id::TEXT,
                edge_type,
                edge_origin,
                confidence,
                reason,
                evidence
            FROM skill_edges
            ORDER BY source_skill_id, target_skill_id, edge_type
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    source_skill_id,
                    target_skill_id,
                    edge_type,
                    origin,
                    confidence,
                    reason,
                    evidence,
                )| {
                    PersistedGraphEdgeRecord {
                        source_skill_id,
                        target_skill_id,
                        edge_type,
                        origin,
                        confidence,
                        reason,
                        evidence: evidence.map(|json| json.0),
                    }
                },
            )
            .collect())
    }

    /// Reads the durable `graph_state.graph_version`.
    ///
    /// Returns `0` on cold start (before any rebuild has written the singleton
    /// row) so callers can build an empty snapshot that still reports the true
    /// version rather than a hardcoded placeholder.
    pub async fn current_graph_version(&self) -> Result<i64, RebuildError> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT graph_version
            FROM graph_state
            WHERE singleton = TRUE
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(version,)| version).unwrap_or(0))
    }
}

#[async_trait]
pub trait RebuildCoordinator: Send + Sync {
    async fn try_acquire_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError>;
    async fn renew_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError>;
    async fn release_lock(&self, lock_name: &str, owner_id: Uuid) -> Result<(), RebuildError>;
    async fn current_graph_version(&self) -> Result<i64, RebuildError>;
    async fn bump_graph_version(&self) -> Result<i64, RebuildError>;
    async fn replace_snapshot_and_bump_version(
        &self,
        mutation: LiveGraphSnapshotMutation,
    ) -> Result<i64, RebuildError>;
}

#[derive(Debug, Clone)]
pub struct PostgresRebuildCoordinator {
    pool: PgPool,
}

impl PostgresRebuildCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Replaces the entire typed-edge set in a single transaction (V1.7 T05).
    ///
    /// Cold-start edges are derived deterministically from the structured skill fields
    /// on every rebuild, so the durable set is fully rebuilt rather than merged: the
    /// method deletes all existing edges, then inserts the supplied set. It must be
    /// called AFTER [`Self::replace_snapshot_and_bump_version`] has committed the skills
    /// for this rebuild, because each edge's `source`/`target` foreign-keys reference
    /// `skills.id` (with `ON DELETE CASCADE`). The same `stable_uuid("skill", …)`
    /// derivation used for the skill rows resolves each edge endpoint.
    ///
    /// The `ON CONFLICT` clause keeps a single edge set idempotent if the same typed
    /// directed pair appears twice; `confidence`, `origin`, `reason`, and `evidence` are
    /// refreshed and `updated_at` is bumped on conflict.
    pub async fn replace_skill_edges(
        &self,
        edges: &[LiveGraphEdgeRecord],
    ) -> Result<(), RebuildError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM skill_edges")
            .execute(&mut *tx)
            .await?;

        for edge in edges {
            let source_id = stable_uuid("skill", &edge.source_stable_id);
            let target_id = stable_uuid("skill", &edge.target_stable_id);
            // Deterministic PK so replays of the same edge map to the same row; the
            // UNIQUE(source, target, edge_type) constraint is the real dedup key.
            let edge_id = stable_uuid(
                "skill_edge",
                &format!(
                    "{}:{}:{}",
                    edge.source_stable_id, edge.target_stable_id, edge.edge_type
                ),
            );
            sqlx::query(
                r#"
                INSERT INTO skill_edges (
                    id, source_skill_id, target_skill_id, edge_type, edge_origin,
                    confidence, reason, evidence, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
                ON CONFLICT (source_skill_id, target_skill_id, edge_type) DO UPDATE
                SET edge_origin = EXCLUDED.edge_origin,
                    confidence = EXCLUDED.confidence,
                    reason = EXCLUDED.reason,
                    evidence = EXCLUDED.evidence,
                    updated_at = NOW()
                "#,
            )
            .bind(edge_id)
            .bind(source_id)
            .bind(target_id)
            .bind(&edge.edge_type)
            .bind(&edge.origin)
            .bind(edge.confidence)
            .bind(&edge.reason)
            .bind(edge.evidence.as_ref().map(sqlx::types::Json))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl RebuildCoordinator for PostgresRebuildCoordinator {
    async fn try_acquire_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError> {
        if lock_name.trim().is_empty() {
            return Err(RebuildError::InvalidContract(
                "lock_name must not be blank".to_owned(),
            ));
        }

        let lease_end = Utc::now()
            + chrono::Duration::from_std(lease_duration)
                .map_err(|err| RebuildError::InvalidContract(err.to_string()))?;

        let result = sqlx::query(
            r#"
            INSERT INTO rebuild_locks (lock_name, owner_id, acquired_at, expires_at)
            VALUES ($1, $2, NOW(), $3)
            ON CONFLICT (lock_name) DO UPDATE
            SET owner_id = EXCLUDED.owner_id,
                acquired_at = EXCLUDED.acquired_at,
                expires_at = EXCLUDED.expires_at,
                updated_at = NOW()
            WHERE rebuild_locks.expires_at <= NOW()
               OR rebuild_locks.owner_id = EXCLUDED.owner_id
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .bind(lease_end)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn renew_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError> {
        let lease_end = Utc::now()
            + chrono::Duration::from_std(lease_duration)
                .map_err(|err| RebuildError::InvalidContract(err.to_string()))?;

        let result = sqlx::query(
            r#"
            UPDATE rebuild_locks
            SET expires_at = $3,
                updated_at = NOW()
            WHERE lock_name = $1
              AND owner_id = $2
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .bind(lease_end)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn release_lock(&self, lock_name: &str, owner_id: Uuid) -> Result<(), RebuildError> {
        sqlx::query(
            r#"
            DELETE FROM rebuild_locks
            WHERE lock_name = $1
              AND owner_id = $2
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn current_graph_version(&self) -> Result<i64, RebuildError> {
        let (version,): (i64,) = sqlx::query_as(
            r#"
            SELECT graph_version
            FROM graph_state
            WHERE singleton = TRUE
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }

    async fn bump_graph_version(&self) -> Result<i64, RebuildError> {
        let (version,): (i64,) = sqlx::query_as(
            r#"
            UPDATE graph_state
            SET graph_version = graph_version + 1,
                updated_at = NOW()
            WHERE singleton = TRUE
            RETURNING graph_version
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }

    async fn replace_snapshot_and_bump_version(
        &self,
        mutation: LiveGraphSnapshotMutation,
    ) -> Result<i64, RebuildError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM community_skills")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM skill_subunits")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM communities")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM subunits")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM skills").execute(&mut *tx).await?;

        for skill in &mutation.skills {
            let skill_id = stable_uuid("skill", &skill.stable_id);
            // Multi-view fields (use_when … produces) are WRITE-AHEAD columns added in
            // migration 009. No production reader SELECTs them yet; T04/T05 will.
            // NULL is stored when a field is empty (Vec is empty → bind as Option::None)
            // to stay consistent with the nullable column definition.
            // Empty Vec → NULL (the column is nullable); non-empty → bind the slice.
            let use_when_opt = (!skill.use_when.is_empty()).then_some(skill.use_when.as_slice());
            let avoid_when_opt =
                (!skill.avoid_when.is_empty()).then_some(skill.avoid_when.as_slice());
            let artifacts_opt = (!skill.artifacts.is_empty()).then_some(skill.artifacts.as_slice());
            let tools_opt = (!skill.tools.is_empty()).then_some(skill.tools.as_slice());
            let invariants_opt =
                (!skill.invariants.is_empty()).then_some(skill.invariants.as_slice());
            let requires_opt = (!skill.requires.is_empty()).then_some(skill.requires.as_slice());
            let produces_opt = (!skill.produces.is_empty()).then_some(skill.produces.as_slice());
            sqlx::query(
                r#"
                INSERT INTO skills (
                    id, name, description, scope, status, lifecycle, tags, source_paths,
                    merged_from_scopes, graph_version,
                    use_when, avoid_when, artifacts, tools, invariants, requires, produces
                ) VALUES (
                    $1, $2, $3, $4, 'ready', 'active', $5, $6, '{}'::TEXT[], 0,
                    $7, $8, $9, $10, $11, $12, $13
                )
                "#,
            )
            .bind(skill_id)
            .bind(&skill.name)
            .bind(&skill.description)
            .bind(scope_to_db_value(skill.scope))
            .bind(&skill.tags)
            .bind(&skill.source_paths)
            .bind(use_when_opt)
            .bind(avoid_when_opt)
            .bind(artifacts_opt)
            .bind(tools_opt)
            .bind(invariants_opt)
            .bind(requires_opt)
            .bind(produces_opt)
            .execute(&mut *tx)
            .await?;

            for (position, subunit) in skill.subunits.iter().enumerate() {
                let subunit_id = stable_uuid(
                    "subunit",
                    &format!("{}:{position}:{}", skill.stable_id, subunit.title),
                );
                sqlx::query(
                    r#"
                    INSERT INTO subunits (id, kind, title, content, lifecycle)
                    VALUES ($1, $2, $3, $4, 'active')
                    "#,
                )
                .bind(subunit_id)
                .bind(subunit_kind_to_db_value(subunit.kind))
                .bind(&subunit.title)
                .bind(&subunit.content)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    r#"
                    INSERT INTO skill_subunits (skill_id, subunit_id, position)
                    VALUES ($1, $2, $3)
                    "#,
                )
                .bind(skill_id)
                .bind(subunit_id)
                .bind(position as i32)
                .execute(&mut *tx)
                .await?;
            }
        }

        for community in &mutation.communities {
            let community_id = stable_uuid("community", &community.stable_id);
            sqlx::query(
                r#"
                INSERT INTO communities (id, name, description, scope, lifecycle)
                VALUES ($1, $2, $3, $4, 'active')
                "#,
            )
            .bind(community_id)
            .bind(&community.name)
            .bind(format!("Auto-generated community {}", community.name))
            .bind(scope_to_db_value(community.scope))
            .execute(&mut *tx)
            .await?;

            for member_skill_id in &community.member_skill_ids {
                sqlx::query(
                    r#"
                    INSERT INTO community_skills (community_id, skill_id, source)
                    VALUES ($1, $2, $3)
                    "#,
                )
                .bind(community_id)
                .bind(stable_uuid("skill", member_skill_id))
                .bind(&community.source)
                .execute(&mut *tx)
                .await?;
            }
        }

        let (graph_version,): (i64,) = sqlx::query_as(
            r#"
            UPDATE graph_state
            SET graph_version = graph_version + 1,
                rebuilt_at = $1,
                updated_at = NOW()
            WHERE singleton = TRUE
            RETURNING graph_version
            "#,
        )
        .bind(mutation.rebuilt_at)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE skills SET graph_version = $1")
            .bind(graph_version)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(graph_version)
    }
}

fn scope_to_db_value(scope: ScopeType) -> &'static str {
    match scope {
        ScopeType::Project => "project",
        ScopeType::Global => "global",
        ScopeType::Team => "team",
    }
}

fn scope_from_db_value(value: &str) -> Result<ScopeType, RebuildError> {
    match value {
        "project" => Ok(ScopeType::Project),
        "global" => Ok(ScopeType::Global),
        "team" => Ok(ScopeType::Team),
        other => Err(RebuildError::InvalidContract(format!(
            "unknown scope value in persistence: {other}"
        ))),
    }
}

fn subunit_kind_to_db_value(kind: SubunitType) -> &'static str {
    match kind {
        SubunitType::Procedure => "procedure",
        SubunitType::Convention => "convention",
        SubunitType::Asset => "asset",
        SubunitType::Evidence => "evidence",
        SubunitType::Summary => "summary",
    }
}

fn stable_uuid(entity_kind: &str, stable_id: &str) -> Uuid {
    let digest = blake3::hash(format!("{entity_kind}:{stable_id}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Re-derives the persisted `skills.id` UUID for a skill's stable id.
///
/// The rebuild persistence path writes `skills.id = stable_uuid("skill",
/// BuiltSkill.id)`, where `BuiltSkill.id` is the blake3 hex of the source path.
/// Consumers that rebuild skills out-of-band from the filesystem (the maintenance
/// worker's merge/retire passes) must key their `skill_usage` queries and joins on
/// this SAME UUID — not the raw blake3 hex — or the usage `unnest($1::uuid[])`
/// lookup rejects the id and, if coerced, would zero-match usage and mass-retire
/// every skill. This is the single source of truth for that derivation.
pub fn stable_skill_uuid(stable_id: &str) -> Uuid {
    stable_uuid("skill", stable_id)
}

#[cfg(test)]
mod tests {
    /// Smoke test: deserializes `tests/fixtures/retrieval_corpus.json` and
    /// asserts the field contract that all positive fixtures carry
    /// `expected_match: true` and all negative fixtures carry
    /// `expected_match: false`. Catches accidental edits that break the
    /// threshold-alignment prose embedded in the fixture.
    #[test]
    fn retrieval_corpus_fixture_field_contract() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/retrieval_corpus.json");
        let raw = std::fs::read_to_string(&fixture_path)
            .expect("tests/fixtures/retrieval_corpus.json must be readable");
        let corpus: serde_json::Value =
            serde_json::from_str(&raw).expect("retrieval_corpus.json must be valid JSON");

        let positives = corpus["positive_fixtures"]
            .as_array()
            .expect("positive_fixtures must be a JSON array");
        for fixture in positives {
            assert_eq!(
                fixture["expected_match"],
                serde_json::Value::Bool(true),
                "positive fixture '{}' must have expected_match: true",
                fixture["id"].as_str().unwrap_or("<unknown>")
            );
        }

        let negatives = corpus["negative_fixtures"]
            .as_array()
            .expect("negative_fixtures must be a JSON array");
        for fixture in negatives {
            assert_eq!(
                fixture["expected_match"],
                serde_json::Value::Bool(false),
                "negative fixture prompt '{}' must have expected_match: false",
                fixture["prompt"].as_str().unwrap_or("<unknown>")
            );
        }

        assert!(!positives.is_empty(), "positive_fixtures must not be empty");
        assert!(!negatives.is_empty(), "negative_fixtures must not be empty");
    }

    /// Live Postgres: proves typed skill edges persist with type, origin, reason, and
    /// evidence (T05 acceptance), that `conflicts_with` is stored (its non-walkability is
    /// a read-side concern enforced by `domain::EdgeType::is_walkable`), and that
    /// `replace_skill_edges` has replace (not append) semantics.
    ///
    /// Isolation: dedicated scratch schema dropped on completion.
    #[tokio::test]
    #[ignore = "requires live postgres"]
    async fn live_replace_and_list_skill_edges_roundtrip() {
        use chrono::Utc;
        use domain::ScopeType;

        use super::{
            LiveGraphEdgeRecord, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
            PostgresGraphSnapshotStore, PostgresRebuildCoordinator, RebuildCoordinator,
        };
        use crate::persistence::postgres::{PostgresAdapter, PostgresConfig};

        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        let admin_pool = sqlx::PgPool::connect(&db_url)
            .await
            .expect("admin pool connect");
        let scratch_schema = format!(
            "test_skill_edges_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        sqlx::query(&format!("CREATE SCHEMA {scratch_schema}"))
            .execute(&admin_pool)
            .await
            .expect("create scratch schema");

        let scratch_url = format!("{db_url}?options=-csearch_path%3D{scratch_schema}");
        let config = PostgresConfig {
            database_url: scratch_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("scratch adapter connect");
        adapter.run_migrations().await.expect("migrations apply");

        let coordinator = PostgresRebuildCoordinator::new(adapter.pool().clone());
        let store = PostgresGraphSnapshotStore::new(adapter.pool().clone());

        // Write two skills so the edges have real FK targets.
        let make_skill = |stable_id: &str, name: &str| LiveGraphSkillRecord {
            stable_id: stable_id.to_owned(),
            name: name.to_owned(),
            description: String::new(),
            scope: ScopeType::Project,
            tags: Vec::new(),
            source_paths: Vec::new(),
            subunits: Vec::new(),
            use_when: Vec::new(),
            avoid_when: Vec::new(),
            artifacts: Vec::new(),
            tools: Vec::new(),
            invariants: Vec::new(),
            requires: Vec::new(),
            produces: Vec::new(),
        };
        coordinator
            .replace_snapshot_and_bump_version(LiveGraphSnapshotMutation {
                rebuilt_at: Utc::now(),
                skills: vec![make_skill("a", "deploy"), make_skill("b", "build")],
                communities: Vec::new(),
            })
            .await
            .expect("write skills");

        // One trusted deterministic depends_on edge with structured evidence, and one
        // non-walkable conflicts_with edge with no evidence.
        coordinator
            .replace_skill_edges(&[
                LiveGraphEdgeRecord {
                    source_stable_id: "a".to_owned(),
                    target_stable_id: "b".to_owned(),
                    edge_type: "depends_on".to_owned(),
                    origin: "cold_start_deterministic".to_owned(),
                    confidence: 0.95,
                    reason: "deploy requires what build produces: binary".to_owned(),
                    evidence: Some(serde_json::json!({ "overlap": ["binary"] })),
                },
                LiveGraphEdgeRecord {
                    source_stable_id: "a".to_owned(),
                    target_stable_id: "b".to_owned(),
                    edge_type: "conflicts_with".to_owned(),
                    origin: "manual".to_owned(),
                    confidence: 0.0,
                    reason: "do not co-select".to_owned(),
                    evidence: None,
                },
            ])
            .await
            .expect("persist edges");

        let edges = store.list_skill_edges().await.expect("read edges");
        assert_eq!(edges.len(), 2, "both edges must persist");

        let depends = edges
            .iter()
            .find(|edge| edge.edge_type == "depends_on")
            .expect("depends_on edge persisted");
        assert_eq!(depends.origin, "cold_start_deterministic");
        assert!((depends.confidence - 0.95).abs() < 1e-5);
        assert!(!depends.reason.is_empty(), "reason must persist");
        assert_eq!(
            depends.evidence.as_ref().expect("evidence must persist")["overlap"][0],
            "binary"
        );

        let conflicts = edges
            .iter()
            .find(|edge| edge.edge_type == "conflicts_with")
            .expect("conflicts_with edge persisted (stored but not walkable)");
        assert!(
            conflicts.evidence.is_none(),
            "absent evidence must round-trip as NULL"
        );

        // Replace (not append) semantics: a second call with no edges clears the set.
        coordinator
            .replace_skill_edges(&[])
            .await
            .expect("replace with empty set");
        let after = store.list_skill_edges().await.expect("read edges again");
        assert!(
            after.is_empty(),
            "replace_skill_edges must replace, not append"
        );

        sqlx::query(&format!("DROP SCHEMA {scratch_schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .expect("drop scratch schema");
        admin_pool.close().await;
    }
}
