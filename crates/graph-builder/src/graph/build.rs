use std::{
    fs,
    path::{Path, PathBuf},
};

use domain::{EmbeddingService, ScopeRoot, ScopeType};
use thiserror::Error;
use walkdir::WalkDir;

use crate::{
    extraction::{ExtractedSubunit, extract_skill},
    watcher::is_active_skill_file,
};

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
    // Collect metadata and embedding text together so a single batch call can embed
    // all skills in one round-trip, preserving order for zip assembly.
    let mut metas: Vec<(
        String,   // stable id
        String,   // scope_id
        ScopeType,
        PathBuf,  // source_path
        String,   // name
        String,   // description
        Vec<String>, // tags
        Vec<ExtractedSubunit>,
    )> = Vec::new();
    let mut texts_owned: Vec<String> = Vec::new();

    for scope in scope_roots {
        for entry in WalkDir::new(&scope.root) {
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
            let text_for_embedding = format!(
                "{}\n{}\n{}",
                extraction.skill_name,
                extraction.description,
                extraction
                    .subunits
                    .iter()
                    .map(|subunit| subunit.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let id = blake3::hash(path.display().to_string().as_bytes())
                .to_hex()
                .to_string();
            metas.push((
                id,
                scope.scope_id.clone(),
                scope.scope_type,
                path.to_path_buf(),
                extraction.skill_name,
                extraction.description,
                extraction.tags,
                extraction.subunits,
            ));
            texts_owned.push(text_for_embedding);
        }
    }

    if metas.is_empty() {
        return Ok(Vec::new());
    }

    let texts: Vec<&str> = texts_owned.iter().map(String::as_str).collect();
    let embeddings = embedding_service
        .embed_batch(&texts)
        .await
        .map_err(|error| GraphBuildError::Embedding {
            message: error.to_string(),
        })?;

    if embeddings.len() != metas.len() {
        return Err(GraphBuildError::Embedding {
            message: format!(
                "embedding count mismatch: expected {}, got {}",
                metas.len(),
                embeddings.len()
            ),
        });
    }

    let mut skills: Vec<BuiltSkill> = metas
        .into_iter()
        .zip(embeddings)
        .map(
            |(
                (id, scope_id, scope_type, source_path, name, description, tags, subunits),
                embedding,
            )| BuiltSkill {
                id,
                scope_id,
                scope_type,
                source_path,
                name,
                description,
                tags,
                subunits,
                embedding,
            },
        )
        .collect();

    skills.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(skills)
}

fn has_retired_marker(active_skill_path: &Path) -> bool {
    active_skill_path
        .with_file_name("SKILL.md.retired")
        .exists()
}
