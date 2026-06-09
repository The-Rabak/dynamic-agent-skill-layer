use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainId(String);

impl DomainId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let value = raw.into();
        let trimmed = value.trim();

        if trimmed.is_empty() {
            return Err(DomainError::InvalidIdentifier(
                "identifier cannot be blank".to_owned(),
            ));
        }

        if trimmed.chars().any(char::is_whitespace) {
            return Err(DomainError::InvalidIdentifier(
                "identifier cannot contain whitespace".to_owned(),
            ));
        }

        Ok(Self(trimmed.to_owned()))
    }

    pub fn new_unchecked(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScopeType {
    Project,
    Global,
    Team,
}

impl ScopeType {
    /// Returns the canonical lowercase string label for this scope variant.
    ///
    /// This is the single source of truth for any code that needs to produce a
    /// human- or agent-visible scope label (e.g. `scope=project` in match-reason
    /// output). Using `as_str()` instead of `format!("{:?}", …).to_lowercase()`
    /// ensures a compile-time exhaustiveness check: adding a new variant without
    /// updating this match will be a compile error.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
            Self::Team => "team",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleStatus {
    Draft,
    Active,
    Retired,
    Rejected,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillStatus {
    Draft,
    Ready,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubunitType {
    Procedure,
    Convention,
    Asset,
    Evidence,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: DomainId,
    pub name: String,
    pub description: String,
    pub scope: ScopeType,
    pub status: SkillStatus,
    pub lifecycle: LifecycleStatus,
    pub tags: Vec<String>,
    pub subunit_ids: Vec<DomainId>,
    pub community_id: Option<DomainId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subunit {
    pub id: DomainId,
    pub skill_id: DomainId,
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
    pub lifecycle: LifecycleStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Community {
    pub id: DomainId,
    pub name: String,
    pub description: String,
    pub scope: ScopeType,
    pub lifecycle: LifecycleStatus,
    pub member_skill_ids: Vec<DomainId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDescriptor {
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub paths: Vec<PathBuf>,
    pub config: BTreeMap<String, String>,
}

/// A scope's filesystem root: the directory the graph builder, watcher, and
/// maintenance workers treat as the boundary of a single scope.
///
/// Relocated to `domain` because its fields are pure domain values
/// (`ScopeType` + identifiers/paths) shared across `graph-builder`,
/// `maintenance`, and `mcp-server`. `graph_builder::ScopeRoot` remains as a
/// transitional re-export alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRoot {
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub root: PathBuf,
}

impl ScopeRoot {
    pub fn new(scope_id: impl Into<String>, scope_type: ScopeType, root: PathBuf) -> Self {
        Self {
            scope_id: scope_id.into(),
            scope_type,
            root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub speaker: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTranscript {
    pub session_id: DomainId,
    pub entries: Vec<TranscriptEntry>,
}

/// A single typed event in a Claude Code session transcript.
///
/// This is the **rich** event model that sits alongside `TranscriptEntry`/`SessionTranscript`.
/// It is **additive** — existing consumers that read the flat `SessionTranscript` are unaffected.
/// The structured variants carry load-bearing fields needed by the extraction-scaling epic:
/// tool names, exit codes, error flags, and edited file paths.
///
/// ## Wire shape (observed from real Claude Code `.jsonl` files)
///
/// Claude Code session files have top-level `type` fields:
/// - `"user"` — human turn OR tool result: `message.content` is either a plain string
///   or an array of `{type:"tool_result", tool_use_id, content: string, is_error?: bool}`.
/// - `"assistant"` — model turn: `message.content` is an array of typed blocks:
///   `{type:"thinking"}`, `{type:"text", text}`, `{type:"tool_use", id, name, input}`.
/// - All other `type` values (`"mode"`, `"attachment"`, `"ai-title"`, …) are not
///   conversational and are mapped to `SessionEvent::Metadata`.
///
/// Exit codes are NOT a separate JSON field — they appear as `"Exit code N\n..."` prefixed
/// inside the `content` string of a `tool_result` block when `is_error: true`.
///
/// File-editing events (Write, Edit) are `tool_use` blocks on the assistant turn with
/// `name` in `{"Write", "Edit", "MultiEdit"}` and `input.file_path`.
///
/// Each variant carries an `index` that reflects source-file line order, ensuring
/// deterministic ordering of events in downstream processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionEvent {
    /// A human-authored message turn (`role: "user"` with plain-text content).
    UserMessage {
        /// Zero-based line index in the source JSONL, for deterministic ordering.
        index: usize,
        content: String,
    },
    /// A model-authored message turn (`role: "assistant"` text blocks).
    AssistantMessage {
        /// Zero-based line index in the source JSONL, for deterministic ordering.
        index: usize,
        content: String,
    },
    /// A tool invocation issued by the assistant (`tool_use` content block).
    ///
    /// `name` is the tool name (e.g. `"Bash"`, `"Read"`, `"Write"`, `"Edit"`).
    /// `input` is the raw JSON input object serialized to a string; downstream
    /// stages parse what they need without forcing all callers to handle every variant.
    ToolCall {
        /// Zero-based line index of the assistant turn that issued this call.
        index: usize,
        /// Stable `tool_use_id` from the wire format, used to correlate with `ToolResult`.
        tool_use_id: String,
        /// Tool name as emitted by Claude Code (e.g. `"Bash"`, `"Write"`, `"Edit"`).
        name: String,
        /// Raw JSON-serialized tool input object. Preserved verbatim so callers
        /// choose what fields to extract without lossy re-encoding here.
        input_json: String,
    },
    /// The result of a tool invocation (`tool_result` content block in a user turn).
    ///
    /// `exit_code` is parsed from a `"Exit code N\n"` prefix in `output` when
    /// `is_error` is true; absent otherwise. `output` is the full content string
    /// with the exit-code prefix still present, so callers can re-derive or display it.
    ToolResult {
        /// Zero-based line index of the user turn carrying this result.
        index: usize,
        /// `tool_use_id` from the wire format, correlates with a `ToolCall`.
        tool_use_id: String,
        /// `true` when the `tool_result` block carries `"is_error": true`.
        is_error: bool,
        /// Exit code parsed from the `"Exit code N\n"` content prefix, if present.
        /// `None` when the content does not start with that prefix.
        exit_code: Option<i32>,
        /// Full tool output string from the `content` field of the `tool_result` block.
        output: String,
    },
    /// A file-edit operation issued by the assistant via Write, Edit, or MultiEdit.
    ///
    /// Derived from `tool_use` blocks whose `name` is in `{"Write","Edit","MultiEdit"}`.
    /// Carries the target `path` and a compact `summary` line for display, while the
    /// raw input JSON is also available via the corresponding `ToolCall` event.
    FileEdit {
        /// Zero-based line index of the assistant turn that issued this edit.
        index: usize,
        /// `tool_use_id` from the wire format, correlates with a `ToolResult`.
        tool_use_id: String,
        /// Path of the file being created or modified, as-is from the tool input.
        path: String,
        /// Operation name: `"Write"`, `"Edit"`, or `"MultiEdit"`.
        operation: String,
    },
    /// A non-conversational Claude Code session event (mode, attachment, ai-title, etc.).
    ///
    /// These lines are present in real session files but carry no extractable dialogue.
    /// They are preserved with their raw JSON to avoid silent drops.
    Metadata {
        /// Zero-based line index in the source JSONL.
        index: usize,
        /// The `type` field from the raw JSON line.
        event_type: String,
    },
}

impl SessionEvent {
    /// Returns the zero-based source-line index for deterministic ordering.
    pub fn index(&self) -> usize {
        match self {
            Self::UserMessage { index, .. }
            | Self::AssistantMessage { index, .. }
            | Self::ToolCall { index, .. }
            | Self::ToolResult { index, .. }
            | Self::FileEdit { index, .. }
            | Self::Metadata { index, .. } => *index,
        }
    }

    /// Returns `true` when this event is a conversational turn (user or assistant
    /// message text) suitable for inclusion in a flat transcript view.
    pub fn is_conversational(&self) -> bool {
        matches!(
            self,
            Self::UserMessage { .. } | Self::AssistantMessage { .. }
        )
    }

    /// Derives a flat `TranscriptEntry` from this event, if it has a conversational
    /// speaker/content representation.
    ///
    /// Returns `None` for `ToolCall`, `ToolResult`, `FileEdit`, and `Metadata` events
    /// since they have no direct `speaker: content` rendering in the legacy flat model.
    pub fn as_transcript_entry(&self) -> Option<TranscriptEntry> {
        match self {
            Self::UserMessage { content, .. } => Some(TranscriptEntry {
                speaker: "user".to_owned(),
                content: content.clone(),
            }),
            Self::AssistantMessage { content, .. } => Some(TranscriptEntry {
                speaker: "assistant".to_owned(),
                content: content.clone(),
            }),
            _ => None,
        }
    }
}

/// Derives a flat `SessionTranscript` from an ordered slice of `SessionEvent`s.
///
/// This is the compatibility bridge for the post-#183 extraction pipeline:
/// `ToolCall`, `ToolResult`, `FileEdit`, and `Metadata` events are skipped;
/// `UserMessage` and `AssistantMessage` events are mapped to `TranscriptEntry`
/// in source-line order. The result is byte-identical to parsing the same JSONL
/// with the legacy `parse_claude_jsonl` path.
///
/// # Panics
/// Never panics; the session ID is taken from the caller since `Vec<SessionEvent>`
/// does not carry it.
pub fn events_to_transcript(session_id: DomainId, events: &[SessionEvent]) -> SessionTranscript {
    let entries = events
        .iter()
        .filter_map(SessionEvent::as_transcript_entry)
        .collect();
    SessionTranscript {
        session_id,
        entries,
    }
}

/// Advisory hint describing whether an extracted skill lesson is project-specific
/// or broadly applicable across projects.
///
/// This is a data field for the downstream maintenance promotion pass (#179) — it
/// is NEVER used as a routing decision during extraction. Extraction always writes
/// project-local regardless of this value.
///
/// The valid string values are exactly: `"project"`, `"general"`, `"uncertain"`.
/// `None` (absent from provider JSON) is treated as `"uncertain"` at mapping time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedSkillCandidate {
    pub name: String,
    pub description: String,
    // Enrichment fields default when absent. Local models (gemma4:12b) occasionally
    // emit partial/truncated JSON — e.g. an unescaped inner quote in a procedure
    // string (`flavor = "multi_thread"`) prematurely closes the object and drops the
    // trailing fields. A candidate carrying name + description (+ usually procedures)
    // is still a useful, actionable skill; rejecting the WHOLE candidate because a
    // secondary field is missing throws away real extraction output. These fields
    // therefore tolerate omission (empty / zero) rather than failing deserialization.
    // `name` and `description` stay required — a candidate without them is meaningless.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub procedures: Vec<String>,
    #[serde(default)]
    pub conventions: Vec<String>,
    #[serde(default)]
    pub assets: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
    /// Advisory scope-generality hint captured by the LLM while the full transcript
    /// is available. Values: `"project"`, `"general"`, `"uncertain"`.
    /// `#[serde(default)]` means absent JSON fields deserialise to `None` so
    /// old provider responses (Ollama, Claude) remain backward-compatible.
    #[serde(default)]
    pub generality: Option<String>,
    /// One-line rationale for the `generality` judgement from the LLM.
    #[serde(default)]
    pub generality_rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub source_session_id: DomainId,
    pub candidates: Vec<ExtractedSkillCandidate>,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredSkill {
    pub skill: Skill,
    pub score: f32,
    pub matched_scope: ScopeType,
    pub rationale: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{ExtractedSkillCandidate, ScopeType};

    #[test]
    fn scope_type_as_str_returns_lowercase_label_for_each_variant() {
        assert_eq!(ScopeType::Project.as_str(), "project");
        assert_eq!(ScopeType::Global.as_str(), "global");
        assert_eq!(ScopeType::Team.as_str(), "team");
    }

    #[test]
    fn candidate_deserializes_when_local_model_truncates_trailing_fields() {
        // Exact failure mode observed live: gemma4:12b emitted a candidate whose
        // procedure string contained an unescaped inner quote
        // (`flavor = "multi_thread"`), prematurely closing the object so that
        // `conventions`, `assets`, and `confidence` never appeared. A candidate with
        // name + description + procedures is still useful; it must NOT be discarded
        // wholesale for the missing trailing fields.
        let truncated = r#"{
            "name": "diagnose-and-fix-fd-exhaustion",
            "description": "Resolve file-descriptor exhaustion causing WouldBlock in Tokio tasks.",
            "tags": ["tokio", "debugging"],
            "procedures": ["Run `ulimit -n 65536` before starting the app."]
        }"#;

        let candidate: ExtractedSkillCandidate =
            serde_json::from_str(truncated).expect("partial candidate must still deserialize");

        assert_eq!(candidate.name, "diagnose-and-fix-fd-exhaustion");
        assert_eq!(candidate.procedures.len(), 1);
        // Omitted enrichment fields default rather than failing the whole parse.
        assert!(candidate.conventions.is_empty());
        assert!(candidate.assets.is_empty());
        assert_eq!(candidate.confidence, 0.0);
        assert!(candidate.generality.is_none());
    }

    #[test]
    fn candidate_without_name_or_description_still_fails_loud() {
        // name + description remain required — a candidate without them is meaningless
        // and must NOT silently deserialize to an empty skill.
        let no_name = r#"{ "description": "x", "procedures": ["a"] }"#;
        assert!(
            serde_json::from_str::<ExtractedSkillCandidate>(no_name).is_err(),
            "a candidate missing `name` must fail deserialization"
        );
        let no_desc = r#"{ "name": "x", "procedures": ["a"] }"#;
        assert!(
            serde_json::from_str::<ExtractedSkillCandidate>(no_desc).is_err(),
            "a candidate missing `description` must fail deserialization"
        );
    }
}
