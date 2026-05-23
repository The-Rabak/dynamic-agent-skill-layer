use std::sync::Arc;

use retrieval::SkillRetriever;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindSkillRequest {
    pub prompt: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMatch {
    pub name: String,
    pub description: String,
    pub score: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindSkillResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub matches: Vec<SkillMatch>,
}

#[derive(Clone)]
pub struct FindSkillTool {
    retriever: Arc<dyn SkillRetriever>,
}

impl FindSkillTool {
    pub fn new(retriever: Arc<dyn SkillRetriever>) -> Self {
        Self { retriever }
    }

    pub async fn invoke(&self, request: FindSkillRequest) -> FindSkillResponse {
        let outcome = self.retriever.retrieve(&request.prompt).await;

        if outcome.is_degraded() {
            return FindSkillResponse {
                status: "degraded".to_owned(),
                reason_code: outcome.reason_codes.first().cloned(),
                matches: Vec::new(),
            };
        }

        let limit = request.limit.unwrap_or(5);
        let matches: Vec<SkillMatch> = outcome
            .skills
            .iter()
            .take(limit)
            .map(|retrieved| SkillMatch {
                name: retrieved.scored_skill.skill.name.clone(),
                description: retrieved.scored_skill.skill.description.clone(),
                score: format!("{:.3}", retrieved.scored_skill.score),
                tags: retrieved.scored_skill.skill.tags.clone(),
            })
            .collect();

        if matches.is_empty() {
            FindSkillResponse {
                status: "no_match".to_owned(),
                reason_code: Some("no_relevant_skills".to_owned()),
                matches,
            }
        } else {
            FindSkillResponse {
                status: "ok".to_owned(),
                reason_code: None,
                matches,
            }
        }
    }
}
