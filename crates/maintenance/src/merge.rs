use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{
    PENDING_SKILL_FILE_NAME, ScopeType, pending_default_expires_at, pending_default_warning_at,
};
use infrastructure::cosine_similarity as shared_cosine_similarity;
use serde::Serialize;
use thiserror::Error;

use crate::audit::{
    MaintenanceAuditEvent, MaintenanceAuditSink, MergeProposalAuditEvent,
};

/// Boundary projection for adapting external seeded-skill models into maintenance snapshots.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededSkillProjection {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_description: String,
    pub scope: ScopeType,
    pub source_paths: Vec<PathBuf>,
    pub tags: Vec<String>,
    pub subunit_contents: Vec<String>,
    pub embedding: Vec<f32>,
}

/// Flattened skill projection consumed by maintenance workflows.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillSnapshot {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope: ScopeType,
    pub source_path: PathBuf,
    pub tags: Vec<String>,
    pub subunits: Vec<String>,
    pub embedding: Vec<f32>,
}

impl SkillSnapshot {
    /// Creates a maintenance snapshot from an explicit seeded-skill boundary projection.
    pub fn from_seeded_skill_projection(projection: SeededSkillProjection) -> Self {
        let source_path = projection
            .source_paths
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("SKILL.md"));
        Self {
            id: projection.skill_id,
            name: projection.skill_name,
            description: projection.skill_description,
            scope: projection.scope,
            source_path,
            tags: projection.tags,
            subunits: projection.subunit_contents,
            embedding: projection.embedding,
        }
    }

    /// Builds a stable text block used in semantic merge verification.
    pub fn semantic_text(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.name,
            self.description,
            self.subunits.join("\n")
        )
    }
}

/// Candidate pair that passed similarity threshold and semantic verification.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeCandidate {
    pub left_skill_id: String,
    pub right_skill_id: String,
    pub cosine_similarity: f32,
}

/// Filesystem proposal output for a merge candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeProposal {
    pub pending_path: PathBuf,
    pub canonical_scope: ScopeType,
    pub merged_from_scopes: Vec<ScopeType>,
    pub merged_from_paths: Vec<PathBuf>,
}

/// Configures duplicate merge proposal generation.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeConfig {
    pub similarity_threshold: f32,
    pub pending_directory_name: String,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.85,
            pending_directory_name: ".skills".to_owned(),
        }
    }
}

/// Canonical scope policy for merged proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSelectionPolicy {
    PreferProjectThenGlobal,
}

impl ScopeSelectionPolicy {
    fn resolve(self, scopes: &[ScopeType]) -> ScopeType {
        match self {
            Self::PreferProjectThenGlobal => {
                if scopes.contains(&ScopeType::Project) {
                    ScopeType::Project
                } else {
                    ScopeType::Global
                }
            }
        }
    }
}

/// Integrates semantic validation before merge proposal emission.
///
/// Implementations must be async because the real gate is an LLM call.
/// Missing or unavailable LLM providers must surface `MergeError::SemanticVerification`
/// — never a silent `false` that quietly suppresses merges.
#[async_trait]
pub trait MergeSemanticVerifier: Send + Sync {
    async fn are_equivalent(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
    ) -> Result<bool, MergeError>;
}

/// Writes merged `.pending` artifacts without mutating active source skills.
pub struct MergeProposalWriter<'s, V, S>
where
    V: MergeSemanticVerifier,
    S: MaintenanceAuditSink,
{
    config: MergeConfig,
    scope_policy: ScopeSelectionPolicy,
    semantic_verifier: V,
    audit_sink: &'s S,
}

impl<'s, V, S> MergeProposalWriter<'s, V, S>
where
    V: MergeSemanticVerifier,
    S: MaintenanceAuditSink,
{
    /// Creates a writer with an explicit merge audit sink.
    pub fn with_audit_sink(config: MergeConfig, semantic_verifier: V, audit_sink: &'s S) -> Self {
        Self {
            config,
            scope_policy: ScopeSelectionPolicy::PreferProjectThenGlobal,
            semantic_verifier,
            audit_sink,
        }
    }

    /// Proposes merged `.pending` files for cross-scope duplicate skill pairs.
    pub async fn propose(
        &self,
        skills: &[SkillSnapshot],
        now: DateTime<Utc>,
    ) -> Result<Vec<MergeProposal>, MergeError> {
        let candidates = self.find_candidates(skills).await?;
        let skills_by_id = skills
            .iter()
            .map(|skill| (skill.id.as_str(), skill))
            .collect::<HashMap<_, _>>();
        let mut proposals = Vec::new();
        for candidate in &candidates {
            let left = skills_by_id
                .get(candidate.left_skill_id.as_str())
                .copied()
                .ok_or_else(|| MergeError::SkillNotFound(candidate.left_skill_id.clone()))?;
            let right = skills_by_id
                .get(candidate.right_skill_id.as_str())
                .copied()
                .ok_or_else(|| MergeError::SkillNotFound(candidate.right_skill_id.clone()))?;
            proposals.push(self.write_proposal(left, right, candidate.cosine_similarity, now)?);
        }
        Ok(proposals)
    }

    /// Finds cross-scope candidates that satisfy cosine similarity and semantic equivalence.
    ///
    /// Cosine ≥ `similarity_threshold` selects the candidate set; the semantic verifier
    /// (LLM-backed) makes the final equivalence call on each surviving pair.
    pub async fn find_candidates(
        &self,
        skills: &[SkillSnapshot],
    ) -> Result<Vec<MergeCandidate>, MergeError> {
        let mut candidates = Vec::new();
        for left in skills {
            for right in skills {
                if left.id >= right.id {
                    continue;
                }
                if left.scope == right.scope {
                    continue;
                }
                let similarity = cosine_similarity(&left.embedding, &right.embedding)?;
                if similarity < self.config.similarity_threshold {
                    continue;
                }
                if !self.semantic_verifier.are_equivalent(left, right).await? {
                    continue;
                }
                candidates.push(MergeCandidate {
                    left_skill_id: left.id.clone(),
                    right_skill_id: right.id.clone(),
                    cosine_similarity: similarity,
                });
            }
        }
        Ok(candidates)
    }

    fn write_proposal(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
        similarity: f32,
        now: DateTime<Utc>,
    ) -> Result<MergeProposal, MergeError> {
        let merged_scopes = vec![left.scope, right.scope];
        let canonical_scope = self.scope_policy.resolve(&merged_scopes);
        let target_root = output_root_for_scope(canonical_scope, left, right)?;
        let canonical_scope_root = canonicalize_scope_root(&target_root)?;
        let pending_directory_name =
            validate_pending_directory_name(&self.config.pending_directory_name)?;
        let pending_root = canonical_scope_root.join(pending_directory_name);
        fs::create_dir_all(&pending_root).map_err(|error| MergeError::WriteFailure {
            path: pending_root.display().to_string(),
            message: error.to_string(),
        })?;
        let canonical_pending_root =
            pending_root
                .canonicalize()
                .map_err(|error| MergeError::WriteFailure {
                    path: pending_root.display().to_string(),
                    message: error.to_string(),
                })?;
        ensure_path_is_within_scope_root(&canonical_pending_root, &canonical_scope_root)?;

        let proposal_root = canonical_pending_root.join(proposal_directory_name(left, right));
        fs::create_dir_all(&proposal_root).map_err(|error| MergeError::WriteFailure {
            path: proposal_root.display().to_string(),
            message: error.to_string(),
        })?;
        let pending_path = proposal_root.join(PENDING_SKILL_FILE_NAME);
        let pending_body = render_pending_markdown(left, right, canonical_scope, similarity, now)?;
        let mut pending_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending_path)
            .map_err(|error| MergeError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;
        pending_file
            .write_all(pending_body.as_bytes())
            .map_err(|error| MergeError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;

        let proposal = MergeProposal {
            pending_path,
            canonical_scope,
            merged_from_scopes: sorted_unique_scopes(&merged_scopes),
            merged_from_paths: vec![left.source_path.clone(), right.source_path.clone()],
        };
        self.emit_merge_proposal_audit(left, right, similarity, now, &proposal)?;
        Ok(proposal)
    }

    fn emit_merge_proposal_audit(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
        similarity: f32,
        now: DateTime<Utc>,
        proposal: &MergeProposal,
    ) -> Result<(), MergeError> {
        let mut merged_from_skill_ids = vec![left.id.clone(), right.id.clone()];
        merged_from_skill_ids.sort_unstable();
        let correlation_id = format!(
            "maintenance.merge_proposal:{}:{}",
            merged_from_skill_ids[0], merged_from_skill_ids[1]
        );
        let audit_event = MaintenanceAuditEvent::MergeProposalWritten(MergeProposalAuditEvent {
            correlation_id,
            happened_at: now,
            proposal_path: proposal.pending_path.clone(),
            canonical_scope: proposal.canonical_scope,
            merged_from_skill_ids,
            merged_from_scopes: proposal.merged_from_scopes.clone(),
            merged_from_paths: proposal.merged_from_paths.clone(),
            similarity,
        });
        self.audit_sink
            .emit(audit_event)
            .map_err(|error| MergeError::AuditEmissionFailure(error.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("embedding dimension mismatch: left={left_dimension}, right={right_dimension}")]
    EmbeddingDimensionMismatch {
        left_dimension: usize,
        right_dimension: usize,
    },
    #[error("cannot compare zero-magnitude embedding vectors")]
    ZeroMagnitudeEmbedding,
    #[error("semantic verification failed: {0}")]
    SemanticVerification(String),
    #[error("skill lookup failed for `{0}`")]
    SkillNotFound(String),
    #[error("cannot resolve output root for merged proposal")]
    OutputRootResolution,
    #[error("invalid pending directory name `{0}`")]
    InvalidPendingDirectoryName(String),
    #[error("scope root `{path}` is invalid: {message}")]
    InvalidScopeRoot { path: String, message: String },
    #[error("write path `{path}` resolves outside scope root `{scope_root}`")]
    WritePathOutsideScopeRoot { path: String, scope_root: String },
    #[error("failed to serialize merge proposal frontmatter: {0}")]
    FrontmatterSerialization(String),
    #[error("failed writing merge proposal `{path}`: {message}")]
    WriteFailure { path: String, message: String },
    #[error("failed emitting merge proposal audit event: {0}")]
    AuditEmissionFailure(String),
}

impl MergeError {
    /// Maps merge workflow failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::EmbeddingDimensionMismatch { .. } => "merge_embedding_dimension_mismatch",
            Self::ZeroMagnitudeEmbedding => "merge_zero_magnitude_embedding",
            Self::SemanticVerification(_) => "merge_semantic_verification_failed",
            Self::SkillNotFound(_) => "merge_skill_not_found",
            Self::OutputRootResolution => "merge_output_root_resolution_failed",
            Self::InvalidPendingDirectoryName(_) => "merge_invalid_pending_directory_name",
            Self::InvalidScopeRoot { .. } => "merge_invalid_scope_root",
            Self::WritePathOutsideScopeRoot { .. } => "merge_write_path_outside_scope_root",
            Self::FrontmatterSerialization(_) => "merge_frontmatter_serialization_failed",
            Self::WriteFailure { .. } => "merge_pending_write_failed",
            Self::AuditEmissionFailure(_) => "merge_audit_emission_failed",
        }
    }
}

/// Canonicalizes scope roots before any filesystem writes.
pub(crate) fn canonicalize_scope_root(scope_root: &Path) -> Result<PathBuf, MergeError> {
    if !scope_root.is_absolute() {
        return Err(MergeError::InvalidScopeRoot {
            path: scope_root.display().to_string(),
            message: "scope root must be absolute".to_owned(),
        });
    }
    let canonical_scope_root =
        scope_root
            .canonicalize()
            .map_err(|error| MergeError::InvalidScopeRoot {
                path: scope_root.display().to_string(),
                message: error.to_string(),
            })?;
    if !canonical_scope_root.is_dir() {
        return Err(MergeError::InvalidScopeRoot {
            path: canonical_scope_root.display().to_string(),
            message: "scope root must resolve to a directory".to_owned(),
        });
    }
    Ok(canonical_scope_root)
}

/// Restricts pending directory configuration to a single normal path component.
pub(crate) fn validate_pending_directory_name(pending_directory_name: &str) -> Result<&str, MergeError> {
    if pending_directory_name.is_empty() {
        return Err(MergeError::InvalidPendingDirectoryName(
            pending_directory_name.to_owned(),
        ));
    }
    let pending_directory_path = Path::new(pending_directory_name);
    let mut components = pending_directory_path.components();
    let Some(first_component) = components.next() else {
        return Err(MergeError::InvalidPendingDirectoryName(
            pending_directory_name.to_owned(),
        ));
    };
    if !matches!(first_component, Component::Normal(_)) || components.next().is_some() {
        return Err(MergeError::InvalidPendingDirectoryName(
            pending_directory_name.to_owned(),
        ));
    }
    Ok(pending_directory_name)
}

/// Enforces write boundaries for pending proposal output.
pub(crate) fn ensure_path_is_within_scope_root(
    candidate_path: &Path,
    canonical_scope_root: &Path,
) -> Result<(), MergeError> {
    if candidate_path.starts_with(canonical_scope_root) {
        return Ok(());
    }
    Err(MergeError::WritePathOutsideScopeRoot {
        path: candidate_path.display().to_string(),
        scope_root: canonical_scope_root.display().to_string(),
    })
}

/// Delegates to the shared [`infrastructure::cosine_similarity`] and maps the
/// error to the local [`MergeError`] variants. This is the single cosine
/// implementation in the repo; the logic lives in `infrastructure::similarity`.
fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32, MergeError> {
    shared_cosine_similarity(left, right).map_err(|error| {
        use infrastructure::CosineSimilarityError;
        match error {
            CosineSimilarityError::DimensionMismatch {
                left_dimension,
                right_dimension,
            } => MergeError::EmbeddingDimensionMismatch {
                left_dimension,
                right_dimension,
            },
            CosineSimilarityError::ZeroMagnitude => MergeError::ZeroMagnitudeEmbedding,
        }
    })
}

fn output_root_for_scope(
    canonical_scope: ScopeType,
    left: &SkillSnapshot,
    right: &SkillSnapshot,
) -> Result<PathBuf, MergeError> {
    let preferred = [left, right]
        .into_iter()
        .find(|skill| skill.scope == canonical_scope);
    preferred
        .and_then(|skill| skill.source_path.parent().map(Path::to_path_buf))
        .ok_or(MergeError::OutputRootResolution)
}

fn render_pending_markdown(
    left: &SkillSnapshot,
    right: &SkillSnapshot,
    canonical_scope: ScopeType,
    similarity: f32,
    now: DateTime<Utc>,
) -> Result<String, MergeError> {
    let merged_name = format!("{} + {}", left.name, right.name);
    let merged_description = format!("{}\n{}", left.description, right.description);
    let mut merged_tags = BTreeSet::new();
    for tag in left.tags.iter().chain(right.tags.iter()) {
        merged_tags.insert(tag.clone());
    }
    let merged_subunits = left
        .subunits
        .iter()
        .chain(right.subunits.iter())
        .cloned()
        .collect::<Vec<_>>();
    let merged_scope_names = sorted_unique_scopes(&[left.scope, right.scope])
        .iter()
        .map(scope_to_string)
        .collect::<Vec<_>>();
    let frontmatter = MergeProposalFrontmatter {
        name: &merged_name,
        description: &merged_description,
        origin: "merge_proposal",
        canonical_scope: scope_to_string(&canonical_scope),
        merged_from_scopes: merged_scope_names,
        merged_from: vec![
            left.source_path.display().to_string(),
            right.source_path.display().to_string(),
        ],
        similarity,
        created_at: now.to_rfc3339(),
        warning_at: pending_default_warning_at(now).to_rfc3339(),
        expires_at: pending_default_expires_at(now).to_rfc3339(),
    };
    let frontmatter_yaml = serialize_frontmatter(&frontmatter)?;

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&frontmatter_yaml);
    if !frontmatter_yaml.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("---\n\n");
    body.push_str(&format!("# {merged_name}\n\n"));
    body.push_str(&merged_description);
    body.push_str("\n\n");
    if !merged_tags.is_empty() {
        body.push_str(&format!(
            "tags: {}\n\n",
            merged_tags.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    body.push_str("## Procedures\n");
    for subunit in merged_subunits {
        body.push_str(&format!("- {subunit}\n"));
    }
    Ok(body)
}

/// Frontmatter payload stored in merge proposal markdown.
#[derive(Serialize)]
struct MergeProposalFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    origin: &'a str,
    canonical_scope: &'a str,
    merged_from_scopes: Vec<&'a str>,
    merged_from: Vec<String>,
    similarity: f32,
    created_at: String,
    warning_at: String,
    expires_at: String,
}

fn serialize_frontmatter(frontmatter: &MergeProposalFrontmatter<'_>) -> Result<String, MergeError> {
    let serialized = serde_yaml::to_string(frontmatter)
        .map_err(|error| MergeError::FrontmatterSerialization(error.to_string()))?;
    Ok(serialized
        .strip_prefix("---\n")
        .unwrap_or(&serialized)
        .to_owned())
}

fn sorted_unique_scopes(scopes: &[ScopeType]) -> Vec<ScopeType> {
    let mut has_project = false;
    let mut has_global = false;
    let mut has_team = false;
    for scope in scopes {
        match scope {
            ScopeType::Project => has_project = true,
            ScopeType::Global => has_global = true,
            ScopeType::Team => has_team = true,
        }
    }
    let mut ordered = Vec::new();
    if has_project {
        ordered.push(ScopeType::Project);
    }
    if has_global {
        ordered.push(ScopeType::Global);
    }
    if has_team {
        ordered.push(ScopeType::Team);
    }
    ordered
}

/// Builds a deterministic proposal directory name that preserves source ID traceability.
fn proposal_directory_name(left: &SkillSnapshot, right: &SkillSnapshot) -> String {
    let merged_name_slug = slugify(&format!("{}-{}", left.name, right.name));
    let left_id_slug = slugify(&left.id);
    let right_id_slug = slugify(&right.id);
    format!("{merged_name_slug}--{left_id_slug}--{right_id_slug}")
}

fn slugify(value: &str) -> String {
    let normalized = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = normalized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "merged-skill".to_owned()
    } else {
        slug
    }
}

fn scope_to_string(scope: &ScopeType) -> &'static str {
    match scope {
        ScopeType::Project => "project",
        ScopeType::Global => "global",
        ScopeType::Team => "team",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_projection_conversion_uses_first_source_path() {
        let snapshot = SkillSnapshot::from_seeded_skill_projection(SeededSkillProjection {
            skill_id: "skill-auth".to_owned(),
            skill_name: "auth".to_owned(),
            skill_description: "auth flow".to_owned(),
            scope: ScopeType::Project,
            source_paths: vec![PathBuf::from("/workspace/project/auth/SKILL.md")],
            tags: vec!["rust".to_owned()],
            subunit_contents: vec!["step one".to_owned()],
            embedding: vec![1.0, 0.0],
        });

        assert_eq!(snapshot.id, "skill-auth");
        assert_eq!(
            snapshot.source_path,
            PathBuf::from("/workspace/project/auth/SKILL.md")
        );
    }

    #[test]
    fn seeded_projection_conversion_falls_back_to_default_source_path() {
        let snapshot = SkillSnapshot::from_seeded_skill_projection(SeededSkillProjection {
            skill_id: "skill-auth".to_owned(),
            skill_name: "auth".to_owned(),
            skill_description: "auth flow".to_owned(),
            scope: ScopeType::Project,
            source_paths: Vec::new(),
            tags: vec!["rust".to_owned()],
            subunit_contents: vec!["step one".to_owned()],
            embedding: vec![1.0, 0.0],
        });

        assert_eq!(snapshot.source_path, PathBuf::from("SKILL.md"));
    }

    #[test]
    fn cosine_similarity_rejects_mismatched_dimensions() {
        let result = cosine_similarity(&[1.0, 2.0], &[1.0]);
        assert!(matches!(
            result,
            Err(MergeError::EmbeddingDimensionMismatch {
                left_dimension: 2,
                right_dimension: 1,
            })
        ));
    }
}
