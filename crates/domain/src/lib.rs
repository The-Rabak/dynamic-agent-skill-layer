pub mod config;
pub mod errors;
pub mod lifecycle_files;
pub mod lifecycle_policy;
pub mod traits;
pub mod types;

pub use config::{
    CompilationConfig, DomainConfig, EmbeddingConfig, ExtractionConfig, HdbscanConfig, ScopeConfig,
};
pub use errors::{
    CompilationError, ConfigError, DomainError, EmbeddingError, ExtractionError, ScopeError,
};
pub use lifecycle_files::{
    ACTIVE_SKILL_FILE_NAME, PENDING_SKILL_FILE_NAME, REJECTED_SKILL_FILE_NAME,
    RETIRED_SKILL_FILE_NAME, has_lifecycle_file_name, is_rejected_tombstone,
};
pub use lifecycle_policy::{
    PENDING_DEFAULT_EXPIRY_AFTER_DAYS, PENDING_DEFAULT_WARNING_AFTER_DAYS,
    pending_default_expires_at, pending_default_warning_at,
};
pub use traits::{
    ContextCompiler, EmbeddingService, ScopeResolver, TranscriptSkillExtractionService,
};
pub use types::{
    Community, DomainId, ExtractedSkillCandidate, ExtractionResult, LifecycleStatus,
    ScopeDescriptor, ScopeRoot, ScopeType, ScoredSkill, SessionEvent, SessionTranscript, Skill,
    SkillStatus, Subunit, SubunitType, TranscriptEntry, events_to_transcript,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_id_parse_rejects_blank_values() {
        let err = DomainId::parse("   ").expect_err("blank ids should be rejected");
        assert!(matches!(err, DomainError::InvalidIdentifier(_)));
    }

    #[test]
    fn domain_config_validation_rejects_zero_embedding_dimension() {
        let mut config = DomainConfig::default();
        config.embedding.dimension = 0;

        let err = config
            .validate()
            .expect_err("zero embedding dimension should fail");

        assert!(matches!(
            err,
            DomainError::Config(ConfigError::InvalidValue {
                field: "embedding.dimension",
                ..
            })
        ));
    }

    #[test]
    fn domain_config_validation_accepts_default_values() {
        let config = DomainConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn extraction_result_keeps_candidate_sections() {
        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-001"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "need retrieval scoring notes".to_owned(),
            }],
        };

        let candidate = ExtractedSkillCandidate {
            name: "weighted-rrf".to_owned(),
            description: "Fuses scoped retrieval ranks".to_owned(),
            tags: vec!["retrieval".to_owned(), "rrf".to_owned()],
            procedures: vec!["Collect per-scope candidates".to_owned()],
            conventions: vec!["Keep domain pure".to_owned()],
            assets: vec!["docs/architecture/...".to_owned()],
            confidence: 0.91,
            generality: None,
            generality_rationale: None,
            ..Default::default()
        };

        let result = ExtractionResult {
            source_session_id: transcript.session_id,
            candidates: vec![candidate],
            provider: "claude".to_owned(),
        };

        assert_eq!(result.candidates[0].name, "weighted-rrf");
        assert_eq!(result.candidates[0].procedures.len(), 1);
        assert_eq!(result.candidates[0].conventions.len(), 1);
        assert_eq!(result.candidates[0].assets.len(), 1);
    }
}
