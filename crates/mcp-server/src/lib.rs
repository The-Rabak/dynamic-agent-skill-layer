mod admin_wiring;
pub mod protocol;
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
use infrastructure::{EnvPathGlobalResolver, GitRootProjectResolver};
use retrieval::{
    DualScopeResolver, RetrievalConfig, RetrievalOrchestrator, SeededGraph, SkillRetriever,
};
use tools::{
    compile_context::{CompileContextRequest, CompileContextResponse, CompileContextTool},
    extract_session::{ExtractSessionRequest, ExtractSessionTool},
    find_skill::{FindSkillRequest, FindSkillResponse, FindSkillTool},
};

use crate::state::SessionSuppressionState;

#[derive(Clone)]
pub struct McpServerApp {
    compile_context: CompileContextTool,
    extract_session: ExtractSessionTool,
    find_skill: FindSkillTool,
    admin_tools: AdminTools,
}

impl McpServerApp {
    pub fn new(retriever: Arc<dyn SkillRetriever>) -> Self {
        let admin_runtime_dependencies = admin_wiring::live_admin_runtime_dependencies();
        Self::new_with_admin(
            retriever,
            admin_runtime_dependencies.rebuild_trigger,
            admin_runtime_dependencies.graph_reader,
        )
    }

    pub fn new_with_admin(
        retriever: Arc<dyn SkillRetriever>,
        rebuild_trigger: Arc<dyn GraphRebuildTrigger>,
        graph_reader: Arc<dyn GraphSnapshotReader>,
    ) -> Self {
        let state = SessionSuppressionState::default();
        let compiler = TemplateOnlyCompiler::default();
        let admin_tools = AdminTools::new(rebuild_trigger, graph_reader);

        Self {
            compile_context: CompileContextTool::new(retriever.clone(), compiler, state),
            extract_session: ExtractSessionTool::from_environment(),
            find_skill: FindSkillTool::new(retriever),
            admin_tools,
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
        self.extract_session.invoke(request).await
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
    )
}
