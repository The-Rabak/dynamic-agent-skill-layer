//! PG-backed store for cross-project skill recurrence detection.
//!
//! # Port
//!
//! [`PromotionRecurrenceStore`] — reads `scope='project'` skills across ALL roots
//! on the machine PG. The merge runner only sees the mounted project root; this
//! store sees every project that shares the same PG cluster, which is the only
//! place the cross-project aggregate exists.
//!
//! # Design decision (re-embed, not Qdrant)
//!
//! Embeddings are stored in Qdrant, keyed by `stable_skill_uuid`, not in the
//! `skills` table. Rather than coupling the recurrence pass to a Qdrant
//! batch-fetch, this store returns the skill text fields and lets the caller
//! re-embed via the injected `EmbeddingService`. This matches how the merge
//! snapshot path builds embeddings in `build_skills_from_scope_roots` and avoids
//! a hard Qdrant dependency in the promotion pass.

use async_trait::async_trait;
use sqlx::PgPool;
use thiserror::Error;

// ─── Row type ─────────────────────────────────────────────────────────────────

/// A project-scoped skill row fetched from Postgres across all project roots.
///
/// Contains only the fields needed by the recurrence pass:
/// - `id` and `name`/`description`/`tags` for semantic text construction.
/// - `source_paths` for deriving the project-root key (distinct-root counting).
///
/// Embeddings are NOT included — they live in Qdrant. The caller re-embeds
/// `semantic_text()` via the injected `EmbeddingService` (see module doc).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSkillRow {
    /// String representation of the skill UUID (`id::TEXT` cast in the query).
    pub id: String,
    /// Skill name.
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Tag strings stored as a `text[]` column.
    pub tags: Vec<String>,
    /// Source paths from `skills.source_paths text[]`. Non-empty (filtered by
    /// `AND source_paths <> '{}'`). Used to derive the project-root key.
    pub source_paths: Vec<String>,
}

impl ProjectSkillRow {
    /// Builds the semantic text block used for embedding and equivalence comparison.
    ///
    /// Matches the `SkillSnapshot::semantic_text()` convention: `name\ndescription`.
    /// Subunits are absent (not fetched) — name + description is sufficient for
    /// cross-project recurrence detection.
    pub fn semantic_text(&self) -> String {
        format!("{}\n{}", self.name, self.description)
    }

    /// Derives a project-root key from the first source path.
    ///
    /// The expected path structure is:
    /// `{project-root}/{skills-namespace-dir}/{skill-id}/{source-file}`
    ///
    /// Strategy:
    /// 1. Walk up the path, starting from the source-file's parent.
    /// 2. Skip ancestors whose directory name contains "skill" or starts with ".".
    ///    Also unconditionally skip the FIRST non-file ancestor (the skill-id dir),
    ///    since it rarely contains "skill" but must not count as the project root.
    /// 3. The first remaining ancestor is the project root.
    ///
    /// Examples:
    /// - `/workspace/project-a/skills/cargo-bin/SKILL.md`
    ///   depth-1: `cargo-bin` (skill-id, unconditional skip)
    ///   depth-2: `skills` (contains "skill", skip)
    ///   depth-3: `project-a` → project root ✓
    /// - `/workspace/project-a/.skills/cargo-bin/SKILL.md`
    ///   depth-1: `cargo-bin` (skill-id, unconditional skip)
    ///   depth-2: `.skills` (starts with ".", skip)
    ///   depth-3: `project-a` → project root ✓
    ///
    /// This key is used for distinct-root counting: two skills under the same
    /// project root must yield the same key and must NOT count as recurrence.
    pub fn project_root_key(&self) -> Option<String> {
        let first_path = self.source_paths.first()?;
        let path = std::path::Path::new(first_path);

        // `ancestors()` walks from the path itself up to "/".
        // skip(1) moves past the source file to its parent.
        let mut depth_from_file: usize = 0;
        for ancestor in path.ancestors().skip(1) {
            depth_from_file += 1;

            let last_component = ancestor.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // depth-1 is the skill-id directory (e.g. "cargo-bin") — always skip
            // it regardless of name, as it is NOT the project root.
            if depth_from_file == 1 {
                continue;
            }

            // Skip root path ("/") which has an empty last component.
            if last_component.is_empty() {
                continue;
            }

            // At depth ≥ 2: skip skill-namespace directories (contain "skill" or ".").
            if last_component.to_ascii_lowercase().contains("skill")
                || last_component.starts_with('.')
            {
                continue;
            }

            // This ancestor is the project root.
            return Some(ancestor.display().to_string());
        }

        // Fallback: use the grandparent (skill-namespace dir's parent) if available.
        path.parent()
            .and_then(|p| p.parent())
            .map(|p| p.display().to_string())
    }
}

// ─── Port ─────────────────────────────────────────────────────────────────────

/// Read port for fetching project skills across all roots from machine-wide PG.
///
/// The merge runner only sees the ONE mounted project root. This store queries
/// `scope='project'` rows across ALL distinct `source_paths` in the entire PG
/// cluster — the only place the cross-project recurrence aggregate exists.
///
/// Implementations must return rows only for skills whose `source_paths` is
/// non-empty (the `source_paths <> '{}'` filter ensures this), since empty
/// paths carry no project-root provenance and cannot be counted distinctly.
#[async_trait]
pub trait PromotionRecurrenceStore: Send + Sync {
    /// Returns all `scope='project'` skills with non-empty `source_paths`
    /// across ALL project roots on this PG cluster.
    ///
    /// Skills with `status NOT IN ('ready', 'draft')` are excluded (retired/
    /// lifecycle-dead skills must not trigger promotion).
    ///
    /// # Errors
    ///
    /// Returns `PromotionRecurrenceError::Database` on any PG query failure.
    /// Never swallows errors — callers must surface them loudly.
    async fn fetch_all_project_skills(
        &self,
    ) -> Result<Vec<ProjectSkillRow>, PromotionRecurrenceError>;
}

// ─── Postgres adapter ─────────────────────────────────────────────────────────

/// Postgres adapter that reads project-scoped skills across ALL roots.
///
/// Implements [`PromotionRecurrenceStore`]. Uses `sqlx::query` with typed
/// `try_get` column access — same error-mapping shape as
/// [`crate::persistence::usage::PostgresUsageSampleStore`]. Column-access errors
/// are propagated rather than swallowed so a schema regression surfaces loudly.
#[derive(Clone)]
pub struct PostgresPromotionRecurrenceStore {
    pool: PgPool,
}

impl PostgresPromotionRecurrenceStore {
    /// Creates a new store backed by the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PromotionRecurrenceStore for PostgresPromotionRecurrenceStore {
    async fn fetch_all_project_skills(
        &self,
    ) -> Result<Vec<ProjectSkillRow>, PromotionRecurrenceError> {
        // Query project skills across ALL roots on this PG cluster.
        //
        // `source_paths <> '{}'` filters out skills without source-path provenance
        // — those cannot be assigned a project-root key and must not be counted.
        //
        // `status IN ('ready', 'draft')` excludes retired or lifecycle-dead rows.
        //
        // We do NOT filter by a mounted project root here — this is the whole
        // point of the recurrence pass: see every project in the aggregate.
        let rows = sqlx::query(
            r#"
            SELECT
                id::TEXT       AS id,
                name           AS name,
                description    AS description,
                tags           AS tags,
                source_paths   AS source_paths
            FROM skills
            WHERE scope = 'project'
              AND status IN ('ready', 'draft')
              AND source_paths <> '{}'
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PromotionRecurrenceError::Database)?;

        use sqlx::Row;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let name: String = row.try_get("name")?;
                let description: String = row.try_get("description")?;
                let tags: Vec<String> = row.try_get("tags")?;
                let source_paths: Vec<String> = row.try_get("source_paths")?;
                Ok(ProjectSkillRow {
                    id,
                    name,
                    description,
                    tags,
                    source_paths,
                })
            })
            .collect::<Result<Vec<ProjectSkillRow>, sqlx::Error>>()
            .map_err(PromotionRecurrenceError::Database)
    }
}

// ─── Error ────────────────────────────────────────────────────────────────────

/// Error type for promotion recurrence store operations.
#[derive(Debug, Error)]
pub enum PromotionRecurrenceError {
    #[error("promotion recurrence database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves `semantic_text()` concatenates name and description with a newline,
    /// matching the SkillSnapshot convention.
    #[test]
    fn project_skill_row_semantic_text_concatenates_name_and_description() {
        let row = ProjectSkillRow {
            id: "abc".to_owned(),
            name: "declare cargo bin explicitly".to_owned(),
            description: "Declare [[bin]] explicitly or the binary is named after the package"
                .to_owned(),
            tags: vec!["rust".to_owned()],
            source_paths: vec!["/workspace/project-a/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let text = row.semantic_text();
        assert!(text.contains("declare cargo bin explicitly"));
        assert!(text.contains("Declare [[bin]] explicitly"));
        assert!(
            text.contains('\n'),
            "name and description must be separated by newline"
        );
    }

    /// Proves `project_root_key()` returns a key from the first source path parent,
    /// stripping skill-namespace sub-directories.
    #[test]
    fn project_root_key_extracts_parent_from_source_path() {
        let row = ProjectSkillRow {
            id: "abc".to_owned(),
            name: "test skill".to_owned(),
            description: "desc".to_owned(),
            tags: vec![],
            source_paths: vec!["/workspace/project-a/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let key = row.project_root_key();
        assert!(key.is_some(), "key must be Some for a valid path");
        // The key should be some ancestor, not SKILL.md itself.
        let key = key.unwrap();
        assert!(
            !key.ends_with("SKILL.md"),
            "key must not be the SKILL.md path itself: {key}"
        );
    }

    /// Proves `project_root_key()` returns `None` when `source_paths` is empty.
    #[test]
    fn project_root_key_returns_none_for_empty_source_paths() {
        let row = ProjectSkillRow {
            id: "abc".to_owned(),
            name: "test skill".to_owned(),
            description: "desc".to_owned(),
            tags: vec![],
            source_paths: vec![],
        };
        assert!(row.project_root_key().is_none());
    }

    /// Proves two rows under different project roots yield different keys.
    #[test]
    fn project_root_key_differs_for_different_project_roots() {
        let row_a = ProjectSkillRow {
            id: "a".to_owned(),
            name: "skill".to_owned(),
            description: "desc".to_owned(),
            tags: vec![],
            source_paths: vec!["/workspace/project-a/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let row_b = ProjectSkillRow {
            id: "b".to_owned(),
            name: "skill".to_owned(),
            description: "desc".to_owned(),
            tags: vec![],
            source_paths: vec!["/workspace/project-b/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let key_a = row_a.project_root_key().unwrap();
        let key_b = row_b.project_root_key().unwrap();
        assert_ne!(
            key_a, key_b,
            "project-a and project-b must yield different keys: a={key_a}, b={key_b}"
        );
    }

    /// Proves two rows under the SAME project root (different skill sub-dirs)
    /// yield the SAME project-root key — they must NOT count as recurrence.
    #[test]
    fn project_root_key_same_for_same_project_root_different_skills() {
        let row_a = ProjectSkillRow {
            id: "a".to_owned(),
            name: "skill-1".to_owned(),
            description: "desc1".to_owned(),
            tags: vec![],
            source_paths: vec!["/workspace/myproject/skills/cargo-bin/SKILL.md".to_owned()],
        };
        let row_b = ProjectSkillRow {
            id: "b".to_owned(),
            name: "skill-2".to_owned(),
            description: "desc2".to_owned(),
            tags: vec![],
            source_paths: vec![
                "/workspace/myproject/skills/musl-cross-compile/SKILL.md".to_owned(),
            ],
        };
        let key_a = row_a.project_root_key().unwrap();
        let key_b = row_b.project_root_key().unwrap();
        assert_eq!(
            key_a, key_b,
            "two skills in the same project must yield the same root key: a={key_a}, b={key_b}"
        );
    }
}
