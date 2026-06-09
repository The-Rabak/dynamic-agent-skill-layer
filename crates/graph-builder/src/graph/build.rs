use std::{
    fs,
    path::{Path, PathBuf},
};

use domain::{EmbeddingService, ScopeRoot, ScopeType};
use thiserror::Error;
use walkdir::WalkDir;

use crate::{
    extraction::{ExtractedSubunit, extract_skill},
    watcher::{is_active_skill_file, is_ignored_walk_dir},
};

/// A fully built skill artifact ready for persistence and vector indexing.
///
/// Multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) are WRITE-AHEAD source data from the
/// SKILL.md frontmatter, persisted to the `skills` table for T04/T05 consumption.
/// They do NOT affect the ℓ₁ embedding text or the subunit list.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltSkill {
    pub id: String,
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub source_path: PathBuf,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<ExtractedSubunit>,
    pub embedding: Vec<f32>,
    /// Task triggers where this skill applies. Empty for body-only skills.
    pub use_when: Vec<String>,
    /// Situations where this skill should NOT be applied. Empty for body-only skills.
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names the skill applies to. Empty for body-only skills.
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, models, or APIs. Empty for body-only skills.
    pub tools: Vec<String>,
    /// Verifier-critical constraints. Empty for body-only skills.
    pub invariants: Vec<String>,
    /// Prerequisites assumed by this skill. Empty for body-only skills.
    pub requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill. Empty for body-only skills.
    pub produces: Vec<String>,
}

#[derive(Debug, Error)]
pub enum GraphBuildError {
    #[error("cannot read skill file `{path}`: {message}")]
    ReadFailure { path: String, message: String },
    #[error("embedding failed: {message}")]
    Embedding { message: String },
}

/// Builds deterministic skill graph artifacts from active `SKILL.md` files in scope roots.
///
/// Uses the provided `embedding_service` to embed skill text in a single batch call,
/// preserving input order. Returns `Ok(Vec::new())` immediately when no skills are
/// discovered so callers never send an empty batch to the real embedder.
pub async fn build_skills_from_scope_roots(
    scope_roots: &[ScopeRoot],
    embedding_service: &dyn EmbeddingService,
) -> Result<Vec<BuiltSkill>, GraphBuildError> {
    // Collect the skills (with their embedding text) so a single batch call can embed
    // them all in one round-trip; the embedding field is filled in by index afterwards.
    let mut skills: Vec<BuiltSkill> = Vec::new();
    let mut texts_owned: Vec<String> = Vec::new();

    for scope in scope_roots {
        for entry in WalkDir::new(&scope.root)
            .into_iter()
            .filter_entry(|entry| !is_ignored_walk_dir(entry))
        {
            let entry = entry.map_err(|error| GraphBuildError::ReadFailure {
                path: scope.root.display().to_string(),
                message: error.to_string(),
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !is_active_skill_file(path) {
                continue;
            }
            if has_retired_marker(path) {
                continue;
            }
            let content =
                fs::read_to_string(path).map_err(|error| GraphBuildError::ReadFailure {
                    path: path.display().to_string(),
                    message: error.to_string(),
                })?;
            let extraction = extract_skill(path, &content);
            // Skill-level (ℓ₁) embedding text = the skill SUMMARY only
            // (name + description + tags). The body is deliberately NOT folded in
            // here: it is already represented at the subunit level (ℓ₀ / eq.3 β
            // term), and concatenating every subunit body produced multi-thousand-
            // token inputs that overflow the embedding model's single-batch limit
            // (nomic-embed-text via Ollama: 2048-token batch → HTTP 500). This now
            // matches the canonical read-path representation in `mcp-server` (the
            // boot-time snapshot that actually ranks the α term), keeping the
            // write-side Qdrant vector and the read-side snapshot vector consistent.
            let text_for_embedding = format!(
                "{} {} {}",
                extraction.skill_name,
                extraction.description,
                extraction.tags.join(" ")
            );
            let id = blake3::hash(path.display().to_string().as_bytes())
                .to_hex()
                .to_string();
            skills.push(BuiltSkill {
                id,
                scope_id: scope.scope_id.clone(),
                scope_type: scope.scope_type,
                source_path: path.to_path_buf(),
                name: extraction.skill_name,
                description: extraction.description,
                tags: extraction.tags,
                subunits: extraction.subunits,
                // Filled in by the single batch embed call below, by index.
                embedding: Vec::new(),
                use_when: extraction.use_when,
                avoid_when: extraction.avoid_when,
                artifacts: extraction.artifacts,
                tools: extraction.tools,
                invariants: extraction.invariants,
                requires: extraction.requires,
                produces: extraction.produces,
            });
            texts_owned.push(text_for_embedding);
        }
    }

    if skills.is_empty() {
        return Ok(Vec::new());
    }

    let texts: Vec<&str> = texts_owned.iter().map(String::as_str).collect();
    let embeddings = embedding_service
        .embed_batch(&texts)
        .await
        .map_err(|error| GraphBuildError::Embedding {
            message: error.to_string(),
        })?;

    if embeddings.len() != skills.len() {
        return Err(GraphBuildError::Embedding {
            message: format!(
                "embedding count mismatch: expected {}, got {}",
                skills.len(),
                embeddings.len()
            ),
        });
    }

    for (skill, embedding) in skills.iter_mut().zip(embeddings) {
        skill.embedding = embedding;
    }

    skills.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(skills)
}

fn has_retired_marker(active_skill_path: &Path) -> bool {
    active_skill_path
        .with_file_name("SKILL.md.retired")
        .exists()
}
