use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Utc};
use domain::{
    PENDING_SKILL_FILE_NAME, REJECTED_SKILL_FILE_NAME, has_lifecycle_file_name,
    is_rejected_tombstone, pending_default_expires_at,
};
use serde::Deserialize;
use thiserror::Error;
use walkdir::WalkDir;

/// Warning emitted for stale `.pending` proposal files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWarning {
    pub pending_path: PathBuf,
    pub age_days: i64,
}

/// Structured diagnostic describing one malformed `.pending` frontmatter document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedPendingFileDiagnostic {
    pub pending_path: PathBuf,
    pub parse_error: String,
}

/// Cleanup scan output that keeps warning results and malformed-file diagnostics together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingScanReport {
    pub warnings: Vec<PendingWarning>,
    pub malformed_pending_files: Vec<MalformedPendingFileDiagnostic>,
}

/// Active `.rejected` tombstone that blocks immediate reproposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproposalBlock {
    pub tombstone_path: PathBuf,
    pub age_days: i64,
}

/// Scans proposal directories and reports stale `.pending` files.
pub struct PendingWarningScanner {
    warning_after_days: i64,
}

impl PendingWarningScanner {
    /// Creates a scanner with a configurable warning threshold.
    pub fn new(warning_after_days: i64) -> Result<Self, CleanupError> {
        if warning_after_days <= 0 {
            return Err(CleanupError::InvalidWarningThreshold(
                "warning threshold must be greater than zero".to_owned(),
            ));
        }
        Ok(Self { warning_after_days })
    }

    /// Reports stale pending proposals without mutating pending files.
    pub fn scan(
        &self,
        root_paths: &[PathBuf],
        now: DateTime<Utc>,
    ) -> Result<Vec<PendingWarning>, CleanupError> {
        Ok(self.scan_with_diagnostics(root_paths, now)?.warnings)
    }

    /// Reports stale pending proposals and malformed files without mutating proposal state.
    pub fn scan_with_diagnostics(
        &self,
        root_paths: &[PathBuf],
        now: DateTime<Utc>,
    ) -> Result<PendingScanReport, CleanupError> {
        let mut warnings = Vec::new();
        let mut malformed_pending_files = Vec::new();
        for proposal_root in self.resolve_scan_roots(root_paths)? {
            for entry in WalkDir::new(&proposal_root)
                .min_depth(1)
                .follow_links(false)
            {
                let entry =
                    entry.map_err(|error| CleanupError::TraversalFailure(error.to_string()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if !has_lifecycle_file_name(path, PENDING_SKILL_FILE_NAME) {
                    continue;
                }
                let lifecycle = match self.read_lifecycle(path, now, FrontmatterParseMode::Strict) {
                    Ok(lifecycle) => lifecycle,
                    Err(CleanupError::FrontmatterParseFailure(_, parse_error)) => {
                        malformed_pending_files.push(MalformedPendingFileDiagnostic {
                            pending_path: path.to_path_buf(),
                            parse_error,
                        });
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if lifecycle.warning_logged_at.is_some() {
                    continue;
                }
                if now >= lifecycle.warning_at {
                    warnings.push(PendingWarning {
                        pending_path: path.to_path_buf(),
                        age_days: now.signed_duration_since(lifecycle.created_at).num_days(),
                    });
                }
            }
        }
        warnings.sort_by(|left, right| left.pending_path.cmp(&right.pending_path));
        malformed_pending_files.sort_by(|left, right| left.pending_path.cmp(&right.pending_path));
        Ok(PendingScanReport {
            warnings,
            malformed_pending_files,
        })
    }

    /// Returns the active `.rejected` tombstone blocking reproposal for a slug, if present.
    pub fn reproposal_block(
        &self,
        root_paths: &[PathBuf],
        proposal_slug: &str,
        now: DateTime<Utc>,
        tombstone_retention_days: i64,
    ) -> Result<Option<ReproposalBlock>, CleanupError> {
        if tombstone_retention_days <= 0 {
            return Err(CleanupError::InvalidTombstoneRetention(
                "tombstone retention must be greater than zero".to_owned(),
            ));
        }
        for proposal_root in self.resolve_scan_roots(root_paths)? {
            let tombstone_path = proposal_root
                .join(proposal_slug)
                .join(REJECTED_SKILL_FILE_NAME);
            if !is_rejected_tombstone(&tombstone_path) {
                continue;
            }
            let lifecycle =
                self.read_lifecycle(&tombstone_path, now, FrontmatterParseMode::Lenient)?;
            let tombstone_age = now.signed_duration_since(lifecycle.created_at).num_days();
            if tombstone_age < tombstone_retention_days {
                return Ok(Some(ReproposalBlock {
                    tombstone_path,
                    age_days: tombstone_age,
                }));
            }
        }
        Ok(None)
    }

    /// Prunes expired `.rejected` tombstones only; active/review artifacts are untouched.
    pub fn prune_expired_tombstones(
        &self,
        root_paths: &[PathBuf],
        now: DateTime<Utc>,
        tombstone_retention_days: i64,
    ) -> Result<Vec<PathBuf>, CleanupError> {
        if tombstone_retention_days <= 0 {
            return Err(CleanupError::InvalidTombstoneRetention(
                "tombstone retention must be greater than zero".to_owned(),
            ));
        }

        let mut pruned_paths = Vec::new();
        for proposal_root in self.resolve_scan_roots(root_paths)? {
            for entry in WalkDir::new(&proposal_root)
                .min_depth(1)
                .follow_links(false)
            {
                let entry =
                    entry.map_err(|error| CleanupError::TraversalFailure(error.to_string()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let tombstone_path = entry.path();
                if !is_rejected_tombstone(tombstone_path) {
                    continue;
                }
                let lifecycle =
                    self.read_lifecycle(tombstone_path, now, FrontmatterParseMode::Lenient)?;
                let tombstone_age = now.signed_duration_since(lifecycle.created_at).num_days();
                if tombstone_age >= tombstone_retention_days {
                    std::fs::remove_file(tombstone_path).map_err(|error| {
                        CleanupError::TombstonePruneFailure(
                            tombstone_path.display().to_string(),
                            error.to_string(),
                        )
                    })?;
                    pruned_paths.push(tombstone_path.to_path_buf());
                }
            }
        }
        pruned_paths.sort();
        Ok(pruned_paths)
    }

    /// Resolves caller roots into canonical `.skills` directories and rejects unsafe paths.
    fn resolve_scan_roots(&self, root_paths: &[PathBuf]) -> Result<Vec<PathBuf>, CleanupError> {
        let mut canonical_scan_roots = BTreeSet::new();
        for requested_root in root_paths {
            if !requested_root.is_absolute() {
                return Err(CleanupError::InvalidScanRoot(format!(
                    "scan root `{}` must be absolute",
                    requested_root.display()
                )));
            }
            let canonical_requested_root = requested_root.canonicalize().map_err(|error| {
                CleanupError::InvalidScanRoot(format!(
                    "scan root `{}` cannot be canonicalized: {error}",
                    requested_root.display()
                ))
            })?;
            if !canonical_requested_root.is_dir() {
                return Err(CleanupError::InvalidScanRoot(format!(
                    "scan root `{}` must resolve to a directory",
                    canonical_requested_root.display()
                )));
            }

            let canonical_scan_root = if canonical_requested_root
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|directory_name| directory_name.eq_ignore_ascii_case(".skills"))
            {
                canonical_requested_root
            } else {
                let proposal_root = canonical_requested_root.join(".skills");
                if !proposal_root.is_dir() {
                    return Err(CleanupError::InvalidScanRoot(format!(
                        "scan root `{}` must be a `.skills` directory or contain one",
                        canonical_requested_root.display()
                    )));
                }
                let canonical_proposal_root = proposal_root.canonicalize().map_err(|error| {
                    CleanupError::InvalidScanRoot(format!(
                        "proposal root `{}` cannot be canonicalized: {error}",
                        proposal_root.display()
                    ))
                })?;
                if !canonical_proposal_root.starts_with(&canonical_requested_root) {
                    return Err(CleanupError::InvalidScanRoot(format!(
                        "proposal root `{}` resolves outside requested root `{}`",
                        canonical_proposal_root.display(),
                        canonical_requested_root.display()
                    )));
                }
                canonical_proposal_root
            };

            canonical_scan_roots.insert(canonical_scan_root);
        }
        Ok(canonical_scan_roots.into_iter().collect())
    }

    fn read_lifecycle(
        &self,
        proposal_path: &Path,
        now: DateTime<Utc>,
        parse_mode: FrontmatterParseMode,
    ) -> Result<PendingLifecycle, CleanupError> {
        let metadata = std::fs::metadata(proposal_path).map_err(|error| {
            CleanupError::MetadataReadFailure(
                proposal_path.display().to_string(),
                error.to_string(),
            )
        })?;
        let modified_at = metadata.modified().map_err(|error| {
            CleanupError::MetadataReadFailure(
                proposal_path.display().to_string(),
                error.to_string(),
            )
        })?;
        let modified_at = DateTime::<Utc>::from(modified_at);
        let frontmatter = parse_frontmatter(proposal_path, parse_mode)?;

        let created_at = frontmatter
            .as_ref()
            .and_then(|value| parse_timestamp(value.created_at.as_deref()))
            .unwrap_or(modified_at);

        let effective_created_at = if created_at > now {
            modified_at
        } else {
            created_at
        };
        let warning_at = frontmatter
            .as_ref()
            .and_then(|value| parse_timestamp(value.warning_at.as_deref()))
            .unwrap_or(effective_created_at + Duration::days(self.warning_after_days));
        let expires_at = frontmatter
            .as_ref()
            .and_then(|value| parse_timestamp(value.expires_at.as_deref()))
            .unwrap_or_else(|| pending_default_expires_at(effective_created_at));
        let warning_logged_at = frontmatter
            .as_ref()
            .and_then(|value| parse_timestamp(value.warning_logged_at.as_deref()));

        Ok(PendingLifecycle {
            created_at: effective_created_at,
            warning_at,
            _expires_at: expires_at,
            warning_logged_at,
        })
    }
}

#[derive(Debug, Error)]
pub enum CleanupError {
    #[error("invalid warning threshold: {0}")]
    InvalidWarningThreshold(String),
    #[error("invalid scan root: {0}")]
    InvalidScanRoot(String),
    #[error("pending file traversal failed: {0}")]
    TraversalFailure(String),
    #[error("cannot read metadata for `{0}`: {1}")]
    MetadataReadFailure(String, String),
    #[error("cannot parse frontmatter for `{0}`: {1}")]
    FrontmatterParseFailure(String, String),
    #[error("invalid tombstone retention: {0}")]
    InvalidTombstoneRetention(String),
    #[error("failed pruning tombstone `{0}`: {1}")]
    TombstonePruneFailure(String, String),
}

impl CleanupError {
    /// Maps cleanup failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidWarningThreshold(_) => "cleanup_invalid_warning_threshold",
            Self::InvalidScanRoot(_) => "cleanup_invalid_scan_root",
            Self::TraversalFailure(_) => "cleanup_traversal_failed",
            Self::MetadataReadFailure(_, _) => "cleanup_metadata_read_failed",
            Self::FrontmatterParseFailure(_, _) => "cleanup_frontmatter_parse_failed",
            Self::InvalidTombstoneRetention(_) => "cleanup_invalid_tombstone_retention",
            Self::TombstonePruneFailure(_, _) => "cleanup_tombstone_prune_failed",
        }
    }
}

#[derive(Debug)]
struct PendingLifecycle {
    created_at: DateTime<Utc>,
    warning_at: DateTime<Utc>,
    _expires_at: DateTime<Utc>,
    warning_logged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct PendingFrontmatter {
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    warning_at: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    warning_logged_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrontmatterParseMode {
    Strict,
    Lenient,
}

fn parse_timestamp(raw: Option<&str>) -> Option<DateTime<Utc>> {
    raw.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn parse_frontmatter(
    proposal_path: &Path,
    parse_mode: FrontmatterParseMode,
) -> Result<Option<PendingFrontmatter>, CleanupError> {
    let contents = std::fs::read_to_string(proposal_path).map_err(|error| {
        CleanupError::MetadataReadFailure(proposal_path.display().to_string(), error.to_string())
    })?;
    let Some(remaining) = contents.strip_prefix("---\n") else {
        return Ok(None);
    };
    let Some((frontmatter_body, _)) = remaining.split_once("\n---\n") else {
        return Ok(None);
    };
    let parsed = match serde_yaml::from_str::<PendingFrontmatter>(frontmatter_body) {
        Ok(value) => value,
        Err(error) => {
            if parse_mode == FrontmatterParseMode::Lenient {
                return Ok(None);
            }
            return Err(CleanupError::FrontmatterParseFailure(
                proposal_path.display().to_string(),
                error.to_string(),
            ));
        }
    };
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use domain::pending_default_expires_at;

    use super::*;

    fn fresh_sandbox(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
        std::fs::create_dir_all(&sandbox).expect("sandbox should be created");
        sandbox
    }

    #[test]
    fn scan_rejects_root_without_proposal_directory() {
        let sandbox = fresh_sandbox("cleanup-invalid-root");
        let scanner = PendingWarningScanner::new(1).expect("warning threshold should be valid");

        let result = scanner.scan(std::slice::from_ref(&sandbox), Utc::now());

        assert!(matches!(result, Err(CleanupError::InvalidScanRoot(_))));
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn scan_uses_pending_frontmatter_warning_timestamp_when_present() {
        let sandbox = fresh_sandbox("cleanup-warning-at");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let stale_pending_path = proposal_root.join("stale/SKILL.md.pending");
        std::fs::create_dir_all(
            stale_pending_path
                .parent()
                .expect("stale pending path should have a parent directory"),
        )
        .expect("proposal directory should be created");
        std::fs::write(
            &stale_pending_path,
            "---\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\nexpires_at: 2026-04-01T00:00:00Z\norigin: session_extraction\n---\n",
        )
        .expect("pending frontmatter should be writable");

        let scanner = PendingWarningScanner::new(10).expect("warning threshold should be valid");
        let now = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
            .expect("timestamp should parse")
            .with_timezone(&Utc);
        let warnings = scanner
            .scan(std::slice::from_ref(&sandbox), now)
            .expect("scan should succeed");

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].pending_path, stale_pending_path);
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn read_lifecycle_uses_shared_default_expiry_when_frontmatter_omits_expires_at() {
        let sandbox = fresh_sandbox("cleanup-shared-default-expiry");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let pending_path = proposal_root.join("stale/SKILL.md.pending");
        std::fs::create_dir_all(
            pending_path
                .parent()
                .expect("pending path should have a parent directory"),
        )
        .expect("proposal directory should be created");
        std::fs::write(
            &pending_path,
            "---\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\norigin: session_extraction\n---\n",
        )
        .expect("pending frontmatter should be writable");

        let scanner = PendingWarningScanner::new(10).expect("warning threshold should be valid");
        let now = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
            .expect("timestamp should parse")
            .with_timezone(&Utc);
        let lifecycle = scanner
            .read_lifecycle(&pending_path, now, FrontmatterParseMode::Strict)
            .expect("lifecycle read should succeed");

        let expected_created_at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("timestamp should parse")
            .with_timezone(&Utc);
        assert_eq!(
            lifecycle._expires_at,
            pending_default_expires_at(expected_created_at)
        );
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn read_lifecycle_derives_defaults_from_effective_created_at_for_future_timestamps() {
        let sandbox = fresh_sandbox("cleanup-future-created-baseline");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let pending_path = proposal_root.join("stale/SKILL.md.pending");
        std::fs::create_dir_all(
            pending_path
                .parent()
                .expect("pending path should have a parent directory"),
        )
        .expect("proposal directory should be created");
        std::fs::write(
            &pending_path,
            "---\ncreated_at: 2099-01-01T00:00:00Z\norigin: session_extraction\n---\n",
        )
        .expect("pending frontmatter should be writable");

        let scanner = PendingWarningScanner::new(10).expect("warning threshold should be valid");
        let now = Utc::now();
        let lifecycle = scanner
            .read_lifecycle(&pending_path, now, FrontmatterParseMode::Strict)
            .expect("lifecycle read should succeed");
        let expected_created_at = DateTime::<Utc>::from(
            std::fs::metadata(&pending_path)
                .expect("pending metadata should be readable")
                .modified()
                .expect("pending modified time should be readable"),
        );

        assert_eq!(lifecycle.created_at, expected_created_at);
        assert_eq!(
            lifecycle.warning_at,
            expected_created_at + Duration::days(10)
        );
        assert_eq!(
            lifecycle._expires_at,
            pending_default_expires_at(expected_created_at)
        );
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn read_lifecycle_uses_effective_created_at_when_warning_and_expiry_are_malformed() {
        let sandbox = fresh_sandbox("cleanup-malformed-lifecycle-thresholds");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let pending_path = proposal_root.join("stale/SKILL.md.pending");
        std::fs::create_dir_all(
            pending_path
                .parent()
                .expect("pending path should have a parent directory"),
        )
        .expect("proposal directory should be created");
        std::fs::write(
            &pending_path,
            "---\ncreated_at: 2099-01-01T00:00:00Z\nwarning_at: not-a-timestamp\nexpires_at: definitely-not-a-timestamp\norigin: session_extraction\n---\n",
        )
        .expect("pending frontmatter should be writable");

        let scanner = PendingWarningScanner::new(14).expect("warning threshold should be valid");
        let now = Utc::now();
        let lifecycle = scanner
            .read_lifecycle(&pending_path, now, FrontmatterParseMode::Strict)
            .expect("lifecycle read should succeed");
        let expected_created_at = DateTime::<Utc>::from(
            std::fs::metadata(&pending_path)
                .expect("pending metadata should be readable")
                .modified()
                .expect("pending modified time should be readable"),
        );

        assert_eq!(lifecycle.created_at, expected_created_at);
        assert_eq!(
            lifecycle.warning_at,
            expected_created_at + Duration::days(14)
        );
        assert_eq!(
            lifecycle._expires_at,
            pending_default_expires_at(expected_created_at)
        );
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn prune_expired_tombstones_removes_only_expired_rejected_markers() {
        let sandbox = fresh_sandbox("cleanup-tombstone-prune");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let expired_tombstone_path = proposal_root.join("expired/SKILL.md.rejected");
        let active_tombstone_path = proposal_root.join("active/SKILL.md.rejected");
        std::fs::create_dir_all(
            expired_tombstone_path
                .parent()
                .expect("expired tombstone path should have parent"),
        )
        .expect("expired proposal directory should exist");
        std::fs::create_dir_all(
            active_tombstone_path
                .parent()
                .expect("active tombstone path should have parent"),
        )
        .expect("active proposal directory should exist");
        std::fs::write(
            &expired_tombstone_path,
            "---\nis_tombstone: true\ncreated_at: 2026-01-01T00:00:00Z\n---\n",
        )
        .expect("expired tombstone should be written");
        std::fs::write(
            &active_tombstone_path,
            "---\nis_tombstone: true\ncreated_at: 2026-01-29T00:00:00Z\n---\n",
        )
        .expect("active tombstone should be written");

        let scanner = PendingWarningScanner::new(1).expect("warning threshold should be valid");
        let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
            .expect("timestamp should parse")
            .with_timezone(&Utc);
        let pruned = scanner
            .prune_expired_tombstones(std::slice::from_ref(&sandbox), now, 30)
            .expect("prune should succeed");

        assert_eq!(pruned, vec![expired_tombstone_path.clone()]);
        assert!(!expired_tombstone_path.exists());
        assert!(active_tombstone_path.exists());
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn reproposal_block_treats_legacy_rejected_file_as_tombstone() {
        let sandbox = fresh_sandbox("cleanup-legacy-tombstone");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
        let legacy_tombstone_path = proposal_root.join("legacy/SKILL.md.rejected");
        std::fs::create_dir_all(
            legacy_tombstone_path
                .parent()
                .expect("legacy tombstone should have parent"),
        )
        .expect("legacy proposal directory should exist");
        std::fs::write(&legacy_tombstone_path, "# legacy tombstone marker\n")
            .expect("legacy tombstone should be written");

        let scanner = PendingWarningScanner::new(1).expect("warning threshold should be valid");
        let now = Utc::now();
        let block = scanner
            .reproposal_block(std::slice::from_ref(&sandbox), "legacy", now, 30)
            .expect("reproposal block should succeed");

        assert_eq!(
            block,
            Some(ReproposalBlock {
                tombstone_path: legacy_tombstone_path,
                age_days: 0,
            })
        );
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn scan_with_diagnostics_skips_malformed_pending_frontmatter_and_continues() {
        let sandbox = fresh_sandbox("cleanup-scan-malformed-pending");
        let proposal_root = sandbox.join(".skills");
        std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");

        let stale_pending_path = proposal_root.join("healthy/SKILL.md.pending");
        let malformed_pending_path = proposal_root.join("malformed/SKILL.md.pending");
        std::fs::create_dir_all(
            stale_pending_path
                .parent()
                .expect("healthy pending path should have a parent directory"),
        )
        .expect("healthy proposal directory should be created");
        std::fs::create_dir_all(
            malformed_pending_path
                .parent()
                .expect("malformed pending path should have a parent directory"),
        )
        .expect("malformed proposal directory should be created");
        std::fs::write(
            &stale_pending_path,
            "---\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\norigin: session_extraction\n---\n",
        )
        .expect("healthy pending frontmatter should be writable");
        std::fs::write(
            &malformed_pending_path,
            "---\ncreated_at: [malformed\nwarning_at: 2026-01-02T00:00:00Z\n---\n",
        )
        .expect("malformed pending frontmatter should be writable");

        let scanner = PendingWarningScanner::new(10).expect("warning threshold should be valid");
        let now = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
            .expect("timestamp should parse")
            .with_timezone(&Utc);

        let report = scanner
            .scan_with_diagnostics(std::slice::from_ref(&sandbox), now)
            .expect("scan should continue for healthy files");

        assert_eq!(report.warnings.len(), 1);
        assert_eq!(report.warnings[0].pending_path, stale_pending_path);
        assert_eq!(report.malformed_pending_files.len(), 1);
        assert_eq!(
            report.malformed_pending_files[0].pending_path,
            malformed_pending_path
        );
        assert!(
            !report.malformed_pending_files[0].parse_error.is_empty(),
            "malformed diagnostics should include parse details"
        );
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }
}
