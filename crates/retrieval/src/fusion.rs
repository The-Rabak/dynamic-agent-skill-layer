use crate::{graph_search::SubunitProjection, qdrant_search::cosine_similarity};

#[derive(Debug, Clone, PartialEq)]
pub struct FusedCandidate {
    pub skill_index: usize,
    pub score: f32,
    pub semantic_score: f32,
    pub lexical_score: f32,
    pub embedding: Vec<f32>,
    pub highlights: Vec<SubunitProjection>,
}

pub fn mmr_select(
    candidates: &[FusedCandidate],
    max_results: usize,
    lambda: f32,
) -> Vec<FusedCandidate> {
    if candidates.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let mut selected_indices: Vec<usize> = Vec::new();
    let mut remaining_indices: Vec<usize> = (0..candidates.len()).collect();
    let clamped_lambda = lambda.clamp(0.0, 1.0);

    while selected_indices.len() < max_results && !remaining_indices.is_empty() {
        let mut best_index: Option<usize> = None;
        let mut best_mmr = f32::MIN;

        for candidate_index in &remaining_indices {
            let candidate = &candidates[*candidate_index];
            let max_similarity = selected_indices
                .iter()
                .map(|selected_index| {
                    let selected = &candidates[*selected_index];
                    cosine_similarity(&candidate.embedding, &selected.embedding).max(0.0)
                })
                .fold(0.0_f32, f32::max);

            let mmr = clamped_lambda * candidate.score - (1.0 - clamped_lambda) * max_similarity;
            if mmr > best_mmr {
                best_mmr = mmr;
                best_index = Some(*candidate_index);
            }
        }

        if let Some(winner) = best_index {
            selected_indices.push(winner);
            remaining_indices.retain(|index| *index != winner);
        } else {
            break;
        }
    }

    selected_indices
        .into_iter()
        .map(|index| candidates[index].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use domain::{DomainId, LifecycleStatus, Subunit, SubunitType};

    use super::*;

    #[test]
    fn mmr_select_prefers_diversity_when_scores_tie() {
        let highlight = SubunitProjection {
            subunit: Subunit {
                id: DomainId::new_unchecked("sub-1"),
                skill_id: DomainId::new_unchecked("skill-1"),
                kind: SubunitType::Procedure,
                title: "sample".to_owned(),
                content: "sample".to_owned(),
                lifecycle: LifecycleStatus::Active,
            },
            relevance: 1.0,
        };

        let candidates = vec![
            FusedCandidate {
                skill_index: 0,
                score: 0.9,
                semantic_score: 0.9,
                lexical_score: 0.9,
                embedding: vec![1.0, 0.0],
                highlights: vec![highlight.clone()],
            },
            FusedCandidate {
                skill_index: 1,
                score: 0.9,
                semantic_score: 0.9,
                lexical_score: 0.9,
                embedding: vec![0.99, 0.01],
                highlights: vec![highlight.clone()],
            },
            FusedCandidate {
                skill_index: 2,
                score: 0.85,
                semantic_score: 0.85,
                lexical_score: 0.8,
                embedding: vec![0.0, 1.0],
                highlights: vec![highlight],
            },
        ];

        let selected = mmr_select(&candidates, 2, 0.55);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].skill_index, 0);
        assert_eq!(selected[1].skill_index, 2);
    }
}
