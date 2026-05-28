use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use domain::{
    ExtractedSkillCandidate, ExtractionResult, PENDING_SKILL_FILE_NAME, REJECTED_SKILL_FILE_NAME,
    is_rejected_tombstone, pending_default_expires_at, pending_default_warning_at,
};
use serde::Serialize;
use thiserror::Error;

use crate::ExtractSessionRequest;

/// Writes extracted candidates as `.pending` drafts for human approval by rename.
#[derive(Clone)]
pub struct PendingDraftWriter {
    global_scope_paths: Vec<PathBuf>,
}

impl PendingDraftWriter {
    /// Creates a writer with configured global scope roots.
    pub fn new(global_scope_paths: Vec<PathBuf>) -> Self {
        Self { global_scope_paths }
    }

    /// Builds a writer from `SKILL_GLOBAL_PATHS`.
    pub fn from_environment() -> Result<Self, WriterError> {
        let configured = std::env::var("SKILL_GLOBAL_PATHS").unwrap_or_default();
        let mut global_scope_paths = configured
            .split(':')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if global_scope_paths.is_empty() {
            global_scope_paths.push(std::env::current_dir().map_err(|error| {
                WriterError::ScopeResolution(format!(
                    "could not resolve current directory fallback: {error}"
                ))
            })?);
        }

        Ok(Self::new(global_scope_paths))
    }

    /// Persists one `.pending` file per extracted skill candidate.
    pub fn write_pending_drafts(
        &self,
        extraction_result: &ExtractionResult,
        request: &ExtractSessionRequest,
        provider_name: &str,
    ) -> Result<Vec<PathBuf>, WriterError> {
        let scope_root = self.resolve_scope_root(request)?;
        let pending_root = scope_root.join(".skills");
        fs::create_dir_all(&pending_root).map_err(|error| {
            WriterError::WriteFailure(pending_root.display().to_string(), error.to_string())
        })?;

        let batch_nonce = batch_nonce();
        let write_plans = build_write_plans(
            &pending_root,
            extraction_result,
            provider_name,
            &batch_nonce,
        )?;
        if write_plans.is_empty() {
            return Ok(Vec::new());
        }

        for plan in &write_plans {
            let proposal_directory = plan.pending_path.parent().ok_or_else(|| {
                WriterError::WriteFailure(
                    plan.pending_path.display().to_string(),
                    "pending path must have parent directory".to_owned(),
                )
            })?;
            fs::create_dir_all(proposal_directory).map_err(|error| {
                WriterError::WriteFailure(
                    proposal_directory.display().to_string(),
                    error.to_string(),
                )
            })?;
        }

        let write_temp_result = write_plans.iter().try_for_each(|plan| {
            fs::write(&plan.temp_path, &plan.markdown).map_err(|error| {
                WriterError::WriteFailure(plan.temp_path.display().to_string(), error.to_string())
            })
        });
        if let Err(error) = write_temp_result {
            cleanup_paths(write_plans.iter().map(|plan| plan.temp_path.as_path()));
            return Err(error);
        }

        let mut moved_backups = Vec::new();
        for plan in write_plans.iter().filter(|plan| plan.target_preexisted) {
            fs::rename(&plan.pending_path, &plan.backup_path).map_err(|error| {
                cleanup_paths(
                    write_plans
                        .iter()
                        .map(|candidate| candidate.temp_path.as_path()),
                );
                rollback_backup_moves(&moved_backups);
                WriterError::WriteFailure(
                    plan.pending_path.display().to_string(),
                    error.to_string(),
                )
            })?;
            moved_backups.push((plan.backup_path.clone(), plan.pending_path.clone()));
        }

        let mut committed_paths = Vec::new();
        for plan in &write_plans {
            if let Err(error) = fs::rename(&plan.temp_path, &plan.pending_path) {
                cleanup_paths(
                    write_plans
                        .iter()
                        .map(|candidate| candidate.temp_path.as_path()),
                );
                rollback_committed_paths(&committed_paths);
                rollback_backup_moves(&moved_backups);
                return Err(WriterError::WriteFailure(
                    plan.pending_path.display().to_string(),
                    error.to_string(),
                ));
            }
            committed_paths.push(plan.pending_path.clone());
        }

        cleanup_paths(write_plans.iter().map(|plan| plan.backup_path.as_path()));
        Ok(committed_paths)
    }

    /// Validates that request scope resolves to an approved writable root.
    pub fn validate_scope_root(&self, request: &ExtractSessionRequest) -> Result<(), WriterError> {
        self.resolve_scope_root(request).map(|_| ())
    }

    fn resolve_scope_root(&self, request: &ExtractSessionRequest) -> Result<PathBuf, WriterError> {
        if let Some(repo_path) = request
            .repo_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            return self.resolve_repo_scope_root(repo_path);
        }

        self.global_scope_paths.first().cloned().ok_or_else(|| {
            WriterError::ScopeResolution("no scope root available for pending drafts".to_owned())
        })
    }

    fn resolve_repo_scope_root(&self, repo_path: &str) -> Result<PathBuf, WriterError> {
        let canonical_repo_path = PathBuf::from(repo_path).canonicalize().map_err(|error| {
            WriterError::InvalidRepoPath(format!(
                "repo_path `{repo_path}` could not be canonicalized: {error}"
            ))
        })?;
        if !canonical_repo_path.is_dir() {
            return Err(WriterError::InvalidRepoPath(format!(
                "repo_path `{}` must resolve to a directory",
                canonical_repo_path.display()
            )));
        }

        let allowed_roots = allowed_roots_from_environment()?;
        if !allowed_roots
            .iter()
            .any(|allowed_root| canonical_repo_path.starts_with(allowed_root))
        {
            return Err(WriterError::InvalidRepoPath(format!(
                "repo_path `{}` resolves outside SKILL_GLOBAL_ALLOWED_ROOTS",
                canonical_repo_path.display()
            )));
        }

        Ok(canonical_repo_path)
    }
}

#[derive(Debug, Error)]
pub enum WriterError {
    #[error("invalid repo_path: {0}")]
    InvalidRepoPath(String),
    #[error("scope resolution failed: {0}")]
    ScopeResolution(String),
    #[error("unable to write pending draft `{0}`: {1}")]
    WriteFailure(String, String),
    #[error("pending draft frontmatter serialization failed: {0}")]
    FrontmatterSerialization(String),
    #[error("pending draft batch validation failed: {0}")]
    BatchValidation(String),
    #[error("rejected tombstone blocks pending draft reproposal: `{0}`")]
    RejectedTombstonePresent(String),
}

impl WriterError {
    /// Maps writer failures to stable reason codes for API responses.
    pub fn reason_code(&self) -> String {
        match self {
            Self::InvalidRepoPath(_) => "invalid_repo_path",
            Self::ScopeResolution(_) => "scope_resolution_failed",
            Self::WriteFailure(_, _) => "pending_draft_write_failed",
            Self::FrontmatterSerialization(_) => "pending_frontmatter_serialization_failed",
            Self::BatchValidation(_) => "pending_draft_batch_validation_failed",
            Self::RejectedTombstonePresent(_) => "rejected_tombstone_present",
        }
        .to_owned()
    }
}

fn allowed_roots_from_environment() -> Result<Vec<PathBuf>, WriterError> {
    let allowed_roots_value = std::env::var("SKILL_GLOBAL_ALLOWED_ROOTS").map_err(|_| {
        WriterError::InvalidRepoPath("SKILL_GLOBAL_ALLOWED_ROOTS is not set".to_owned())
    })?;
    let root_entries = split_env_paths(&allowed_roots_value);
    if root_entries.is_empty() {
        return Err(WriterError::InvalidRepoPath(
            "SKILL_GLOBAL_ALLOWED_ROOTS must include at least one root path".to_owned(),
        ));
    }

    root_entries
        .into_iter()
        .map(|entry| {
            let root = PathBuf::from(&entry);
            if !root.is_absolute() {
                return Err(WriterError::InvalidRepoPath(format!(
                    "allowed root `{entry}` must be absolute"
                )));
            }
            root.canonicalize().map_err(|error| {
                WriterError::InvalidRepoPath(format!("allowed root `{entry}` is invalid: {error}"))
            })
        })
        .collect()
}

fn split_env_paths(value: &str) -> Vec<String> {
    value
        .split([':', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

fn slugify_skill_name(name: &str) -> String {
    let slug = name
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "session-extracted-skill".to_owned()
    } else {
        slug
    }
}

fn render_pending_markdown(
    candidate: &ExtractedSkillCandidate,
    source_session_id: &str,
    provider_name: &str,
) -> Result<String, WriterError> {
    let created_at = Utc::now();
    let warning_at = pending_default_warning_at(created_at);
    let expires_at = pending_default_expires_at(created_at);
    let frontmatter = PendingDraftFrontmatter {
        name: candidate.name.as_str(),
        description: candidate.description.as_str(),
        suggested_tags: &candidate.tags,
        origin: "session_extraction",
        source_session_id,
        source_provider: provider_name,
        created_at: created_at.to_rfc3339(),
        warning_at: warning_at.to_rfc3339(),
        expires_at: expires_at.to_rfc3339(),
    };
    let frontmatter_yaml = serialize_frontmatter(&frontmatter)?;
    let tags_line = if candidate.tags.is_empty() {
        String::new()
    } else {
        format!("tags: {}\n", candidate.tags.join(", "))
    };

    let mut markdown = String::new();
    markdown.push_str("---\n");
    markdown.push_str(&frontmatter_yaml);
    if !frontmatter_yaml.ends_with('\n') {
        markdown.push('\n');
    }
    markdown.push_str("---\n\n");
    markdown.push_str(&format!("# {}\n", candidate.name));
    markdown.push_str(&tags_line);
    markdown.push('\n');
    markdown.push_str(&candidate.description);
    markdown.push_str("\n\n");

    append_section(&mut markdown, "Procedures", &candidate.procedures);
    append_section(&mut markdown, "Conventions", &candidate.conventions);
    append_section(&mut markdown, "Assets", &candidate.assets);

    Ok(markdown)
}

/// Frontmatter payload for pending skill proposals.
#[derive(Serialize)]
struct PendingDraftFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    suggested_tags: &'a [String],
    origin: &'a str,
    source_session_id: &'a str,
    source_provider: &'a str,
    created_at: String,
    warning_at: String,
    expires_at: String,
}

#[derive(Debug, Clone)]
struct PendingWritePlan {
    pending_path: PathBuf,
    temp_path: PathBuf,
    backup_path: PathBuf,
    target_preexisted: bool,
    markdown: String,
}

fn serialize_frontmatter(frontmatter: &PendingDraftFrontmatter<'_>) -> Result<String, WriterError> {
    let serialized = serde_yaml::to_string(frontmatter)
        .map_err(|error| WriterError::FrontmatterSerialization(error.to_string()))?;
    Ok(serialized
        .strip_prefix("---\n")
        .unwrap_or(&serialized)
        .to_owned())
}

fn build_write_plans(
    pending_root: &Path,
    extraction_result: &ExtractionResult,
    provider_name: &str,
    batch_nonce: &str,
) -> Result<Vec<PendingWritePlan>, WriterError> {
    let mut unique_pending_paths = HashSet::new();
    extraction_result
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let proposal_slug = slugify_skill_name(&candidate.name);
            let proposal_directory = pending_root.join(proposal_slug);
            let pending_path = proposal_directory.join(PENDING_SKILL_FILE_NAME);
            let rejected_tombstone_path = proposal_directory.join(REJECTED_SKILL_FILE_NAME);
            if is_rejected_tombstone(&rejected_tombstone_path) {
                return Err(WriterError::RejectedTombstonePresent(
                    rejected_tombstone_path.display().to_string(),
                ));
            }
            if !unique_pending_paths.insert(pending_path.clone()) {
                return Err(WriterError::BatchValidation(format!(
                    "duplicate pending path resolved for candidate `{}` at `{}`",
                    candidate.name,
                    pending_path.display()
                )));
            }
            let markdown = render_pending_markdown(
                candidate,
                extraction_result.source_session_id.as_str(),
                provider_name,
            )?;

            Ok(PendingWritePlan {
                target_preexisted: pending_path.exists(),
                temp_path: proposal_directory.join(format!(
                    ".{}.{batch_nonce}.{index}.tmp",
                    PENDING_SKILL_FILE_NAME
                )),
                backup_path: proposal_directory.join(format!(
                    ".{}.{batch_nonce}.{index}.bak",
                    PENDING_SKILL_FILE_NAME
                )),
                pending_path,
                markdown,
            })
        })
        .collect()
}

fn rollback_committed_paths(paths: &[PathBuf]) {
    cleanup_paths(paths.iter().map(PathBuf::as_path));
}

fn rollback_backup_moves(backup_moves: &[(PathBuf, PathBuf)]) {
    for (backup_path, pending_path) in backup_moves.iter().rev() {
        let _ = fs::rename(backup_path, pending_path);
    }
}

fn cleanup_paths<'a>(paths: impl Iterator<Item = &'a Path>) {
    for path in paths {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

fn batch_nonce() -> String {
    let process_id = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{process_id}-{nanos}")
}

fn append_section(output: &mut String, heading: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    output.push_str(&format!("## {heading}\n"));
    for line in lines {
        output.push_str(&format!("- {line}\n"));
    }
    output.push('\n');
}
