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
    EnvPathGlobalResolver, GitRootProjectResolver, OllamaEmbeddingConfig, OllamaEmbeddingService,
    PostgresAdapter, PostgresConfig, PostgresGraphSnapshotStore, PostgresGraphWriteCoordinator,
    PostgresPool, PostgresRebuildCoordinator, PostgresUsageSampleStore, PostgresUsageWriter,
    QdrantAdapter, QdrantConfig, RedisClient, RedisStreamsAdapter, RedisStreamsConfig,
    SessionUsageRecord, SkillSelectionRecord, TranscriptIngestQueue, UsagePersistencePort,
    UsageSampleStore,
};
use maintenance::RetirementConfig;
use retrieval::{
    DualScopeResolver, RetrievalConfig, RetrievalOrchestrator, RetrievalSnapshot, SkillRetriever,
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
    /// `None` when usage logging is disabled (`MCP_USAGE_LOGGING=off`) or when the
    /// server was constructed without a Postgres pool (test/in-memory constructors).
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
    /// so test/in-memory constructors stay free of Postgres. When not called (or when
    /// `MCP_USAGE_LOGGING=off`), usage writes are silently skipped and the health
    /// marker stays `"ok"`.
    ///
    /// Returns `Option<JoinHandle<()>>` so the caller (e.g. `build_live_server`) can
    /// stash the handle on `LiveServerComponents` for deterministic drain during
    /// teardown. Dropping the sender signals end-of-channel; awaiting the handle waits
    /// for the last in-flight write to complete — must be done BEFORE `TRUNCATE` to
    /// avoid the RowExclusive vs ACCESS EXCLUSIVE deadlock.
    pub fn with_usage_writer(
        mut self,
        writer: Arc<dyn UsagePersistencePort>,
    ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
        let health = self.usage_write_health.clone();
        let join_handle = if let Some(UsageWriterHandle {
            sender,
            join_handle,
        }) = spawn_usage_writer(writer, health)
        {
            self.usage_sender = Some(sender);
            Some(join_handle)
        } else {
            None
        };
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
        let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
        let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
        let project_resolver: Arc<dyn ScopeResolver> =
            Arc::new(GitRootProjectResolver::new(start_dir));
        let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
        let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

        let retriever =
            RetrievalOrchestrator::new_dual_scope(embedding_service, graph, config, scope_resolver);
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
        // Three distinct states are emitted:
        //   "ok"       — writer active and last write succeeded (key suppressed for brevity)
        //   "disabled" — MCP_USAGE_LOGGING=off; usage_sender is None; no rows written
        //   "failed"   — writer active but last write or channel post failed
        //
        // "disabled" is always injected (not absent) so an agent can distinguish it
        // from "healthy" when the rollback flag is in effect.
        let usage_health = if self.usage_sender.is_none() {
            "disabled".to_owned()
        } else {
            read_usage_write_health(&self.usage_write_health)
        };
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
    /// Join handle for the background usage-writer task.
    ///
    /// `None` when usage logging was disabled at boot. Teardown must drop
    /// `app.usage_sender` then await this handle before calling `truncate_all_tables`.
    #[cfg(any(test, feature = "test-utils"))]
    pub usage_writer_join_handle: Option<tokio::task::JoinHandle<()>>,
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
        if let Some(handle) = self.usage_writer_join_handle.take() {
            // Abort-and-await is safe: the task caught all Result paths and never
            // holds application state that requires graceful flushing. The abort
            // ensures we do not wait indefinitely if a write is in progress; the
            // await guarantees the OS-level task resources are reclaimed.
            handle.abort();
            let _ = handle.await;
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

/// Live-component assembly used exclusively by [`McpServerApp::from_environment`].
/// Kept private so the public surface stays at exactly two constructors.
async fn build_live_server(
    config: RetrievalConfig,
) -> Result<LiveServerComponents, Box<dyn std::error::Error + Send + Sync>> {
    let (pg_adapter, (qdrant_adapter, embedding_service), redis_streams, redis_client) = tokio::try_join!(
        build_pg_adapter(),
        async {
            let q = build_qdrant_adapter().await?;
            let e = build_embedding_service()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((q, e))
        },
        build_redis_streams_adapter(),
        async {
            let c = build_redis_client()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(c)
        },
    )?;

    let write_coordinator = PostgresGraphWriteCoordinator::new(pg_adapter.pool().clone());
    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg_adapter.pool().clone());

    let usage_sample_store = Arc::new(PostgresUsageSampleStore::new(pg_adapter.pool().clone()));
    let graph = self::build_graph_from_pg(
        pg_adapter.pool(),
        embedding_service.as_ref(),
        usage_sample_store.as_ref(),
    )
    .await?;

    let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
    let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let project_resolver: Arc<dyn ScopeResolver> = Arc::new(GitRootProjectResolver::new(start_dir));
    let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
    let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

    let retriever = Arc::new(RetrievalOrchestrator::new_dual_scope(
        embedding_service.clone(),
        graph,
        config,
        scope_resolver,
    ));

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
    // production builds the handle variable is elided via #[cfg].
    #[cfg(any(test, feature = "test-utils"))]
    let (app, usage_writer_join_handle) = app_builder.with_usage_writer(usage_writer);
    #[cfg(not(any(test, feature = "test-utils")))]
    let (app, _usage_writer_join_handle) = app_builder.with_usage_writer(usage_writer);

    // Online refresh-without-restart (T02): subscribe to `graph.rebuilt`
    // and atomically swap the in-memory read model. Spawned on its own
    // task so it never blocks the HTTP server; gated by a rollback flag.
    // T06: pass the usage_sample_store so graph refreshes also populate
    // the deterministic usage prior.
    spawn_graph_refresh_if_enabled(
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
    let pg_adapter = PostgresAdapter::connect(&pg_config).await?;
    pg_adapter.run_migrations().await?;
    Ok(Arc::new(pg_adapter))
}

async fn build_qdrant_adapter()
-> Result<Arc<QdrantAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    let qdrant_config = QdrantConfig {
        endpoint: env_var("QDRANT_URL")?,
        timeout_ms: 3_000,
        collection_name: "skills".to_owned(),
    };
    let qdrant_adapter = QdrantAdapter::from_config(qdrant_config)?;
    qdrant_adapter.check_connectivity().await?;
    qdrant_adapter.ensure_collection("skills", 768).await?;
    Ok(Arc::new(qdrant_adapter))
}

fn build_embedding_service()
-> Result<Arc<OllamaEmbeddingService>, Box<dyn std::error::Error + Send + Sync>> {
    let ollama_config = OllamaEmbeddingConfig {
        base_url: env_var("OLLAMA_URL")?,
        model: "nomic-embed-text".to_owned(),
        timeout_ms: 5_000,
        batch_timeout_ms: 10_000,
        max_concurrency: 4,
    };
    let embedding_service = OllamaEmbeddingService::from_config(ollama_config)
        .map_err(|e| format!("ollama init failed: {e}"))?;
    Ok(Arc::new(embedding_service))
}

async fn build_redis_streams_adapter()
-> Result<Arc<RedisStreamsAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    // Stream key / consumer group / tuning come from the single canonical source
    // (`RedisStreamsConfig::default`, which uses `SKILL_LAYER_STREAM_KEY` /
    // `SKILL_LAYER_CONSUMER_GROUP`). Only the URL is environment-driven here — this
    // guarantees the subscriber can never drift from the publisher's stream/group.
    let redis_config = RedisStreamsConfig {
        redis_url: env_var("REDIS_URL")?,
        ..RedisStreamsConfig::default()
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

/// Rollback flag: set `MCP_GRAPH_REFRESH=off` to disable the online subscriber
/// and fall back to boot-only graph loading.
///
// TODO(remove-after-v1.5-green): delete this flag and always-spawn the
// subscriber once the online refresh path is proven. Removal criterion: first
// green CI on `main`.
const GRAPH_REFRESH_FLAG: &str = "MCP_GRAPH_REFRESH";

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

/// Spawns the graph-refresh subscriber unless rolled back via [`GRAPH_REFRESH_FLAG`].
///
/// Runs on a detached Tokio task so a slow/failed reload never blocks request
/// handling. Returns immediately; the loop owns its own backoff/reconnect.
fn spawn_graph_refresh_if_enabled(
    redis_streams: Arc<RedisStreamsAdapter>,
    pg_adapter: Arc<PostgresAdapter>,
    embedding_service: Arc<OllamaEmbeddingService>,
    retriever: Arc<RetrievalOrchestrator<OllamaEmbeddingService>>,
    usage_sample_store: Arc<PostgresUsageSampleStore>,
) {
    if std::env::var(GRAPH_REFRESH_FLAG).as_deref() == Ok("off") {
        warn!(
            flag = GRAPH_REFRESH_FLAG,
            "graph refresh subscriber disabled by rollback flag; graph is boot-only"
        );
        return;
    }

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
    let mut skills = store
        .list_skills()
        .await
        .map_err(|e| format!("failed to list skills from PG: {e}"))?;

    if skills.is_empty() {
        return Ok(RetrievalSnapshot::new(vec![], graph_version));
    }

    // Safety guard against unbounded memory growth. Truncating (rather than
    // erroring) keeps boot resilient: a degraded-but-serving graph beats a panic
    // on startup.
    const MAX_SKILLS_TO_LOAD: usize = 5000;
    if skills.len() > MAX_SKILLS_TO_LOAD {
        warn!(
            skill_count = skills.len(),
            max = MAX_SKILLS_TO_LOAD,
            "too many skills for in-memory snapshot; truncating to cap"
        );
        skills.truncate(MAX_SKILLS_TO_LOAD);
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

    // Live-loaded skills have no per-file provenance (the `skills` table stores no
    // source path), so their searchable scope is the configured scope root. Without
    // this, `seeded_skill_matches_scope` rejects every live skill against a
    // path-constrained scope and boot retrieval always returns `no_match`.
    let global_scope_paths = scope_paths_from_env("SKILL_GLOBAL_PATHS");
    // Canonicalize the project scope path to align with the scope resolver's own
    // canonicalization so `starts_with` scope matching succeeds at query time.
    let project_scope_paths = std::env::current_dir()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| vec![p])
        .unwrap_or_default();

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

    let seeded_skills: Vec<retrieval::SeededSkill> = skills
        .into_iter()
        .zip(embeddings.into_iter())
        .map(|(record, embedding)| {
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
                record
                    .source_paths
                    .iter()
                    .map(|p| {
                        // The path may not exist on the current host (e.g. the skill
                        // was built on another machine). Parse the raw string so
                        // scope matching can still attempt a prefix check; the
                        // starts_with will simply fail for non-canonical paths, which
                        // is safer than silently falling back to the scope root.
                        std::fs::canonicalize(p).unwrap_or_else(|_| std::path::PathBuf::from(p))
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
                community_id: record.community_id.map(domain::DomainId::new_unchecked),
            };
            let community_boost = if skill.community_id.is_some() {
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
                prior,
                community_boost,
            }
        })
        .collect();

    Ok(RetrievalSnapshot::new(seeded_skills, graph_version))
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
