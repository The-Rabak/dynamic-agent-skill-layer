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

/// Validates write targets are within approved output roots and not inside
/// protected skill source directories.
#[derive(Debug, Clone)]
pub struct WriteTargetGuard {
    write_allowed_roots: Vec<PathBuf>,
}

impl WriteTargetGuard {
    /// Creates a permissive guard that allows writes everywhere.
    pub fn permissive() -> Self {
        Self {
            write_allowed_roots: Vec::new(),
        }
    }

    pub fn new(write_allowed_roots: Vec<PathBuf>) -> Self {
        Self {
            write_allowed_roots,
        }
    }

    pub fn from_environment() -> Result<Self, WriterError> {
        let write_roots = resolve_write_allowed_roots()?;
        Ok(Self::new(write_roots))
    }

    /// Verify that `scope_root` — already resolved and canonicalized in the
    /// scope-resolution step — lies within at least one write-allowed root.
    /// This prevents `.skills` output from landing inside skill-source
    /// directories that should be treated as read-only.
    pub fn check_scope_root(&self, scope_root: &Path) -> Result<(), WriterError> {
        if self.write_allowed_roots.is_empty() {
            return Ok(());
        }

        let canonical = scope_root.canonicalize().map_err(|error| {
            WriterError::WriteDenied(format!(
                "cannot canonicalize output root `{}`: {error}",
                scope_root.display()
            ))
        })?;

        let allowed = self
            .write_allowed_roots
            .iter()
            .any(|root| canonical.starts_with(root));

        if !allowed {
            return Err(WriterError::WriteDenied(format!(
                "write denied: scope root `{}` is outside write-allowed output roots; \
                 check SKILL_GLOBAL_WRITE_ROOTS configuration",
                canonical.display()
            )));
        }
        Ok(())
    }
}

/// Writes extracted candidates as `.pending` drafts for human approval by rename.
#[derive(Clone)]
pub struct PendingDraftWriter {
    global_scope_paths: Vec<PathBuf>,
    write_guard: WriteTargetGuard,
}

impl PendingDraftWriter {
    /// Creates a writer with configured global scope roots and a write-target guard.
    pub fn new(global_scope_paths: Vec<PathBuf>) -> Self {
        Self {
            global_scope_paths,
            write_guard: WriteTargetGuard::permissive(),
        }
    }

    /// Creates a writer with an explicit write-target guard for testing.
    pub fn new_with_guard(
        global_scope_paths: Vec<PathBuf>,
        write_guard: WriteTargetGuard,
    ) -> Self {
        Self {
            global_scope_paths,
            write_guard,
        }
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

        let write_guard = WriteTargetGuard::from_environment()?;

        Ok(Self {
            global_scope_paths,
            write_guard,
        })
    }

    /// Persists one `.pending` file per extracted skill candidate.
    pub fn write_pending_drafts(
        &self,
        extraction_result: &ExtractionResult,
        request: &ExtractSessionRequest,
        provider_name: &str,
    ) -> Result<Vec<PathBuf>, WriterError> {
        let scope_root = self.resolve_scope_root(request)?;
        self.write_guard.check_scope_root(&scope_root)?;
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
    #[error("write denied: path `{0}` is outside write-allowed output roots")]
    WriteDenied(String),
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
            Self::WriteDenied(_) => "write_denied",
        }
        .to_owned()
    }
}

fn resolve_write_allowed_roots() -> Result<Vec<PathBuf>, WriterError> {
    let env_value = std::env::var("SKILL_GLOBAL_WRITE_ROOTS")
        .or_else(|_| std::env::var("SKILL_GLOBAL_ALLOWED_ROOTS"))
        .map_err(|_| {
            WriterError::InvalidRepoPath(
                "neither SKILL_GLOBAL_WRITE_ROOTS nor SKILL_GLOBAL_ALLOWED_ROOTS is set"
                    .to_owned(),
            )
        })?;
    let entries = split_env_paths(&env_value);
    if entries.is_empty() {
        return Err(WriterError::InvalidRepoPath(
            "SKILL_GLOBAL_WRITE_ROOTS must include at least one root path".to_owned(),
        ));
    }

    entries
        .into_iter()
        .map(|entry| {
            let root = PathBuf::from(&entry);
            if !root.is_absolute() {
                return Err(WriterError::InvalidRepoPath(format!(
                    "write-allowed root `{entry}` must be absolute"
                )));
            }
            root.canonicalize().map_err(|error| {
                WriterError::InvalidRepoPath(format!(
                    "write-allowed root `{entry}` is invalid: {error}"
                ))
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

    fn sandbox() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!("dasl-writer-{}-{}", std::process::id(), nonce));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("sandbox root should be creatable");
        path
    }

    fn stub_request(repo_path: Option<&str>) -> ExtractSessionRequest {
        ExtractSessionRequest {
            transcript_ref: "session-123.json".to_owned(),
            transcript_inline: None,
            session_id: "session-123".to_owned(),
            repo_path: repo_path.map(str::to_owned),
        }
    }

    fn minimal_candidate(name: &str) -> ExtractedSkillCandidate {
        ExtractedSkillCandidate {
            name: name.to_owned(),
            description: "test description".to_owned(),
            tags: vec!["test".to_owned()],
            procedures: vec!["do the thing".to_owned()],
            conventions: vec![],
            assets: vec![],
            confidence: 0.9,
        }
    }

    fn minimal_result(candidates: Vec<ExtractedSkillCandidate>) -> ExtractionResult {
        ExtractionResult {
            source_session_id: domain::DomainId::new_unchecked("session-123"),
            candidates,
            provider: "test".to_owned(),
        }
    }

    #[test]
    fn write_guard_allows_pending_root_inside_write_allowed_root() {
        let sandbox = sandbox();
        let write_root = sandbox.join("output");
        fs::create_dir_all(&write_root).expect("write root should be creatable");

        let write_guard = WriteTargetGuard::new(vec![write_root.clone()]);

        assert!(write_guard.check_scope_root(&write_root).is_ok());
    }

    #[test]
    fn write_guard_denies_pending_root_outside_write_allowed_root() {
        let sandbox = sandbox();
        let write_root = sandbox.join("output");
        let other_root = sandbox.join("other");

        fs::create_dir_all(&write_root).expect("write root should be creatable");
        fs::create_dir_all(&other_root).expect("other root should be creatable");

        let write_guard = WriteTargetGuard::new(vec![write_root]);

        let err = write_guard
            .check_scope_root(&other_root)
            .expect_err("write outside allowed root must be denied");
        assert!(
            matches!(err, WriterError::WriteDenied(_)),
            "expected WriteDenied, got {err:?}"
        );
        assert!(
            err.to_string().contains("write denied"),
            "error message must say 'write denied': {err}"
        );
    }

    #[test]
    fn write_guard_denies_nonexistent_scope_root() {
        let sandbox = sandbox();
        let write_root = sandbox.join("output");
        fs::create_dir_all(&write_root).expect("write root should be creatable");

        let write_guard = WriteTargetGuard::new(vec![write_root]);

        let nonexistent = sandbox.join("nonexistent");
        let err = write_guard
            .check_scope_root(&nonexistent)
            .expect_err("nonexistent path must be denied");
        assert!(matches!(err, WriterError::WriteDenied(_)));
    }

    #[test]
    fn write_guard_block_skill_source_directory() {
        let sandbox = sandbox();
        let output_root = sandbox.join("output");
        let skill_source_root = sandbox.join("skills");

        fs::create_dir_all(&output_root).expect("output root should be creatable");
        fs::create_dir_all(skill_source_root.join("my-skill")).expect("skill source should be creatable");

        // only output_root is in the write allowlist — skill_source_root is NOT
        let write_guard = WriteTargetGuard::new(vec![output_root.clone()]);

        let err = write_guard
            .check_scope_root(&skill_source_root)
            .expect_err("skill source root must be denied for writes");
        assert!(matches!(err, WriterError::WriteDenied(_)));
        assert!(
            err.to_string().contains("write denied"),
            "error message must say 'write denied' for skill source protection"
        );
    }

    #[test]
    fn write_guard_respects_env_var() {
        let sandbox = sandbox();
        let write_root = sandbox.join("output-env");
        fs::create_dir_all(&write_root).expect("output root should be creatable");

        unsafe {
            env::set_var("SKILL_GLOBAL_WRITE_ROOTS", write_root.display().to_string());
            env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
        }

        let guard = WriteTargetGuard::from_environment()
            .expect("guard should build from SKILL_GLOBAL_WRITE_ROOTS");

        let result = guard.check_scope_root(&write_root);
        assert!(result.is_ok(), "write to env-configured root must be allowed");

        unsafe {
            env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
        }
    }

    #[test]
    fn write_guard_falls_back_to_allowed_roots_env_var() {
        let sandbox = sandbox();
        let write_root = sandbox.join("fallback-root");
        fs::create_dir_all(&write_root).expect("fallback root should be creatable");

        unsafe {
            env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
            env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", write_root.display().to_string());
        }

        let guard = WriteTargetGuard::from_environment()
            .expect("guard should fall back to SKILL_GLOBAL_ALLOWED_ROOTS");

        let result = guard.check_scope_root(&write_root);
        assert!(result.is_ok(), "write to allowed-roots fallback must be permitted");

        unsafe {
            env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
        }
    }

    #[test]
    fn writer_rejects_pending_drafts_when_pending_root_is_outside_guard() {
        let sandbox = sandbox();
        let allowed_root = sandbox.join("allowed");
        let blocked_root = sandbox.join("blocked");

        fs::create_dir_all(&allowed_root).expect("allowed root should be creatable");
        fs::create_dir_all(&blocked_root).expect("blocked root should be creatable");

        // repo_path points inside allowed_root so resolve_scope_root succeeds,
        // but the guard only allows a different root
        let writer = PendingDraftWriter::new_with_guard(
            vec![sandbox.clone()],
            WriteTargetGuard::new(vec![allowed_root.clone()]),
        );
        let request = stub_request(Some(blocked_root.to_str().unwrap()));

        unsafe {
            env::set_var(
                "SKILL_GLOBAL_ALLOWED_ROOTS",
                sandbox.display().to_string(),
            );
            env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
        }

        let result = writer.write_pending_drafts(
            &minimal_result(vec![minimal_candidate("blocked-skill")]),
            &request,
            "test",
        );

        unsafe {
            env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
        }

        let err = result.expect_err("write outside guard must be denied");
        assert!(
            matches!(err, WriterError::WriteDenied(_)),
            "expected WriteDenied, got {err:?}"
        );
    }

    #[test]
    fn writer_allows_pending_drafts_inside_guard() {
        let sandbox = sandbox();
        let output_root = sandbox.join("output");
        fs::create_dir_all(&output_root).expect("output root should be creatable");

        let writer = PendingDraftWriter::new_with_guard(
            vec![sandbox.clone()],
            WriteTargetGuard::new(vec![output_root.clone()]),
        );

        // repo_path inside output_root => pending_root = output_root/.skills
        let repo_subdir = output_root.join("project");
        fs::create_dir_all(&repo_subdir).expect("project subdir should be creatable");
        let request = stub_request(Some(repo_subdir.to_str().unwrap()));

        unsafe {
            env::set_var(
                "SKILL_GLOBAL_ALLOWED_ROOTS",
                sandbox.display().to_string(),
            );
            env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
        }

        let result = writer.write_pending_drafts(
            &minimal_result(vec![minimal_candidate("allowed-skill")]),
            &request,
            "test",
        );

        unsafe {
            env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
        }

        let paths = result.expect("write inside guard must succeed");
        assert!(!paths.is_empty(), "at least one path should be committed");
        assert!(paths[0].starts_with(&output_root));
    }

    #[test]
    fn write_denied_error_includes_clear_message() {
        let sandbox = sandbox();
        let write_root = sandbox.join("output");
        let blocked = sandbox.join("skills");
        fs::create_dir_all(&write_root).expect("write root should be creatable");
        fs::create_dir_all(&blocked).expect("blocked path should be creatable");

        let guard = WriteTargetGuard::new(vec![write_root]);

        let err = guard
            .check_scope_root(&blocked)
            .expect_err("must be denied");

        let msg = err.to_string();
        assert!(msg.contains("write denied"));
        assert!(msg.contains(&blocked.canonicalize().unwrap().display().to_string()));
        assert!(msg.contains("SKILL_GLOBAL_WRITE_ROOTS"));
    }

    #[test]
    fn reason_code_write_denied_returns_stable_string() {
        let err = WriterError::WriteDenied("write denied: path `/skills/global/skills/foo/SKILL.md` is a skill source directory (read-only)".to_owned());
        assert_eq!(err.reason_code(), "write_denied");
    }
}
