pub mod protocol;
pub mod state;
pub mod tools {
    pub mod compile_context;
    pub mod find_skill;
}

use std::sync::Arc;

use compiler::TemplateOnlyCompiler;
use domain::{EmbeddingService, ScopeResolver};
use infrastructure::{EnvPathGlobalResolver, GitRootProjectResolver};
use retrieval::{
    DualScopeResolver, RetrievalConfig, RetrievalOrchestrator, SeededGraph, SkillRetriever,
};
use tools::{
    compile_context::{CompileContextRequest, CompileContextResponse, CompileContextTool},
    find_skill::{FindSkillRequest, FindSkillResponse, FindSkillTool},
};

use crate::state::SessionSuppressionState;

#[derive(Clone)]
pub struct McpServerApp {
    compile_context: CompileContextTool,
    find_skill: FindSkillTool,
    registered_tools: Vec<String>,
}

impl McpServerApp {
    pub fn new(retriever: Arc<dyn SkillRetriever>) -> Self {
        let state = SessionSuppressionState::default();
        let compiler = TemplateOnlyCompiler::default();

        Self {
            compile_context: CompileContextTool::new(retriever.clone(), compiler, state),
            find_skill: FindSkillTool::new(retriever),
            registered_tools: vec!["compile_context".to_owned(), "find_skill".to_owned()],
        }
    }

    pub fn registered_tools(&self) -> &[String] {
        &self.registered_tools
    }

    pub async fn compile_context(&self, request: CompileContextRequest) -> CompileContextResponse {
        self.compile_context.invoke(request).await
    }

    pub async fn find_skill(&self, request: FindSkillRequest) -> FindSkillResponse {
        self.find_skill.invoke(request).await
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
    let start_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let project_resolver: Arc<dyn ScopeResolver> = Arc::new(GitRootProjectResolver::new(start_dir));
    let global_resolver: Arc<dyn ScopeResolver> = Arc::new(EnvPathGlobalResolver::default());
    let scope_resolver = DualScopeResolver::new(project_resolver, global_resolver);

    let retriever =
        RetrievalOrchestrator::new_dual_scope(embedding_service, graph, config, scope_resolver);
    McpServerApp::new(Arc::new(retriever))
}
