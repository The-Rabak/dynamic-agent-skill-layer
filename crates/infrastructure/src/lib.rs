pub mod embeddings {
    pub mod ollama;
}

pub mod extraction {
    pub mod claude;
    pub(crate) mod http;
    pub(crate) mod limits;
    pub mod ollama;
}

pub mod health;
pub mod logging;
pub mod persistence {
    pub mod outbox;
    pub mod outbox_reconciler;
    pub mod postgres;
    pub mod rebuild;
}
pub mod resilience;
pub mod scope;
pub mod streaming {
    pub mod redis;
}
pub mod vector {
    pub mod qdrant;
}

pub use embeddings::ollama::{OllamaEmbeddingConfig, OllamaEmbeddingService};
pub use extraction::claude::{ClaudeExtractionConfig, ClaudeExtractor};
pub use extraction::ollama::{OllamaExtractionConfig, OllamaExtractor};
pub use health::{DependencyFactory, HealthComponent, HealthReport, InfrastructureHealthChecker};
pub use persistence::outbox::{
    GraphWriteCoordinator, OutboxEvent, OutboxInspection, OutboxRecord, OutboxRelay,
    OutboxRelayError, OutboxRelayRunReport, OutboxVectorStore, PostgresGraphWriteCoordinator,
    VECTOR_UPSERT_EVENT_TYPE, VectorPointListing, parse_vector_upsert_request,
    qdrant_point_id_from_content_hash,
};
pub use persistence::outbox_reconciler::{OutboxReconciler, OutboxReconciliationReport};
pub use persistence::postgres::{PostgresAdapter, PostgresConfig, PostgresError};
pub use persistence::rebuild::{
    LiveGraphCommunityRecord, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
    LiveGraphSubunitRecord, PersistedGraphCommunityRecord, PersistedGraphSkillRecord,
    PersistedGraphSubunitRecord, PostgresGraphSnapshotStore, PostgresRebuildCoordinator,
    RebuildCoordinator, RebuildError,
};
pub use resilience::{
    CircuitBreaker, CircuitState, ResilienceError, RetryPolicy, execute_with_resilience,
    retry_with_backoff,
};
pub use scope::{EnvPathGlobalResolver, GitRootProjectResolver};
pub use streaming::redis::{
    EventEnvelope, RedisStreamError, RedisStreamsAdapter, RedisStreamsConfig, StreamMessage,
};
pub use vector::qdrant::{QdrantAdapter, QdrantConfig, QdrantError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_constructor_sets_contract_fields() {
        let envelope = EventEnvelope::new(
            "skill.file_changed",
            "skill-file-changed",
            serde_json::json!({ "path": "skills/example/SKILL.md" }),
        );

        assert_eq!(envelope.event_type, "skill.file_changed");
        assert_eq!(envelope.idempotency_key, "skill-file-changed");
        assert_eq!(envelope.schema_version, 1);
    }
}
