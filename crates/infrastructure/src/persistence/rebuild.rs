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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphSkillRecord {
    pub stable_id: String,
    pub name: String,
    pub description: String,
    pub scope: ScopeType,
    pub tags: Vec<String>,
    pub subunits: Vec<LiveGraphSubunitRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGraphCommunityRecord {
    pub stable_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_skill_ids: Vec<String>,
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

/// Persisted skill projection used by live graph read adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGraphSkillRecord {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub scope: String,
    pub tags: Vec<String>,
    pub community_id: Option<String>,
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
        let skill_rows =
            sqlx::query_as::<_, (String, String, String, Vec<String>, Option<String>, String)>(
                r#"
            SELECT
                skills.id::TEXT,
                skills.name,
                skills.description,
                skills.tags,
                communities.id::TEXT,
                skills.scope
            FROM skills
            LEFT JOIN community_skills
                ON community_skills.skill_id = skills.id
            LEFT JOIN communities
                ON communities.id = community_skills.community_id
            WHERE skills.lifecycle = 'active'
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
            .map(|(skill_id, name, description, tags, community_id, scope)| {
                PersistedGraphSkillRecord {
                    subunits: subunits_by_skill.remove(&skill_id).unwrap_or_default(),
                    skill_id,
                    name,
                    description,
                    scope,
                    tags,
                    community_id,
                }
            })
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
                    id, name, description, scope, status, lifecycle, tags, merged_from_scopes, graph_version
                ) VALUES (
                    $1, $2, $3, $4, 'ready', 'active', $5, '{}'::TEXT[], 0
                )
                "#,
            )
            .bind(skill_id)
            .bind(&skill.name)
            .bind(&skill.description)
            .bind(scope_to_db_value(skill.scope))
            .bind(&skill.tags)
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
                    INSERT INTO community_skills (community_id, skill_id)
                    VALUES ($1, $2)
                    "#,
                )
                .bind(community_id)
                .bind(stable_uuid("skill", member_skill_id))
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
