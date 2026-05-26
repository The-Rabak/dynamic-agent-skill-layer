use std::{collections::BTreeSet, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use thiserror::Error;
use walkdir::WalkDir;

/// Warning emitted for stale `.pending` proposal files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWarning {
    pub pending_path: PathBuf,
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

    /// Reports stale pending proposals without mutating files.
    pub fn scan(
        &self,
        root_paths: &[PathBuf],
        now: DateTime<Utc>,
    ) -> Result<Vec<PendingWarning>, CleanupError> {
        let mut warnings = Vec::new();
        for root in self.resolve_scan_roots(root_paths)? {
            for entry in WalkDir::new(&root)
                .min_depth(1)
                .max_depth(1)
                .follow_links(false)
            {
                let entry =
                    entry.map_err(|error| CleanupError::TraversalFailure(error.to_string()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if !path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pending"))
                {
                    continue;
                }
                let metadata = std::fs::metadata(path).map_err(|error| {
                    CleanupError::MetadataReadFailure(path.display().to_string(), error.to_string())
                })?;
                let modified_at = metadata.modified().map_err(|error| {
                    CleanupError::MetadataReadFailure(path.display().to_string(), error.to_string())
                })?;
                let modified_at = DateTime::<Utc>::from(modified_at);
                let age = now.signed_duration_since(modified_at);
                if age >= Duration::days(self.warning_after_days) {
                    warnings.push(PendingWarning {
                        pending_path: path.to_path_buf(),
                        age_days: age.num_days(),
                    });
                }
            }
        }
        warnings.sort_by(|left, right| left.pending_path.cmp(&right.pending_path));
        Ok(warnings)
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
}

impl CleanupError {
    /// Maps cleanup failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidWarningThreshold(_) => "cleanup_invalid_warning_threshold",
            Self::InvalidScanRoot(_) => "cleanup_invalid_scan_root",
            Self::TraversalFailure(_) => "cleanup_traversal_failed",
            Self::MetadataReadFailure(_, _) => "cleanup_metadata_read_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

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

        let result = scanner.scan(&[sandbox.clone()], Utc::now());

        assert!(matches!(result, Err(CleanupError::InvalidScanRoot(_))));
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn scan_limits_traversal_to_proposal_directory_and_reports_stale_files() {
        let sandbox = fresh_sandbox("cleanup-proposal-root");
        let proposal_root = sandbox.join(".skills");
        let nested_root = proposal_root.join("nested");
        std::fs::create_dir_all(&nested_root).expect("proposal directories should be created");
        let stale_pending_path = proposal_root.join("stale.pending");
        std::fs::write(&stale_pending_path, "stale marker")
            .expect("stale marker should be created");
        let nested_pending_path = nested_root.join("nested.pending");
        std::fs::write(&nested_pending_path, "nested marker")
            .expect("nested marker should be created");

        let scanner = PendingWarningScanner::new(1).expect("warning threshold should be valid");
        let now = Utc::now() + Duration::days(2);

        let warnings = scanner
            .scan(&[sandbox.clone()], now)
            .expect("scan should succeed for valid proposal root");

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].pending_path, stale_pending_path);
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[test]
    fn scan_rejects_relative_roots_even_when_proposal_directory_exists() {
        let scanner = PendingWarningScanner::new(1).expect("warning threshold should be valid");
        let result = scanner.scan(&[PathBuf::from("relative-root/.skills")], Utc::now());

        assert!(matches!(result, Err(CleanupError::InvalidScanRoot(_))));
    }
}
