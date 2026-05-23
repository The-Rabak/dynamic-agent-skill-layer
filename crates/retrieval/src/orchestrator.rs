use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use domain::{
    EmbeddingError, EmbeddingService, ScopeType, ScoredSkill, Skill, Subunit, SubunitType,
};

use crate::{
    fusion::{FusedCandidate, mmr_select},
    graph_search::{GraphHit, search_graph},
    qdrant_search::search_qdrant,
    scoring::{ScoreComponents, ScoringWeights, score_eq3},
};

#[derive(Debug, Clone)]
pub struct SeededSkill {
    pub skill: Skill,
    pub embedding: Vec<f32>,
    pub subunits: Vec<Subunit>,
    pub prior: f32,
    pub community_boost: f32,
}

#[derive(Debug, Clone)]
pub struct SeededGraph {
    pub graph_version: i64,
    pub skills: Vec<SeededSkill>,
}

impl SeededGraph {
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
        }
    }
}

#[async_trait]
pub trait SkillRetriever: Send + Sync {
    async fn retrieve(&self, prompt: &str) -> RetrievalOutcome;
    fn current_graph_version(&self) -> i64;
    fn configured_scopes(&self) -> Vec<String>;
}

pub struct RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    embedding_service: Arc<E>,
    graph: Arc<SeededGraph>,
    config: RetrievalConfig,
}

impl<E> RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    pub fn new(embedding_service: Arc<E>, graph: SeededGraph, config: RetrievalConfig) -> Self {
        Self {
            embedding_service,
            graph: Arc::new(graph),
            config,
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
}

#[async_trait]
impl<E> SkillRetriever for RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    async fn retrieve(&self, prompt: &str) -> RetrievalOutcome {
        let started = Instant::now();
        let scope = self.config.scope_id.clone();

        let prompt_embedding = match self.embedding_service.embed_text(prompt).await {
            Ok(embedding) => embedding,
            Err(error) => {
                let reason = Self::map_embedding_error_to_reason(&error);
                return RetrievalOutcome {
                    skills: Vec::new(),
                    rescue_pool: Vec::new(),
                    degraded_scopes: vec![scope.clone()],
                    reason_codes: vec![reason.clone()],
                    health: Self::degraded_marker(&reason),
                    scopes_considered: vec![scope],
                    graph_version: self.graph.graph_version,
                    latency_ms: started.elapsed().as_millis(),
                };
            }
        };

        let skill_embeddings: Vec<Vec<f32>> = self
            .graph
            .skills
            .iter()
            .map(|seeded_skill| seeded_skill.embedding.clone())
            .collect();
        let qdrant_hits = search_qdrant(
            &prompt_embedding,
            &skill_embeddings,
            self.config.candidate_limit,
        );
        let candidate_indices: Vec<usize> = qdrant_hits.iter().map(|hit| hit.skill_index).collect();

        let skill_text: Vec<String> = self
            .graph
            .skills
            .iter()
            .map(|seeded_skill| {
                format!(
                    "{} {} {}",
                    seeded_skill.skill.name,
                    seeded_skill.skill.description,
                    seeded_skill.skill.tags.join(" ")
                )
            })
            .collect();

        let skill_subunits: Vec<Vec<Subunit>> = self
            .graph
            .skills
            .iter()
            .map(|seeded_skill| seeded_skill.subunits.clone())
            .collect();

        let graph_hits = search_graph(
            prompt,
            &skill_text,
            &skill_subunits,
            &candidate_indices,
            self.config.max_subunits_per_skill,
        );

        let graph_hits_by_skill: HashMap<usize, GraphHit> = graph_hits
            .into_iter()
            .map(|hit| (hit.skill_index, hit))
            .collect();

        let mut fused_candidates: Vec<FusedCandidate> = qdrant_hits
            .iter()
            .filter_map(|qdrant_hit| {
                let seeded_skill = self.graph.skills.get(qdrant_hit.skill_index)?;
                let graph_hit = graph_hits_by_skill.get(&qdrant_hit.skill_index);
                let lexical_score = graph_hit.map_or(0.0, |hit| hit.lexical_score);
                let score = score_eq3(
                    ScoreComponents {
                        l1_semantic: qdrant_hit.semantic_score,
                        l0_lexical: lexical_score,
                        prior: seeded_skill.prior,
                        community_boost: seeded_skill.community_boost,
                    },
                    self.config.scoring_weights,
                );

                Some(FusedCandidate {
                    skill_index: qdrant_hit.skill_index,
                    score,
                    semantic_score: qdrant_hit.semantic_score,
                    lexical_score,
                    embedding: seeded_skill.embedding.clone(),
                    highlights: graph_hit
                        .map(|hit| hit.projections.clone())
                        .unwrap_or_default(),
                })
            })
            .filter(|candidate| candidate.score >= self.config.relevance_threshold)
            .collect();

        fused_candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
        let selected = mmr_select(
            &fused_candidates,
            self.config.max_results,
            self.config.mmr_lambda,
        );
        let selected_indexes: HashSet<usize> = selected
            .iter()
            .map(|candidate| candidate.skill_index)
            .collect();

        let selected_skills: Vec<RetrievedSkill> = selected
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
                        matched_scope: self.config.scope_type,
                        rationale: vec![
                            format!("semantic={:.3}", candidate.semantic_score),
                            format!("lexical={:.3}", candidate.lexical_score),
                        ],
                    },
                    highlights,
                })
            })
            .collect();

        let rescue_pool: Vec<RescueCue> = fused_candidates
            .iter()
            .filter(|candidate| !selected_indexes.contains(&candidate.skill_index))
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

        RetrievalOutcome {
            skills: selected_skills,
            rescue_pool,
            degraded_scopes: Vec::new(),
            reason_codes: Vec::new(),
            health: Self::healthy_markers(),
            scopes_considered: vec![self.config.scope_id.clone()],
            graph_version: self.graph.graph_version,
            latency_ms: started.elapsed().as_millis(),
        }
    }

    fn current_graph_version(&self) -> i64 {
        self.graph.graph_version
    }

    fn configured_scopes(&self) -> Vec<String> {
        vec![self.config.scope_id.clone()]
    }
}
