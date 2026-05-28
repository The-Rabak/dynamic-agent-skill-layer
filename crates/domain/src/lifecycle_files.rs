use std::path::Path;

/// Canonical active skill markdown file name.
pub const ACTIVE_SKILL_FILE_NAME: &str = "SKILL.md";
/// Canonical pending approval proposal file name.
pub const PENDING_SKILL_FILE_NAME: &str = "SKILL.md.pending";
/// Canonical rejected tombstone file name.
pub const REJECTED_SKILL_FILE_NAME: &str = "SKILL.md.rejected";
/// Canonical retired archive file name.
pub const RETIRED_SKILL_FILE_NAME: &str = "SKILL.md.retired";

/// Returns true when `path` resolves to the provided canonical lifecycle file name.
pub fn has_lifecycle_file_name(path: &Path, expected_file_name: &str) -> bool {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|file_name| file_name.eq_ignore_ascii_case(expected_file_name))
}

/// Returns true when `path` points to an on-disk canonical `.rejected` tombstone file.
///
/// This keeps tombstone interpretation fail-closed by filename presence rather than frontmatter
/// metadata so writer blocking and maintenance cleanup cannot diverge.
pub fn is_rejected_tombstone(path: &Path) -> bool {
    path.is_file() && has_lifecycle_file_name(path, REJECTED_SKILL_FILE_NAME)
}
