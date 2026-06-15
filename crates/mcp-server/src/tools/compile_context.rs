use std::{collections::BTreeMap, sync::Arc, time::Instant};

use compiler::{
    CompilerHighlightInput, CompilerRescueCueInput, CompilerSkillInput, TemplateOnlyCompiler,
};
use retrieval::{RetrievalIntent, RetrievalOutcome, SkillRetriever};
use serde::{Deserialize, Serialize};

use crate::state::{CachedContext, CompiledContextCache, SessionSuppressionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileContextStatus {
    Ok,
    NoMatch,
    Degraded,
    DuplicateSuppressed,
    /// Snapshot build or background reload is in flight.
    ///
    /// Returned by the `McpServerApp::compile_context` coordination layer (T17 AC1)
    /// when the readiness handle is in `Warming` or `Failed` state. The tool-level
    /// `CompileContextTool::invoke` is never called in this state — the guard runs
    /// before any query embed so the embedding semaphore is not acquired.
    Warming,
}

/// The lifecycle event that caused a `compile_context` call.
///
/// Bounded to a fixed set of meaningful variants so callers cannot pass
/// arbitrary strings. Unknown string values from JSON deserialize to `Other`,
/// which preserves the "unknown = no bypass" behavior safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    /// Post-compaction re-inject: bypasses session suppression for this single call
    /// so the agent receives fresh context after Claude Code summarizes the conversation.
    Compact,
    /// SessionStart priming — routes retrieval to `RetrievalIntent::Priming` so the
    /// orchestrator can apply intent-appropriate candidate selection in later T12 units.
    /// Session suppression semantics are unchanged by this trigger.
    SessionStart,
    /// Any unrecognized trigger value. Treated as an ordinary call — no bypass.
    #[serde(other)]
    Other,
}

/// Request to compile skill context for the current session.
///
/// The optional `trigger` hint identifies the lifecycle event that caused this
/// call. When `trigger` is `TriggerKind::Compact` (a post-compaction re-inject),
/// session suppression is bypassed for this single call so the agent receives
/// fresh context after summarization. All other trigger values (or `None`) leave
/// suppression semantics unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileContextRequest {
    pub prompt: String,
    pub session_id: String,
    pub repo_path: String,
    /// Lifecycle event that triggered this call.
    /// `TriggerKind::Compact` bypasses session suppression for a single re-inject.
    /// Unknown string values deserialize to `TriggerKind::Other` (no bypass).
    #[serde(default)]
    pub trigger: Option<TriggerKind>,
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
    pub source: String,
}

#[derive(Clone)]
pub struct CompileContextTool {
    retriever: Arc<dyn SkillRetriever>,
    compiler: TemplateOnlyCompiler,
    state: SessionSuppressionState,
    cache: CompiledContextCache,
}

impl CompileContextTool {
    pub fn new(
        retriever: Arc<dyn SkillRetriever>,
        compiler: TemplateOnlyCompiler,
        state: SessionSuppressionState,
        cache: CompiledContextCache,
    ) -> Self {
        Self {
            retriever,
            compiler,
            state,
            cache,
        }
    }

    pub async fn invoke(&self, request: CompileContextRequest) -> CompileContextResponse {
        self.invoke_and_capture_outcome(request).await.0
    }

    /// Invokes `compile_context` and returns both the response and the retrieval
    /// outcome (if a live retrieval was performed).
    ///
    /// The second element is `Some` only when a real retrieval ran (not when the
    /// response was served from cache or suppression). Callers at the coordination
    /// layer (`McpServerApp::compile_context`) use this to extract selected skill
    /// IDs and scores for the async usage write — without adding persistence into
    /// this pure query-compile unit.
    pub async fn invoke_and_capture_outcome(
        &self,
        request: CompileContextRequest,
    ) -> (CompileContextResponse, Option<RetrievalOutcome>) {
        let started_at = Instant::now();
        let graph_version = self.retriever.current_graph_version();
        let scopes = self.retriever.configured_scopes();

        // A `compact`-triggered call re-injects context after Claude Code summarizes
        // the conversation. Suppression would block it with `DuplicateSuppressed`,
        // making the re-inject hook a silent no-op. Bypass suppression for this
        // single call only — production suppression semantics remain unchanged (T08).
        let compact_bypass = matches!(request.trigger, Some(TriggerKind::Compact));

        if !compact_bypass
            && let Some(response) = self
                .try_suppression_or_cache(&request, &scopes, graph_version, started_at)
                .await
        {
            return (response, None);
        }

        // Derive retrieval intent from the trigger. SessionStart priming is labelled
        // `Priming` so the orchestrator can apply intent-appropriate behavior in later
        // T12 units. All other triggers (Compact, Other, None) remain Task — their
        // suppression-bypass and other semantics are independent of retrieval intent.
        let intent = match request.trigger {
            Some(TriggerKind::SessionStart) => RetrievalIntent::Priming,
            _ => RetrievalIntent::Task,
        };

        let outcome = self
            .retriever
            .retrieve(&request.prompt, Some(request.repo_path.as_str()), intent)
            .await;

        let response = self
            .handle_retrieval_result(&request, &outcome, &scopes)
            .await;
        (response, Some(outcome))
    }

    async fn try_suppression_or_cache(
        &self,
        request: &CompileContextRequest,
        scopes: &[String],
        graph_version: i64,
        started_at: Instant,
    ) -> Option<CompileContextResponse> {
        if self
            .state
            .is_suppressed(&request.session_id, &request.repo_path, graph_version)
            .await
        {
            let gv = self
                .state
                .graph_version(&request.session_id, &request.repo_path)
                .await
                .unwrap_or(graph_version);
            let sc = self
                .state
                .scopes_considered(&request.session_id, &request.repo_path)
                .await
                .unwrap_or_else(|| scopes.to_vec());
            return Some(CompileContextResponse {
                status: CompileContextStatus::DuplicateSuppressed,
                reason_code: Some("already_compiled_for_session".to_owned()),
                additional_context: None,
                health: BTreeMap::new(),
                scopes_considered: sc,
                graph_version: gv,
                latency_ms: started_at.elapsed().as_millis(),
                source: "suppression".to_owned(),
            });
        }

        if let Some(cached) = self
            .cache
            .get(&request.session_id, &request.prompt, scopes, graph_version)
            .await
        {
            return Some(CompileContextResponse {
                status: cached.status,
                reason_code: cached.reason_code,
                additional_context: cached.additional_context,
                health: cached.health,
                scopes_considered: cached.scopes_considered,
                graph_version: cached.graph_version,
                latency_ms: started_at.elapsed().as_millis(),
                source: "cache".to_owned(),
            });
        }

        None
    }

    async fn handle_retrieval_result(
        &self,
        request: &CompileContextRequest,
        outcome: &RetrievalOutcome,
        scopes: &[String],
    ) -> CompileContextResponse {
        if outcome.skills.is_empty() {
            if outcome.is_degraded() {
                return CompileContextResponse {
                    status: CompileContextStatus::Degraded,
                    reason_code: outcome
                        .reason_codes
                        .first()
                        .cloned()
                        .or_else(|| Some("retrieval_degraded".to_owned())),
                    additional_context: None,
                    health: outcome.health.clone(),
                    scopes_considered: outcome.scopes_considered.clone(),
                    graph_version: outcome.graph_version,
                    latency_ms: outcome.latency_ms,
                    source: "retrieval".to_owned(),
                };
            }

            self.state
                .mark_healthy(
                    &request.session_id,
                    &request.repo_path,
                    outcome.graph_version,
                    &outcome.scopes_considered,
                )
                .await;

            self.cache
                .set(
                    &request.session_id,
                    &request.prompt,
                    scopes,
                    CachedContext {
                        status: CompileContextStatus::NoMatch,
                        reason_code: Some("no_relevant_skills".to_owned()),
                        additional_context: None,
                        scopes_considered: outcome.scopes_considered.clone(),
                        graph_version: outcome.graph_version,
                        health: outcome.health.clone(),
                    },
                )
                .await;

            return CompileContextResponse {
                status: CompileContextStatus::NoMatch,
                reason_code: Some("no_relevant_skills".to_owned()),
                additional_context: None,
                health: outcome.health.clone(),
                scopes_considered: outcome.scopes_considered.clone(),
                graph_version: outcome.graph_version,
                latency_ms: outcome.latency_ms,
                source: "retrieval".to_owned(),
            };
        }

        let markdown = self.compile_markdown(&request.prompt, outcome);

        if outcome.is_degraded() {
            return CompileContextResponse {
                status: CompileContextStatus::Degraded,
                reason_code: outcome
                    .reason_codes
                    .first()
                    .cloned()
                    .or_else(|| Some("retrieval_degraded".to_owned())),
                additional_context: Some(markdown),
                health: outcome.health.clone(),
                scopes_considered: outcome.scopes_considered.clone(),
                graph_version: outcome.graph_version,
                latency_ms: outcome.latency_ms,
                source: "retrieval".to_owned(),
            };
        }

        self.cache_and_suppress_ok(request, outcome, &markdown, scopes)
            .await;

        CompileContextResponse {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some(markdown),
            health: outcome.health.clone(),
            scopes_considered: outcome.scopes_considered.clone(),
            graph_version: outcome.graph_version,
            latency_ms: outcome.latency_ms,
            source: "retrieval".to_owned(),
        }
    }

    async fn cache_and_suppress_ok(
        &self,
        request: &CompileContextRequest,
        outcome: &RetrievalOutcome,
        markdown: &str,
        scopes: &[String],
    ) {
        self.state
            .mark_healthy(
                &request.session_id,
                &request.repo_path,
                outcome.graph_version,
                &outcome.scopes_considered,
            )
            .await;

        self.cache
            .set(
                &request.session_id,
                &request.prompt,
                scopes,
                CachedContext {
                    status: CompileContextStatus::Ok,
                    reason_code: None,
                    additional_context: Some(markdown.to_owned()),
                    scopes_considered: outcome.scopes_considered.clone(),
                    graph_version: outcome.graph_version,
                    health: outcome.health.clone(),
                },
            )
            .await;
    }

    fn compile_markdown(&self, prompt: &str, outcome: &RetrievalOutcome) -> String {
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
                // Thread scope + rationale through for the deterministic
                // "Why These Skills" section in render_markdown.
                matched_scope: retrieved.scored_skill.matched_scope.as_str().to_owned(),
                rationale: retrieved.scored_skill.rationale.clone(),
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

        self.compiler
            .compile_with_rescue(prompt, &compiled_skills, &rescue_pool)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use retrieval::{RetrievalIntent, RetrievalOutcome, SkillRetriever};

    use super::TriggerKind;

    // ── T12 Unit 1: TriggerKind::SessionStart serde round-trip ─────────────

    /// Proves `"session_start"` deserializes to `TriggerKind::SessionStart`.
    #[test]
    fn trigger_kind_session_start_deserializes() {
        let parsed: TriggerKind = serde_json::from_str("\"session_start\"").unwrap();
        assert_eq!(
            parsed,
            TriggerKind::SessionStart,
            "'session_start' must deserialize to TriggerKind::SessionStart"
        );
    }

    /// Proves `"compact"` still deserializes to `TriggerKind::Compact` (regression guard).
    #[test]
    fn trigger_kind_compact_still_deserializes() {
        let parsed: TriggerKind = serde_json::from_str("\"compact\"").unwrap();
        assert_eq!(
            parsed,
            TriggerKind::Compact,
            "'compact' must still deserialize to TriggerKind::Compact"
        );
    }

    /// Proves an unknown trigger string deserializes to `TriggerKind::Other` (catchall guard).
    #[test]
    fn trigger_kind_unknown_deserializes_to_other() {
        let parsed: TriggerKind = serde_json::from_str("\"frobnicate\"").unwrap();
        assert_eq!(
            parsed,
            TriggerKind::Other,
            "unknown trigger string must deserialize to TriggerKind::Other"
        );
    }

    // ── T12 Unit 1: intent-routing through invoke_and_capture_outcome ───────

    /// Intent-capturing test retriever: records the `RetrievalIntent` it was called with.
    struct IntentCapturingRetriever {
        captured_intent: Arc<Mutex<Option<RetrievalIntent>>>,
    }

    impl IntentCapturingRetriever {
        fn new() -> (Arc<Mutex<Option<RetrievalIntent>>>, Arc<Self>) {
            let captured = Arc::new(Mutex::new(None));
            let retriever = Arc::new(Self {
                captured_intent: captured.clone(),
            });
            (captured, retriever)
        }
    }

    #[async_trait]
    impl SkillRetriever for IntentCapturingRetriever {
        async fn retrieve(
            &self,
            _prompt: &str,
            _repo_path: Option<&str>,
            intent: RetrievalIntent,
        ) -> RetrievalOutcome {
            *self.captured_intent.lock().unwrap() = Some(intent);
            RetrievalOutcome {
                skills: Vec::new(),
                rescue_pool: Vec::new(),
                degraded_scopes: Vec::new(),
                reason_codes: Vec::new(),
                health: Default::default(),
                scopes_considered: vec!["global".to_owned()],
                graph_version: 1,
                latency_ms: 0,
            }
        }

        fn current_graph_version(&self) -> i64 {
            1
        }

        fn configured_scopes(&self) -> Vec<String> {
            vec!["global".to_owned()]
        }
    }

    fn make_tool(retriever: Arc<dyn SkillRetriever>) -> super::CompileContextTool {
        use crate::state::{CompiledContextCache, SessionSuppressionState};
        use compiler::TemplateOnlyCompiler;

        super::CompileContextTool::new(
            retriever,
            TemplateOnlyCompiler::default(),
            SessionSuppressionState::default(),
            CompiledContextCache::default(),
        )
    }

    /// T12 Unit 1: `trigger: Some(TriggerKind::SessionStart)` routes to `Priming` intent.
    #[tokio::test]
    async fn session_start_trigger_routes_to_priming_intent() {
        let (captured, retriever) = IntentCapturingRetriever::new();
        let tool = make_tool(retriever as Arc<dyn SkillRetriever>);

        tool.invoke(super::CompileContextRequest {
            prompt: "start session".to_owned(),
            session_id: "s1".to_owned(),
            repo_path: "/repo".to_owned(),
            trigger: Some(TriggerKind::SessionStart),
        })
        .await;

        assert_eq!(
            *captured.lock().unwrap(),
            Some(RetrievalIntent::Priming),
            "SessionStart trigger must route to RetrievalIntent::Priming"
        );
    }

    /// T12 Unit 1: `trigger: None` routes to `Task` intent.
    #[tokio::test]
    async fn no_trigger_routes_to_task_intent() {
        let (captured, retriever) = IntentCapturingRetriever::new();
        let tool = make_tool(retriever as Arc<dyn SkillRetriever>);

        tool.invoke(super::CompileContextRequest {
            prompt: "task prompt".to_owned(),
            session_id: "s2".to_owned(),
            repo_path: "/repo".to_owned(),
            trigger: None,
        })
        .await;

        assert_eq!(
            *captured.lock().unwrap(),
            Some(RetrievalIntent::Task),
            "trigger: None must route to RetrievalIntent::Task"
        );
    }

    /// T12 Unit 1: `trigger: Some(TriggerKind::Compact)` routes to `Task` intent.
    #[tokio::test]
    async fn compact_trigger_routes_to_task_intent() {
        let (captured, retriever) = IntentCapturingRetriever::new();
        let tool = make_tool(retriever as Arc<dyn SkillRetriever>);

        tool.invoke(super::CompileContextRequest {
            prompt: "compact prompt".to_owned(),
            session_id: "s3".to_owned(),
            repo_path: "/repo".to_owned(),
            trigger: Some(TriggerKind::Compact),
        })
        .await;

        assert_eq!(
            *captured.lock().unwrap(),
            Some(RetrievalIntent::Task),
            "Compact trigger must route to RetrievalIntent::Task (Compact = suppression-bypass, not Priming)"
        );
    }

    /// T12 Unit 1: `trigger: Some(TriggerKind::Other)` routes to `Task` intent.
    #[tokio::test]
    async fn other_trigger_routes_to_task_intent() {
        let (captured, retriever) = IntentCapturingRetriever::new();
        let tool = make_tool(retriever as Arc<dyn SkillRetriever>);

        tool.invoke(super::CompileContextRequest {
            prompt: "other trigger".to_owned(),
            session_id: "s4".to_owned(),
            repo_path: "/repo".to_owned(),
            trigger: Some(TriggerKind::Other),
        })
        .await;

        assert_eq!(
            *captured.lock().unwrap(),
            Some(RetrievalIntent::Task),
            "Other trigger must route to RetrievalIntent::Task (unknown triggers are ordinary calls)"
        );
    }
}
