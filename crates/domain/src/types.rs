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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedSkillCandidate {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub procedures: Vec<String>,
    pub conventions: Vec<String>,
    pub assets: Vec<String>,
    pub confidence: f32,
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
    use super::ScopeType;

    #[test]
    fn scope_type_as_str_returns_lowercase_label_for_each_variant() {
        assert_eq!(ScopeType::Project.as_str(), "project");
        assert_eq!(ScopeType::Global.as_str(), "global");
        assert_eq!(ScopeType::Team.as_str(), "team");
    }
}
