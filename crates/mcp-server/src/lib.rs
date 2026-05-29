mod admin_wiring;
mod context_cache;
pub mod protocol;
mod suppression_state;
pub mod state;
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
use compiler::TemplateOnlyCompiler;
use domain::{EmbeddingService, ScopeResolver};
use infrastructure::{
    EnvPathGlobalResolver, GitRootProjectResolver, OllamaEmbeddingService,
    OllamaEmbeddingConfig,
    OutboxVectorStore,
    PostgresAdapter, PostgresConfig,
    PostgresGraphSnapshotStore,
    PostgresGraphWriteCoordinator,
    PostgresRebuildCoordinator,
    QdrantAdapter, QdrantConfig,
    RedisStreamsAdapter, RedisStreamsConfig,
};
use retrieval::{
    DualScopeResolver, RetrievalConfig, RetrievalOrchestrator, SeededGraph, SkillRetriever,
};
use tools::{
    compile_context::{CompileContextRequest, CompileContextResponse, CompileContextTool},
    extract_session::{ExtractSessionRequest, ExtractSessionTool},
    find_skill::{FindSkillRequest, FindSkillResponse, FindSkillTool},
};

use crate::state::{CompiledContextCache, SessionSuppressionState};

#[derive(Clone)]
pub struct McpServerApp {
    compile_context: CompileContextTool,
    extract_session: ExtractSessionTool,
    find_skill: FindSkillTool,
    admin_tools: AdminTools,
    session_state: SessionSuppressionState,
    cache: CompiledContextCache,
}

impl McpServerApp {
    pub fn new(retriever: Arc<dyn SkillRetriever>, redis_client: Option<redis::Client>) -> Self {
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
        redis_client: Option<redis::Client>,
    ) -> Self {
        let state = SessionSuppressionState::new(redis_client.clone(), SessionSuppressionState::DEFAULT_TTL_SECS);
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
        }
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

pub fn build_seeded_server<E>(
    embedding_service: Arc<E>,
    graph: SeededGraph,
    config: RetrievalConfig,
    redis_client: Option<redis::Client>,
) -> McpServerApp
where
    E: EmbeddingService + Send + Sync + 'static,
{
    let graph_for_retrieval = graph.clone();
    let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
    let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let project_resolver: Arc<dyn ScopeResolver> = Arc::new(GitRootProjectResolver::new(start_dir));
    let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
    let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

    let retriever = RetrievalOrchestrator::new_dual_scope(
        embedding_service,
        graph_for_retrieval,
        config,
        scope_resolver,
    );
    McpServerApp::new_with_admin(
        Arc::new(retriever),
        admin_runtime_dependencies.rebuild_trigger,
        admin_runtime_dependencies.graph_reader,
        redis_client,
    )
}

#[derive(Debug)]
pub enum ServerMode {
    Live,
    Deterministic,
}

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
    pub async fn teardown(self) -> Result<(), Box<dyn std::error::Error>> {
        let pool = self.pg_adapter.pool();
        sqlx::query("TRUNCATE TABLE community_skills, skill_subunits, communities, subunits, skills, outbox_events, rebuild_locks CASCADE")
            .execute(pool)
            .await?;

        let listing = self.qdrant_adapter.list_point_ids().await
            .map_err(|e| format!("qdrant list failed: {e}"))?;
        if !listing.point_ids.is_empty() {
            self.qdrant_adapter.delete_points(&listing.point_ids).await
                .map_err(|e| format!("qdrant delete failed: {e}"))?;
        }

        // Redis flush via separate client for teardown
        if let Ok(redis_url) = std::env::var("REDIS_URL") {
            if let Ok(client) = redis::Client::open(redis_url) {
                if let Ok(mut conn) = client.get_multiplexed_async_connection().await {
                    let _: Result<(), _> = redis::cmd("DEL")
                        .arg("skill-layer-events")
                        .query_async(&mut conn)
                        .await;
                }
            }
        }

        Ok(())
    }
}

pub async fn build_live_server(
    mode: ServerMode,
    config: RetrievalConfig,
) -> Result<LiveServerComponents, Box<dyn std::error::Error>> {
    match mode {
        ServerMode::Deterministic => {
            Err("Deterministic mode not supported via build_live_server—use build_seeded_server directly".into())
        }
        ServerMode::Live => {
            let pg_config = PostgresConfig {
                database_url: env_var("DATABASE_URL")?,
                connect_timeout_secs: 5,
                acquire_timeout_secs: 3,
                max_connections: 10,
                min_connections: 1,
            };
            let pg_adapter = PostgresAdapter::connect(&pg_config).await?;
            pg_adapter.run_migrations().await?;

            let qdrant_config = QdrantConfig {
                endpoint: env_var("QDRANT_URL")?,
                timeout_ms: 3_000,
                collection_name: "skills".to_owned(),
            };
            let qdrant_adapter = QdrantAdapter::new(
                reqwest::Client::new(),
                qdrant_config,
            )?;
            qdrant_adapter.check_connectivity().await?;
            qdrant_adapter.ensure_collection("skills", 768).await?;

            let ollama_config = OllamaEmbeddingConfig {
                base_url: env_var("OLLAMA_URL")?,
                model: "nomic-embed-text".to_owned(),
                timeout_ms: 5_000,
                batch_timeout_ms: 10_000,
                max_concurrency: 4,
            };
            let embedding_service = OllamaEmbeddingService::new(
                reqwest::Client::new(),
                ollama_config,
            )
            .map_err(|e| format!("ollama init failed: {e}"))?;

            let redis_config = RedisStreamsConfig {
                redis_url: env_var("REDIS_URL")?,
                stream_key: "skill-layer-events".to_owned(),
                consumer_group: "skill-layer".to_owned(),
                consumer_name: "worker-1".to_owned(),
                idempotency_ttl_secs: 86_400,
                max_stream_len: 100_000,
            };
            let redis_streams = RedisStreamsAdapter::new(redis_config)?;
            redis_streams.ensure_consumer_group().await?;

            let redis_client = redis::Client::open(env_var("REDIS_URL")?)?;

            let write_coordinator = PostgresGraphWriteCoordinator::new(
                pg_adapter.pool().clone(),
            );
            let rebuild_coordinator = PostgresRebuildCoordinator::new(
                pg_adapter.pool().clone(),
            );

            let graph = self::build_graph_from_pg(pg_adapter.pool(), &embedding_service).await?;

            let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
            let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let project_resolver: Arc<dyn ScopeResolver> = Arc::new(GitRootProjectResolver::new(start_dir));
            let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
            let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

            let retriever = RetrievalOrchestrator::new_dual_scope(
                Arc::new(embedding_service.clone()),
                graph,
                config,
                scope_resolver,
            );
            let app = McpServerApp::new_with_admin(
                Arc::new(retriever),
                admin_runtime_dependencies.rebuild_trigger,
                admin_runtime_dependencies.graph_reader,
                Some(redis_client),
            );

            Ok(LiveServerComponents {
                app,
                embedding_service: Arc::new(embedding_service),
                write_coordinator: Arc::new(write_coordinator),
                qdrant_adapter: Arc::new(qdrant_adapter),
                redis_adapter: Arc::new(redis_streams),
                pg_adapter: Arc::new(pg_adapter),
                rebuild_coordinator: Arc::new(rebuild_coordinator),
            })
        }
    }
}

fn env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} must be set"))
}

async fn build_graph_from_pg(
    pool: &sqlx::PgPool,
    embedding_service: &OllamaEmbeddingService,
) -> Result<SeededGraph, Box<dyn std::error::Error>> {
    let store = PostgresGraphSnapshotStore::new(pool.clone());
    let skills = store.list_skills().await
        .map_err(|e| format!("failed to list skills from PG: {e}"))?;

    if skills.is_empty() {
        return Ok(SeededGraph::new(vec![], 0));
    }

    let texts: Vec<String> = skills.iter().map(|s| {
        format!("{} {} {}", s.name, s.description, s.tags.join(" "))
    }).collect();
    let text_refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let embeddings = embedding_service.embed_batch(&text_refs).await?;

    let seeded_skills: Vec<retrieval::SeededSkill> = skills
        .into_iter()
        .zip(embeddings.into_iter())
        .map(|(record, embedding)| {
            let scope = match record.community_id.as_deref() {
                Some(_) => domain::ScopeType::Global,
                None => domain::ScopeType::Project,
            };
            let skill = domain::Skill {
                id: domain::DomainId::new_unchecked(&record.skill_id),
                name: record.name,
                description: record.description,
                scope,
                status: domain::SkillStatus::Ready,
                lifecycle: domain::LifecycleStatus::Active,
                tags: record.tags,
                subunit_ids: record.subunits.iter()
                    .map(|s| domain::DomainId::new_unchecked(&s.title))
                    .collect(),
                community_id: record.community_id.map(|id| domain::DomainId::new_unchecked(id)),
            };
            let subunits: Vec<domain::Subunit> = record.subunits.into_iter().map(|s| {
                domain::Subunit {
                    id: domain::DomainId::new_unchecked(&s.title),
                    skill_id: skill.id.clone(),
                    kind: domain::SubunitType::Procedure,
                    title: s.title,
                    content: s.content,
                    lifecycle: domain::LifecycleStatus::Active,
                }
            }).collect();

            retrieval::SeededSkill {
                skill,
                scope_id: "global".to_owned(),
                source_paths: vec![],
                embedding,
                subunits,
                prior: 0.1,
                community_boost: 0.2,
            }
        })
        .collect();

    Ok(SeededGraph::new(seeded_skills, 1))
}