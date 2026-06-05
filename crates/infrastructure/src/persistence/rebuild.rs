use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{ScopeType, SubunitType};
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
}

/// Persisted community projection used by live graph read adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGraphCommunityRecord {
    pub community_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_skill_ids: Vec<String>,
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
        let skill_rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Vec<String>,
                Vec<String>,
                Vec<String>,
                String,
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
                skills.scope
            FROM skills
            LEFT JOIN community_skills
                ON community_skills.skill_id = skills.id
            LEFT JOIN communities
                ON communities.id = community_skills.community_id
            WHERE skills.lifecycle = 'active'
            GROUP BY skills.id, skills.name, skills.description,
                     skills.tags, skills.source_paths, skills.scope
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
                |(skill_id, name, description, tags, source_paths, community_ids, scope)| {
                    PersistedGraphSkillRecord {
                        subunits: subunits_by_skill.remove(&skill_id).unwrap_or_default(),
                        skill_id,
                        name,
                        description,
                        scope,
                        tags,
                        source_paths,
                        community_ids,
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
            sqlx::query(
                r#"
                INSERT INTO skills (
                    id, name, description, scope, status, lifecycle, tags, source_paths, merged_from_scopes, graph_version
                ) VALUES (
                    $1, $2, $3, $4, 'ready', 'active', $5, $6, '{}'::TEXT[], 0
                )
                "#,
            )
            .bind(skill_id)
            .bind(&skill.name)
            .bind(&skill.description)
            .bind(scope_to_db_value(skill.scope))
            .bind(&skill.tags)
            .bind(&skill.source_paths)
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
}
