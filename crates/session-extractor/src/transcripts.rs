use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use domain::{DomainId, SessionTranscript, TranscriptEntry};
use serde_json::Value;
use thiserror::Error;

/// Maximum allowed size for an inline transcript payload.
///
/// Enforced in [`TranscriptLoader::load`] before any parsing occurs, so
/// callers that pass gigantic strings fail fast instead of consuming unbounded
/// memory. The ingest path can reference this constant to apply the same limit
/// at validation time.
pub const MAX_INLINE_BYTES: usize = 10 * 1024 * 1024;

/// Validates transcript references and parses Claude-compatible JSONL payloads.
#[derive(Clone)]
pub struct TranscriptLoader {
    transcript_root: PathBuf,
}

impl TranscriptLoader {
    /// Builds the loader from `CLAUDE_TRANSCRIPT_ROOT`.
    pub fn from_environment() -> Result<Self, TranscriptError> {
        let configured_root = std::env::var("CLAUDE_TRANSCRIPT_ROOT")
            .map(PathBuf::from)
            .map_err(|_| {
                TranscriptError::InvalidRoot("CLAUDE_TRANSCRIPT_ROOT is not set".to_owned())
            })?;
        Self::new(configured_root)
    }

    /// Creates a loader with an explicit trust boundary root.
    pub fn new(transcript_root: PathBuf) -> Result<Self, TranscriptError> {
        let canonical_root = transcript_root
            .canonicalize()
            .map_err(|error| TranscriptError::InvalidRoot(error.to_string()))?;
        if !canonical_root.is_dir() {
            return Err(TranscriptError::InvalidRoot(format!(
                "transcript root `{}` is not a directory",
                canonical_root.display()
            )));
        }

        Ok(Self {
            transcript_root: canonical_root,
        })
    }

    /// Performs trust-boundary checks for `transcript_ref`.
    pub fn validate_ref(&self, transcript_ref: &str) -> Result<PathBuf, TranscriptError> {
        let trimmed = transcript_ref.trim();
        if trimmed.is_empty() {
            return Err(TranscriptError::InvalidReference(
                "transcript_ref must not be blank".to_owned(),
            ));
        }

        let candidate = Path::new(trimmed);
        if candidate.is_absolute() {
            return Err(TranscriptError::InvalidReference(
                "absolute transcript paths are not allowed".to_owned(),
            ));
        }
        if candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(TranscriptError::InvalidReference(
                "path traversal is not allowed for transcript_ref".to_owned(),
            ));
        }

        let joined = self.transcript_root.join(candidate);
        let canonical = joined.canonicalize().map_err(|error| {
            TranscriptError::InvalidReference(format!(
                "unable to resolve transcript_ref `{trimmed}`: {error}"
            ))
        })?;

        if !canonical.starts_with(&self.transcript_root) {
            return Err(TranscriptError::InvalidReference(
                "transcript_ref resolved outside CLAUDE_TRANSCRIPT_ROOT".to_owned(),
            ));
        }

        if canonical.is_dir() {
            return Err(TranscriptError::InvalidReference(
                "transcript_ref must point to a file".to_owned(),
            ));
        }

        Ok(canonical)
    }

    /// Loads and parses transcript content from either inline JSONL or a rooted reference.
    pub fn load(
        &self,
        session_id: &str,
        transcript_ref: &str,
        transcript_inline: Option<&str>,
    ) -> Result<SessionTranscript, TranscriptError> {
        let transcript_payload = if let Some(inline_payload) = transcript_inline {
            if inline_payload.len() > MAX_INLINE_BYTES {
                return Err(TranscriptError::InvalidReference(format!(
                    "transcript_inline exceeds {MAX_INLINE_BYTES} bytes"
                )));
            }
            inline_payload.to_owned()
        } else {
            let path = self.validate_ref(transcript_ref)?;
            fs::read_to_string(&path).map_err(|error| {
                TranscriptError::ReadFailure(path.display().to_string(), error.to_string())
            })?
        };

        parse_claude_jsonl(session_id, &transcript_payload)
    }
}

#[derive(Debug, Error)]
pub enum TranscriptError {
    #[error("invalid transcript root: {0}")]
    InvalidRoot(String),
    #[error("invalid transcript_ref: {0}")]
    InvalidReference(String),
    #[error("unable to read transcript `{0}`: {1}")]
    ReadFailure(String, String),
    #[error("invalid transcript payload: {0}")]
    InvalidPayload(String),
}

impl TranscriptError {
    pub fn reason_code(&self) -> String {
        match self {
            Self::InvalidRoot(_) => "invalid_transcript_root",
            Self::InvalidReference(_) => "invalid_transcript_ref",
            Self::ReadFailure(_, _) => "transcript_read_failed",
            Self::InvalidPayload(_) => "invalid_transcript_payload",
        }
        .to_owned()
    }
}

fn parse_claude_jsonl(
    session_id: &str,
    jsonl_payload: &str,
) -> Result<SessionTranscript, TranscriptError> {
    let parsed_session_id = DomainId::parse(session_id.to_owned())
        .map_err(|error| TranscriptError::InvalidPayload(error.to_string()))?;

    let mut entries = Vec::new();
    for (index, line) in jsonl_payload.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(trimmed).map_err(|error| {
            TranscriptError::InvalidPayload(format!(
                "line {} is not valid JSON: {error}",
                index + 1
            ))
        })?;
        let speaker = extract_speaker(&value).ok_or_else(|| {
            TranscriptError::InvalidPayload(format!("line {} is missing speaker/role", index + 1))
        })?;
        let content = extract_content(&value).ok_or_else(|| {
            TranscriptError::InvalidPayload(format!("line {} is missing content text", index + 1))
        })?;

        entries.push(TranscriptEntry { speaker, content });
    }

    if entries.is_empty() {
        return Err(TranscriptError::InvalidPayload(
            "transcript contains no conversational entries".to_owned(),
        ));
    }

    Ok(SessionTranscript {
        session_id: parsed_session_id,
        entries,
    })
}

fn extract_speaker(value: &Value) -> Option<String> {
    value
        .get("speaker")
        .and_then(Value::as_str)
        .or_else(|| value.get("role").and_then(Value::as_str))
        .or_else(|| value.pointer("/message/role").and_then(Value::as_str))
        .map(str::trim)
        .filter(|speaker| !speaker.is_empty())
        .map(str::to_owned)
}

fn extract_content(value: &Value) -> Option<String> {
    if let Some(content) = value.get("content").and_then(parse_content_value) {
        return Some(content);
    }

    value
        .pointer("/message/content")
        .and_then(parse_content_value)
}

fn parse_content_value(value: &Value) -> Option<String> {
    if let Some(content) = value.as_str() {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    if let Some(parts) = value.as_array() {
        let stitched = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !stitched.is_empty() {
            return Some(stitched);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        ffi::OsString,
        sync::{LazyLock, Mutex, MutexGuard},
    };

    use super::{TranscriptError, TranscriptLoader};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn remove(key: &'static str) -> Self {
            let previous = env::var_os(key);
            // SAFETY: tests mutate process env only while holding ENV_LOCK.
            unsafe {
                env::remove_var(key);
            }

            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: tests mutate process env only while holding ENV_LOCK.
            unsafe {
                if let Some(value) = &self.previous {
                    env::set_var(self.key, value);
                } else {
                    env::remove_var(self.key);
                }
            }
        }
    }

    struct TranscriptRootEnvGuard {
        _lock: MutexGuard<'static, ()>,
        _transcript_root: EnvVarGuard,
    }

    fn without_transcript_root() -> TranscriptRootEnvGuard {
        let lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
        TranscriptRootEnvGuard {
            _lock: lock,
            _transcript_root: EnvVarGuard::remove("CLAUDE_TRANSCRIPT_ROOT"),
        }
    }

    #[test]
    fn load_rejects_inline_payload_exceeding_max_bytes() {
        let dir = std::env::temp_dir();
        let loader = TranscriptLoader::new(dir).expect("temp dir should be valid");

        // Build a string that is one byte over the limit.
        let oversized = "x".repeat(super::MAX_INLINE_BYTES + 1);
        let error = loader
            .load("00000000-0000-0000-0000-000000000000", "", Some(&oversized))
            .expect_err("oversize inline should be rejected");

        match error {
            TranscriptError::InvalidReference(msg) => {
                assert!(
                    msg.contains("transcript_inline exceeds"),
                    "error message should name the limit, got: {msg}"
                );
            }
            other => panic!("expected InvalidReference, got {other:?}"),
        }
    }

    #[test]
    fn from_environment_fails_when_transcript_root_is_missing() {
        let _env_guard = without_transcript_root();

        let error = match TranscriptLoader::from_environment() {
            Ok(_) => panic!("loader should reject missing transcript root"),
            Err(error) => error,
        };

        match error {
            TranscriptError::InvalidRoot(message) => {
                assert!(message.contains("CLAUDE_TRANSCRIPT_ROOT is not set"));
            }
            other => panic!("expected invalid root error, got {other:?}"),
        }
    }
}
