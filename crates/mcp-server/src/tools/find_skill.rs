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
    use std::{collections::BTreeMap, sync::Arc};

    use async_trait::async_trait;
    use domain::{DomainId, LifecycleStatus, ScopeType, ScoredSkill, Skill, SkillStatus};
    use retrieval::{RetrievalOutcome, RetrievedSkill, SkillRetriever};

    use super::{FindSkillRequest, FindSkillTool};

    // ---------------------------------------------------------------------------
    // Stub retriever: returns two skills with identical RRF score (0.016393) but
    // distinct semantic_score values (0.80 and 0.50), mirroring the exact
    // scenario that produced the #260 bug.  degraded_scopes is empty so
    // invoke() takes the normal (non-degraded) branch.
    // ---------------------------------------------------------------------------

    struct TwoSkillStub;

    #[async_trait]
    impl SkillRetriever for TwoSkillStub {
        async fn retrieve(&self, _prompt: &str, _repo_path: Option<&str>) -> RetrievalOutcome {
            RetrievalOutcome {
                skills: vec![
                    RetrievedSkill {
                        scored_skill: ScoredSkill {
                            skill: minimal_skill("skill-high"),
                            score: 0.016393, // RRF rank artifact (same for both)
                            semantic_score: 0.800, // eq.3 relevance — should surface as `score`
                            matched_scope: ScopeType::Global,
                            rationale: vec!["rrf=0.016393".to_owned(), "semantic=0.800".to_owned()],
                        },
                        highlights: Vec::new(),
                    },
                    RetrievedSkill {
                        scored_skill: ScoredSkill {
                            skill: minimal_skill("skill-low"),
                            score: 0.016393,       // same RRF artifact
                            semantic_score: 0.500, // lower cosine
                            matched_scope: ScopeType::Global,
                            rationale: vec!["rrf=0.016393".to_owned(), "semantic=0.500".to_owned()],
                        },
                        highlights: Vec::new(),
                    },
                ],
                rescue_pool: Vec::new(),
                degraded_scopes: Vec::new(), // must be empty — non-empty triggers "degraded" branch
                reason_codes: Vec::new(),
                health: BTreeMap::new(),
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

    /// #260 RED→GREEN seam test: drives `FindSkillTool::invoke` through a stub that
    /// returns two skills with the *same* RRF artifact (`score=0.016393`) but
    /// *different* cosine relevance (`semantic_score` 0.80 / 0.50).
    ///
    /// Asserts:
    /// 1. The two agent-facing `score` strings are DISTINCT — they track semantic cosine,
    ///    not the shared RRF constant.
    /// 2. For each match, `score != fusion_rank_score` — the two fields expose different
    ///    quantities.
    /// 3. The two `fusion_rank_score` strings are IDENTICAL — confirms the RRF input was
    ///    the same for both, making the `score` distinctness meaningful.
    ///
    /// RED trigger: revert `invoke()` line ~131 to
    ///   `score: format!("{:.3}", retrieved.scored_skill.score)`
    /// and invariant (1) MUST fail because both scores collapse to "0.016".
    #[tokio::test]
    async fn invoke_exposes_semantic_score_not_rrf_artifact() {
        let tool = FindSkillTool::new(Arc::new(TwoSkillStub));
        let req = FindSkillRequest {
            prompt: "test query".to_owned(),
            limit: Some(5),
        };
        let resp = tool.invoke(req).await;

        assert_eq!(
            resp.status, "ok",
            "stub with two skills must return status 'ok'"
        );
        assert_eq!(resp.matches.len(), 2, "invoke must return both matches");

        let m0 = &resp.matches[0];
        let m1 = &resp.matches[1];

        // (1) Agent-facing scores must differ — semantic_score (0.80 vs 0.50), not the
        //     shared RRF artifact (0.016393 for both).
        assert_ne!(
            m0.score, m1.score,
            "#260: matches with different semantic_score but identical RRF artifact must \
             produce different agent-facing `score` values; got m0={}, m1={}",
            m0.score, m1.score
        );

        // (2) Per match: score != fusion_rank_score (different quantities in the response).
        assert_ne!(
            m0.score, m0.fusion_rank_score,
            "#260: m0.score ({}) must differ from m0.fusion_rank_score ({}); \
             score=semantic relevance, fusion_rank_score=RRF artifact",
            m0.score, m0.fusion_rank_score
        );
        assert_ne!(
            m1.score, m1.fusion_rank_score,
            "#260: m1.score ({}) must differ from m1.fusion_rank_score ({})",
            m1.score, m1.fusion_rank_score
        );

        // (3) Both fusion_rank_scores are identical (same RRF input) — confirms the
        //     score distinctness above comes from semantic_score, not RRF variation.
        assert_eq!(
            m0.fusion_rank_score, m1.fusion_rank_score,
            "#260: both matches had the same RRF artifact; fusion_rank_score must match; \
             got m0={}, m1={}",
            m0.fusion_rank_score, m1.fusion_rank_score
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
