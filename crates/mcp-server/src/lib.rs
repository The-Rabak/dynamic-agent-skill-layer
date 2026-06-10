// FOLLOW-UP (post-v1.5): split this module — see todo #128. Intended split:
// `app.rs` (McpServerApp struct + builders), `request_handlers.rs` (compile_context,
// extract_session, find_skill dispatch), `admin.rs` (admin tool delegation).
mod admin_wiring;
mod context_cache;
mod graph_refresh_subscriber;
pub mod protocol;
pub mod state;
pub(crate) mod suppression_state;
mod usage_writer;

/// Re-exports of internal suppression state for integration and E2E tests.
///
/// Gated on `test-utils` to keep the public API clean in production builds.
/// Tests use this to prove the clear_session fix and cross-instance isolation
/// without going through live Redis.
#[cfg(any(test, feature = "test-utils"))]
pub mod suppression_state_for_tests {
    pub use crate::suppression_state::SessionSuppressionState;
}
pub mod tools {
    pub mod compile_context;
    pub mod extract_session;
    pub mod find_skill;
}

use std::sync::Arc;

use admin::tools::{
    AdminTools, GraphRebuildTrigger, GraphSnapshotReader, ListCommunitiesRequest,
    ListCommunitiesResponse, RebuildGraphRequest, RebuildGraphResponse, RebuildGraphStatusRequest,
    RebuildGraphStatusResponse,
};
use async_trait::async_trait;
use compiler::TemplateOnlyCompiler;
use domain::{EmbeddingService, ScopeResolver};
#[cfg(any(test, feature = "test-utils"))]
use infrastructure::OutboxVectorStore;
use infrastructure::{
    EmbeddingModelInfo, EnvPathGlobalResolver, FsMarkerProjectResolver, OllamaEmbeddingConfig,
    OllamaEmbeddingService, PostgresAdapter, PostgresConfig, PostgresGraphSnapshotStore,
    PostgresGraphWriteCoordinator, PostgresPool, PostgresRebuildCoordinator,
    PostgresUsageSampleStore, PostgresUsageWriter, QdrantAdapter, QdrantConfig, QdrantError,
    RedisClient, RedisStreamsAdapter, RedisStreamsConfig, SessionUsageRecord, SkillSelectionRecord,
    TranscriptIngestQueue, UsagePersistencePort, UsageSampleStore, model_keyed_collection_name,
    model_keyed_hybrid_collection_name,
};
use maintenance::RetirementConfig;
use retrieval::{
    CircuitBreaker, DualScopeResolver, HybridCandidate, HybridCandidateSource, HybridQueryError,
    RetrievalBackend, RetrievalConfig, RetrievalOrchestrator, RetrievalSnapshot,
    SkillLexicalFields, SkillRetriever, skill_lexical_document,
};
use tokio::sync::mpsc;
use tools::{
    compile_context::{
        CompileContextRequest, CompileContextResponse, CompileContextStatus, CompileContextTool,
    },
    extract_session::{ExtractSessionRequest, ExtractSessionTool},
    find_skill::{FindSkillRequest, FindSkillResponse, FindSkillTool},
};
use tracing::{debug, info, warn};

use crate::graph_refresh_subscriber::{GraphReloader, run_graph_refresh_loop};
use crate::state::{CompiledContextCache, SessionSuppressionState};
use crate::usage_writer::{
    UsageWriteHealth, UsageWriterHandle, new_usage_write_health, post_usage_record,
    read_usage_write_health, spawn_usage_writer,
};

#[derive(Clone)]
pub struct McpServerApp {
    compile_context: CompileContextTool,
    extract_session: ExtractSessionTool,
    find_skill: FindSkillTool,
    admin_tools: AdminTools,
    session_state: SessionSuppressionState,
    cache: CompiledContextCache,
    /// Durable transcript-ingest queue backing the localhost `/ingest/transcript`
    /// endpoint (todo 103). `None` for in-memory/test constructors that have no
    /// Postgres pool; wired in [`build_live_server`] from the live PG adapter.
    transcript_ingest: Option<TranscriptIngestQueue>,
    /// Sender side of the bounded channel feeding the background usage writer (T06).
    ///
    /// `None` only for in-memory/test constructors that do not wire a Postgres pool.
    /// When `None`, no usage rows are written and the health marker stays `"ok"`.
    usage_sender: Option<mpsc::Sender<SessionUsageRecord>>,
    /// Shared health cell for the usage-write observability seam (T06).
    ///
    /// Set to `"failed"` on DB error or channel-full backpressure, reset to `"ok"`
    /// after the next successful write. Injected into every `compile_context`
    /// response under `health["usage_write"]`.
    usage_write_health: UsageWriteHealth,
}

impl McpServerApp {
    pub fn new_with_admin(
        retriever: Arc<dyn SkillRetriever>,
        rebuild_trigger: Arc<dyn GraphRebuildTrigger>,
        graph_reader: Arc<dyn GraphSnapshotReader>,
        redis_client: Option<RedisClient>,
    ) -> Self {
        let state = SessionSuppressionState::new(
            redis_client.clone(),
            SessionSuppressionState::DEFAULT_TTL_SECS,
        );
        let cache = CompiledContextCache::new(redis_client, CompiledContextCache::DEFAULT_TTL_SECS);
        let compiler = TemplateOnlyCompiler::default();
        let admin_tools = AdminTools::new(rebuild_trigger, graph_reader);

        Self {
            compile_context: CompileContextTool::new(
                retriever.clone(),
                compiler,
                state.clone(),
                cache.clone(),
            ),
            extract_session: ExtractSessionTool::from_environment(),
            find_skill: FindSkillTool::new(retriever),
            admin_tools,
            session_state: state,
            cache,
            transcript_ingest: None,
            usage_sender: None,
            usage_write_health: new_usage_write_health(),
        }
    }

    /// Attaches the durable transcript-ingest queue used by the localhost
    /// `/ingest/transcript` endpoint. Builder-style so test/in-memory
    /// constructors stay free of a Postgres dependency.
    pub fn with_transcript_ingest(mut self, queue: TranscriptIngestQueue) -> Self {
        self.transcript_ingest = Some(queue);
        self
    }

    /// Wires the background usage writer (T06) and returns the join handle.
    ///
    /// Spawns the bounded-channel writer task and stores the sender so every
    /// `compile_context` call can post usage records off the hot path. Builder-style
    /// so test/in-memory constructors stay free of Postgres. When not called,
    /// usage writes are silently skipped and the health marker stays `"ok"`.
    ///
    /// Returns a `JoinHandle<()>` that the caller (e.g. `build_live_server`) can
    /// stash on `LiveServerComponents` for deterministic drain during teardown.
    /// Dropping the sender signals end-of-channel; awaiting the handle waits for
    /// the last in-flight write to complete — must be done BEFORE `TRUNCATE` to
    /// avoid the RowExclusive vs ACCESS EXCLUSIVE deadlock.
    pub fn with_usage_writer(
        mut self,
        writer: Arc<dyn UsagePersistencePort>,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let health = self.usage_write_health.clone();
        let UsageWriterHandle {
            sender,
            join_handle,
        } = spawn_usage_writer(writer, health);
        self.usage_sender = Some(sender);
        (self, join_handle)
    }

    /// Builds a server from an explicit in-memory [`RetrievalSnapshot`].
    ///
    /// This is the explicit-graph constructor used by tests and benches to wire a
    /// deterministic graph without live infrastructure. Production boot uses
    /// [`McpServerApp::from_environment`] instead.
    pub fn with_explicit_graph<E>(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
        redis_client: Option<RedisClient>,
    ) -> McpServerApp
    where
        E: EmbeddingService + Send + Sync + 'static,
    {
        // Explicit-graph constructor (tests/benches): reuse the caller's embedder for the
        // admin rebuild trigger instead of demanding a live OLLAMA_URL. The embedder is only
        // exercised if an admin rebuild is triggered; production boot uses the fail-loud
        // `live_admin_runtime_dependencies` path instead.
        let admin_runtime_dependencies = admin_wiring::admin_runtime_dependencies_with_embedder(
            embedding_service.clone() as Arc<dyn EmbeddingService>,
        );
        let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
        let project_resolver: Arc<dyn ScopeResolver> =
            Arc::new(FsMarkerProjectResolver::new(start_dir));
        let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
        let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

        let embedding_breaker =
            RetrievalOrchestrator::<E>::build_embedding_circuit_breaker_from_env();
        let retriever = RetrievalOrchestrator::new_dual_scope(
            embedding_service,
            graph,
            config,
            scope_resolver,
            embedding_breaker,
        );
        McpServerApp::new_with_admin(
            Arc::new(retriever),
            admin_runtime_dependencies.rebuild_trigger,
            admin_runtime_dependencies.graph_reader,
            redis_client,
        )
    }

    /// Drops the `usage_sender` to signal channel closure to the background writer task.
    ///
    /// Called by `LiveServerComponents::teardown` before awaiting the writer join handle
    /// to ensure the task sees end-of-channel and exits cleanly. Must be called before
    /// `truncate_all_tables` to release RowExclusive locks held by in-flight writes.
    ///
    /// Gated on `test-utils` — this is a test-teardown seam; production servers run
    /// until process exit, at which point the OS reclaims all resources.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn close_usage_sender(&mut self) {
        drop(self.usage_sender.take());
    }

    /// Builds the production server from the runtime environment.
    ///
    /// Connects to live PG/Qdrant/Redis/Ollama, loads the durable graph snapshot
    /// (at its real `graph_version`), and wires the dual-scope retriever. This is
    /// the production boot path — the deployed binary calls this so a clean
    /// deployment retrieves the skills the graph already contains.
    ///
    /// Returns the full [`LiveServerComponents`] bundle (app + adapters) so that
    /// callers can also drive seeding/teardown in tests; production only consumes
    /// `.app`. Fails explicitly if any dependency cannot be reached.
    pub async fn from_environment(
        config: RetrievalConfig,
    ) -> Result<LiveServerComponents, Box<dyn std::error::Error + Send + Sync>> {
        build_live_server(config).await
    }

    pub fn registered_tools(&self) -> Vec<&'static str> {
        protocol::registered_tool_descriptors()
            .iter()
            .map(|tool| tool.name)
            .collect::<Vec<&'static str>>()
    }

    /// Compiles skill context for the current session.
    ///
    /// Coordination layer: delegates retrieval + compilation to [`CompileContextTool`]
    /// (pure query-compile unit, T04), then asynchronously records usage via the
    /// background writer (T06). Usage writes are off the response path — failures
    /// set `health["usage_write"]="failed"` and emit a `warn` log but never affect
    /// latency or the returned response.
    pub async fn compile_context(&self, request: CompileContextRequest) -> CompileContextResponse {
        let prompt_hash = compute_prompt_hash(&request.prompt);
        // Extract only the fields needed for usage capture before consuming the
        // request, so `invoke_and_capture_outcome` can take ownership without
        // forcing a full clone of the prompt String.
        let session_id = request.session_id.clone();
        let repo_path = request.repo_path.clone();
        let (mut response, outcome) = self
            .compile_context
            .invoke_and_capture_outcome(request)
            .await;

        // Post usage record to the background writer if a live retrieval ran.
        if let Some(outcome) = outcome
            && let Some(sender) = &self.usage_sender
        {
            let record = build_session_usage_record(
                &session_id,
                &repo_path,
                &response,
                &outcome,
                &prompt_hash,
            );
            post_usage_record(sender, record, &self.usage_write_health);
        }

        // Inject the usage-write health marker into the response so the
        // observability seam is visible to callers.
        //
        // Two runtime states are emitted:
        //   "ok"     — writer active and last write succeeded (key suppressed for brevity)
        //   "failed" — writer active but last write or channel post failed
        //
        // The key is only injected when non-ok so healthy responses stay compact.
        // Usage writes are always on — `usage_sender` is `None` only for in-memory
        // test constructors that do not wire a Postgres pool.
        let usage_health = read_usage_write_health(&self.usage_write_health);
        if usage_health != "ok" {
            response.health.insert(
                usage_writer::USAGE_WRITE_HEALTH_KEY.to_owned(),
                usage_health,
            );
        }

        response
    }

    pub async fn find_skill(&self, request: FindSkillRequest) -> FindSkillResponse {
        self.find_skill.invoke(request).await
    }

    pub async fn extract_session(
        &self,
        request: ExtractSessionRequest,
    ) -> session_extractor::ExtractSessionResponse {
        let session_id = request.session_id.clone();
        let response = self.extract_session.invoke(request).await;
        if response.status != "failed" {
            tracing::info!(
                session_id = %session_id,
                "clearing session suppression and cache after extraction enqueue"
            );
            self.cache.clear_session(&session_id);
            self.session_state.clear_session(&session_id);
        }
        response
    }

    /// Ingests transcript content into the durable queue (todo 103).
    ///
    /// Called by the localhost `/ingest/transcript` HTTP handler after the
    /// shared-secret check. Returns [`TranscriptIngestOutcome`] describing
    /// whether a new row was written, the payload was a duplicate, the queue is
    /// not configured, or the contract was violated (empty/oversize/bad source).
    pub async fn ingest_transcript(
        &self,
        request: TranscriptIngestHttpRequest,
    ) -> TranscriptIngestOutcome {
        let Some(queue) = self.transcript_ingest.as_ref() else {
            return TranscriptIngestOutcome::Unavailable;
        };

        let source = match infrastructure::TranscriptSource::parse(&request.source) {
            Ok(source) => source,
            Err(error) => return TranscriptIngestOutcome::InvalidContract(error),
        };

        let ingest_request = infrastructure::TranscriptIngestRequest {
            session_id: request.session_id,
            repo_path: request.repo_path,
            source,
            content: request.content,
        };

        match queue.enqueue(&ingest_request).await {
            Ok(outcome) => TranscriptIngestOutcome::Accepted(outcome),
            Err(error) => match error {
                infrastructure::TranscriptQueueError::EmptyContent
                | infrastructure::TranscriptQueueError::ContentTooLarge { .. }
                | infrastructure::TranscriptQueueError::InvalidContract(_) => {
                    TranscriptIngestOutcome::InvalidContract(error)
                }
                infrastructure::TranscriptQueueError::Persistence(_) => {
                    TranscriptIngestOutcome::PersistenceError(error)
                }
            },
        }
    }

    pub async fn rebuild_graph(&self, request: RebuildGraphRequest) -> RebuildGraphResponse {
        self.admin_tools.rebuild_graph(request).await
    }

    pub async fn rebuild_graph_status(
        &self,
        request: RebuildGraphStatusRequest,
    ) -> RebuildGraphStatusResponse {
        self.admin_tools.rebuild_graph_status(request).await
    }

    pub async fn inspect_skill(
        &self,
        request: admin::tools::InspectSkillRequest,
    ) -> admin::tools::InspectSkillResponse {
        self.admin_tools.inspect_skill(request).await
    }

    pub async fn list_communities(
        &self,
        request: ListCommunitiesRequest,
    ) -> ListCommunitiesResponse {
        self.admin_tools.list_communities(request).await
    }
}

/// HTTP-facing transcript-ingest request (todo 103).
///
/// Shipped by the host `command` capture hooks (`SessionEnd` / `PreCompact`):
/// they read the transcript file where its path is natively valid and POST its
/// content here, so the server never sees a host path it cannot resolve.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranscriptIngestHttpRequest {
    pub session_id: String,
    #[serde(default)]
    pub repo_path: Option<String>,
    pub source: String,
    pub content: String,
}

/// Result of an ingest attempt, mapped to HTTP status by the protocol layer.
#[derive(Debug)]
pub enum TranscriptIngestOutcome {
    /// Row written or deduped.
    Accepted(infrastructure::EnqueueOutcome),
    /// Contract violation (empty/oversize content, unknown source). -> 4xx.
    InvalidContract(infrastructure::TranscriptQueueError),
    /// Database failure. -> 503.
    PersistenceError(infrastructure::TranscriptQueueError),
    /// Queue not configured on this server instance. -> 503.
    Unavailable,
}

/// LiveServerComponents bundles the fully-wired live server graph.
/// The `teardown()` method is gated to test builds only to prevent
/// accidental destructive operations in production.
///
/// The `usage_writer_join_handle` field holds the join handle for the background
/// usage-writer task so teardown can drain it (drop the sender, then await the
/// handle) before issuing TRUNCATE, preventing the RowExclusive vs ACCESS EXCLUSIVE
/// deadlock between the writer and teardown.
pub struct LiveServerComponents {
    pub app: McpServerApp,
    pub embedding_service: Arc<OllamaEmbeddingService>,
    pub write_coordinator: Arc<PostgresGraphWriteCoordinator>,
    pub qdrant_adapter: Arc<QdrantAdapter>,
    pub redis_adapter: Arc<RedisStreamsAdapter>,
    pub pg_adapter: Arc<PostgresAdapter>,
    pub rebuild_coordinator: Arc<PostgresRebuildCoordinator>,
    /// The embedding model identity discovered at boot via `discover_dimension`.
    ///
    /// Carries the active model name and real vector dimension so callers (e.g.
    /// `main.rs`) can surface them on `/health` without repeating the Ollama call.
    /// Note: the `embedding_model_metadata` table (populated by #228 after each
    /// graph rebuild) is the future canonical source; this boot-time field is used
    /// until a rebuild has run.
    pub embedding_model_info: EmbeddingModelInfo,
    /// Join handle for the background usage-writer task.
    ///
    /// Only present in test/test-utils builds (elided via `#[cfg]` in production).
    /// Teardown must drop `app.usage_sender` then await this handle before calling
    /// `truncate_all_tables` to release RowExclusive locks held by in-flight writes.
    #[cfg(any(test, feature = "test-utils"))]
    pub usage_writer_join_handle: tokio::task::JoinHandle<()>,
}

impl LiveServerComponents {
    /// Destructive teardown for test isolation.
    ///
    /// Drains the background usage-writer before truncating PG tables to prevent
    /// the RowExclusive vs ACCESS EXCLUSIVE deadlock: the writer task holds
    /// RowExclusive locks on `session_logs`/`skill_usage` while TRUNCATE … CASCADE
    /// requires ACCESS EXCLUSIVE. Drain sequence: drop the sender (signals
    /// end-of-channel), then await the join handle (waits for in-flight writes).
    ///
    /// Continues cleanup even if individual steps fail, reporting all errors at the end.
    ///
    /// Gated to `#[cfg(any(test, feature = "test-utils"))]` so production builds have zero destructive
    /// teardown surface.
    #[cfg(any(test, feature = "test-utils"))]
    #[tracing::instrument(skip_all)]
    pub async fn teardown(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut errors: Vec<String> = Vec::new();

        // Drain the usage writer before TRUNCATE to release RowExclusive locks.
        // Drop the sender (signals channel closure to the writer task) then abort and
        // await the task handle (ensures the task is no longer holding DB locks).
        self.app.close_usage_sender();
        {
            // Abort-and-await is safe: the task caught all Result paths and never
            // holds application state that requires graceful flushing. The abort
            // ensures we do not wait indefinitely if a write is in progress; the
            // await guarantees the OS-level task resources are reclaimed.
            self.usage_writer_join_handle.abort();
            let _ = self.usage_writer_join_handle.await;
        }

        if let Err(e) = self.pg_adapter.truncate_all_tables().await {
            errors.push(format!("pg truncate failed: {e}"));
        }

        match self.qdrant_adapter.list_point_ids().await {
            Ok(listing) => {
                if !listing.point_ids.is_empty()
                    && let Err(e) = self.qdrant_adapter.delete_points(&listing.point_ids).await
                {
                    errors.push(format!("qdrant delete failed: {e}"));
                }
            }
            Err(e) => {
                errors.push(format!("qdrant list failed: {e}"));
            }
        }

        if let Err(e) = self.redis_adapter.delete_stream().await {
            errors.push(format!("redis delete_stream failed: {e}"));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; ").into())
        }
    }
}

/// Implements `HybridCandidateSource` for the `QdrantHybrid` retrieval arm.
///
/// Wraps a `QdrantAdapter` and a model-keyed hybrid collection name to provide
/// real dense+sparse hybrid candidate lookups at request time. This is the only
/// production implementation of the trait; it lives in `mcp-server` because
/// `mcp-server` is the only crate that can depend on both `retrieval` and
/// `infrastructure`.
///
/// # CQRS contract break
///
/// Under `QdrantHybrid`, Qdrant is queried at read time. This intentionally
/// breaks the "Qdrant-down cannot degrade compile_context" invariant that holds
/// for `SnapshotDense` and `SnapshotHybrid`. The trade-off is documented in the
/// T08 ADR. Do NOT add a silent fallback to dense here.
struct QdrantHybridCandidateSource {
    adapter: Arc<QdrantAdapter>,
    hybrid_collection: String,
}

#[async_trait]
impl HybridCandidateSource for QdrantHybridCandidateSource {
    /// Queries the hybrid Qdrant collection and maps hits to `HybridCandidate`s.
    ///
    /// The graph-builder outbox event carries:
    ///   `{ "content_hash": ..., "vector": [...], "payload": { "skill_id": ..., ... } }`
    /// `parse_vector_upsert_request` extracts the inner `payload["payload"]` object and
    /// passes THAT to `upsert_hybrid_point`. Qdrant stores it verbatim, so the point's
    /// native payload is `{ "skill_id": ..., "name": ..., ... }`. `skill_id` is therefore
    /// at the **top level** of the point payload (`payload["skill_id"]`), not nested.
    /// This method extracts that field and returns `HybridCandidate { skill_stable_id: ..., fused_score: ... }`.
    ///
    /// Returns `Err(HybridQueryError::Transport)` on any network failure or
    /// `Err(HybridQueryError::Status)` on an unexpected Qdrant response. The
    /// orchestrator MUST NOT silently fall back to dense on any error.
    async fn query_hybrid(
        &self,
        dense: &[f32],
        sparse_indices: &[u32],
        sparse_values: &[f32],
        limit: u64,
    ) -> Result<Vec<HybridCandidate>, HybridQueryError> {
        use infrastructure::SparseVector;

        let sparse = SparseVector {
            indices: sparse_indices.to_vec(),
            values: sparse_values.to_vec(),
        };

        let hits = self
            .adapter
            .query_hybrid(&self.hybrid_collection, dense, &sparse, limit)
            .await
            .map_err(|e| HybridQueryError::Transport(e.to_string()))?;

        let candidates = hits
            .into_iter()
            .filter_map(|hit| {
                // The relay stores the inner payload object directly as the Qdrant point
                // payload. Graph-builder constructs the outbox event as:
                //   { "content_hash": ..., "vector": [...], "payload": { "skill_id": ..., ... } }
                // `parse_vector_upsert_request` extracts `payload["payload"]` and passes
                // THAT inner object to `upsert_hybrid_point`. Qdrant stores it verbatim,
                // so the point's native payload is `{ "skill_id": ..., "name": ..., ... }`.
                // `skill_id` is therefore at the top level, not nested under "payload".
                let skill_id = hit
                    .payload
                    .get("skill_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)?;
                Some(HybridCandidate {
                    skill_stable_id: skill_id,
                    fused_score: hit.score,
                })
            })
            .collect();

        Ok(candidates)
    }
}

/// Live-component assembly used exclusively by [`McpServerApp::from_environment`].
/// Kept private so the public surface stays at exactly two constructors.
async fn build_live_server(
    config: RetrievalConfig,
) -> Result<LiveServerComponents, Box<dyn std::error::Error + Send + Sync>> {
    // Build the embedding service first (sync, no network) so we can discover
    // its real vector dimension before setting up the Qdrant collection.
    // The model is read from OLLAMA_EMBED_MODEL (defaults to nomic-embed-text).
    let embedding_service = build_embedding_service()?;

    let (pg_adapter, (qdrant_adapter, embedding_model_info), redis_streams, redis_client) = tokio::try_join!(
        build_pg_adapter(),
        async {
            // Discover the real vector dimension from the live model BEFORE
            // creating/validating the Qdrant collection. This ensures the
            // collection is sized to the actual model output, not a hardcoded value.
            let embedding_model_info = embedding_service
                .discover_dimension()
                .await
                .map_err(|e| format!("embedding dimension discovery failed: {e}"))?;
            let q = build_qdrant_adapter_with_model(&embedding_model_info).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((q, embedding_model_info))
        },
        build_redis_streams_adapter(),
        async {
            let c = build_redis_client()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(c)
        },
    )?;

    info!(
        embedder_model = %embedding_model_info.model_name,
        dimension = embedding_model_info.dimension,
        collection = %qdrant_adapter.config.collection_name,
        "embedding model and qdrant collection confirmed"
    );

    // When RETRIEVAL_BACKEND=qdrant_hybrid, ensure the hybrid collection exists so
    // the C3 query arm can read from it even before the first rebuild populates it.
    // Non-fatal per the same Option-A resilience contract as the dense collection:
    // the collection will be created on the next access if Qdrant is briefly down.
    if config.backend == RetrievalBackend::QdrantHybrid {
        match model_keyed_hybrid_collection_name(&embedding_model_info.model_name) {
            Ok(hybrid_name) => {
                let vector_size = embedding_model_info.dimension as u64;
                match qdrant_adapter
                    .ensure_hybrid_collection(&hybrid_name, vector_size)
                    .await
                {
                    Ok(()) => {
                        info!(
                            collection = %hybrid_name,
                            dense_dim = vector_size,
                            "qdrant hybrid collection ensured at mcp-server boot"
                        );
                    }
                    Err(error) => {
                        warn!(
                            %error,
                            collection = %hybrid_name,
                            "ensure_hybrid_collection failed at mcp-server boot; \
                             C3 hybrid queries will fail until Qdrant is reachable"
                        );
                    }
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    "could not derive hybrid collection name at mcp-server boot; \
                     hybrid collection will not be ensured"
                );
            }
        }
    }

    let write_coordinator = PostgresGraphWriteCoordinator::new(pg_adapter.pool().clone());
    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg_adapter.pool().clone());

    let usage_sample_store = Arc::new(PostgresUsageSampleStore::new(pg_adapter.pool().clone()));
    let graph = self::build_graph_from_pg(
        pg_adapter.pool(),
        embedding_service.as_ref(),
        usage_sample_store.as_ref(),
    )
    .await?;

    // Pay the Ollama JIT model-load cost at boot rather than on the first real
    // session.  build_graph_from_pg reads precomputed embeddings from Postgres
    // without calling the embedder, so the model is not yet resident in Ollama
    // memory after that call.  One warmup embed makes the model resident before
    // the server signals ready, keeping first-request latency inside the <500ms
    // budget.  A warmup failure does NOT abort boot — the embedder's own health
    // endpoint and circuit-breaker already cover real runtime failures.
    match embedding_service.embed_text("warmup").await {
        Ok(_) => info!("embedding warmup succeeded — Ollama model now resident"),
        Err(e) => warn!(
            error = %e,
            "embedding warmup failed — first request may exceed latency budget; \
             check Ollama reachability and OLLAMA_EMBED_MODEL"
        ),
    }

    let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
    let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let project_resolver: Arc<dyn ScopeResolver> =
        Arc::new(FsMarkerProjectResolver::new(start_dir));
    let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
    let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

    // Build the embedding circuit breaker from environment variables.  Reads
    // EMBED_CIRCUIT_FAILURE_THRESHOLD and EMBED_CIRCUIT_OPEN_FOR_SECS with
    // sane defaults; panics loudly on malformed values (per no-stubs mandate).
    let embedding_breaker: CircuitBreaker =
        RetrievalOrchestrator::<OllamaEmbeddingService>::build_embedding_circuit_breaker_from_env();

    // When QdrantHybrid is active, inject the real HybridCandidateSource so the
    // C3 read arm can query Qdrant at request time. The collection name is derived
    // from the live model identity (same algorithm as the write-side collection name).
    // Fails loud if the hybrid collection name cannot be derived (degenerate model name).
    let retriever = {
        let base = RetrievalOrchestrator::new_dual_scope(
            embedding_service.clone(),
            graph,
            config.clone(),
            scope_resolver,
            embedding_breaker,
        );
        if config.backend == RetrievalBackend::QdrantHybrid {
            match model_keyed_hybrid_collection_name(&embedding_model_info.model_name) {
                Ok(hybrid_collection) => {
                    let source: Arc<dyn HybridCandidateSource> =
                        Arc::new(QdrantHybridCandidateSource {
                            adapter: qdrant_adapter.clone(),
                            hybrid_collection,
                        });
                    Arc::new(base.with_hybrid_candidate_source(source))
                }
                Err(error) => {
                    return Err(format!(
                        "RETRIEVAL_BACKEND=qdrant_hybrid but cannot derive hybrid collection \
                         name for model '{}': {error}",
                        embedding_model_info.model_name
                    )
                    .into());
                }
            }
        } else {
            Arc::new(base)
        }
    };

    let usage_writer: Arc<dyn UsagePersistencePort> =
        Arc::new(PostgresUsageWriter::new(pg_adapter.pool().clone()));

    let app_builder = McpServerApp::new_with_admin(
        retriever.clone(),
        admin_runtime_dependencies.rebuild_trigger,
        admin_runtime_dependencies.graph_reader,
        Some(redis_client),
    )
    // Back the localhost `/ingest/transcript` endpoint with the live PG pool so
    // host capture hooks can push transcript content into the durable queue.
    .with_transcript_ingest(TranscriptIngestQueue::new(pg_adapter.pool().clone()));

    // Wire the background usage writer so compile_context records usage (T06).
    // In test/test-utils builds the join handle is stashed on `LiveServerComponents`
    // so teardown can drain the writer before TRUNCATE (prevents deadlock). In
    // production builds the handle is intentionally dropped — the process runs
    // until OS exit; the OS reclaims the task at that point.
    #[cfg(any(test, feature = "test-utils"))]
    let (app, usage_writer_join_handle) = app_builder.with_usage_writer(usage_writer);
    #[cfg(not(any(test, feature = "test-utils")))]
    let (app, _) = app_builder.with_usage_writer(usage_writer);

    // Online refresh-without-restart (T02): subscribe to `graph.rebuilt`
    // and atomically swap the in-memory read model. Spawned on its own
    // task so it never blocks the HTTP server.
    // T06: pass the usage_sample_store so graph refreshes also populate
    // the deterministic usage prior.
    spawn_graph_refresh_subscriber(
        redis_streams.clone(),
        pg_adapter.clone(),
        embedding_service.clone(),
        retriever,
        usage_sample_store.clone(),
    );

    Ok(LiveServerComponents {
        app,
        embedding_service,
        write_coordinator: Arc::new(write_coordinator),
        qdrant_adapter,
        redis_adapter: redis_streams,
        pg_adapter,
        rebuild_coordinator: Arc::new(rebuild_coordinator),
        // Carry the boot-discovered arm identity so `main.rs` can wire it into
        // the health checker without repeating the Ollama dimension-discovery call.
        embedding_model_info,
        // `usage_writer_join_handle` is defined only in test/test-utils builds
        // so the production binary never carries a reference to the task handle.
        #[cfg(any(test, feature = "test-utils"))]
        usage_writer_join_handle,
    })
}

async fn build_pg_adapter() -> Result<Arc<PostgresAdapter>, Box<dyn std::error::Error + Send + Sync>>
{
    let pg_config = PostgresConfig {
        database_url: env_var("DATABASE_URL")?,
        connect_timeout_secs: 5,
        acquire_timeout_secs: 3,
        max_connections: 10,
        min_connections: 1,
    };
    // Self-heal a missing application database (stale/reused/test-initialized
    // volume) before connecting, so boot doesn't crash-loop on
    // `database "X" does not exist`.
    infrastructure::ensure_database_exists(&pg_config.database_url).await?;
    let pg_adapter = PostgresAdapter::connect(&pg_config).await?;
    pg_adapter.run_migrations().await?;
    Ok(Arc::new(pg_adapter))
}

/// Builds the Qdrant adapter with a model-keyed collection name and the real
/// vector dimension discovered from the live embedding model.
///
/// Collection naming: if `QDRANT_COLLECTION` is set explicitly (used by the
/// per-run test isolation guard, #164), that value overrides the model-keyed
/// default. Otherwise the collection name is derived from the active model so
/// nomic (768-dim) and qwen (2560-dim) collections coexist side by side.
///
/// Fails loud on dimension mismatch — a wrong-dim collection can never be
/// silently reused (see `QdrantError::DimensionMismatch`).
async fn build_qdrant_adapter_with_model(
    model_info: &EmbeddingModelInfo,
) -> Result<Arc<QdrantAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    // Per-run test isolation (#164) overrides the collection with a unique name.
    // Production and live containers leave QDRANT_COLLECTION unset, so the
    // model-keyed default is used.
    let collection_name = match std::env::var("QDRANT_COLLECTION") {
        Ok(override_name) if !override_name.trim().is_empty() => override_name,
        _ => model_keyed_collection_name(&model_info.model_name)?,
    };
    let qdrant_config = QdrantConfig {
        endpoint: env_var("QDRANT_URL")?,
        timeout_ms: 3_000,
        collection_name: collection_name.clone(),
    };
    let qdrant_adapter = QdrantAdapter::from_config(qdrant_config)?;
    let vector_size = model_info.dimension as u64;
    // Qdrant boot-resilience (Option A / ADR-0001, DS-004): Qdrant is a WRITE-SIDE
    // store only — the read path serves entirely from the in-memory snapshot. A
    // Qdrant outage at boot must therefore NOT prevent the server from coming up and
    // serving compile_context; it only delays outbox→Qdrant draining, which the
    // OutboxRelay already retries per-operation (and reconciliation closes any gap).
    // So a failed connectivity check / collection ensure is logged loudly and boot
    // proceeds, instead of aborting. This is what lets the durability contract
    // (relay restarts while Qdrant is down, then replays the backlog once Qdrant
    // returns — the collection persists in Qdrant's own volume) hold.
    //
    // EXCEPTION: DimensionMismatch is fatal WHEN QDRANT IS REACHABLE AT BOOT.
    // If Qdrant is offline at boot, ensure_collection is skipped entirely and the
    // dimension guard cannot run. The corruption window is then: Qdrant returns,
    // the OutboxRelay begins writing, and a stale wrong-dim collection would accept
    // writes silently. To narrow this window, dimension-mismatch write errors from
    // the OutboxRelay are surfaced as explicit log lines (not buried in generic
    // Qdrant errors), so an operator will see them in the first relay batch.
    if let Err(error) = qdrant_adapter.check_connectivity().await {
        warn!(
            %error,
            collection = %collection_name,
            expected_dimension = vector_size,
            "Qdrant unreachable at boot — dimension guard skipped (OFFLINE BYPASS). \
             Starting in write-side-degraded mode (read path unaffected, Option A). \
             Outbox draining and dimension verification resume when Qdrant returns. \
             If a wrong-dimension collection exists, the first relay write batch will \
             surface the mismatch as an explicit error."
        );
    } else {
        match qdrant_adapter
            .ensure_collection(&collection_name, vector_size)
            .await
        {
            Ok(()) => {}
            Err(QdrantError::DimensionMismatch { .. }) => {
                // Dimension mismatch is fatal — propagate immediately rather than
                // degrading, because a wrong-dim collection would silently corrupt
                // every cosine-similarity ranking.
                return Err(format!(
                    "qdrant collection '{collection_name}' has the wrong dimension for \
                     model '{}' (expected {vector_size}); drop the collection or change \
                     OLLAMA_EMBED_MODEL",
                    model_info.model_name
                )
                .into());
            }
            Err(error) => {
                warn!(
                    %error,
                    collection = %collection_name,
                    "Qdrant reachable but ensure_collection failed at boot — the OutboxRelay will \
                     retry; read path unaffected."
                );
            }
        }
    }
    Ok(Arc::new(qdrant_adapter))
}

/// Delegates to the shared infrastructure helper so the resolution policy is
/// defined in exactly one place.
///
/// Kept as a local name so the `embedding_model_env_tests` module below can call
/// it without import churn; #240 will migrate those tests to use the pure
/// `infrastructure::resolve_embedding_model` directly.
fn embedding_model_from_env() -> String {
    infrastructure::embedding_model_from_env()
}

fn build_embedding_service()
-> Result<Arc<OllamaEmbeddingService>, Box<dyn std::error::Error + Send + Sync>> {
    // Concurrency cap for embedding requests. The previous hardcoded `4` throttled
    // the warm read path: under concurrent sessions, per-request `compile_context`
    // embed calls piled up behind a 4-permit semaphore, driving p95 latency far over
    // the 500ms SLA even though a single embed is ~50ms and Ollama parallelizes well
    // (DS-007). Default raised to 16 and made tunable via `EMBED_MAX_CONCURRENCY`;
    // pair with `OLLAMA_NUM_PARALLEL` on the Ollama server for real parallelism.
    let max_concurrency: usize = match std::env::var("EMBED_MAX_CONCURRENCY") {
        Ok(raw) => raw
            .parse()
            .map_err(|_| format!("EMBED_MAX_CONCURRENCY is set but not a valid usize: {raw:?}"))?,
        Err(_) => 16,
    };
    if max_concurrency == 0 {
        return Err("EMBED_MAX_CONCURRENCY must be greater than zero".into());
    }
    let ollama_config = OllamaEmbeddingConfig {
        base_url: env_var("OLLAMA_URL")?,
        model: embedding_model_from_env(),
        max_concurrency,
    };
    let embedding_service = OllamaEmbeddingService::from_config(ollama_config)
        .map_err(|e| format!("ollama init failed: {e}"))?;
    Ok(Arc::new(embedding_service))
}

async fn build_redis_streams_adapter()
-> Result<Arc<RedisStreamsAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    // Stream key / consumer group / consumer name default to the single canonical
    // source (`RedisStreamsConfig::default`, which uses `SKILL_LAYER_STREAM_KEY` /
    // `SKILL_LAYER_CONSUMER_GROUP`) so production and the live containers stay on the
    // shared bus and the subscriber can never drift from the publisher. The optional
    // `REDIS_STREAM_KEY` / `REDIS_CONSUMER_GROUP` / `REDIS_CONSUMER_NAME` overrides
    // exist ONLY for per-run test isolation (#164) — they mirror the same env names
    // the session-extractor already honors, so a namespaced test consumes its own
    // stream/group and its destructive teardown never DELs the shared one.
    let defaults = RedisStreamsConfig::default();
    let redis_config = RedisStreamsConfig {
        redis_url: env_var("REDIS_URL")?,
        stream_key: env_or("REDIS_STREAM_KEY", &defaults.stream_key),
        consumer_group: env_or("REDIS_CONSUMER_GROUP", &defaults.consumer_group),
        consumer_name: env_or("REDIS_CONSUMER_NAME", &defaults.consumer_name),
        ..defaults
    };
    let redis_streams = RedisStreamsAdapter::new(redis_config)?;
    redis_streams.ensure_consumer_group().await?;
    Ok(Arc::new(redis_streams))
}

fn build_redis_client() -> Result<RedisClient, Box<dyn std::error::Error + Send + Sync>> {
    Ok(RedisClient::open(env_var("REDIS_URL")?)?)
}

fn env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} must be set"))
}

/// Reads an optional env override, falling back to `default` when unset or blank.
///
/// Used for the per-run test-isolation overrides (#164): unset in production and
/// in the live containers, so they keep the canonical stream/group/collection
/// names; set only by the in-process test harness's namespace guard.
fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => default.to_owned(),
    }
}

/// Reads a `PATH`-style env var into canonicalized scope-root paths.
///
/// Used to give live-loaded skills the scope provenance the persisted schema
/// lacks. Paths are canonicalized to align with the scope resolver's own
/// canonicalization so `starts_with` scope matching succeeds; unset or invalid
/// entries are skipped (boot stays resilient).
fn scope_paths_from_env(name: &str) -> Vec<std::path::PathBuf> {
    let Ok(raw) = std::env::var(name) else {
        return Vec::new();
    };
    raw.split([':', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| std::fs::canonicalize(entry).ok())
        .collect()
}

/// Reloads the bounded PG snapshot and atomically swaps it into the live
/// retriever, reusing the SAME [`build_graph_from_pg`] loader used at boot
/// (caps at 5000, reads the real `graph_version`, populates `source_paths`).
///
/// This is the `mcp-server`-side bridge that keeps `retrieval` persistence- and
/// transport-agnostic: the subscriber depends only on the [`GraphReloader`] seam.
struct PostgresGraphReloader {
    pg_adapter: Arc<PostgresAdapter>,
    embedding_service: Arc<OllamaEmbeddingService>,
    retriever: Arc<RetrievalOrchestrator<OllamaEmbeddingService>>,
    usage_sample_store: Arc<PostgresUsageSampleStore>,
}

#[async_trait]
impl GraphReloader for PostgresGraphReloader {
    async fn reload_and_swap(&self) -> Result<i64, String> {
        let snapshot = build_graph_from_pg(
            self.pg_adapter.pool(),
            self.embedding_service.as_ref(),
            self.usage_sample_store.as_ref(),
        )
        .await
        .map_err(|error| format!("graph reload from PG failed: {error}"))?;
        let target_version = snapshot.graph_version;
        // `swap_graph` is idempotent: re-applying the current/older version is a
        // no-op, so a coalesced burst that resolves to an already-applied version
        // still lets the triggering event be ACKed.
        let applied = self.retriever.swap_graph(snapshot);
        if applied {
            info!(target_version, "graph refresh applied");
        } else {
            debug!(target_version, "graph refresh no-op (already current)");
        }
        Ok(target_version)
    }
}

/// Spawns the graph-refresh subscriber on a detached Tokio task.
///
/// Runs on a detached Tokio task so a slow/failed reload never blocks request
/// handling. Returns immediately; the loop owns its own backoff/reconnect.
fn spawn_graph_refresh_subscriber(
    redis_streams: Arc<RedisStreamsAdapter>,
    pg_adapter: Arc<PostgresAdapter>,
    embedding_service: Arc<OllamaEmbeddingService>,
    retriever: Arc<RetrievalOrchestrator<OllamaEmbeddingService>>,
    usage_sample_store: Arc<PostgresUsageSampleStore>,
) {
    let reloader: Arc<dyn GraphReloader> = Arc::new(PostgresGraphReloader {
        pg_adapter,
        embedding_service,
        retriever,
        usage_sample_store,
    });
    tokio::spawn(run_graph_refresh_loop(redis_streams, reloader));
}

/// Loads the full skill graph from Postgres and populates each skill's
/// deterministic usage prior from the live `skill_usage` aggregates.
///
/// The `usage_sample_store` is queried once per graph load with all skill IDs
/// in a single batched query (no N+1). Skills with zero usage rows receive
/// `prior=0.0` (honest cold-start). The prior is a pure function of
/// `usage_count` and `age_days` — it is never written back to the DB.
#[tracing::instrument(skip(pool, embedding_service, usage_sample_store))]
async fn build_graph_from_pg(
    pool: &PostgresPool,
    embedding_service: &dyn EmbeddingService,
    usage_sample_store: &dyn UsageSampleStore,
) -> Result<RetrievalSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let store = PostgresGraphSnapshotStore::new(pool.clone());
    // Read the real durable version so the snapshot (and the version-keyed cache)
    // reflects the actual graph state, even on cold start with no skills.
    let graph_version = store
        .current_graph_version()
        .await
        .map_err(|e| format!("failed to read graph_version from graph_state: {e}"))?;
    let skills = store
        .list_skills()
        .await
        .map_err(|e| format!("failed to list skills from PG: {e}"))?;

    if skills.is_empty() {
        // Build an empty-but-valid BM25 index even for an empty corpus so the
        // `SnapshotHybrid` arm never observes a `None` index in production. A
        // present BM25 index is a construction invariant of every snapshot this
        // function returns — not an optional add-on — so the hybrid candidate
        // expander can treat a missing index as a hard programming error (#247).
        // `Bm25Index::build(&[])` returns a valid empty index (empty query → no hits).
        let bm25_index = Arc::new(retrieval::Bm25Index::build(&[]));
        return Ok(RetrievalSnapshot::new(vec![], graph_version).with_bm25_index(bm25_index));
    }

    // Fail loud when the corpus exceeds the configured cap. Silent truncation
    // violates the no-arbitrary-limits-on-churners standing rule: a degraded,
    // partially-loaded graph is worse than a clear boot failure that tells the
    // operator exactly what to do. Raise the cap via MAX_SKILLS_TO_LOAD or
    // prune the corpus.
    const DEFAULT_MAX_SKILLS_TO_LOAD: usize = 100_000;
    let max_skills_to_load: usize = match std::env::var("MAX_SKILLS_TO_LOAD") {
        Ok(raw) => raw
            .parse()
            .map_err(|_| format!("MAX_SKILLS_TO_LOAD is set but not a valid usize: {raw:?}"))?,
        Err(_) => DEFAULT_MAX_SKILLS_TO_LOAD,
    };
    if skills.len() > max_skills_to_load {
        return Err(format!(
            "corpus has {} skills which exceeds the MAX_SKILLS_TO_LOAD cap of {}; \
             raise the cap by setting MAX_SKILLS_TO_LOAD=<n> in the environment, \
             or prune the skill corpus to fit within the current limit",
            skills.len(),
            max_skills_to_load,
        )
        .into());
    }

    let texts: Vec<String> = skills
        .iter()
        .map(|s| format!("{} {} {}", s.name, s.description, s.tags.join(" ")))
        .collect();
    let text_refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let embeddings = embedding_service.embed_batch(&text_refs).await?;

    // Fail loudly if the embedding provider returned a mismatched batch. A shorter
    // vector would cause silent zip truncation, loading a graph with fewer skills
    // than PG contains and no diagnostic. Crashing the reload is preferable.
    if embeddings.len() != skills.len() {
        return Err(format!(
            "embed_batch returned {} vectors for {} skills",
            embeddings.len(),
            skills.len()
        )
        .into());
    }

    // Embed every subunit's text so the read path can score the β
    // (subunit_evidence) term of eq.3 semantically at request time (issue #172).
    // Subunit embeddings live ONLY in the in-memory snapshot — recomputed at boot
    // exactly like skill embeddings above, so no migration or write-side storage is
    // needed. One flat batch keeps this to a single provider round trip; it is
    // re-sliced back into per-skill groups in the SeededSkill map below.
    let subunit_texts: Vec<String> = skills
        .iter()
        .flat_map(|s| {
            s.subunits
                .iter()
                .map(|su| format!("{} {}", su.title, su.content))
        })
        .collect();
    let per_skill_subunit_embeddings: Vec<Vec<Vec<f32>>> = if subunit_texts.is_empty() {
        skills.iter().map(|_| Vec::new()).collect()
    } else {
        let subunit_text_refs: Vec<&str> = subunit_texts.iter().map(String::as_str).collect();
        let flat = embedding_service.embed_batch(&subunit_text_refs).await?;
        // Fail loudly on a mismatched batch for the same reason as the skill path:
        // a short vector would silently mis-align subunit embeddings to subunits.
        if flat.len() != subunit_texts.len() {
            return Err(format!(
                "embed_batch returned {} subunit vectors for {} subunit texts",
                flat.len(),
                subunit_texts.len()
            )
            .into());
        }
        let mut flat = flat.into_iter();
        skills
            .iter()
            .map(|s| {
                (0..s.subunits.len())
                    .map(|_| {
                        flat.next().expect(
                            "flat subunit embedding stream exhausted before all subunits were \
                             assigned — len check above guarantees this cannot happen",
                        )
                    })
                    .collect()
            })
            .collect()
    };

    // ── T09: Dense multi-view embeddings ────────────────────────────────────
    // Build e_task / e_needs / e_negative texts from the T03 multi-view fields
    // and embed them as three flat batches. Built UNCONDITIONALLY at boot so
    // RETRIEVAL_DENSE_VIEWS can be toggled at request time without a graph
    // rebuild — the scoring path simply does not read these vectors when the flag
    // is off. The single-source-of-truth field→text mapping lives in
    // `retrieval::dense_views` (mirrors BM25's `skill_lexical_document`).
    //
    // Subunit procedure text for e_task uses short form (title only, not full
    // body) to honour the embedding-window discipline (plan line 251):
    //   subunit_title_text = subunit headings joined with spaces.
    //
    // Three separate embed_batch calls are intentional: the provider may have
    // a per-request token budget; keeping the per-batch size bounded (== corpus
    // size per view) is safer than one mega-batch.
    //
    // Fail-loud guards: each batch length is checked against `skills.len()`
    // before zipping (mirrors the e_summary and subunit batch guards above).
    let dense_view_embedding_dim: usize;
    let (
        per_skill_e_task_embeddings,
        per_skill_e_needs_embeddings,
        per_skill_e_negative_embeddings,
    ) = {
        use retrieval::{SkillDenseViewFields, build_e_needs, build_e_negative, build_e_task};

        // Build all three view texts in one pass over `skills` before it is
        // consumed by `.into_iter()`. The subunit_procedure_text for e_task
        // uses titles only — not full content — to stay bounded.
        let (mut e_task_texts, mut e_needs_texts, mut e_negative_texts) = (
            Vec::with_capacity(skills.len()),
            Vec::with_capacity(skills.len()),
            Vec::with_capacity(skills.len()),
        );
        for record in &skills {
            let subunit_title_text: String = record
                .subunits
                .iter()
                .map(|su| su.title.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let fields = SkillDenseViewFields {
                use_when: &record.use_when,
                avoid_when: &record.avoid_when,
                artifacts: &record.artifacts,
                tools: &record.tools,
                invariants: &record.invariants,
                requires: &record.requires,
                subunit_procedure_text: &subunit_title_text,
            };
            e_task_texts.push(build_e_task(&fields));
            e_needs_texts.push(build_e_needs(&fields));
            e_negative_texts.push(build_e_negative(&fields));
        }

        let e_task_refs: Vec<&str> = e_task_texts.iter().map(String::as_str).collect();
        let e_task_flat = embedding_service.embed_batch(&e_task_refs).await?;
        if e_task_flat.len() != skills.len() {
            return Err(format!(
                "embed_batch returned {} e_task vectors for {} skills",
                e_task_flat.len(),
                skills.len()
            )
            .into());
        }

        let e_needs_refs: Vec<&str> = e_needs_texts.iter().map(String::as_str).collect();
        let e_needs_flat = embedding_service.embed_batch(&e_needs_refs).await?;
        if e_needs_flat.len() != skills.len() {
            return Err(format!(
                "embed_batch returned {} e_needs vectors for {} skills",
                e_needs_flat.len(),
                skills.len()
            )
            .into());
        }

        let e_negative_refs: Vec<&str> = e_negative_texts.iter().map(String::as_str).collect();
        let e_negative_flat = embedding_service.embed_batch(&e_negative_refs).await?;
        if e_negative_flat.len() != skills.len() {
            return Err(format!(
                "embed_batch returned {} e_negative vectors for {} skills",
                e_negative_flat.len(),
                skills.len()
            )
            .into());
        }

        // Capture the embedding dimensionality from the first non-empty vector
        // for the DenseViewsMetadata (observability only; all views share the same dim).
        dense_view_embedding_dim = e_task_flat.first().map_or(0, |v| v.len());

        (e_task_flat, e_needs_flat, e_negative_flat)
    };

    // Live-loaded skills have no per-file provenance (the `skills` table stores no
    // source path), so their searchable scope is the configured scope root. Without
    // this, `seeded_skill_matches_scope` rejects every live skill against a
    // path-constrained scope and boot retrieval always returns `no_match`.
    // Resolve the configured scope-root paths OFF the async executor: each
    // `canonicalize` is a blocking filesystem syscall, and running it on a runtime
    // worker thread stalls other tasks at boot / `graph.rebuilt` (#142). Boot-only;
    // not on the request hot path.
    //
    // - `global_scope_paths`: from `SKILL_GLOBAL_PATHS`.
    // - `project_scope_paths`: fallback project root for skills with empty
    //   `source_paths`. Prefers the operator-declared `SKILL_PROJECT_ROOT` (#154)
    //   so it aligns with `FsMarkerProjectResolver` in a container (working dir `/`),
    //   falling back to the process working directory. Canonicalized to match the
    //   resolver's own canonicalization so `starts_with` scope matching succeeds.
    let (global_scope_paths, project_scope_paths) = tokio::task::spawn_blocking(|| {
        let global = scope_paths_from_env("SKILL_GLOBAL_PATHS");
        let project = std::env::var("SKILL_PROJECT_ROOT")
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .and_then(|p| std::fs::canonicalize(p).ok())
            .map(|p| vec![p])
            .unwrap_or_default();
        (global, project)
    })
    .await?;

    // Batch-query usage for all skills in one round trip so the prior is
    // populated at load time without N+1 queries. Skills with no rows get
    // total_count=0 → prior=0.0 (honest cold-start).
    // Source the window from RetirementConfig so the prior and retirement eligibility
    // always use the same lookback period — no silent divergence on config change.
    let skill_ids: Vec<String> = skills.iter().map(|s| s.skill_id.clone()).collect();
    let usage_by_skill: std::collections::HashMap<String, retrieval::UsagePriorInputs> =
        match usage_sample_store
            .recent_usage(&skill_ids, RetirementConfig::default().scoring_window_days)
            .await
        {
            Ok(summaries) => summaries
                .into_iter()
                .map(|s| {
                    (
                        s.skill_id.clone(),
                        retrieval::UsagePriorInputs {
                            usage_count: s.total_count,
                            age_days: s.age_days.unwrap_or(u32::MAX),
                        },
                    )
                })
                .collect(),
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to load usage priors at graph load time; cold-start priors (0.0) used"
                );
                std::collections::HashMap::new()
            }
        };

    // T04-B: Pre-build per-skill BM25 lexical documents from the full
    // PersistedGraphSkillRecord (including migration-009 multi-view fields) BEFORE
    // `skills` is consumed by `.into_iter()` below.
    //
    // Field policy and avoid_when exclusion rationale live in
    // `retrieval::bm25::skill_lexical_document` — the single source of truth shared
    // with the write-side sparse-vector path in graph-builder.
    let bm25_raw_docs: Vec<String> = skills
        .iter()
        .map(|record| {
            let subunit_text: String = record
                .subunits
                .iter()
                .map(|su| format!("{} {}", su.title, su.content))
                .collect::<Vec<_>>()
                .join(" ");
            skill_lexical_document(&SkillLexicalFields {
                name: &record.name,
                description: &record.description,
                tags: &record.tags,
                tools: &record.tools,
                artifacts: &record.artifacts,
                invariants: &record.invariants,
                use_when: &record.use_when,
                requires: &record.requires,
                produces: &record.produces,
                subunit_text: &subunit_text,
            })
        })
        .collect();

    // Canonicalize every skill `source_path` OFF the async executor: at boot this
    // is one blocking filesystem syscall per path (O(skills)), which would
    // otherwise stall a runtime worker thread (#142). Precompute a raw→canonical
    // map here so the synchronous SeededSkill assembly below is a pure in-memory
    // lookup.
    let raw_source_paths: Vec<String> = skills
        .iter()
        .flat_map(|s| s.source_paths.iter().cloned())
        .collect();
    let canonical_source_paths: std::collections::HashMap<String, std::path::PathBuf> =
        tokio::task::spawn_blocking(move || {
            raw_source_paths
                .into_iter()
                .map(|raw| {
                    // The path may not exist on this host (skill built elsewhere);
                    // fall back to the raw string so a prefix check can still run.
                    let canonical = std::fs::canonicalize(&raw)
                        .unwrap_or_else(|_| std::path::PathBuf::from(&raw));
                    (raw, canonical)
                })
                .collect()
        })
        .await?;

    let seeded_skills: Vec<retrieval::SeededSkill> = skills
        .into_iter()
        .zip(embeddings.into_iter())
        .zip(per_skill_subunit_embeddings.into_iter())
        .zip(per_skill_e_task_embeddings.into_iter())
        .zip(per_skill_e_needs_embeddings.into_iter())
        .zip(per_skill_e_negative_embeddings.into_iter())
        .map(
            |(
                ((((record, embedding), subunit_embeddings), e_task_embedding), e_needs_embedding),
                e_negative_embedding,
            )| {
                // Derive scope type, scope_id, and the fallback scope-root paths from
                // the `skills.scope` column in one match so the two arms cannot drift.
                let (scope, scope_id, fallback_scope_paths) = match record.scope.as_str() {
                    "global" => (
                        domain::ScopeType::Global,
                        "global".to_owned(),
                        global_scope_paths.clone(),
                    ),
                    // "team" is reserved-but-unwired forward-compat: the scope column
                    // and DB CHECK constraint already allow it so future wiring is a
                    // pure addition. The DualScopeResolver currently emits only
                    // "project" and "global"; no team resolver exists yet.
                    //
                    // The fallback is intentionally empty (no team scope root is
                    // configured); skills that also have empty `source_paths` will
                    // receive zero paths and be invisible to all `starts_with` scope
                    // gates.  The warn! below makes that observable so the silent
                    // drop is not mistaken for a DB or retrieval bug.
                    "team" => (domain::ScopeType::Team, "team".to_owned(), Vec::new()),
                    _ => (
                        domain::ScopeType::Project,
                        "project".to_owned(),
                        project_scope_paths.clone(),
                    ),
                };
                // Use the real per-skill SKILL.md provenance from the `source_paths`
                // column (migration 005). Fall back to the configured scope root for
                // pre-migration rows whose column value is empty — this matches T01's
                // stand-in exactly and keeps old graphs scope-matching correctly.
                let source_paths = if record.source_paths.is_empty() {
                    fallback_scope_paths
                } else {
                    // Look up the canonical form precomputed off-executor above. Falls
                    // back to the raw path (parsed) when canonicalization failed — safer
                    // than silently substituting the scope root.
                    record
                        .source_paths
                        .iter()
                        .map(|p| {
                            canonical_source_paths
                                .get(p)
                                .cloned()
                                .unwrap_or_else(|| std::path::PathBuf::from(p))
                        })
                        .collect()
                };
                // Team scope has no configured scope root (no team resolver is wired
                // yet).  A team skill whose DB row also carries no `source_paths`
                // ends up with zero paths and is invisible to every `starts_with`
                // scope gate — it will be silently dropped from retrieval results.
                // Log here so the gap is observable without a debugger.
                if scope_id == "team" && source_paths.is_empty() {
                    warn!(
                        skill_id = %record.skill_id,
                        "team-scope skill has no source_paths and no configured team \
                         scope root — it will be excluded from all scope-filtered \
                         retrievals until a team scope root is wired or the skill \
                         gains explicit source_paths"
                    );
                }
                // Deterministic prior from real usage: ln(1+count)·e^(-age/30),
                // clamped at 0.15. Zero for cold-start (no usage rows) so unseen
                // skills never receive a phantom boost. V1.5 fixed formula — see
                // `retrieval::scoring::usage_prior` for the sealed constants.
                let prior_inputs = usage_by_skill.get(&record.skill_id).copied().unwrap_or(
                    retrieval::UsagePriorInputs {
                        usage_count: 0,
                        age_days: 0,
                    },
                );
                let prior = retrieval::usage_prior(prior_inputs.usage_count, prior_inputs.age_days);

                // For the domain::Skill, expose the first (lowest-ID) community
                // membership so callers that rely on a single community_id still work.
                // The community_boost uses the full membership set: any membership earns
                // the boost, consistent with the dual-membership spec.
                let mut sorted_community_ids = record.community_ids.clone();
                sorted_community_ids.sort();
                let primary_community_id = sorted_community_ids.into_iter().next();

                let skill = domain::Skill {
                    id: domain::DomainId::new_unchecked(&record.skill_id),
                    name: record.name,
                    description: record.description,
                    scope,
                    status: domain::SkillStatus::Ready,
                    lifecycle: domain::LifecycleStatus::Active,
                    tags: record.tags,
                    subunit_ids: record
                        .subunits
                        .iter()
                        .map(|s| domain::DomainId::new_unchecked(&s.subunit_id))
                        .collect(),
                    community_id: primary_community_id
                        .map(|id| domain::DomainId::new_unchecked(&id)),
                };
                // Community boost: applies whenever a skill belongs to any community
                // (hdbscan OR tag) — matching the dual-membership spec.
                let community_boost = if !record.community_ids.is_empty() {
                    0.2
                } else {
                    0.0
                };
                let subunits: Vec<domain::Subunit> = record
                    .subunits
                    .into_iter()
                    .map(|s| domain::Subunit {
                        id: domain::DomainId::new_unchecked(&s.subunit_id),
                        skill_id: skill.id.clone(),
                        kind: subunit_kind_from_db(&s.kind),
                        title: s.title,
                        content: s.content,
                        lifecycle: domain::LifecycleStatus::Active,
                    })
                    .collect();

                retrieval::SeededSkill {
                    skill,
                    scope_id,
                    source_paths,
                    embedding,
                    subunits,
                    subunit_embeddings,
                    prior,
                    community_boost,
                    // T09 dense multi-view embeddings (unconditionally built; the
                    // RETRIEVAL_DENSE_VIEWS flag gates whether the scoring path reads them).
                    e_task_embedding,
                    e_needs_embedding,
                    e_negative_embedding,
                }
            },
        )
        .collect();

    // T04-B: Build the BM25 lexical index from the pre-built `bm25_raw_docs`
    // (assembled above from PersistedGraphSkillRecord before `skills` was consumed).
    // Built unconditionally — cheap (~ms for 5000 skills) — so `RETRIEVAL_BACKEND`
    // can switch dense↔hybrid at request time without a graph rebuild. The index is
    // Arc-wrapped so a clone on each ArcSwap is one atomic ref-count increment.
    let bm25_docs: Vec<(usize, String)> = bm25_raw_docs.into_iter().enumerate().collect();
    let bm25_index = Arc::new(retrieval::Bm25Index::build(&bm25_docs));

    // #208: per-community centroids for CommunityBoostMode::CentroidAffinity.
    // Centroid of community c = mean of the ℓ₁ embeddings of skills whose PRIMARY
    // community is c (matches the query-time lookup by skill.community_id).
    // cosine_similarity normalizes at use, so the raw mean is sufficient. Empty for
    // the binary/off modes' purposes is harmless (the modes never read it).
    let mut community_sums: std::collections::HashMap<String, (Vec<f32>, usize)> =
        std::collections::HashMap::new();
    for s in &seeded_skills {
        let Some(cid) = s.skill.community_id.as_ref() else {
            continue;
        };
        if s.embedding.is_empty() {
            continue;
        }
        let entry = community_sums
            .entry(cid.as_str().to_owned())
            .or_insert_with(|| (vec![0.0_f32; s.embedding.len()], 0));
        if entry.0.len() == s.embedding.len() {
            for (acc, v) in entry.0.iter_mut().zip(s.embedding.iter()) {
                *acc += v;
            }
            entry.1 += 1;
        }
    }
    let community_centroids: std::collections::HashMap<String, Vec<f32>> = community_sums
        .into_iter()
        .filter(|(_, (_, n))| *n > 0)
        .map(|(cid, (sum, n))| (cid, sum.iter().map(|x| x / n as f32).collect::<Vec<f32>>()))
        .collect();

    // T09: build DenseViewsMetadata for observability (health endpoint / snapshot
    // metadata). Attached unconditionally — even when all view texts were empty —
    // so the health endpoint always reports whether views were built.
    let dense_views_metadata = retrieval::DenseViewsMetadata {
        view_names: vec![
            "e_task".to_owned(),
            "e_needs".to_owned(),
            "e_negative".to_owned(),
        ],
        embedding_dim: dense_view_embedding_dim,
        skill_count_with_views: seeded_skills.len(),
    };

    Ok(RetrievalSnapshot::new(seeded_skills, graph_version)
        .with_community_centroids(community_centroids)
        .with_bm25_index(bm25_index)
        .with_dense_views_metadata(dense_views_metadata))
}

/// Computes the BLAKE3 hash of a raw prompt string for safe storage.
///
/// Security P3 (T06): `session_logs.prompt_hash` stores this hash, never the
/// raw prompt text. Matches the cache-key hash pattern used elsewhere in the
/// server so cache and usage rows share the same key space.
fn compute_prompt_hash(prompt: &str) -> String {
    blake3::hash(prompt.as_bytes()).to_hex().to_string()
}

/// Builds a [`SessionUsageRecord`] from the minimal request fields needed for
/// persistence, the compilation response, and the retrieval outcome.
///
/// Called at the coordination layer in `McpServerApp::compile_context` AFTER
/// the tool returns so this function never touches `CompileContextTool`'s
/// internal state. Skills with no UUID-format IDs are silently skipped rather
/// than aborting the entire write — an invalid ID in the graph is a data
/// hygiene issue, not a reason to drop the whole session's usage.
///
/// Accepts `session_id` and `repo_path` as separate borrowed strings rather than
/// a full `&CompileContextRequest` so the caller can pass ownership of the request
/// to `invoke_and_capture_outcome` without cloning the prompt String.
fn build_session_usage_record(
    session_id: &str,
    repo_path: &str,
    response: &CompileContextResponse,
    outcome: &retrieval::RetrievalOutcome,
    prompt_hash: &str,
) -> SessionUsageRecord {
    let context_status = match response.status {
        CompileContextStatus::Ok => "ok",
        CompileContextStatus::NoMatch => "no_match",
        CompileContextStatus::Degraded => "degraded",
        CompileContextStatus::DuplicateSuppressed => "duplicate_suppressed",
    };

    // Determine the scope string from the first resolved scope, falling back to
    // "project" so the DB CHECK constraint is always satisfied.
    let scope = if repo_path.is_empty() {
        "global"
    } else {
        "project"
    };

    let selected_skills: Vec<SkillSelectionRecord> = outcome
        .skills
        .iter()
        .filter_map(|retrieved| {
            let raw_id = retrieved.scored_skill.skill.id.as_str();
            // Only skills whose IDs parse as UUIDs can be persisted in skill_usage
            // (foreign key to skills.id which is UUID). Log non-UUID IDs at debug
            // level rather than dropping the whole write.
            if raw_id.parse::<uuid::Uuid>().is_err() {
                debug!(
                    skill_id = raw_id,
                    "skill_id is not a UUID; skipping usage row for this skill"
                );
                return None;
            }
            Some(SkillSelectionRecord {
                skill_id: raw_id.to_owned(),
                relevance_score: retrieved.scored_skill.score,
                context_status: context_status.to_owned(),
            })
        })
        .collect();

    SessionUsageRecord {
        session_id: session_id.to_owned(),
        prompt_hash: prompt_hash.to_owned(),
        scope: scope.to_owned(),
        latency_ms: response.latency_ms as i64,
        status: context_status.to_owned(),
        selected_skills,
    }
}

/// Maps a DB subunit kind string to the domain [`domain::SubunitType`].
///
/// This is the inverse of `infrastructure::persistence::rebuild::subunit_kind_to_db_value`.
/// String values must match exactly what that function writes to avoid silent mismatches.
/// Unknown strings default to `Procedure` so unrecognized future variants degrade
/// gracefully rather than causing a boot failure.
fn subunit_kind_from_db(raw: &str) -> domain::SubunitType {
    match raw {
        "procedure" => domain::SubunitType::Procedure,
        "convention" => domain::SubunitType::Convention,
        "asset" => domain::SubunitType::Asset,
        "evidence" => domain::SubunitType::Evidence,
        "summary" => domain::SubunitType::Summary,
        _ => domain::SubunitType::Procedure,
    }
}

#[cfg(test)]
mod embedding_model_env_tests {
    use infrastructure::resolve_embedding_model;

    /// Proves the resolution logic returns the default when no value is present.
    ///
    /// Tests `resolve_embedding_model` directly so no global env mutation is needed.
    /// The exhaustive behavioral suite lives in `infrastructure`; these tests
    /// confirm the key contract from the mcp-server perspective.
    #[test]
    fn resolve_embedding_model_returns_nomic_default_when_raw_is_none() {
        assert_eq!(
            resolve_embedding_model(None),
            "nomic-embed-text",
            "None (env var unset) must yield the nomic-embed-text default"
        );
    }

    /// Proves blank raw values fall back to the default, matching docker-compose
    /// interpolation behaviour (`OLLAMA_EMBED_MODEL: ${OLLAMA_EMBED_MODEL:-}`).
    #[test]
    fn resolve_embedding_model_returns_nomic_default_when_raw_is_blank() {
        assert_eq!(
            resolve_embedding_model(Some("")),
            "nomic-embed-text",
            "empty string must yield the nomic-embed-text default"
        );
    }

    /// Proves an explicit non-blank model name is returned as-is.
    #[test]
    fn resolve_embedding_model_returns_configured_model_when_raw_is_set() {
        assert_eq!(
            resolve_embedding_model(Some("qwen3-embedding:4b")),
            "qwen3-embedding:4b",
            "a non-blank model name must be returned unchanged"
        );
    }

    /// Smoke-tests that the local `embedding_model_from_env` wrapper returns a
    /// non-empty String, proving it delegates to the infrastructure resolution
    /// path rather than returning a hard-coded constant or panicking.
    ///
    /// This test reads the real environment but never mutates it, so it is
    /// safe to run concurrently with any other test.
    #[test]
    fn embedding_model_from_env_returns_non_empty_string() {
        let model = super::embedding_model_from_env();
        assert!(
            !model.is_empty(),
            "embedding_model_from_env must always return a non-empty model name"
        );
    }
}
