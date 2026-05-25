use std::{fs, path::PathBuf};

use domain::{ExtractedSkillCandidate, ExtractionResult};
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

        let mut written_paths = Vec::new();
        for candidate in &extraction_result.candidates {
            let pending_name = format!("{}.pending", slugify_skill_name(&candidate.name));
            let pending_path = pending_root.join(pending_name);
            let markdown = render_pending_markdown(
                candidate,
                extraction_result.source_session_id.as_str(),
                provider_name,
            );
            fs::write(&pending_path, markdown).map_err(|error| {
                WriterError::WriteFailure(pending_path.display().to_string(), error.to_string())
            })?;
            written_paths.push(pending_path);
        }

        Ok(written_paths)
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
}

impl WriterError {
    /// Maps writer failures to stable reason codes for API responses.
    pub fn reason_code(&self) -> String {
        match self {
            Self::InvalidRepoPath(_) => "invalid_repo_path",
            Self::ScopeResolution(_) => "scope_resolution_failed",
            Self::WriteFailure(_, _) => "pending_draft_write_failed",
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
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();

    let tags_yaml = if candidate.tags.is_empty() {
        "[]".to_owned()
    } else {
        format!(
            "[{}]",
            candidate
                .tags
                .iter()
                .map(|tag| format!("\"{tag}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let tags_line = if candidate.tags.is_empty() {
        String::new()
    } else {
        format!("tags: {}\n", candidate.tags.join(", "))
    };

    let mut markdown = String::new();
    markdown.push_str("---\n");
    markdown.push_str(&format!("name: {}\n", candidate.name));
    markdown.push_str(&format!("description: {}\n", candidate.description));
    markdown.push_str(&format!("suggested_tags: {tags_yaml}\n"));
    markdown.push_str("origin: session_extraction\n");
    markdown.push_str(&format!("source_session_id: {source_session_id}\n"));
    markdown.push_str(&format!("source_provider: {provider_name}\n"));
    markdown.push_str(&format!("created_at_unix: {now}\n"));
    markdown.push_str("---\n\n");
    markdown.push_str(&format!("# {}\n", candidate.name));
    markdown.push_str(&tags_line);
    markdown.push('\n');
    markdown.push_str(&candidate.description);
    markdown.push_str("\n\n");

    append_section(&mut markdown, "Procedures", &candidate.procedures);
    append_section(&mut markdown, "Conventions", &candidate.conventions);
    append_section(&mut markdown, "Assets", &candidate.assets);

    markdown
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
