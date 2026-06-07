use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{
    EmbeddingService, PENDING_SKILL_FILE_NAME, ScopeType, pending_default_expires_at,
    pending_default_warning_at,
};
use infrastructure::cosine_similarity as shared_cosine_similarity;
use serde::Serialize;
use thiserror::Error;

use crate::audit::{MaintenanceAuditEvent, MaintenanceAuditSink, MergeProposalAuditEvent};

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
    /// Cosine similarity gate applied to the body-inclusive merge vector.
    ///
    /// This is the MERGE-ONLY dedup signal — distinct from the retrieval ℓ₁ summary-only
    /// embedding used in graph-builder. The body-inclusive vector covers name, description,
    /// tags, and a bounded subunit digest so that paraphrased-summary / shared-procedure
    /// duplicate pairs (which the summary-only vector misses) are caught here. The default
    /// 0.58 clears the real-world duplicate floor (~0.69–0.81) while staying above the
    /// non-duplicate control ceiling (~0.39–0.47); the LLM verifier remains the precision gate.
    pub merge_candidate_threshold: f32,
    pub pending_directory_name: String,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            merge_candidate_threshold: 0.58,
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
    /// Embedding service used to compute body-inclusive merge candidate vectors.
    ///
    /// This is merge's OWN dedup signal — distinct from the summary-only retrieval ℓ₁
    /// vector. It embeds name + description + tags + a bounded subunit digest so that
    /// shared-procedure / divergent-summary pairs are detected here without polluting
    /// the retrieval graph's ℓ₁ vectors.
    candidate_embedder: Arc<dyn EmbeddingService>,
}

impl<'s, V, S> MergeProposalWriter<'s, V, S>
where
    V: MergeSemanticVerifier,
    S: MaintenanceAuditSink,
{
    /// Creates a writer with an explicit merge audit sink and candidate embedder.
    ///
    /// `candidate_embedder` is used exclusively to compute body-inclusive merge vectors
    /// for duplicate candidate detection. It must NOT be the same logical use as the
    /// retrieval ℓ₁ embedding (which is summary-only and lives in graph-builder).
    pub fn with_audit_sink(
        config: MergeConfig,
        semantic_verifier: V,
        audit_sink: &'s S,
        candidate_embedder: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            config,
            scope_policy: ScopeSelectionPolicy::PreferProjectThenGlobal,
            semantic_verifier,
            audit_sink,
            candidate_embedder,
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
    /// Body-inclusive merge vectors (name + description + tags + bounded subunit digest)
    /// are pre-computed once per skill and used for cosine comparison against
    /// `merge_candidate_threshold`. This catches shared-procedure / divergent-summary
    /// pairs that the summary-only retrieval ℓ₁ vector would miss.
    ///
    /// The LLM semantic verifier remains the final precision gate on every candidate
    /// that clears the cosine threshold.
    pub async fn find_candidates(
        &self,
        skills: &[SkillSnapshot],
    ) -> Result<Vec<MergeCandidate>, MergeError> {
        // Pre-compute body-inclusive merge vectors once per skill to avoid redundant
        // embedding calls in the O(n²) pair loop below.
        let mut merge_vectors: HashMap<&str, Vec<f32>> = HashMap::new();
        for skill in skills {
            let merge_text = body_inclusive_merge_text(skill);
            let vector = self
                .candidate_embedder
                .embed_text(&merge_text)
                .await
                .map_err(|error| MergeError::CandidateEmbedding(error.to_string()))?;
            merge_vectors.insert(skill.id.as_str(), vector);
        }

        let mut candidates = Vec::new();
        for left in skills {
            for right in skills {
                if left.id >= right.id {
                    continue;
                }
                if left.scope == right.scope {
                    continue;
                }
                let left_vector = merge_vectors
                    .get(left.id.as_str())
                    .expect("pre-computed merge vector must exist for every skill in the input slice");
                let right_vector = merge_vectors
                    .get(right.id.as_str())
                    .expect("pre-computed merge vector must exist for every skill in the input slice");
                let similarity = cosine_similarity(left_vector, right_vector)?;
                if similarity < self.config.merge_candidate_threshold {
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
    /// Body-inclusive merge vector embedding failed for a skill.
    ///
    /// This is a hard failure — there is no silent fallback to the ℓ₁ summary vector.
    /// The embedding provider must be available and healthy for the merge pass to run.
    #[error("candidate embedding failed: {0}")]
    CandidateEmbedding(String),
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
            Self::CandidateEmbedding(_) => "merge_candidate_embedding_failed",
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

/// Builds the body-inclusive text used for merge candidate vector computation.
///
/// Includes name, description, tags, and a bounded subunit digest. The digest is bounded
/// to respect nomic-embed-text's 2048-token single-batch limit (the reason the retrieval
/// ℓ₁ embedding excludes the body). Each subunit is capped to its first ~100 whitespace
/// tokens; the total digest is capped to ~1500 whitespace tokens. Truncation is silent —
/// the signal degrades gracefully for very large skills rather than erroring.
///
/// This text must NOT be used for the retrieval ℓ₁ summary vector in graph-builder;
/// it is merge's own dedup signal, separated to catch shared-procedure / divergent-summary
/// duplicate pairs without polluting retrieval quality.
fn body_inclusive_merge_text(skill: &SkillSnapshot) -> String {
    const MAX_TOKENS_PER_SUBUNIT: usize = 100;
    const MAX_TOTAL_DIGEST_TOKENS: usize = 1500;

    let tags_joined = skill.tags.join(" ");
    let header = format!("{} {} {}\n", skill.name, skill.description, tags_joined);

    let mut digest_tokens_used: usize = 0;
    let mut digest_parts: Vec<String> = Vec::new();

    'outer: for subunit in &skill.subunits {
        // Split on whitespace to approximate token count (conservative but safe).
        let words: Vec<&str> = subunit.split_whitespace().collect();
        let capped_words = &words[..words.len().min(MAX_TOKENS_PER_SUBUNIT)];
        let word_count = capped_words.len();

        if digest_tokens_used + word_count > MAX_TOTAL_DIGEST_TOKENS {
            // Only include as many words as fit within the total cap.
            let remaining = MAX_TOTAL_DIGEST_TOKENS - digest_tokens_used;
            if remaining == 0 {
                break 'outer;
            }
            let partial_words = &capped_words[..remaining];
            digest_parts.push(partial_words.join(" "));
            break 'outer;
        }

        digest_parts.push(capped_words.join(" "));
        digest_tokens_used += word_count;
    }

    format!("{}{}", header, digest_parts.join("\n"))
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
pub(crate) fn validate_pending_directory_name(
    pending_directory_name: &str,
) -> Result<&str, MergeError> {
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
    let merged_tags: Vec<String> = merged_tags.into_iter().collect();
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
        tags: merged_tags.clone(),
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
    // Tags live in the frontmatter (single source of truth); the body carries
    // only the title, description, and subunit sections.
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
    /// Canonical merged tag list (single source of truth). The markdown body no
    /// longer carries a `tags:` line — the graph-builder reader reads tags here.
    tags: Vec<String>,
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

    /// Proves that body-inclusive merge vectors catch a shared-body / divergent-summary
    /// pair that the old summary-only cosine at 0.85 would have missed.
    ///
    /// The two skills have deliberately DIFFERENT description summaries (simulating
    /// paraphrased wording) but near-identical procedure bodies — the exact scenario
    /// the body-inclusive merge vector is designed to catch. The deterministic embedder
    /// produces hash-based cosine well above the 0.58 threshold for high shared-token
    /// overlap in the body, proving a candidate IS produced.
    ///
    /// For contrast, the OLD summary-only ℓ₁ cosine at threshold 0.85 would NOT have
    /// produced a candidate — the divergent summaries would have cosine below 0.85.
    #[tokio::test]
    async fn body_inclusive_merge_vector_catches_shared_body_divergent_summary_pair() {
        use std::sync::Arc;

        // This simulates a skill fixture sandbox: source paths under temp dir.
        let sandbox = std::env::temp_dir().join(format!(
            "merge_body_inclusive_test_{}",
            std::process::id()
        ));
        let project_path = sandbox.join("project/auth/SKILL.md");
        let global_path = sandbox.join("global/auth/SKILL.md");
        std::fs::create_dir_all(project_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(global_path.parent().unwrap()).unwrap();
        std::fs::write(&project_path, b"placeholder").unwrap();
        std::fs::write(&global_path, b"placeholder").unwrap();

        // Divergent summaries, near-identical procedure bodies.
        let project_skill = SkillSnapshot {
            id: "a-project-auth".to_owned(),
            name: "Rust JWT Authentication".to_owned(),
            description: "Validates bearer tokens and enforces authorization policies".to_owned(),
            scope: ScopeType::Project,
            source_path: project_path,
            tags: vec!["rust".to_owned(), "auth".to_owned(), "jwt".to_owned()],
            subunits: vec![
                "Validate JWT tokens using the jsonwebtoken crate".to_owned(),
                "Check scope permissions against the required claims set".to_owned(),
                "Renew short-lived access tokens before expiry using refresh flow".to_owned(),
            ],
            embedding: vec![1.0, 0.0, 0.0], // ℓ₁ summary-only — NOT used by merge
        };
        let global_skill = SkillSnapshot {
            id: "b-global-auth".to_owned(),
            name: "Distributed Auth Patterns".to_owned(),
            description: "Common authentication patterns for microservice architectures".to_owned(),
            scope: ScopeType::Global,
            source_path: global_path,
            tags: vec!["rust".to_owned(), "auth".to_owned(), "jwt".to_owned()],
            subunits: vec![
                "Validate JWT tokens using the jsonwebtoken crate".to_owned(),
                "Check scope permissions against the required claims set".to_owned(),
                "Renew short-lived access tokens before expiry using refresh flow".to_owned(),
            ],
            embedding: vec![0.0, 0.0, 1.0], // ℓ₁ summary-only — NOT used by merge
        };

        // Verify: old summary-only cosine at 0.85 would NOT catch this pair.
        // (divergent summaries → low cosine)
        let old_summary_cosine =
            cosine_similarity(&project_skill.embedding, &global_skill.embedding)
                .expect("cosine must succeed");
        assert!(
            old_summary_cosine < 0.10,
            "old summary-only cosine should be low for divergent-summary skills, got {old_summary_cosine}"
        );

        // Verify: body-inclusive merge vector at 0.58 DOES catch this pair.
        let embedder: Arc<dyn domain::EmbeddingService> = Arc::new(
            graph_builder::graph::embeddings::DeterministicEmbeddingService,
        );

        // AlwaysEquivalentVerifier (test-only) ensures the pipeline reaches proposal output.
        #[derive(Clone)]
        struct AlwaysEquivalentVerifier;

        #[async_trait::async_trait]
        impl MergeSemanticVerifier for AlwaysEquivalentVerifier {
            async fn are_equivalent(
                &self,
                _left: &SkillSnapshot,
                _right: &SkillSnapshot,
            ) -> Result<bool, MergeError> {
                Ok(true)
            }
        }

        let writer = MergeProposalWriter::with_audit_sink(
            MergeConfig::default(), // merge_candidate_threshold = 0.58
            AlwaysEquivalentVerifier,
            &crate::audit::NoopMaintenanceAuditSink,
            embedder,
        );

        let candidates = writer
            .find_candidates(&[project_skill, global_skill])
            .await
            .expect("find_candidates must succeed with deterministic embedder");

        assert_eq!(
            candidates.len(),
            1,
            "body-inclusive merge vector must catch the shared-body/divergent-summary pair; \
             got {} candidates",
            candidates.len()
        );
        assert!(
            candidates[0].cosine_similarity >= 0.58,
            "body-inclusive cosine must clear the merge_candidate_threshold 0.58, \
             got {}",
            candidates[0].cosine_similarity
        );

        let _ = std::fs::remove_dir_all(&sandbox);
    }

    /// Live nomic-embed-text regression: over 6 labeled duplicate pairs, the body-inclusive
    /// merge vector at threshold 0.58 must achieve recall ≥ 5/6, and the 2 control
    /// (non-duplicate) pairs must NOT be proposed.
    ///
    /// Requires a live Ollama instance at `OLLAMA_URL` with the `nomic-embed-text` model.
    /// Run with: `cargo test -p maintenance --features test-utils -- --ignored nomic_embed_text_body_inclusive_recall`
    #[tokio::test]
    #[ignore = "requires live Ollama instance with nomic-embed-text at OLLAMA_URL"]
    async fn nomic_embed_text_body_inclusive_recall_over_labeled_duplicate_pairs() {
        use std::sync::Arc;

        let ollama_url = std::env::var("OLLAMA_URL")
            .expect("OLLAMA_URL must be set to run this live regression test");

        let embedder: Arc<dyn domain::EmbeddingService> = {
            let config = infrastructure::OllamaEmbeddingConfig {
                base_url: ollama_url,
                model: "nomic-embed-text".to_owned(),
                max_concurrency: 4,
            };
            Arc::new(
                infrastructure::OllamaEmbeddingService::from_config(config)
                    .expect("OllamaEmbeddingService must initialize from OLLAMA_URL"),
            )
        };

        let sandbox = std::env::temp_dir().join(format!(
            "merge_nomic_regression_{}",
            std::process::id()
        ));

        // Helper that creates a SkillSnapshot with a source file on disk.
        let make_skill = |id: &str,
                          name: &str,
                          description: &str,
                          tags: &[&str],
                          subunits: &[&str],
                          scope: ScopeType,
                          scope_dir: &str| {
            let path = sandbox.join(format!("{scope_dir}/{id}/SKILL.md"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("# {name}\n\n{description}")).unwrap();
            SkillSnapshot {
                id: id.to_owned(),
                name: name.to_owned(),
                description: description.to_owned(),
                scope,
                source_path: path,
                tags: tags.iter().map(|s| s.to_string()).collect(),
                subunits: subunits.iter().map(|s| s.to_string()).collect(),
                embedding: vec![0.0],
            }
        };

        // 6 TRUE duplicate pairs: same procedure, divergent summary wording.
        let duplicate_pairs: Vec<(SkillSnapshot, SkillSnapshot)> = vec![
            (
                make_skill(
                    "a-jwt-project",
                    "JWT Authentication Flow",
                    "Validates bearer tokens and enforces authorization policies in Rust services",
                    &["rust", "auth", "jwt"],
                    &[
                        "Validate JWT tokens using the jsonwebtoken crate",
                        "Check scope permissions against required claims",
                        "Renew short-lived tokens before expiry using refresh flow",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "b-jwt-global",
                    "Distributed JWT Patterns",
                    "Common JWT authentication patterns for microservice architectures",
                    &["rust", "auth", "jwt"],
                    &[
                        "Validate JWT tokens using the jsonwebtoken crate",
                        "Check scope permissions against required claims",
                        "Renew short-lived tokens before expiry using refresh flow",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "c-pg-project",
                    "Postgres Connection Pool Setup",
                    "Initializes a sqlx connection pool with retry backoff for Postgres",
                    &["rust", "postgres", "sqlx"],
                    &[
                        "Create PgPoolOptions with max_connections from config",
                        "Connect to DATABASE_URL with connect_lazy for deferred validation",
                        "Run sqlx::migrate!() at boot to apply pending migrations",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "d-pg-global",
                    "Sqlx Postgres Database Wiring",
                    "Wires sqlx PgPool across service boundaries with migration guard",
                    &["rust", "postgres", "sqlx"],
                    &[
                        "Create PgPoolOptions with max_connections from config",
                        "Connect to DATABASE_URL with connect_lazy for deferred validation",
                        "Run sqlx::migrate!() at boot to apply pending migrations",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "e-tracing-project",
                    "Structured Tracing Setup",
                    "Configures tokio-tracing with JSON output for production Rust services",
                    &["rust", "tracing", "observability"],
                    &[
                        "Initialize tracing_subscriber with EnvFilter from RUST_LOG",
                        "Use fmt::json layer for structured log output in production",
                        "Propagate span context across async boundaries with instrument",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "f-tracing-global",
                    "Observability Initialization Pattern",
                    "Service-level observability bootstrap using tracing ecosystem",
                    &["rust", "tracing", "observability"],
                    &[
                        "Initialize tracing_subscriber with EnvFilter from RUST_LOG",
                        "Use fmt::json layer for structured log output in production",
                        "Propagate span context across async boundaries with instrument",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "g-error-project",
                    "Error Handling with thiserror",
                    "Defines domain error enums using thiserror derive macros",
                    &["rust", "errors", "thiserror"],
                    &[
                        "Define error enum with #[derive(Debug, Error)]",
                        "Use #[error(...)] attributes for Display messages",
                        "Implement From conversions for underlying error types",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "h-error-global",
                    "Idiomatic Rust Error Types",
                    "Establishes a consistent error taxonomy across Rust crates",
                    &["rust", "errors", "thiserror"],
                    &[
                        "Define error enum with #[derive(Debug, Error)]",
                        "Use #[error(...)] attributes for Display messages",
                        "Implement From conversions for underlying error types",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "i-async-project",
                    "Tokio Async Runtime Configuration",
                    "Sets up a multi-threaded tokio runtime for production services",
                    &["rust", "tokio", "async"],
                    &[
                        "Annotate main with #[tokio::main(flavor = \"multi_thread\")]",
                        "Set worker_threads count from TOKIO_WORKER_THREADS env var",
                        "Use tokio::spawn for independent concurrent tasks",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "j-async-global",
                    "Multi-threaded Async Runtime Bootstrap",
                    "Bootstrap pattern for production-grade tokio async applications",
                    &["rust", "tokio", "async"],
                    &[
                        "Annotate main with #[tokio::main(flavor = \"multi_thread\")]",
                        "Set worker_threads count from TOKIO_WORKER_THREADS env var",
                        "Use tokio::spawn for independent concurrent tasks",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "k-serde-project",
                    "JSON Serialization with Serde",
                    "Derives Serialize and Deserialize for Rust domain structs",
                    &["rust", "serde", "json"],
                    &[
                        "Add #[derive(Serialize, Deserialize)] to domain structs",
                        "Use #[serde(rename_all = \"camelCase\")] for API compatibility",
                        "Handle Option fields with #[serde(skip_serializing_if = \"Option::is_none\")]",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "l-serde-global",
                    "Domain Struct Serialization Patterns",
                    "Establishes consistent serde usage across the codebase",
                    &["rust", "serde", "json"],
                    &[
                        "Add #[derive(Serialize, Deserialize)] to domain structs",
                        "Use #[serde(rename_all = \"camelCase\")] for API compatibility",
                        "Handle Option fields with #[serde(skip_serializing_if = \"Option::is_none\")]",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
        ];

        // 2 control pairs: genuinely different skills that must NOT be proposed.
        let control_pairs: Vec<(SkillSnapshot, SkillSnapshot)> = vec![
            (
                make_skill(
                    "m-http-project",
                    "HTTP Client Patterns",
                    "Uses reqwest to make typed HTTP requests with timeout handling",
                    &["rust", "http", "reqwest"],
                    &[
                        "Build reqwest::Client with timeout from environment",
                        "Use .json() for typed request/response deserialization",
                        "Retry on 5xx with exponential backoff using tower",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "n-metrics-global",
                    "Prometheus Metrics Export",
                    "Exports Prometheus metrics via axum /metrics endpoint",
                    &["rust", "prometheus", "metrics"],
                    &[
                        "Register Counter and Histogram with prometheus::register_*",
                        "Mount /metrics handler returning TextEncoder output",
                        "Label metrics with service name and version from env",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
            (
                make_skill(
                    "o-cli-project",
                    "Clap CLI Argument Parsing",
                    "Defines structured CLI arguments using clap derive macros",
                    &["rust", "cli", "clap"],
                    &[
                        "Define Args struct with #[derive(Parser)]",
                        "Add --config flag for path to config file",
                        "Validate required arguments at parse time with value_parser",
                    ],
                    ScopeType::Project,
                    "project",
                ),
                make_skill(
                    "p-docker-global",
                    "Docker Multi-stage Build Pattern",
                    "Produces minimal production Docker images using multi-stage builds",
                    &["docker", "build", "deployment"],
                    &[
                        "Use FROM rust:bookworm AS builder for the compile stage",
                        "Install musl-tools for static linking with target x86_64-unknown-linux-musl",
                        "Copy only the binary to FROM scratch or distroless final image",
                    ],
                    ScopeType::Global,
                    "global",
                ),
            ),
        ];

        // AlwaysEquivalentVerifier so cosine threshold alone determines candidates.
        #[derive(Clone)]
        struct AlwaysEquivalentVerifier;

        #[async_trait::async_trait]
        impl MergeSemanticVerifier for AlwaysEquivalentVerifier {
            async fn are_equivalent(
                &self,
                _left: &SkillSnapshot,
                _right: &SkillSnapshot,
            ) -> Result<bool, MergeError> {
                Ok(true)
            }
        }

        let writer = MergeProposalWriter::with_audit_sink(
            MergeConfig::default(), // merge_candidate_threshold = 0.58
            AlwaysEquivalentVerifier,
            &crate::audit::NoopMaintenanceAuditSink,
            Arc::clone(&embedder),
        );

        // Probe duplicate pairs: count how many are detected.
        let mut recall_count: usize = 0;
        for (left, right) in &duplicate_pairs {
            let candidates = writer
                .find_candidates(&[left.clone(), right.clone()])
                .await
                .expect("find_candidates must succeed for duplicate pair");
            if !candidates.is_empty() {
                recall_count += 1;
            }
        }

        // Probe control pairs: assert none are proposed.
        for (left, right) in &control_pairs {
            let candidates = writer
                .find_candidates(&[left.clone(), right.clone()])
                .await
                .expect("find_candidates must succeed for control pair");
            assert!(
                candidates.is_empty(),
                "control (non-duplicate) pair ({} / {}) must NOT produce a merge candidate; \
                 body-inclusive cosine was above 0.58 but should be below for unrelated skills",
                left.name,
                right.name
            );
        }

        assert!(
            recall_count >= 5,
            "body-inclusive merge vector must recall ≥ 5/6 true duplicate pairs at threshold 0.58; \
             got {recall_count}/6"
        );

        let _ = std::fs::remove_dir_all(&sandbox);
    }
}
