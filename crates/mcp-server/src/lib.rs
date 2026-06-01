mod admin_wiring;
mod context_cache;
mod graph_refresh_subscriber;
pub mod protocol;
pub mod state;
mod suppression_state;
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
    PostgresPool, PostgresRebuildCoordinator, QdrantAdapter, QdrantConfig, RedisClient,
    RedisStreamsAdapter, RedisStreamsConfig, TranscriptIngestQueue,
};
use retrieval::{
    DualScopeResolver, RetrievalConfig, RetrievalOrchestrator, RetrievalSnapshot, SkillRetriever,
};
use tools::{
    compile_context::{CompileContextRequest, CompileContextResponse, CompileContextTool},
    extract_session::{ExtractSessionRequest, ExtractSessionTool},
    find_skill::{FindSkillRequest, FindSkillResponse, FindSkillTool},
};
use tracing::{debug, info, warn};

use crate::graph_refresh_subscriber::{GraphReloader, run_graph_refresh_loop};
use crate::state::{CompiledContextCache, SessionSuppressionState};

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
}

impl McpServerApp {
    pub fn new(retriever: Arc<dyn SkillRetriever>, redis_client: Option<RedisClient>) -> Self {
        let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
        Self::new_with_admin(
            retriever,
            admin_runtime_dependencies.rebuild_trigger,
            admin_runtime_dependencies.graph_reader,
            redis_client,
        )
    }

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
        }
    }

    /// Attaches the durable transcript-ingest queue used by the localhost
    /// `/ingest/transcript` endpoint. Builder-style so test/in-memory
    /// constructors stay free of a Postgres dependency.
    pub fn with_transcript_ingest(mut self, queue: TranscriptIngestQueue) -> Self {
        self.transcript_ingest = Some(queue);
        self
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

    pub async fn compile_context(&self, request: CompileContextRequest) -> CompileContextResponse {
        self.compile_context.invoke(request).await
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
#[derive(Clone)]
pub struct LiveServerComponents {
    pub app: McpServerApp,
    pub embedding_service: Arc<OllamaEmbeddingService>,
    pub write_coordinator: Arc<PostgresGraphWriteCoordinator>,
    pub qdrant_adapter: Arc<QdrantAdapter>,
    pub redis_adapter: Arc<RedisStreamsAdapter>,
    pub pg_adapter: Arc<PostgresAdapter>,
    pub rebuild_coordinator: Arc<PostgresRebuildCoordinator>,
}

impl LiveServerComponents {
    /// Destructive teardown for test isolation.
    /// Continues cleanup even if individual steps fail, reporting all errors at the end.
    ///
    /// Gated to `#[cfg(any(test, feature = "test-utils"))]` so production builds have zero destructive
    /// teardown surface.
    #[cfg(any(test, feature = "test-utils"))]
    #[tracing::instrument(skip_all)]
    pub async fn teardown(self) -> Result<(), Box<dyn std::error::Error>> {
        let mut errors: Vec<String> = Vec::new();

        if let Err(e) = self.pg_adapter.truncate_all_tables().await {
            errors.push(format!("pg truncate failed: {e}"));
        }

        match self.qdrant_adapter.list_point_ids().await {
            Ok(listing) => {
                if !listing.point_ids.is_empty() {
                    if let Err(e) = self.qdrant_adapter.delete_points(&listing.point_ids).await {
                        errors.push(format!("qdrant delete failed: {e}"));
                    }
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

    let graph = self::build_graph_from_pg(pg_adapter.pool(), embedding_service.as_ref()).await?;

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
    let app = McpServerApp::new_with_admin(
        retriever.clone(),
        admin_runtime_dependencies.rebuild_trigger,
        admin_runtime_dependencies.graph_reader,
        Some(redis_client),
    )
    // Back the localhost `/ingest/transcript` endpoint with the live PG pool so
    // host capture hooks can push transcript content into the durable queue.
    .with_transcript_ingest(TranscriptIngestQueue::new(pg_adapter.pool().clone()));

    // Online refresh-without-restart (T02): subscribe to `graph.rebuilt`
    // and atomically swap the in-memory read model. Spawned on its own
    // task so it never blocks the HTTP server; gated by a rollback flag.
    spawn_graph_refresh_if_enabled(
        redis_streams.clone(),
        pg_adapter.clone(),
        embedding_service.clone(),
        retriever,
    );

    Ok(LiveServerComponents {
        app,
        embedding_service,
        write_coordinator: Arc::new(write_coordinator),
        qdrant_adapter,
        redis_adapter: redis_streams,
        pg_adapter,
        rebuild_coordinator: Arc::new(rebuild_coordinator),
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
}

#[async_trait]
impl GraphReloader for PostgresGraphReloader {
    async fn reload_and_swap(&self) -> Result<i64, String> {
        let snapshot = build_graph_from_pg(self.pg_adapter.pool(), self.embedding_service.as_ref())
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
    });
    tokio::spawn(run_graph_refresh_loop(redis_streams, reloader));
}

#[tracing::instrument(skip(pool, embedding_service))]
async fn build_graph_from_pg(
    pool: &PostgresPool,
    embedding_service: &dyn EmbeddingService,
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

    let seeded_skills: Vec<retrieval::SeededSkill> = skills
        .into_iter()
        .zip(embeddings.into_iter())
        .map(|(record, embedding)| {
            // Derive scope type, scope_id, and source_paths from the single
            // `skills.scope` column in one match to prevent the two arms drifting
            // apart (previously two separate match blocks could disagree).
            let (scope, scope_id, source_paths) = match record.scope.as_str() {
                "global" => (
                    domain::ScopeType::Global,
                    "global".to_owned(),
                    global_scope_paths.clone(),
                ),
                "team" => (domain::ScopeType::Team, "team".to_owned(), Vec::new()),
                _ => (
                    domain::ScopeType::Project,
                    "project".to_owned(),
                    project_scope_paths.clone(),
                ),
            };
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
                community_id: record
                    .community_id
                    .map(|id| domain::DomainId::new_unchecked(id)),
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
                prior: 0.1,
                community_boost: 0.2,
            }
        })
        .collect();

    Ok(RetrievalSnapshot::new(seeded_skills, graph_version))
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
