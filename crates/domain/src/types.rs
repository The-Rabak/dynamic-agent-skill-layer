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

/// Typed relation between two skills in the skill graph (V1.7 SkillDAG-style edges).
///
/// This enum is the single source of truth for inter-skill relation semantics. The
/// graph builder, persistence layer, and (later) the agent-facing retrieval tools must
/// all derive walkability and acyclicity rules from here rather than re-encoding the
/// vocabulary, so the meaning of an edge never diverges across crates.
///
/// Design Decision #4 (parent V1.7 plan): graph structure is SEPARATE evidence, never a
/// scalar rank multiplier. `conflicts_with` is a one-hop prune signal and is deliberately
/// NOT walkable — see [`EdgeType::is_walkable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    /// Source skill requires the target skill as a prerequisite. Backbone edge:
    /// directed-acyclic. Walkable.
    DependsOn,
    /// Source skill is a specialisation of the (more general) target skill. Backbone
    /// edge: directed-acyclic. Walkable.
    Specializes,
    /// The two skills naturally compose together. Walkable, not backbone (cycles allowed).
    ComposesWith,
    /// The two skills are semantically similar. Walkable, not backbone (cycles allowed).
    SimilarTo,
    /// The two skills should NOT be co-selected. Returned as a separate prune signal and
    /// NEVER traversed as a positive neighbour. Not walkable, not backbone.
    ConflictsWith,
}

impl EdgeType {
    /// Canonical lowercase DB label. Single source of truth matching the
    /// `skill_edges.edge_type` CHECK constraint in migration 010.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::DependsOn => "depends_on",
            Self::Specializes => "specializes",
            Self::ComposesWith => "composes_with",
            Self::SimilarTo => "similar_to",
            Self::ConflictsWith => "conflicts_with",
        }
    }

    /// Parses a DB label back into an [`EdgeType`], failing loudly on any value
    /// outside the closed vocabulary rather than defaulting to a placeholder.
    pub fn from_db_str(value: &str) -> Result<Self, DomainError> {
        match value {
            "depends_on" => Ok(Self::DependsOn),
            "specializes" => Ok(Self::Specializes),
            "composes_with" => Ok(Self::ComposesWith),
            "similar_to" => Ok(Self::SimilarTo),
            "conflicts_with" => Ok(Self::ConflictsWith),
            other => Err(DomainError::InvalidIdentifier(format!(
                "unknown skill edge type: {other}"
            ))),
        }
    }

    /// Whether graph traversal may follow this edge as a positive neighbour.
    ///
    /// `conflicts_with` is the only non-walkable type: it is a do-not-co-select
    /// signal surfaced separately, never expanded as a neighbour.
    pub fn is_walkable(self) -> bool {
        !matches!(self, Self::ConflictsWith)
    }

    /// Whether this edge participates in the directed-acyclic backbone.
    ///
    /// Backbone edges (`depends_on`, `specializes`) express hierarchy, so a cycle
    /// among them is contradictory and must be rejected at validation time.
    pub fn is_backbone(self) -> bool {
        matches!(self, Self::DependsOn | Self::Specializes)
    }
}

/// Provenance of a typed skill edge: how it entered the graph and how much it is trusted.
///
/// Origin drives the trust boundary. Deterministic cold-start edges above the
/// auto-commit confidence threshold are committed as trusted graph state;
/// lower-confidence ones stay as observable proposals until promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeOrigin {
    /// Deterministic proposal from structured skill fields (requires/produces/tools/
    /// artifacts) at or above the auto-commit confidence threshold. Trusted graph state.
    ColdStartDeterministic,
    /// Same structured-field source, but below the auto-commit threshold. Persisted as an
    /// observable proposal, not yet a trusted walkable edge.
    ColdStartProposal,
    /// Operator-authored edge.
    Manual,
    /// Agent-classified edge (requires evidence). Reserved for T06+; not produced in T05.
    AgentDerived,
}

impl EdgeOrigin {
    /// Canonical DB label matching the `skill_edges.edge_origin` CHECK constraint.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::ColdStartDeterministic => "cold_start_deterministic",
            Self::ColdStartProposal => "cold_start_proposal",
            Self::Manual => "manual",
            Self::AgentDerived => "agent_derived",
        }
    }

    /// Parses a DB label back into an [`EdgeOrigin`], failing loudly on unknown values.
    pub fn from_db_str(value: &str) -> Result<Self, DomainError> {
        match value {
            "cold_start_deterministic" => Ok(Self::ColdStartDeterministic),
            "cold_start_proposal" => Ok(Self::ColdStartProposal),
            "manual" => Ok(Self::Manual),
            "agent_derived" => Ok(Self::AgentDerived),
            other => Err(DomainError::InvalidIdentifier(format!(
                "unknown skill edge origin: {other}"
            ))),
        }
    }

    /// Whether an edge of this origin is committed as trusted, walkable graph state.
    ///
    /// Proposals (`cold_start_proposal`) are observable but not yet trusted, so
    /// traversal logic must exclude them until they are promoted.
    pub fn is_trusted(self) -> bool {
        matches!(
            self,
            Self::ColdStartDeterministic | Self::Manual | Self::AgentDerived
        )
    }
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

    /// Projects an event to the text content relevant for grounding evidence checks.
    ///
    /// Returns `Some(text)` for every event type that carries extractable text, and
    /// `None` only for `Metadata` events (which carry no searchable content).
    ///
    /// This wider projection is used by the grounding haystack so that evidence
    /// anchors citing commands, error strings, or file paths — content that lives in
    /// tool events rather than prose turns — can ground correctly:
    ///
    /// | Variant         | Included text                         |
    /// |-----------------|---------------------------------------|
    /// | UserMessage     | `content`                             |
    /// | AssistantMessage| `content`                             |
    /// | ToolCall        | `name` + `" "` + `input_json`         |
    /// | ToolResult      | `output`                              |
    /// | FileEdit        | `path` + `" "` + `operation`          |
    /// | Metadata        | `None` (no searchable content)        |
    ///
    /// The extraction *input* is unchanged by this projection — only the grounding
    /// haystack (a post-extraction check) uses it.
    pub fn grounding_text(&self) -> Option<String> {
        match self {
            Self::UserMessage { content, .. } | Self::AssistantMessage { content, .. } => {
                Some(content.clone())
            }
            Self::ToolCall {
                name, input_json, ..
            } => Some(format!("{} {}", name, input_json)),
            Self::ToolResult { output, .. } => Some(output.clone()),
            Self::FileEdit {
                path, operation, ..
            } => Some(format!("{} {}", path, operation)),
            Self::Metadata { .. } => None,
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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
    /// Short list of task triggers or situations where this skill applies.
    ///
    /// Advisory — feeds T04 multi-view dense/BM25 matching. Empty when the
    /// provider did not emit this field (local models may truncate).
    #[serde(default)]
    pub use_when: Vec<String>,
    /// Short list of situations where this skill should NOT be applied.
    ///
    /// Advisory — feeds T04 multi-view matching. Empty when absent.
    #[serde(default)]
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names, or repo objects the skill applies to.
    ///
    /// Advisory — feeds T05 typed-edge proposals. Empty when absent.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, models, or APIs used by this skill.
    ///
    /// Advisory — feeds T05 typed-edge proposals. Empty when absent.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Verifier-critical constraints that must hold for the skill to be correct.
    ///
    /// Advisory — feeds T04/T05. Empty when absent.
    #[serde(default)]
    pub invariants: Vec<String>,
    /// Prerequisites the skill assumes are already in place.
    ///
    /// Advisory — feeds T05 typed-edge proposals. Empty when absent.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Outcome or artifact produced by following this skill.
    ///
    /// Advisory — feeds T05 typed-edge proposals. Empty when absent.
    #[serde(default)]
    pub produces: Vec<String>,
    /// The knowledge type this skill encodes (advisory taxonomy tag): one of
    /// `procedure`, `rule`, `anti_pattern`, `failure_fix`, `prerequisite`,
    /// `preference`, `best_practice`, `principle`, `refinement`, `diagnostic`.
    ///
    /// JSON key is `type` (renamed to avoid the Rust keyword). `#[serde(default)]`
    /// → absent provider responses deserialise to `None` (backward compatible).
    #[serde(rename = "type", default)]
    pub skill_type: Option<String>,
    /// Transcript anchors that ground this skill — the exact command, error
    /// string, or file it was derived from. Used by the extraction grounding
    /// validator (rejects a candidate that cites content absent from the source
    /// transcript) and written to the `## Evidence` body section. Empty when absent.
    #[serde(default)]
    pub evidence: Vec<String>,
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
