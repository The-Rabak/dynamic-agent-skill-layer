use std::{collections::BTreeMap, sync::Arc};

use compiler::{
    CompilerHighlightInput, CompilerRescueCueInput, CompilerSkillInput, TemplateOnlyCompiler,
};
use retrieval::SkillRetriever;
use serde::{Deserialize, Serialize};

use crate::state::SessionSuppressionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileContextStatus {
    Ok,
    NoMatch,
    Degraded,
    DuplicateSuppressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileContextRequest {
    pub prompt: String,
    pub session_id: String,
    pub repo_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompileContextResponse {
    pub status: CompileContextStatus,
    pub reason_code: Option<String>,
    pub additional_context: Option<String>,
    pub health: BTreeMap<String, String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
    pub latency_ms: u128,
}

#[derive(Clone)]
pub struct CompileContextTool {
    retriever: Arc<dyn SkillRetriever>,
    compiler: TemplateOnlyCompiler,
    state: SessionSuppressionState,
}

impl CompileContextTool {
    pub fn new(
        retriever: Arc<dyn SkillRetriever>,
        compiler: TemplateOnlyCompiler,
        state: SessionSuppressionState,
    ) -> Self {
        Self {
            retriever,
            compiler,
            state,
        }
    }

    pub async fn invoke(&self, request: CompileContextRequest) -> CompileContextResponse {
        let current_graph_version = self.retriever.current_graph_version();
        let configured_scopes = self.retriever.configured_scopes();

        if self.state.is_suppressed(
            &request.session_id,
            &request.repo_path,
            current_graph_version,
        ) {
            let graph_version = self
                .state
                .graph_version(&request.session_id, &request.repo_path)
                .unwrap_or(current_graph_version);
            let scopes_considered = self
                .state
                .scopes_considered(&request.session_id, &request.repo_path)
                .unwrap_or(configured_scopes);
            return CompileContextResponse {
                status: CompileContextStatus::DuplicateSuppressed,
                reason_code: Some("already_compiled_for_session".to_owned()),
                additional_context: None,
                health: BTreeMap::new(),
                scopes_considered,
                graph_version,
                latency_ms: 0,
            };
        }

        let outcome = self
            .retriever
            .retrieve(&request.prompt, Some(request.repo_path.as_str()))
            .await;

        if outcome.skills.is_empty() && outcome.is_degraded() {
            return CompileContextResponse {
                status: CompileContextStatus::Degraded,
                reason_code: outcome
                    .reason_codes
                    .first()
                    .cloned()
                    .or_else(|| Some("retrieval_degraded".to_owned())),
                additional_context: None,
                health: outcome.health,
                scopes_considered: outcome.scopes_considered,
                graph_version: outcome.graph_version,
                latency_ms: outcome.latency_ms,
            };
        }

        if outcome.skills.is_empty() {
            self.state.mark_healthy(
                &request.session_id,
                &request.repo_path,
                outcome.graph_version,
                &outcome.scopes_considered,
            );

            return CompileContextResponse {
                status: CompileContextStatus::NoMatch,
                reason_code: Some("no_relevant_skills".to_owned()),
                additional_context: None,
                health: outcome.health,
                scopes_considered: outcome.scopes_considered,
                graph_version: outcome.graph_version,
                latency_ms: outcome.latency_ms,
            };
        }

        let compiled_skills: Vec<CompilerSkillInput> = outcome
            .skills
            .iter()
            .map(|retrieved| CompilerSkillInput {
                name: retrieved.scored_skill.skill.name.clone(),
                description: retrieved.scored_skill.skill.description.clone(),
                score: retrieved.scored_skill.score,
                highlights: retrieved
                    .highlights
                    .iter()
                    .map(|highlight| CompilerHighlightInput {
                        kind: highlight.kind,
                        title: highlight.title.clone(),
                        content: highlight.content.clone(),
                        relevance: highlight.relevance,
                    })
                    .collect(),
            })
            .collect();
        let rescue_pool: Vec<CompilerRescueCueInput> = outcome
            .rescue_pool
            .iter()
            .map(|cue| CompilerRescueCueInput {
                source_skill: cue.source_skill.clone(),
                title: cue.title.clone(),
                content: cue.content.clone(),
                relevance: cue.relevance,
            })
            .collect();

        let markdown =
            self.compiler
                .compile_with_rescue(&request.prompt, &compiled_skills, &rescue_pool);

        if outcome.is_degraded() {
            return CompileContextResponse {
                status: CompileContextStatus::Degraded,
                reason_code: outcome
                    .reason_codes
                    .first()
                    .cloned()
                    .or_else(|| Some("retrieval_degraded".to_owned())),
                additional_context: Some(markdown),
                health: outcome.health,
                scopes_considered: outcome.scopes_considered,
                graph_version: outcome.graph_version,
                latency_ms: outcome.latency_ms,
            };
        }

        self.state.mark_healthy(
            &request.session_id,
            &request.repo_path,
            outcome.graph_version,
            &outcome.scopes_considered,
        );

        CompileContextResponse {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some(markdown),
            health: outcome.health,
            scopes_considered: outcome.scopes_considered,
            graph_version: outcome.graph_version,
            latency_ms: outcome.latency_ms,
        }
    }
}
