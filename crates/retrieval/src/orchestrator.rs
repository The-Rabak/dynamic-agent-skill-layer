use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use domain::{
    EmbeddingError, EmbeddingService, ScopeDescriptor, ScopeType, ScoredSkill, Skill, Subunit,
    SubunitType,
};

use crate::{
    dual_scope::search_scopes_concurrently,
    fusion::{ScopeRanking, weighted_reciprocal_rank_fusion},
    scope_resolution::DualScopeResolver,
    scoring::ScoringWeights,
};

#[derive(Debug, Clone)]
pub struct SeededSkill {
    pub skill: Skill,
    pub scope_id: String,
    pub source_paths: Vec<PathBuf>,
    pub embedding: Vec<f32>,
    pub subunits: Vec<Subunit>,
    pub prior: f32,
    pub community_boost: f32,
}

/// Immutable in-memory snapshot of the skill graph the read path retrieves from.
///
/// `graph_version` is the durable `graph_state.graph_version` the snapshot was
/// built from; it keys the version-aware compiled-context cache so a rebuild
/// invalidates stale entries. An empty `skills` vector is a valid cold-start
/// snapshot and yields `no_match` rather than an error.
///
/// T02 wraps this type as `GraphSnapshot { graph, version }` under `ArcSwap` to
/// support refresh-without-restart; keep it free of swap/refresh concerns here.
#[derive(Debug, Clone)]
pub struct RetrievalSnapshot {
    pub graph_version: i64,
    pub skills: Vec<SeededSkill>,
}

impl RetrievalSnapshot {
    pub fn new(skills: Vec<SeededSkill>, graph_version: i64) -> Self {
        Self {
            graph_version,
            skills,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedSubunit {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedSkill {
    pub scored_skill: ScoredSkill,
    pub highlights: Vec<RetrievedSubunit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RescueCue {
    pub source_skill: String,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalOutcome {
    pub skills: Vec<RetrievedSkill>,
    pub rescue_pool: Vec<RescueCue>,
    pub degraded_scopes: Vec<String>,
    pub reason_codes: Vec<String>,
    pub health: BTreeMap<String, String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
    pub latency_ms: u128,
}

impl RetrievalOutcome {
    pub fn is_degraded(&self) -> bool {
        !self.degraded_scopes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub candidate_limit: usize,
    pub max_results: usize,
    pub max_subunits_per_skill: usize,
    pub rescue_threshold: f32,
    pub relevance_threshold: f32,
    pub mmr_lambda: f32,
    pub scoring_weights: ScoringWeights,
    pub project_scope_weight: f32,
    pub global_scope_weight: f32,
    pub rrf_k: f32,
    pub scope_timeout_ms: u64,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            candidate_limit: 50,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: 0.20,
            mmr_lambda: 0.65,
            scoring_weights: ScoringWeights::default(),
            project_scope_weight: 1.0,
            global_scope_weight: 0.7,
            rrf_k: 60.0,
            scope_timeout_ms: 400,
        }
    }
}

#[async_trait]
pub trait SkillRetriever: Send + Sync {
    async fn retrieve(&self, prompt: &str, repo_path: Option<&str>) -> RetrievalOutcome;
    fn current_graph_version(&self) -> i64;
    fn configured_scopes(&self) -> Vec<String>;
}

pub struct RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    embedding_service: Arc<E>,
    graph: Arc<RetrievalSnapshot>,
    config: RetrievalConfig,
    scope_resolver: Option<DualScopeResolver>,
}

impl<E> RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    pub fn new(embedding_service: Arc<E>, graph: RetrievalSnapshot, config: RetrievalConfig) -> Self {
        Self {
            embedding_service,
            graph: Arc::new(graph),
            config,
            scope_resolver: None,
        }
    }

    pub fn new_dual_scope(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
        scope_resolver: DualScopeResolver,
    ) -> Self {
        Self {
            embedding_service,
            graph: Arc::new(graph),
            config,
            scope_resolver: Some(scope_resolver),
        }
    }

    fn healthy_markers() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ollama".to_owned(), "ok".to_owned()),
            ("qdrant".to_owned(), "ok".to_owned()),
            ("postgres".to_owned(), "ok".to_owned()),
            ("redis".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
        ])
    }

    fn degraded_marker(reason: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ollama".to_owned(), "degraded".to_owned()),
            ("qdrant".to_owned(), "ok".to_owned()),
            ("postgres".to_owned(), "ok".to_owned()),
            ("redis".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
            ("reason".to_owned(), reason.to_owned()),
        ])
    }

    fn map_embedding_error_to_reason(error: &EmbeddingError) -> String {
        match error {
            EmbeddingError::ProviderUnavailable(_) => "embedding_provider_unavailable".to_owned(),
            EmbeddingError::Timeout { .. } => "embedding_timeout".to_owned(),
            EmbeddingError::InvalidInput(_) => "embedding_invalid_input".to_owned(),
            EmbeddingError::Unexpected(_) => "embedding_unexpected".to_owned(),
        }
    }

    async fn resolve_scopes(
        &self,
        repo_path: Option<&str>,
    ) -> (Vec<ScopeDescriptor>, Vec<String>, Vec<String>, Vec<String>) {
        if let Some(scope_resolver) = &self.scope_resolver {
            let outcome = scope_resolver.resolve(repo_path).await;
            (
                outcome.resolved_scopes(),
                outcome.scopes_considered(),
                outcome.degraded_scopes,
                outcome.reason_codes,
            )
        } else {
            (
                vec![ScopeDescriptor {
                    scope_id: self.config.scope_id.clone(),
                    scope_type: self.config.scope_type,
                    paths: Vec::new(),
                    config: BTreeMap::from([("resolver".to_owned(), "static".to_owned())]),
                }],
                vec![self.config.scope_id.clone()],
                Vec::new(),
                Vec::new(),
            )
        }
    }

    fn scope_weight(&self, scope_type: ScopeType) -> f32 {
        match scope_type {
            ScopeType::Project => self.config.project_scope_weight,
            ScopeType::Global => self.config.global_scope_weight,
            ScopeType::Team => self.config.global_scope_weight,
        }
    }

    fn dedupe(values: &mut Vec<String>) {
        let mut seen = HashSet::new();
        values.retain(|value| seen.insert(value.clone()));
    }

    fn build_degraded_outcome(
        &self,
        started: Instant,
        mut degraded_scopes: Vec<String>,
        mut reason_codes: Vec<String>,
        scopes_considered: Vec<String>,
    ) -> RetrievalOutcome {
        Self::dedupe(&mut degraded_scopes);
        Self::dedupe(&mut reason_codes);

        if degraded_scopes.is_empty() {
            degraded_scopes = scopes_considered.clone();
        }

        let reason = reason_codes
            .first()
            .cloned()
            .unwrap_or_else(|| "retrieval_degraded".to_owned());

        RetrievalOutcome {
            skills: Vec::new(),
            rescue_pool: Vec::new(),
            degraded_scopes,
            reason_codes,
            health: Self::degraded_marker(&reason),
            scopes_considered,
            graph_version: self.graph.graph_version,
            latency_ms: started.elapsed().as_millis(),
        }
    }
}

#[async_trait]
impl<E> SkillRetriever for RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    async fn retrieve(&self, prompt: &str, repo_path: Option<&str>) -> RetrievalOutcome {
        let started = Instant::now();
        let (scopes, scopes_considered, mut degraded_scopes, mut reason_codes) =
            self.resolve_scopes(repo_path).await;

        if scopes.is_empty() {
            return self.build_degraded_outcome(
                started,
                degraded_scopes,
                reason_codes,
                scopes_considered,
            );
        }

        let prompt_embedding = match self.embedding_service.embed_text(prompt).await {
            Ok(embedding) => embedding,
            Err(error) => {
                reason_codes.push(Self::map_embedding_error_to_reason(&error));
                return self.build_degraded_outcome(
                    started,
                    scopes_considered.clone(),
                    reason_codes,
                    scopes_considered,
                );
            }
        };

        let (scope_results, scope_failures) = search_scopes_concurrently(
            prompt,
            &prompt_embedding,
            self.graph.clone(),
            &self.config,
            &scopes,
        )
        .await;

        for failure in scope_failures {
            degraded_scopes.push(failure.scope_id);
            reason_codes.push(failure.reason_code);
        }

        if scope_results.is_empty() {
            return self.build_degraded_outcome(
                started,
                degraded_scopes,
                reason_codes,
                scopes_considered,
            );
        }

        let scope_rankings: Vec<ScopeRanking> = scope_results
            .into_iter()
            .map(|result| ScopeRanking {
                scope_id: result.scope_id,
                weight: self.scope_weight(result.scope_type),
                candidates: result.candidates,
            })
            .collect();

        let fusion_limit = scope_rankings
            .iter()
            .map(|ranking| ranking.candidates.len())
            .sum::<usize>()
            .max(self.config.max_results);

        let ranked_candidates =
            weighted_reciprocal_rank_fusion(&scope_rankings, self.config.rrf_k, fusion_limit);

        let selected_candidates: Vec<_> = ranked_candidates
            .iter()
            .take(self.config.max_results)
            .cloned()
            .collect();
        let selected_ids: HashSet<String> = selected_candidates
            .iter()
            .map(|candidate| candidate.skill_id.clone())
            .collect();

        let selected_skills: Vec<RetrievedSkill> = selected_candidates
            .into_iter()
            .filter_map(|candidate| {
                let seeded_skill = self.graph.skills.get(candidate.skill_index)?;

                let highlights = candidate
                    .highlights
                    .into_iter()
                    .filter(|projection| projection.relevance > 0.0)
                    .map(|projection| RetrievedSubunit {
                        kind: projection.subunit.kind,
                        title: projection.subunit.title,
                        content: projection.subunit.content,
                        relevance: projection.relevance,
                    })
                    .collect();

                Some(RetrievedSkill {
                    scored_skill: ScoredSkill {
                        skill: seeded_skill.skill.clone(),
                        score: candidate.score,
                        matched_scope: candidate.matched_scope,
                        rationale: vec![
                            format!("rrf={:.6}", candidate.score),
                            format!("semantic={:.3}", candidate.semantic_score),
                            format!("lexical={:.3}", candidate.lexical_score),
                        ],
                    },
                    highlights,
                })
            })
            .collect();

        let rescue_pool: Vec<RescueCue> = ranked_candidates
            .iter()
            .filter(|candidate| !selected_ids.contains(&candidate.skill_id))
            .flat_map(|candidate| {
                let skill_name = self
                    .graph
                    .skills
                    .get(candidate.skill_index)
                    .map(|seeded| seeded.skill.name.clone())
                    .unwrap_or_else(|| "unknown-skill".to_owned());

                candidate
                    .highlights
                    .iter()
                    .filter(|projection| projection.relevance >= self.config.rescue_threshold)
                    .map(move |projection| RescueCue {
                        source_skill: skill_name.clone(),
                        title: projection.subunit.title.clone(),
                        content: projection.subunit.content.clone(),
                        relevance: projection.relevance,
                    })
            })
            .collect();

        Self::dedupe(&mut degraded_scopes);
        Self::dedupe(&mut reason_codes);

        let health = if degraded_scopes.is_empty() {
            Self::healthy_markers()
        } else {
            let reason = reason_codes
                .first()
                .cloned()
                .unwrap_or_else(|| "retrieval_degraded".to_owned());
            Self::degraded_marker(&reason)
        };

        RetrievalOutcome {
            skills: selected_skills,
            rescue_pool,
            degraded_scopes,
            reason_codes,
            health,
            scopes_considered,
            graph_version: self.graph.graph_version,
            latency_ms: started.elapsed().as_millis(),
        }
    }

    fn current_graph_version(&self) -> i64 {
        self.graph.graph_version
    }

    fn configured_scopes(&self) -> Vec<String> {
        if let Some(scope_resolver) = &self.scope_resolver {
            scope_resolver.configured_scope_ids()
        } else {
            vec![self.config.scope_id.clone()]
        }
    }
}
