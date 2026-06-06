//! PG-backed store for scope-demotion scanning.
//!
//! # Port
//!
//! [`ScopeDemotionStore`] — reads `scope='global'` skills from PG so the
//! demotion pass can check whether any of them reference project-local
//! identifiers (which would make them mis-scoped).
//!
//! A global skill that contains a project-local identifier pollutes every
//! project's retrieval at weight 0.7 — demotion proposals flag these for
//! human review.

use async_trait::async_trait;
use sqlx::PgPool;
use thiserror::Error;

// ─── Row type ─────────────────────────────────────────────────────────────────

/// A global-scoped skill row fetched from Postgres.
///
/// Contains only the fields needed by the demotion pass:
/// - `id` and `name`/`description` for semantic text construction and proposal attribution.
/// - `source_paths` for audit trail in the proposal frontmatter.
///
/// Tags are included for full semantic text matching (the identifier veto must
/// check the same text surface used during promotion — name + description).
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSkillRow {
    /// String representation of the skill UUID (`id::TEXT` cast in the query).
    pub id: String,
    /// Skill name.
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Tag strings stored as a `text[]` column.
    pub tags: Vec<String>,
    /// Source paths from `skills.source_paths text[]`. May be empty for
    /// hand-authored global skills that pre-date automatic source tracking.
    pub source_paths: Vec<String>,
}

impl GlobalSkillRow {
    /// Builds the semantic text block used for identifier scanning.
    ///
    /// Matches the `SkillSnapshot::semantic_text()` convention: `name\ndescription`.
    /// This is the same surface the identifier veto checks during promotion so that
    /// the demotion check is symmetric.
    pub fn semantic_text(&self) -> String {
        format!("{}\n{}", self.name, self.description)
    }
}

// ─── Port ─────────────────────────────────────────────────────────────────────

/// Read port for fetching global-scoped skills from machine-wide PG.
///
/// Used by the demotion pass to find global skills that reference project-local
/// identifiers and should be demoted. Only active/draft skills are returned —
/// retired or lifecycle-dead skills need no demotion proposal.
#[async_trait]
pub trait ScopeDemotionStore: Send + Sync {
    /// Returns all `scope='global'` skills with status `'ready'` or `'draft'`.
    ///
    /// Skills with an empty `source_paths` are included — hand-authored global
    /// skills frequently have no recorded source path, and they must still be
    /// scanned.
    ///
    /// # Errors
    ///
    /// Returns `ScopeDemotionError::Database` on any PG query failure. Never
    /// swallows errors — callers must surface them loudly with a `reason_code`.
    async fn fetch_all_global_skills(&self) -> Result<Vec<GlobalSkillRow>, ScopeDemotionError>;
}

// ─── Postgres adapter ─────────────────────────────────────────────────────────

/// Postgres adapter that reads global-scoped skills for demotion scanning.
///
/// Implements [`ScopeDemotionStore`]. Uses `sqlx::query` with typed `try_get`
/// column access — same error-mapping shape as [`super::promotion_recurrence::PostgresPromotionRecurrenceStore`].
#[derive(Clone)]
pub struct PostgresScopeDemotionStore {
    pool: PgPool,
}

impl PostgresScopeDemotionStore {
    /// Creates a new store backed by the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScopeDemotionStore for PostgresScopeDemotionStore {
    async fn fetch_all_global_skills(&self) -> Result<Vec<GlobalSkillRow>, ScopeDemotionError> {
        // Query global skills that are active or in draft.
        //
        // `status IN ('ready', 'draft')` excludes retired or lifecycle-dead rows —
        // there is no value in emitting a demotion proposal for a skill that is
        // already being retired.
        //
        // We do NOT filter by source_paths non-empty: hand-authored global SKILL.md
        // files may have no tracked source path, and they are exactly the category
        // most likely to contain project-local identifiers.
        let rows = sqlx::query(
            r#"
            SELECT
                id::TEXT       AS id,
                name           AS name,
                description    AS description,
                tags           AS tags,
                source_paths   AS source_paths
            FROM skills
            WHERE scope = 'global'
              AND status IN ('ready', 'draft')
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(ScopeDemotionError::Database)?;

        use sqlx::Row;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let name: String = row.try_get("name")?;
                let description: String = row.try_get("description")?;
                let tags: Vec<String> = row.try_get("tags")?;
                let source_paths: Vec<String> = row.try_get("source_paths")?;
                Ok(GlobalSkillRow {
                    id,
                    name,
                    description,
                    tags,
                    source_paths,
                })
            })
            .collect::<Result<Vec<GlobalSkillRow>, sqlx::Error>>()
            .map_err(ScopeDemotionError::Database)
    }
}

// ─── Error ────────────────────────────────────────────────────────────────────

/// Error type for scope demotion store operations.
#[derive(Debug, Error)]
pub enum ScopeDemotionError {
    #[error("scope demotion database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves `semantic_text()` concatenates name and description with a newline.
    #[test]
    fn global_skill_row_semantic_text_concatenates_name_and_description() {
        let row = GlobalSkillRow {
            id: "abc".to_owned(),
            name: "declare cargo bin explicitly".to_owned(),
            description: "Declare [[bin]] explicitly or the binary is named after the package"
                .to_owned(),
            tags: vec!["rust".to_owned()],
            source_paths: vec!["/home/user/.claude/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let text = row.semantic_text();
        assert!(text.contains("declare cargo bin explicitly"));
        assert!(text.contains("Declare [[bin]] explicitly"));
        assert!(text.contains('\n'), "name and description must be separated by newline");
    }

    /// Proves `semantic_text()` works for a global skill with no source paths.
    #[test]
    fn global_skill_row_semantic_text_works_with_empty_source_paths() {
        let row = GlobalSkillRow {
            id: "hand-authored".to_owned(),
            name: "musl cross-compile".to_owned(),
            description: "Cross-compiling Rust to musl needs musl-tools in dynamic-agent-skill-layer"
                .to_owned(),
            tags: vec![],
            source_paths: vec![],
        };
        let text = row.semantic_text();
        assert!(text.contains("musl cross-compile"));
        assert!(text.contains("dynamic-agent-skill-layer"));
    }
}
