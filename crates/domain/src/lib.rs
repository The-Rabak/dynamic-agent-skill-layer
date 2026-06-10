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
    Community, DomainId, EdgeOrigin, EdgeType, ExtractedSkillCandidate, ExtractionResult,
    LifecycleStatus, ScopeDescriptor, ScopeRoot, ScopeType, ScoredSkill, SessionEvent,
    SessionTranscript, Skill, SkillStatus, Subunit, SubunitType, TranscriptEntry,
    events_to_transcript,
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

    #[test]
    fn edge_type_db_labels_round_trip() {
        for edge_type in [
            EdgeType::DependsOn,
            EdgeType::Specializes,
            EdgeType::ComposesWith,
            EdgeType::SimilarTo,
            EdgeType::ConflictsWith,
        ] {
            let label = edge_type.as_db_str();
            let parsed = EdgeType::from_db_str(label).expect("known label must parse");
            assert_eq!(parsed, edge_type, "round trip must preserve variant");
        }
    }

    #[test]
    fn edge_type_from_db_str_rejects_unknown_value() {
        let err = EdgeType::from_db_str("boosts")
            .expect_err("unknown edge type must fail loudly, not default");
        assert!(matches!(err, DomainError::InvalidIdentifier(_)));
    }

    #[test]
    fn conflicts_with_is_the_only_non_walkable_edge_type() {
        assert!(!EdgeType::ConflictsWith.is_walkable());
        for walkable in [
            EdgeType::DependsOn,
            EdgeType::Specializes,
            EdgeType::ComposesWith,
            EdgeType::SimilarTo,
        ] {
            assert!(
                walkable.is_walkable(),
                "{walkable:?} must be walkable as a positive neighbour"
            );
        }
    }

    #[test]
    fn only_depends_on_and_specializes_are_backbone() {
        assert!(EdgeType::DependsOn.is_backbone());
        assert!(EdgeType::Specializes.is_backbone());
        assert!(!EdgeType::ComposesWith.is_backbone());
        assert!(!EdgeType::SimilarTo.is_backbone());
        assert!(!EdgeType::ConflictsWith.is_backbone());
    }

    #[test]
    fn edge_origin_db_labels_round_trip_and_reject_unknown() {
        for origin in [
            EdgeOrigin::ColdStartDeterministic,
            EdgeOrigin::ColdStartProposal,
            EdgeOrigin::Manual,
            EdgeOrigin::AgentDerived,
        ] {
            let parsed = EdgeOrigin::from_db_str(origin.as_db_str()).expect("known label parses");
            assert_eq!(parsed, origin);
        }
        assert!(EdgeOrigin::from_db_str("guessed").is_err());
    }

    #[test]
    fn cold_start_proposal_origin_is_not_trusted() {
        assert!(!EdgeOrigin::ColdStartProposal.is_trusted());
        assert!(EdgeOrigin::ColdStartDeterministic.is_trusted());
        assert!(EdgeOrigin::Manual.is_trusted());
        assert!(EdgeOrigin::AgentDerived.is_trusted());
    }
}
