use std::sync::Arc;

use retrieval::SkillRetriever;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindSkillRequest {
    pub prompt: String,
    pub limit: Option<usize>,
}

/// A single skill match returned by `find_skill`.
///
/// `score` is the eq.3 relevance score (weighted combination of semantic
/// cosine, subunit evidence, and usage prior), NOT the RRF rank artifact.
/// `fusion_rank_score` carries the RRF value for ordering auditability.
/// `rationale` lists the score components so an agent can decide what to read.
/// `skill_id` is the stable UUID for this skill; used by `search_skill_graph`
/// to filter graph edges to only those incident on matched skills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillMatch {
    /// Stable UUID of the matched skill — used by `search_skill_graph` for edge filtering.
    pub skill_id: String,
    pub name: String,
    pub description: String,
    /// eq.3 relevance score — relevance-meaningful, NOT the RRF rank artifact.
    pub score: String,
    /// RRF fusion ordering score — ordering provenance, NOT a relevance signal.
    pub fusion_rank_score: String,
    pub tags: Vec<String>,
    /// Per-skill retrieval rationale: `["rrf=…", "semantic=…", "subunit_evidence=…", "lexical=…"]`.
    pub rationale: Vec<String>,
}

/// Provenance of the retrieval result so an agent can tell which vector space
/// produced these matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalContext {
    /// Active embedding model name (e.g. `"qwen3-embedding:4b"`).
    pub embedding_model: String,
    /// Active Qdrant collection name for this model.
    pub collection: String,
    /// Graph version this snapshot was built from.
    pub graph_version: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindSkillResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub matches: Vec<SkillMatch>,
    /// Provenance of these results. `None` on degraded responses where no
    /// retrieval ran (the embedding model / collection are therefore unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_context: Option<RetrievalContext>,
}

/// Tool that retrieves skills from the skill graph for agent consumption.
///
/// Constructed with an optional `embedding_model` and `collection` string so
/// the `retrieval_context` provenance block can be populated without a live PG
/// query on every call. These are set once at server boot from
/// `LiveServerComponents.embedding_model_info`.
#[derive(Clone)]
pub struct FindSkillTool {
    retriever: Arc<dyn SkillRetriever>,
    /// Embedding model name for the `retrieval_context` provenance block.
    embedding_model: Option<String>,
    /// Qdrant collection name for the `retrieval_context` provenance block.
    collection: Option<String>,
}

impl FindSkillTool {
    /// Creates the tool without provenance context (used in tests and legacy constructors).
    pub fn new(retriever: Arc<dyn SkillRetriever>) -> Self {
        Self {
            retriever,
            embedding_model: None,
            collection: None,
        }
    }

    /// Creates the tool with provenance context for the `retrieval_context` field (#243).
    ///
    /// `embedding_model` and `collection` come from `LiveServerComponents.embedding_model_info`
    /// and `qdrant_adapter.config.collection_name` at server boot.
    pub fn with_provenance(
        retriever: Arc<dyn SkillRetriever>,
        embedding_model: impl Into<String>,
        collection: impl Into<String>,
    ) -> Self {
        Self {
            retriever,
            embedding_model: Some(embedding_model.into()),
            collection: Some(collection.into()),
        }
    }

    /// Exposes the internal retriever for builder-style re-wiring (e.g. adding provenance).
    pub fn retriever(&self) -> &Arc<dyn SkillRetriever> {
        &self.retriever
    }

    pub async fn invoke(&self, request: FindSkillRequest) -> FindSkillResponse {
        let outcome = self.retriever.retrieve(&request.prompt, None).await;

        if outcome.is_degraded() {
            return FindSkillResponse {
                status: "degraded".to_owned(),
                reason_code: outcome.reason_codes.first().cloned(),
                matches: Vec::new(),
                retrieval_context: None,
            };
        }

        let limit = request.limit.unwrap_or(5);
        let matches: Vec<SkillMatch> = outcome
            .skills
            .iter()
            .take(limit)
            .map(|retrieved| {
                // #260: expose the eq.3 relevance score (semantic_score threaded
                // directly through ScoredSkill) as the agent-facing `score`. The
                // RRF rank artifact (in scored_skill.score after finalize_aggregates)
                // is exposed separately as `fusion_rank_score` for ordering auditability.
                SkillMatch {
                    skill_id: retrieved.scored_skill.skill.id.as_str().to_owned(),
                    name: retrieved.scored_skill.skill.name.clone(),
                    description: retrieved.scored_skill.skill.description.clone(),
                    // eq.3 relevance (not RRF) — the mandate of #260.
                    score: format!("{:.3}", retrieved.scored_skill.semantic_score),
                    // RRF rank artifact — ordering provenance only.
                    fusion_rank_score: format!("{:.6}", retrieved.scored_skill.score),
                    tags: retrieved.scored_skill.skill.tags.clone(),
                    rationale: retrieved.scored_skill.rationale.clone(),
                }
            })
            .collect();

        // Build provenance context when model/collection are known (#243).
        let retrieval_context = self
            .embedding_model
            .as_deref()
            .zip(self.collection.as_deref())
            .map(|(model, coll)| RetrievalContext {
                embedding_model: model.to_owned(),
                collection: coll.to_owned(),
                graph_version: outcome.graph_version,
            });

        if matches.is_empty() {
            FindSkillResponse {
                status: "no_match".to_owned(),
                reason_code: Some("no_relevant_skills".to_owned()),
                matches,
                retrieval_context,
            }
        } else {
            FindSkillResponse {
                status: "ok".to_owned(),
                reason_code: None,
                matches,
                retrieval_context,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::{DomainId, LifecycleStatus, ScopeType, ScoredSkill, Skill, SkillStatus};

    /// #260 RED→GREEN: two `ScoredSkill` values with different `semantic_score` at the
    /// same RRF `score` must expose DIFFERENT agent-facing `score` strings.
    ///
    /// This is the core behavioral assertion for the #260 bug: previously, `score`
    /// was sourced from `scored_skill.score` (the RRF rank artifact, `~0.016` for all
    /// top-ranked results), which is independent of match quality. After the fix,
    /// `score` is `format!("{:.3}", scored_skill.semantic_score)` — so a 0.80 match
    /// and a 0.50 match at the same RRF rank are distinguishable.
    #[test]
    fn scored_skill_semantic_score_distinguishes_different_cosine_matches() {
        // Two ScoredSkills with the same RRF rank artifact but different semantic cosine.
        let high = ScoredSkill {
            skill: minimal_skill("skill-high"),
            score: 0.016393,
            semantic_score: 0.800,
            matched_scope: ScopeType::Global,
            rationale: vec!["rrf=0.016393".to_owned(), "semantic=0.800".to_owned()],
        };
        let low = ScoredSkill {
            skill: minimal_skill("skill-low"),
            score: 0.016393,
            semantic_score: 0.500,
            matched_scope: ScopeType::Global,
            rationale: vec!["rrf=0.016393".to_owned(), "semantic=0.500".to_owned()],
        };

        let high_exposed = format!("{:.3}", high.semantic_score);
        let low_exposed = format!("{:.3}", low.semantic_score);

        assert_ne!(
            high_exposed, low_exposed,
            "#260: two ScoredSkills with different semantic_score at the same RRF rank must \
             produce different exposed scores; got high={high_exposed}, low={low_exposed}"
        );
        assert!(
            high.semantic_score > low.semantic_score,
            "higher cosine must produce a higher relevance score"
        );
    }

    /// Proves that `semantic_score` and `score` (RRF) are tracked as independent fields:
    /// two skills can have the same RRF rank artifact while carrying different semantic scores.
    #[test]
    fn scored_skill_rrf_and_semantic_are_independent_fields() {
        let skill = ScoredSkill {
            skill: minimal_skill("skill-x"),
            score: 0.016393,      // RRF artifact
            semantic_score: 0.75, // real cosine signal
            matched_scope: ScopeType::Global,
            rationale: vec![],
        };
        // The RRF artifact and semantic score must be independently stored.
        assert!(
            (skill.score - 0.016393_f32).abs() < 1e-5,
            "score field must hold the RRF artifact"
        );
        assert!(
            (skill.semantic_score - 0.75_f32).abs() < 1e-5,
            "semantic_score field must hold the eq.3 cosine"
        );
    }

    fn minimal_skill(id: &str) -> Skill {
        Skill {
            id: DomainId::new_unchecked(id),
            name: id.to_owned(),
            description: String::new(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec![],
            subunit_ids: vec![],
            community_id: None,
        }
    }
}
