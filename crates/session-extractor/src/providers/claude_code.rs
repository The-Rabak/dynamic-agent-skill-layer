use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{
    ClaudeCodeExtractionConfig, ClaudeCodeExtractor, ClaudeCodeTextLlm, StructuredTextLlm,
};

/// Reads the claude-code provider configuration from the environment.
///
/// Shared by [`build_extractor`] (map step) and [`build_text_llm`] (orchestration
/// seams) so both honour the same env vars:
/// - `CLAUDE_CLI_PATH` (default `claude` — resolved via `$PATH`; validated when set)
/// - `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`)
/// - `CLAUDE_CODE_EXTRACTION_TIMEOUT_MS` (optional inner timeout override; default 120 000 ms)
fn config_from_environment() -> Result<ClaudeCodeExtractionConfig, ExtractionError> {
    let mut config = ClaudeCodeExtractionConfig::default();

    if let Ok(cli_path) = std::env::var("CLAUDE_CLI_PATH")
        && !cli_path.trim().is_empty()
    {
        validate_cli_path(&cli_path)?;
        config.cli_path = cli_path;
    }
    if let Ok(model) = std::env::var("EXTRACT_SESSION_MODEL")
        && !model.trim().is_empty()
    {
        config.model = model;
    }
    if let Ok(timeout_str) = std::env::var("CLAUDE_CODE_EXTRACTION_TIMEOUT_MS") {
        config.timeout_ms = timeout_str.parse().map_err(|error| {
            ExtractionError::InvalidTranscript(format!(
                "invalid CLAUDE_CODE_EXTRACTION_TIMEOUT_MS value: {error}"
            ))
        })?;
    }

    Ok(config)
}

/// Builds the claude-code-backed orchestration-seam text transport.
///
/// Returns a [`StructuredTextLlm`] (the seam transport) configured from the same
/// environment as [`build_extractor`]. Used by `SessionExtractor::from_environment`
/// to drive the skeleton/synthesis/preamble/equivalence seams on Sonnet when
/// `EXTRACT_SESSION_PROVIDER=claude-code`, so the whole reduce-step LLM workload
/// runs on the same provider as the map step. Host-only (the `claude` CLI must be
/// present and authenticated where this runs).
pub fn build_text_llm() -> Result<Arc<dyn StructuredTextLlm>, ExtractionError> {
    let config = config_from_environment()?;
    Ok(Arc::new(ClaudeCodeTextLlm::new(config)?))
}

/// Builds the Claude Code CLI-backed extraction adapter (subscription-based, no API key).
///
/// Selected via `EXTRACT_SESSION_PROVIDER=claude-code` (or the accepted alias `=claude-cli`).
///
/// Reads opt-in provider configuration from the environment:
/// - `CLAUDE_CLI_PATH` (default `claude` — resolved via `$PATH`)
/// - `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`)
/// - `CLAUDE_CODE_EXTRACTION_TIMEOUT_MS` (optional inner timeout override; default 120 000 ms)
///
/// **Environment constraint:** This builder/provider does not read, store, or pass any
/// credentials. It just invokes the `claude` binary, which uses whatever login already
/// exists in its environment (`~/.claude`). The only requirement is that the `claude` CLI
/// is installed and already authenticated where the extractor runs — true on a host where
/// `claude` has been used interactively, but NOT in the stock compose container (no CLI, no
/// login). In containerised environments use `EXTRACT_SESSION_PROVIDER=claude`
/// (Anthropic Messages API + API key) or `=ollama` (local; the compose default).
///
/// No `ANTHROPIC_API_KEY` is read or required for this provider.
///
/// **Security:** When `CLAUDE_CLI_PATH` is provided, it is validated at construction time:
/// the path must exist, be a regular file, and have at least one execute bit set. This
/// prevents an env-variable misconfiguration from spawning an unintended binary. When
/// the default `claude` name is used (resolved via `$PATH`), the check is skipped because
/// PATH resolution is intentional and probing every candidate would be fragile.
pub fn build_extractor() -> Result<Arc<dyn TranscriptSkillExtractionService>, ExtractionError> {
    let config = config_from_environment()?;
    ClaudeCodeExtractor::new(config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}

/// Validates that an explicit `CLAUDE_CLI_PATH` is safe to use as a subprocess command.
///
/// Checks: the path exists, is a regular file (not a directory or symlink to one),
/// and has at least one execute bit set (`S_IXUSR | S_IXGRP | S_IXOTH`).
///
/// This is defense-in-depth: `Command::new` + `.args()` does not invoke a shell so
/// there is no injection risk, but an invalid or non-executable path with a minimized
/// subprocess env (no `PATH`) would produce a confusing spawn error. Failing loudly at
/// construction time gives a clear operator error instead.
///
/// Not called for the default `claude` value because PATH-resolved binaries are
/// intentional and their path is not known until spawn time.
fn validate_cli_path(path: &str) -> Result<(), ExtractionError> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = std::fs::metadata(path).map_err(|error| {
        ExtractionError::ProviderUnavailable(format!(
            "CLAUDE_CLI_PATH {path:?} is not accessible: {error}"
        ))
    })?;

    if !metadata.is_file() {
        return Err(ExtractionError::ProviderUnavailable(format!(
            "CLAUDE_CLI_PATH {path:?} exists but is not a regular file"
        )));
    }

    let mode = metadata.permissions().mode();
    // Check any of user/group/other execute bits.
    if mode & 0o111 == 0 {
        return Err(ExtractionError::ProviderUnavailable(format!(
            "CLAUDE_CLI_PATH {path:?} is not executable (mode {mode:#o})"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_cli_path_rejects_missing_file() {
        let error =
            validate_cli_path("/nonexistent/path/to/claude").expect_err("missing path must fail");
        assert!(
            matches!(error, ExtractionError::ProviderUnavailable(_)),
            "got {error:?}"
        );
        assert!(error.to_string().contains("not accessible"));
    }

    #[test]
    fn validate_cli_path_rejects_non_executable_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = std::env::temp_dir().join(format!("non-exec-claude-{}.sh", std::process::id()));
        std::fs::write(&tmp, "#!/bin/sh\n").expect("write tmp file");
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
            .expect("set non-executable permissions");

        let error =
            validate_cli_path(tmp.to_str().unwrap()).expect_err("non-executable path must fail");
        assert!(
            matches!(error, ExtractionError::ProviderUnavailable(_)),
            "got {error:?}"
        );
        assert!(error.to_string().contains("not executable"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn validate_cli_path_accepts_executable_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = std::env::temp_dir().join(format!("exec-claude-{}.sh", std::process::id()));
        std::fs::write(&tmp, "#!/bin/sh\n").expect("write tmp file");
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .expect("set executable permissions");

        validate_cli_path(tmp.to_str().unwrap()).expect("executable file must pass validation");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn validate_cli_path_rejects_directory() {
        let tmp_dir = std::env::temp_dir();
        let error = validate_cli_path(tmp_dir.to_str().unwrap())
            .expect_err("directory must fail validation");
        assert!(
            matches!(error, ExtractionError::ProviderUnavailable(_)),
            "got {error:?}"
        );
        assert!(error.to_string().contains("not a regular file"));
    }
}
