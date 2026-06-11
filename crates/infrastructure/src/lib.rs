pub mod embeddings {
    pub mod ollama;
}

pub mod extraction;
pub mod similarity;

pub mod health;
pub mod logging;
pub mod persistence {
    pub mod embedding_cache;
    pub mod outbox;
    pub mod outbox_reconciler;
    pub mod postgres;
    pub mod promotion_recurrence;
    pub mod rebuild;
    pub mod scope_demotion;
    pub mod transcript_queue;
    pub mod usage;
}
pub mod resilience;
pub mod scope;
pub mod streaming {
    pub mod redis;
}
pub mod vector {
    pub mod qdrant;
}

pub use embeddings::ollama::{
    EmbeddingModelInfo, OllamaEmbeddingConfig, OllamaEmbeddingService, embedding_model_from_env,
    resolve_embedding_model,
};
pub use extraction::claude::{ClaudeExtractionConfig, ClaudeExtractor};
pub use extraction::claude_code::{
    ClaudeCodeExtractionConfig, ClaudeCodeExtractor, claude_code_generate_text,
};
pub use extraction::generality_verifier::{
    ClaudeGeneralityVerifier, ClaudeGeneralityVerifierConfig, GeneralityDecision,
    OllamaGeneralityVerifier, OllamaGeneralityVerifierConfig, SkillGeneralityVerifier,
};
pub use extraction::http::{
    EXTRACTION_OLLAMA_NUM_CTX, OllamaGenerateTextOptions, OllamaGenerateTextRequest,
    extraction_ollama_num_ctx, ollama_generate_text,
};
pub use extraction::merge_verifier::{
    ClaudeMergeVerifier, ClaudeMergeVerifierConfig, EquivalenceDecision, LlmEquivalenceVerifier,
    OllamaMergeVerifier, OllamaMergeVerifierConfig, TextLlmEquivalenceVerifier,
};
pub use extraction::ollama::{OllamaExtractionConfig, OllamaExtractor};
pub use extraction::prompt_contract::{
    build_text_json_extraction_prompt, render_sanitized_transcript_lines,
};
pub use extraction::text_llm::{ClaudeCodeTextLlm, OllamaTextLlm, StructuredTextLlm};
pub use health::{DependencyFactory, HealthComponent, HealthReport, InfrastructureHealthChecker};
pub use persistence::embedding_cache::{
    EmbeddingCacheError, EmbeddingCacheRow, EmbeddingCacheStore, LoadedEmbedding,
    VIEW_KIND_E_NEEDS, VIEW_KIND_E_NEGATIVE, VIEW_KIND_E_SUMMARY, VIEW_KIND_E_TASK,
    content_hash_for_view_text, decode_f32_vector, encode_f32_vector, subunit_view_kind,
};
pub use persistence::outbox::{
    GraphWriteCoordinator, OutboxEvent, OutboxInspection, OutboxRecord, OutboxRelay,
    OutboxRelayError, OutboxRelayRunReport, OutboxVectorStore, PostgresGraphWriteCoordinator,
    SparseVectorPayload, VECTOR_UPSERT_EVENT_TYPE, VectorPointListing, VectorUpsertRequest,
    parse_vector_upsert_request, qdrant_point_id_from_content_hash,
};
pub use persistence::outbox_reconciler::{OutboxReconciler, OutboxReconciliationReport};
pub use persistence::postgres::{
    PostgresAdapter, PostgresConfig, PostgresError, ensure_database_exists,
};
pub use persistence::promotion_recurrence::{
    PostgresPromotionRecurrenceStore, ProjectSkillRow, PromotionRecurrenceError,
    PromotionRecurrenceStore,
};
pub use persistence::rebuild::{
    LiveGraphCommunityRecord, LiveGraphEdgeRecord, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
    LiveGraphSubunitRecord, PersistedGraphCommunityRecord, PersistedGraphEdgeRecord,
    PersistedGraphSkillRecord, PersistedGraphSubunitRecord, PostgresGraphSnapshotStore,
    PostgresRebuildCoordinator, RebuildCoordinator, RebuildError, stable_skill_uuid,
};
pub use persistence::scope_demotion::{
    GlobalSkillRow, PostgresScopeDemotionStore, ScopeDemotionError, ScopeDemotionStore,
};
pub use persistence::transcript_queue::{
    EnqueueOutcome, MAX_TRANSCRIPT_DRAIN_ATTEMPTS, MAX_TRANSCRIPT_INGEST_BYTES,
    TranscriptIngestQueue, TranscriptIngestRequest, TranscriptQueueError, TranscriptQueueRecord,
    TranscriptSource,
};
pub use persistence::usage::{
    PostgresUsageSampleStore, PostgresUsageWriter, SessionUsageRecord, SkillSelectionRecord,
    SkillUsageSummary, UsagePersistenceError, UsagePersistencePort, UsageSampleStore,
};
pub use resilience::{
    CircuitBreaker, CircuitState, ResilienceError, RetryPolicy, execute_with_resilience,
    retry_with_backoff,
};
pub use scope::{EnvPathGlobalResolver, FsMarkerProjectResolver, GitRootProjectResolver};
pub use similarity::{CosineSimilarityError, cosine_similarity};
pub use streaming::redis::{
    EventEnvelope, RedisStreamError, RedisStreamsAdapter, RedisStreamsConfig,
    SKILL_LAYER_CONSUMER_GROUP, SKILL_LAYER_STREAM_KEY, StreamMessage,
};
pub use vector::qdrant::{
    HybridHit, QdrantAdapter, QdrantConfig, QdrantError, SparseVector, model_keyed_collection_name,
    model_keyed_hybrid_collection_name, validate_qdrant_url,
};

// Re-export infrastructure-bound types so service crates never import
// reqwest, sqlx, or redis directly.
pub use redis::AsyncCommands;
pub use redis::Client as RedisClient;
pub use redis::cmd as redis_cmd;
pub use sqlx::PgPool as PostgresPool;

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
