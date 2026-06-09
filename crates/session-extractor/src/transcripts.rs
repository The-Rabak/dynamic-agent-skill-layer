use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use domain::{DomainId, SessionEvent, SessionTranscript, TranscriptEntry};
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, warn};

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

    /// Loads raw JSONL payload bytes from either inline content or a file reference,
    /// without parsing.
    ///
    /// Used by the orchestrated extraction path which needs to pass the raw JSONL to
    /// [`parse_session_events`] rather than the flat [`SessionTranscript`] parser.
    /// The same size limit as [`Self::load`] is enforced on inline payloads.
    pub fn load_raw(
        &self,
        transcript_ref: &str,
        transcript_inline: Option<&str>,
    ) -> Result<String, TranscriptError> {
        if let Some(inline_payload) = transcript_inline {
            if inline_payload.len() > MAX_INLINE_BYTES {
                return Err(TranscriptError::InvalidReference(format!(
                    "transcript_inline exceeds {MAX_INLINE_BYTES} bytes"
                )));
            }
            Ok(inline_payload.to_owned())
        } else {
            let path = self.validate_ref(transcript_ref)?;
            fs::read_to_string(&path).map_err(|error| {
                TranscriptError::ReadFailure(path.display().to_string(), error.to_string())
            })
        }
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

/// Parses a Claude Code JSONL payload into the flat [`SessionTranscript`] format.
///
/// Extracts only `speaker: content` pairs. Non-conversational meta lines emitted by
/// Claude Code sessions (`{"type":"mode",...}`, `{"type":"summary",...}`,
/// `{"type":"file-history-snapshot",...}`, tool-use/tool-result blocks, etc.) are
/// skipped — they carry no role and no conversational content.
///
/// Prefer [`parse_session_events`] for the orchestrated path, which preserves rich event
/// types (tool calls, file edits, tool results) rather than only flat speaker/content pairs.
///
/// The zero-conversational-entries guard is absolute: a transcript that contains NO
/// speaker/content entries after skipping all meta lines fails loud with
/// `"transcript contains no conversational entries"`. A genuinely empty transcript must
/// not silently succeed.
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

        // Skip non-conversational Claude Code session events (mode, summary,
        // file-history-snapshot, system, tool_use/tool_result blocks, etc.).
        // These lines have a "type" field but no speaker/role — they are valid
        // wire format but carry no conversational content.
        if extract_speaker(&value).is_none() {
            let event_type = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("<no type>");
            debug!(
                line = index + 1,
                event_type,
                "parse_claude_jsonl: skipping non-conversational meta line"
            );
            continue;
        }

        // extract_speaker returned Some above; re-extracting is cheap and avoids
        // carrying a side-effecting binding across the skip guard.
        let speaker = extract_speaker(&value)
            .expect("speaker checked Some before this branch; extracting again cannot be None");

        // Skip turns that have a speaker/role but no extractable text content.
        // This covers tool-use-only assistant turns (e.g. a Bash call with no text block)
        // which carry a role but no conversational prose — they add nothing to the flat
        // transcript. The zero-entries guard below still catches empty transcripts.
        let Some(content) = extract_content(&value) else {
            debug!(
                line = index + 1,
                speaker,
                "parse_claude_jsonl: skipping speaker turn with no extractable text content"
            );
            continue;
        };

        entries.push(TranscriptEntry { speaker, content });
    }

    // Fail loud when the transcript has no conversational content at all.
    // This catches genuinely empty transcripts and session files that contain
    // only metadata (e.g. a session that captured no human/assistant turns).
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

/// Parses a Claude Code JSONL payload into an ordered sequence of [`SessionEvent`]s.
///
/// This is the **rich** parsing path that preserves tool calls, tool results (with exit
/// codes and error flags), file-edit events, and non-conversational metadata — as opposed
/// to [`parse_claude_jsonl`] which only extracts `speaker: content` pairs.
///
/// ## Wire shape handled
///
/// Each JSONL line has a top-level `type` field:
/// - `"user"` with `message.content` as a **string** → [`SessionEvent::UserMessage`]
/// - `"user"` with `message.content` as an **array** containing `tool_result` blocks →
///   [`SessionEvent::ToolResult`] per block (with `is_error` and parsed `exit_code`)
/// - `"assistant"` with `message.content` blocks:
///   - `{type:"text", text}` → [`SessionEvent::AssistantMessage`]
///   - `{type:"tool_use", id, name, input}` where `name` ∈ {Write, Edit, MultiEdit} →
///     emits BOTH a [`SessionEvent::FileEdit`] and a [`SessionEvent::ToolCall`]
///   - `{type:"tool_use", ...}` for all other tools → [`SessionEvent::ToolCall`]
///   - `{type:"thinking"}` → skipped (internal reasoning, not conversational)
/// - All other `type` values → [`SessionEvent::Metadata`]
///
/// ## Error handling
///
/// Lines that are not valid JSON are counted, logged, and emitted as a
/// `SessionEvent::UserMessage` with content `"[unparseable line N]"` — they are never
/// silently dropped. The `malformed_count` in the returned [`ParsedEvents`] lets callers
/// surface this to the user.
///
/// Unknown or future-shaped `tool_use` or `tool_result` blocks are mapped to the
/// best-fitting typed variant (ToolCall / ToolResult) without panicking.
///
/// ## Ordering
///
/// The `index` on every event is the zero-based source-line index from the JSONL file,
/// providing a stable, deterministic ordering that survives any downstream shuffle.
pub fn parse_session_events(jsonl_payload: &str) -> ParsedEvents {
    let mut events = Vec::new();
    let mut malformed_count: usize = 0;

    for (line_index, line) in jsonl_payload.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(error) => {
                warn!(
                    line = line_index + 1,
                    error = %error,
                    "transcript line is not valid JSON; mapping to placeholder UserMessage"
                );
                malformed_count += 1;
                events.push(SessionEvent::UserMessage {
                    index: line_index,
                    content: format!("[unparseable line {}]", line_index + 1),
                });
                continue;
            }
        };

        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        match event_type {
            "user" => {
                let message_content = value.pointer("/message/content");
                match message_content {
                    // Plain string content → human text turn
                    Some(Value::String(text)) => {
                        let trimmed_text = text.trim().to_owned();
                        if !trimmed_text.is_empty() {
                            events.push(SessionEvent::UserMessage {
                                index: line_index,
                                content: trimmed_text,
                            });
                        } else {
                            debug!(
                                line = line_index + 1,
                                "user line has empty string content; skipping"
                            );
                        }
                    }
                    // Array content → tool_result blocks
                    Some(Value::Array(blocks)) => {
                        for block in blocks {
                            let block_type =
                                block.get("type").and_then(Value::as_str).unwrap_or("");
                            if block_type == "tool_result" {
                                events.push(parse_tool_result_block(block, line_index));
                            } else {
                                debug!(
                                    line = line_index + 1,
                                    block_type,
                                    "unexpected block type in user message content; skipping"
                                );
                            }
                        }
                    }
                    None => {
                        // Fall back to top-level content or speaker+content shape for
                        // legacy inline JSONL (the format used by tests/fixtures/*.jsonl).
                        if let Some(speaker) = extract_speaker(&value) {
                            if let Some(content) = extract_content(&value) {
                                let event = if speaker == "user" {
                                    SessionEvent::UserMessage {
                                        index: line_index,
                                        content,
                                    }
                                } else {
                                    SessionEvent::AssistantMessage {
                                        index: line_index,
                                        content,
                                    }
                                };
                                events.push(event);
                            } else {
                                debug!(
                                    line = line_index + 1,
                                    "user-type line missing content; skipping"
                                );
                            }
                        } else {
                            debug!(
                                line = line_index + 1,
                                "user-type line has no message.content and no speaker; skipping"
                            );
                        }
                    }
                    Some(_unexpected) => {
                        warn!(
                            line = line_index + 1,
                            "user message.content has unexpected type; mapping to placeholder"
                        );
                        malformed_count += 1;
                        events.push(SessionEvent::UserMessage {
                            index: line_index,
                            content: format!(
                                "[unexpected content shape on line {}]",
                                line_index + 1
                            ),
                        });
                    }
                }
            }
            "assistant" => {
                // An assistant turn has an array of typed content blocks.
                let content_blocks = value.pointer("/message/content").and_then(Value::as_array);

                match content_blocks {
                    Some(blocks) => {
                        for block in blocks {
                            let block_type =
                                block.get("type").and_then(Value::as_str).unwrap_or("");
                            match block_type {
                                "text" => {
                                    let text = block
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .trim()
                                        .to_owned();
                                    if !text.is_empty() {
                                        events.push(SessionEvent::AssistantMessage {
                                            index: line_index,
                                            content: text,
                                        });
                                    }
                                }
                                "tool_use" => {
                                    events.extend(parse_tool_use_block(block, line_index));
                                }
                                "thinking" => {
                                    // Internal reasoning — not conversational, not useful for
                                    // extraction. Skip without counting as malformed.
                                    debug!(line = line_index + 1, "skipping thinking block");
                                }
                                other => {
                                    debug!(
                                        line = line_index + 1,
                                        block_type = other,
                                        "unknown assistant content block type; skipping"
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        // Inline JSONL fallback: top-level speaker+content format.
                        if let Some(content) = extract_content(&value) {
                            events.push(SessionEvent::AssistantMessage {
                                index: line_index,
                                content,
                            });
                        } else {
                            debug!(
                                line = line_index + 1,
                                "assistant line missing content blocks; skipping"
                            );
                        }
                    }
                }
            }
            // Legacy inline JSONL shape: {"type":"message","message":{"role":"user","content":"..."}}
            "message" => {
                if let Some(speaker) = extract_speaker(&value) {
                    if let Some(content) = extract_content(&value) {
                        let event = if speaker == "user" {
                            SessionEvent::UserMessage {
                                index: line_index,
                                content,
                            }
                        } else {
                            SessionEvent::AssistantMessage {
                                index: line_index,
                                content,
                            }
                        };
                        events.push(event);
                    } else {
                        debug!(
                            line = line_index + 1,
                            "message-type line missing content; skipping"
                        );
                    }
                } else {
                    debug!(
                        line = line_index + 1,
                        "message-type line missing role/speaker; skipping"
                    );
                }
            }
            other => {
                // Bare / legacy JSONL shape with NO "type" field but a top-level
                // speaker/role + content, e.g. {"speaker":"user","content":"..."} or
                // {"role":"assistant","content":"..."}. This is the shape the original
                // `parse_claude_jsonl` accepted and the shape many inline ingest payloads
                // use — it is a real conversational turn, NOT non-conversational metadata.
                // Misclassifying it as Metadata starves the extractor of all content.
                if let (Some(speaker), Some(content)) =
                    (extract_speaker(&value), extract_content(&value))
                {
                    let event = if speaker == "user" {
                        SessionEvent::UserMessage {
                            index: line_index,
                            content,
                        }
                    } else {
                        SessionEvent::AssistantMessage {
                            index: line_index,
                            content,
                        }
                    };
                    events.push(event);
                } else {
                    // Genuinely non-conversational Claude Code session events
                    // (mode, attachment, etc.) → Metadata.
                    debug!(
                        line = line_index + 1,
                        event_type = other,
                        "non-conversational event mapped to Metadata"
                    );
                    events.push(SessionEvent::Metadata {
                        index: line_index,
                        event_type: other.to_owned(),
                    });
                }
            }
        }
    }

    ParsedEvents {
        events,
        malformed_count,
    }
}

/// Holds the output of [`parse_session_events`].
///
/// `events` are in source-line order (stable by `index`). `malformed_count` is the
/// number of lines that were not valid JSON or had an irrecoverably unexpected shape;
/// each such line was mapped to a placeholder `UserMessage` rather than dropped.
#[derive(Debug)]
pub struct ParsedEvents {
    pub events: Vec<SessionEvent>,
    /// Count of JSONL lines that were not valid JSON (each was logged and mapped to a
    /// placeholder `UserMessage` rather than silently discarded).
    pub malformed_count: usize,
}

/// Parses a `tool_use` content block (from an assistant turn) into one or two
/// [`SessionEvent`]s.
///
/// File-editing tools (Write, Edit, MultiEdit) emit a `FileEdit` event followed by a
/// `ToolCall` event so callers interested only in file edits can match just `FileEdit`,
/// while callers that need the raw input JSON can correlate via `tool_use_id`.
/// All other tools emit only a `ToolCall` event.
fn parse_tool_use_block(block: &Value, line_index: usize) -> Vec<SessionEvent> {
    let tool_use_id = block
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let input_json = block
        .get("input")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_owned());

    let mut result = Vec::new();

    // File-editing tools carry a target file path that is load-bearing for #185/#188.
    if matches!(name.as_str(), "Write" | "Edit" | "MultiEdit") {
        let path = block
            .pointer("/input/file_path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        result.push(SessionEvent::FileEdit {
            index: line_index,
            tool_use_id: tool_use_id.clone(),
            path,
            operation: name.clone(),
        });
    }

    result.push(SessionEvent::ToolCall {
        index: line_index,
        tool_use_id,
        name,
        input_json,
    });

    result
}

/// Parses a `tool_result` content block (from a user turn) into a [`SessionEvent::ToolResult`].
///
/// `exit_code` is derived from a `"Exit code N\n"` prefix in the `content` string, which
/// is how Claude Code encodes non-zero Bash exit codes. The prefix is detected but the full
/// `output` string is preserved verbatim so callers can display or re-derive as needed.
fn parse_tool_result_block(block: &Value, line_index: usize) -> SessionEvent {
    let tool_use_id = block
        .get("tool_use_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let is_error = block
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let output = block
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    let exit_code = extract_exit_code_from_output(&output);

    SessionEvent::ToolResult {
        index: line_index,
        tool_use_id,
        is_error,
        exit_code,
        output,
    }
}

/// Parses the `"Exit code N\n"` prefix from a Bash tool result's content string.
///
/// Claude Code embeds the exit code as the first line of the content when a Bash command
/// exits non-zero. The prefix format is exactly `"Exit code "` followed by an integer and
/// a newline. Returns `None` when the output does not start with this prefix.
fn extract_exit_code_from_output(output: &str) -> Option<i32> {
    let prefix = "Exit code ";
    if !output.starts_with(prefix) {
        return None;
    }
    let rest = &output[prefix.len()..];
    let digits = rest.split('\n').next().unwrap_or("").trim();
    digits.parse::<i32>().ok()
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

    use domain::{DomainId, SessionEvent, events_to_transcript};

    use super::{ParsedEvents, TranscriptError, TranscriptLoader, parse_claude_jsonl, parse_session_events};

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

    // ── parse_session_events tests ──────────────────────────────────────────────

    /// Returns the path to a fixture file relative to the workspace root.
    ///
    /// `CARGO_MANIFEST_DIR` for session-extractor is `crates/session-extractor/`;
    /// fixtures live in `tests/fixtures/` at the workspace root.
    fn fixture_path(name: &str) -> std::path::PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        std::path::Path::new(manifest_dir)
            .join("../../tests/fixtures")
            .join(name)
    }

    fn read_fixture(name: &str) -> String {
        let path = fixture_path(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("could not read fixture {}: {err}", path.display()))
    }

    /// AC1 — The real Claude Code JSONL fixture parses into typed SessionEvents with
    /// tool calls, tool results (incl. exit codes / is_error flags), and file edits.
    #[test]
    fn real_fixture_parses_tool_calls_results_and_file_edits() {
        let payload = read_fixture("claude-code-session-real.jsonl");
        let ParsedEvents {
            events,
            malformed_count,
        } = parse_session_events(&payload);

        assert_eq!(
            malformed_count, 0,
            "real fixture should have no malformed lines"
        );
        assert!(!events.is_empty(), "events must not be empty");

        // Must contain at least one UserMessage (the human text turn)
        let user_messages: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::UserMessage { .. }))
            .collect();
        assert!(
            !user_messages.is_empty(),
            "must parse at least one UserMessage"
        );

        // Must contain at least one ToolCall (Bash tool)
        let tool_calls: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolCall { .. }))
            .collect();
        assert!(!tool_calls.is_empty(), "must parse at least one ToolCall");

        // The Bash tool call must be present
        let bash_call = tool_calls
            .iter()
            .find(|e| matches!(e, SessionEvent::ToolCall { name, .. } if name == "Bash"));
        assert!(bash_call.is_some(), "must parse a Bash ToolCall");

        // Must contain at least one ToolResult
        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolResult { .. }))
            .collect();
        assert!(
            !tool_results.is_empty(),
            "must parse at least one ToolResult"
        );

        // Must parse a ToolResult with is_error=false
        let ok_result = tool_results.iter().find(|e| {
            matches!(
                e,
                SessionEvent::ToolResult {
                    is_error: false,
                    ..
                }
            )
        });
        assert!(
            ok_result.is_some(),
            "must parse at least one non-error ToolResult"
        );

        // Must parse a ToolResult with is_error=true and exit_code Some(2)
        let error_result = tool_results.iter().find(|e| {
            matches!(
                e,
                SessionEvent::ToolResult {
                    is_error: true,
                    exit_code: Some(2),
                    ..
                }
            )
        });
        assert!(
            error_result.is_some(),
            "must parse a ToolResult with is_error=true and exit_code=Some(2); results: {tool_results:?}"
        );

        // Must contain at least one FileEdit (Write tool)
        let file_edits: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::FileEdit { .. }))
            .collect();
        assert!(!file_edits.is_empty(), "must parse at least one FileEdit");

        let write_edit = file_edits.iter().find(
            |e| matches!(e, SessionEvent::FileEdit { operation, .. } if operation == "Write"),
        );
        assert!(write_edit.is_some(), "must parse a Write FileEdit");

        // FileEdit path must be non-empty and contain the sanitized repo marker
        if let Some(SessionEvent::FileEdit { path, .. }) = write_edit {
            assert!(!path.is_empty(), "FileEdit path must not be empty");
            assert!(
                path.contains("<repo>"),
                "sanitized path must contain <repo> marker"
            );
        }

        // Events must be in source-line order (non-decreasing index)
        let indices: Vec<usize> = events.iter().map(SessionEvent::index).collect();
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(indices, sorted, "events must be in source-line order");
    }

    /// AC2 — The inline-JSONL ingest path (legacy {type:"message",...} shape) produces
    /// the same UserMessage / AssistantMessage event model.
    #[test]
    fn inline_jsonl_ingest_path_produces_event_model() {
        // This is the shape used by tests/fixtures/sample-transcript.jsonl and the inline path.
        let payload = r#"{"type":"message","message":{"role":"user","content":"I keep repeating Rust file I/O setup steps in new repos."}}
{"type":"message","message":{"role":"assistant","content":"Capture a reusable skill covering safe read/write helpers and explicit Result handling."}}
"#;

        let ParsedEvents {
            events,
            malformed_count,
        } = parse_session_events(payload);

        assert_eq!(malformed_count, 0);
        assert_eq!(events.len(), 2);

        assert!(
            matches!(&events[0], SessionEvent::UserMessage { content, .. } if content.contains("Rust file I/O")),
            "first event must be UserMessage with 'Rust file I/O'; got {:?}",
            events[0]
        );
        assert!(
            matches!(&events[1], SessionEvent::AssistantMessage { content, .. } if content.contains("reusable skill")),
            "second event must be AssistantMessage with 'reusable skill'; got {:?}",
            events[1]
        );

        // Indices must be 0-based source-line order
        assert_eq!(events[0].index(), 0);
        assert_eq!(events[1].index(), 1);
    }

    /// AC2b — The real Claude Code "user"/"assistant" wire shape (no "type":"message" wrapper)
    /// also produces UserMessage / AssistantMessage events via the inline path.
    #[test]
    fn real_wire_user_assistant_shape_parses_to_events() {
        let payload = read_fixture("claude-code-session-real.jsonl");
        let ParsedEvents { events, .. } = parse_session_events(&payload);

        // There should be at least one UserMessage from the human text turn.
        // (Line 0 is the fixture metadata header → Metadata event; user text is line 1.)
        let user_msg = events
            .iter()
            .find(|e| matches!(e, SessionEvent::UserMessage { .. }));
        assert!(
            user_msg.is_some(),
            "real fixture must produce at least one UserMessage"
        );
    }

    /// AC3 — flat_lines() derived from events is byte-compatible with the legacy parse path.
    ///
    /// `events_to_transcript()` produces a `SessionTranscript` from the events. When that
    /// transcript is rendered by `render_sanitized_transcript_lines` it must be identical to
    /// rendering the transcript produced by the legacy `parse_claude_jsonl` path from the
    /// same inline JSONL input.
    ///
    /// Scope: we test against the inline JSONL shape (the shape `TranscriptLoader::load`
    /// uses for inline payloads) to avoid I/O in unit tests. The renderer lives in
    /// `infrastructure` which is not a dependency of `session-extractor`, so we verify the
    /// flat `TranscriptEntry` sequence is identical instead — the renderer is a pure function
    /// of those entries, so identical entries guarantees identical rendered output.
    #[test]
    fn flat_view_from_events_is_byte_compatible_with_legacy_parse_path() {
        let payload = r#"{"type":"message","message":{"role":"user","content":"I keep repeating the same Rust file I/O setup in every new repo and it wastes time. Can we make it reusable?"}}
{"type":"message","message":{"role":"assistant","content":"Let's capture a reusable skill."}}
{"type":"message","message":{"role":"user","content":"Right, and we should create the parent directory before writing a file."}}
"#;

        // Legacy path: parse_claude_jsonl (via TranscriptLoader::load)
        let loader = TranscriptLoader::new(std::env::temp_dir()).expect("temp dir should be valid");
        let legacy_transcript = loader
            .load("00000000-0000-0000-0000-000000000001", "", Some(payload))
            .expect("legacy parse must succeed");

        // New path: parse_session_events → events_to_transcript
        let ParsedEvents { events, .. } = parse_session_events(payload);
        let session_id = DomainId::parse("00000000-0000-0000-0000-000000000001".to_owned())
            .expect("session id must parse");
        let events_transcript = events_to_transcript(session_id, &events);

        // The two transcripts must produce identical flat entries (order + content).
        assert_eq!(
            legacy_transcript.entries, events_transcript.entries,
            "events_to_transcript entries must be byte-identical to legacy parse entries"
        );
    }

    /// AC4 — Malformed and foreign-shaped lines are counted and never silently dropped.
    #[test]
    fn malformed_and_foreign_lines_are_counted_and_mapped_to_placeholder() {
        let payload = read_fixture("claude-code-session-edge-cases.jsonl");
        let ParsedEvents {
            events,
            malformed_count,
        } = parse_session_events(&payload);

        // The malformed JSON line must be counted
        assert_eq!(
            malformed_count, 1,
            "exactly one malformed JSON line must be counted; got {malformed_count}"
        );

        // No events should be silently absent: every non-empty line must produce at least
        // one event (except the malformed line, which still produces a placeholder UserMessage).
        // Non-empty lines: 6; empty payload lines: 0.
        // Line breakdown:
        //   0: user text → UserMessage
        //   1: malformed JSON → placeholder UserMessage (counted)
        //   2: mode event → Metadata
        //   3: assistant with unknown tool → ToolCall
        //   4: user with tool_result → ToolResult
        //   5: no type field → Metadata (unknown type)
        assert!(
            events.len() >= 5,
            "must have at least 5 events (all lines accounted for); got {} events: {events:#?}",
            events.len()
        );

        // The malformed line must have been mapped to a placeholder UserMessage
        let placeholder = events.iter().find(|e| {
            matches!(e, SessionEvent::UserMessage { content, .. } if content.contains("unparseable line"))
        });
        assert!(
            placeholder.is_some(),
            "malformed line must produce a placeholder UserMessage, not be silently dropped"
        );

        // The foreign/unknown tool_use must still produce a ToolCall (not be dropped)
        let unknown_tool_call = events.iter().find(
            |e| matches!(e, SessionEvent::ToolCall { name, .. } if name == "UnknownFutureTool"),
        );
        assert!(
            unknown_tool_call.is_some(),
            "unknown tool_use must produce a ToolCall (forward-compatibility)"
        );
    }

    /// Regression: the bare legacy JSONL shape `{"speaker":..,"content":..}` with NO
    /// "type" field must parse to conversational User/Assistant messages, NOT Metadata.
    /// Misclassifying these as Metadata starved the orchestrated extractor of all content
    /// and produced zero candidates on inline payloads (the #176 Tokio-repro shape).
    #[test]
    fn bare_speaker_content_lines_parse_to_conversational_messages_not_metadata() {
        let payload = concat!(
            r#"{"speaker":"user","content":"How do I fix WouldBlock under load?"}"#,
            "\n",
            r#"{"speaker":"assistant","content":"Run ulimit -n 65536 then use tokio-console."}"#,
        );
        let ParsedEvents {
            events,
            malformed_count,
        } = parse_session_events(payload);

        assert_eq!(
            malformed_count, 0,
            "well-formed JSON lines must not be counted malformed"
        );
        assert_eq!(
            events.len(),
            2,
            "both turns must produce events; got {events:#?}"
        );
        assert!(
            matches!(&events[0], SessionEvent::UserMessage { content, .. } if content.contains("WouldBlock")),
            "first bare line must be a UserMessage with its content, not Metadata; got {:#?}",
            events[0]
        );
        assert!(
            matches!(&events[1], SessionEvent::AssistantMessage { content, .. } if content.contains("ulimit")),
            "second bare line must be an AssistantMessage with its content, not Metadata; got {:#?}",
            events[1]
        );
        // Critically: NONE of these conversational lines may be dropped to Metadata.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::Metadata { .. })),
            "bare speaker/content lines must never be classified as non-conversational Metadata"
        );
    }

    /// Verifies exit_code parsing from the "Exit code N\n" prefix format.
    #[test]
    fn exit_code_parsed_from_bash_output_prefix() {
        assert_eq!(
            super::extract_exit_code_from_output("Exit code 2\nsome output"),
            Some(2)
        );
        assert_eq!(
            super::extract_exit_code_from_output("Exit code 127\n"),
            Some(127)
        );
        assert_eq!(
            super::extract_exit_code_from_output("normal output without exit code"),
            None
        );
        assert_eq!(super::extract_exit_code_from_output(""), None);
    }

    /// Verifies that tool_use_id correlates between FileEdit and ToolCall for the same edit.
    #[test]
    fn file_edit_and_tool_call_share_tool_use_id() {
        let payload = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_abc123","name":"Write","input":{"file_path":"/tmp/test.md","content":"hello"}}]}}"#;
        let ParsedEvents { events, .. } = parse_session_events(payload);

        // Should produce a FileEdit and a ToolCall, both with the same tool_use_id
        let file_edit = events
            .iter()
            .find(|e| matches!(e, SessionEvent::FileEdit { .. }));
        let tool_call = events
            .iter()
            .find(|e| matches!(e, SessionEvent::ToolCall { name, .. } if name == "Write"));

        assert!(
            file_edit.is_some(),
            "Write tool_use must produce a FileEdit"
        );
        assert!(
            tool_call.is_some(),
            "Write tool_use must also produce a ToolCall"
        );

        if let (
            Some(SessionEvent::FileEdit {
                tool_use_id: fe_id, ..
            }),
            Some(SessionEvent::ToolCall {
                tool_use_id: tc_id, ..
            }),
        ) = (file_edit, tool_call)
        {
            assert_eq!(
                fe_id, tc_id,
                "FileEdit and ToolCall must share the same tool_use_id"
            );
        }
    }

    // ── parse_claude_jsonl real wire shape tests (todo #221) ──────────────────

    /// Real Claude Code wire shape: `{"type":"mode",...}` meta first line, then
    /// user + assistant conversational turns, a tool_use block, and a `{"type":"summary",...}`
    /// trailing line. The single-shot parser must tolerate all non-conversational meta lines
    /// and still extract the conversational entries.
    ///
    /// This is the exact format that broke the live drain before #221: line 1 was a
    /// `{"type":"mode",...}` line with no speaker/role, causing `parse_claude_jsonl` to
    /// reject the transcript with `invalid_transcript_payload: line 1 is missing speaker/role`.
    #[test]
    fn parse_claude_jsonl_tolerates_meta_lines_and_extracts_conversational_entries() {
        // Reproduces the real wire shape from ~/.claude/projects/*.jsonl:
        //   line 1: mode meta event  (no speaker/role)
        //   line 2: user turn        (conversational)
        //   line 3: assistant turn   (conversational)
        //   line 4: tool_use block   (no speaker/role at top level)
        //   line 5: summary meta     (no speaker/role)
        let payload = concat!(
            r#"{"type":"mode","mode":"normal","sessionId":"00000000-0000-0000-0000-000000000221"}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"How do I make a Rust struct derive Debug?"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Add #[derive(Debug)] above the struct definition."}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"echo done"}}]}}"#,
            "\n",
            r#"{"type":"summary","summary":"User asked about Rust derive macro","leafUuid":"abc123"}"#,
        );

        let transcript = parse_claude_jsonl("00000000-0000-0000-0000-000000000221", payload)
            .expect("real wire shape with meta lines must parse without error");

        // Must have extracted exactly the two conversational turns; tool_use/mode/summary skipped.
        assert_eq!(
            transcript.entries.len(),
            2,
            "expected 2 conversational entries (user + assistant text); got {:?}",
            transcript.entries
        );

        assert_eq!(
            transcript.entries[0].speaker, "user",
            "first entry must be the user turn"
        );
        assert!(
            transcript.entries[0].content.contains("derive Debug"),
            "user entry must contain the question content"
        );
        assert_eq!(
            transcript.entries[1].speaker, "assistant",
            "second entry must be the assistant turn"
        );
        assert!(
            transcript.entries[1].content.contains("#[derive(Debug)]"),
            "assistant entry must contain the answer content"
        );
    }

    /// A transcript that contains ONLY meta lines (no user/assistant conversational turns)
    /// must still fail loud with `"transcript contains no conversational entries"`.
    ///
    /// This proves the zero-conversational-entries guard survives the tolerance fix:
    /// skipping meta lines is not the same as accepting an empty transcript.
    #[test]
    fn parse_claude_jsonl_rejects_transcript_with_only_meta_lines() {
        let payload = concat!(
            r#"{"type":"mode","mode":"normal","sessionId":"00000000-0000-0000-0000-000000000221"}"#,
            "\n",
            r#"{"type":"summary","summary":"Nothing happened","leafUuid":"abc123"}"#,
            "\n",
            r#"{"type":"file-history-snapshot","files":[]}"#,
        );

        let error = parse_claude_jsonl("00000000-0000-0000-0000-000000000221", payload)
            .expect_err("all-meta transcript must be rejected with a loud error");

        match error {
            TranscriptError::InvalidPayload(msg) => {
                assert!(
                    msg.contains("no conversational entries"),
                    "error must name the zero-entries condition; got: {msg}"
                );
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }
}
