use std::{cmp::Ordering, collections::HashMap};

use crate::{graph_search::SubunitProjection, qdrant_search::cosine_similarity};
use domain::ScopeType;

#[derive(Debug, Clone, PartialEq)]
pub struct FusedCandidate {
    pub skill_index: usize,
    pub skill_id: String,
    pub matched_scope: ScopeType,
    pub score: f32,
    pub semantic_score: f32,
    pub lexical_score: f32,
    pub embedding: Vec<f32>,
    pub highlights: Vec<SubunitProjection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeRanking {
    /// Scope identifier used for upstream tracing/telemetry. Fusion scoring is scope-weight based.
    pub scope_id: String,
    pub weight: f32,
    pub candidates: Vec<FusedCandidate>,
}

#[derive(Debug, Clone)]
struct Aggregate {
    rrf_score: f32,
    representative: FusedCandidate,
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

pub fn weighted_reciprocal_rank_fusion(
    scope_rankings: &[ScopeRanking],
    k: f32,
    max_results: usize,
) -> Vec<FusedCandidate> {
    if scope_rankings.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let rrf_k = if k.is_finite() && k > 0.0 { k } else { 60.0 };

    let mut aggregated: HashMap<String, Aggregate> = HashMap::new();

    for scope_ranking in scope_rankings {
        accumulate_scope_ranking(&mut aggregated, scope_ranking, rrf_k);
    }

    let mut fused = finalize_aggregates(aggregated);
    fused.sort_by(fused_candidate_order);

    fused.truncate(max_results);
    fused
}

fn accumulate_scope_ranking(
    aggregated: &mut HashMap<String, Aggregate>,
    scope_ranking: &ScopeRanking,
    rrf_k: f32,
) {
    if scope_ranking.weight <= 0.0 {
        return;
    }

    for (rank, candidate) in scope_ranking.candidates.iter().enumerate() {
        let contribution = scope_ranking.weight / (rrf_k + (rank as f32 + 1.0));
        upsert_aggregate(aggregated, candidate, contribution);
    }
}

fn upsert_aggregate(
    aggregated: &mut HashMap<String, Aggregate>,
    candidate: &FusedCandidate,
    contribution: f32,
) {
    aggregated
        .entry(candidate.skill_id.clone())
        .and_modify(|aggregate| {
            aggregate.rrf_score += contribution;
            if should_replace_representative(candidate, &aggregate.representative) {
                aggregate.representative = candidate.clone();
            }
        })
        .or_insert_with(|| Aggregate {
            rrf_score: contribution,
            representative: candidate.clone(),
        });
}

fn finalize_aggregates(aggregated: HashMap<String, Aggregate>) -> Vec<FusedCandidate> {
    aggregated
        .into_values()
        .map(|aggregate| {
            let mut representative = aggregate.representative;
            representative.score = aggregate.rrf_score;
            representative
        })
        .collect()
}

fn fused_candidate_order(left: &FusedCandidate, right: &FusedCandidate) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| scope_priority(right.matched_scope).cmp(&scope_priority(left.matched_scope)))
        .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
        .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
}

fn should_replace_representative(candidate: &FusedCandidate, current: &FusedCandidate) -> bool {
    candidate.score > current.score
        || (candidate.score == current.score
            && scope_priority(candidate.matched_scope) > scope_priority(current.matched_scope))
}

fn scope_priority(scope: ScopeType) -> usize {
    match scope {
        ScopeType::Project => 3,
        ScopeType::Global => 2,
        ScopeType::Team => 1,
    }
}

#[cfg(test)]
mod tests {
    use domain::{DomainId, LifecycleStatus, ScopeType, Subunit, SubunitType};

    use super::*;

    fn candidate(skill_index: usize, score: f32, scope: ScopeType) -> FusedCandidate {
        let skill_id = format!("skill-{skill_index}");
        FusedCandidate {
            skill_index,
            skill_id,
            matched_scope: scope,
            score,
            semantic_score: score,
            lexical_score: score,
            embedding: vec![score, 1.0 - score],
            highlights: Vec::new(),
        }
    }

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
                skill_id: "skill-0".to_owned(),
                matched_scope: ScopeType::Global,
                score: 0.9,
                semantic_score: 0.9,
                lexical_score: 0.9,
                embedding: vec![1.0, 0.0],
                highlights: vec![highlight.clone()],
            },
            FusedCandidate {
                skill_index: 1,
                skill_id: "skill-1".to_owned(),
                matched_scope: ScopeType::Global,
                score: 0.9,
                semantic_score: 0.9,
                lexical_score: 0.9,
                embedding: vec![0.99, 0.01],
                highlights: vec![highlight.clone()],
            },
            FusedCandidate {
                skill_index: 2,
                skill_id: "skill-2".to_owned(),
                matched_scope: ScopeType::Global,
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

    #[test]
    fn weighted_rrf_favors_project_scope_when_ranks_tie() {
        let fused = weighted_reciprocal_rank_fusion(
            &[
                ScopeRanking {
                    scope_id: "project".to_owned(),
                    weight: 1.0,
                    candidates: vec![candidate(0, 0.7, ScopeType::Project)],
                },
                ScopeRanking {
                    scope_id: "global".to_owned(),
                    weight: 0.7,
                    candidates: vec![candidate(1, 0.9, ScopeType::Global)],
                },
            ],
            60.0,
            2,
        );

        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].matched_scope, ScopeType::Project);
    }

    #[test]
    fn weighted_rrf_sums_scores_for_same_skill_across_scopes() {
        let fused = weighted_reciprocal_rank_fusion(
            &[
                ScopeRanking {
                    scope_id: "project".to_owned(),
                    weight: 1.0,
                    candidates: vec![FusedCandidate {
                        skill_id: "shared-skill".to_owned(),
                        ..candidate(0, 0.7, ScopeType::Project)
                    }],
                },
                ScopeRanking {
                    scope_id: "global".to_owned(),
                    weight: 0.7,
                    candidates: vec![FusedCandidate {
                        skill_id: "shared-skill".to_owned(),
                        ..candidate(0, 0.8, ScopeType::Global)
                    }],
                },
            ],
            60.0,
            5,
        );

        assert_eq!(fused.len(), 1);
        let expected = (1.0 / 61.0) + (0.7 / 61.0);
        assert!((fused[0].score - expected).abs() < 1e-6);
    }

    #[test]
    fn weighted_rrf_keeps_best_representative_for_same_skill() {
        let fused = weighted_reciprocal_rank_fusion(
            &[
                ScopeRanking {
                    scope_id: "project".to_owned(),
                    weight: 1.0,
                    candidates: vec![FusedCandidate {
                        skill_id: "shared-skill".to_owned(),
                        ..candidate(0, 0.6, ScopeType::Project)
                    }],
                },
                ScopeRanking {
                    scope_id: "global".to_owned(),
                    weight: 1.0,
                    candidates: vec![FusedCandidate {
                        skill_id: "shared-skill".to_owned(),
                        ..candidate(0, 0.9, ScopeType::Global)
                    }],
                },
            ],
            60.0,
            1,
        );

        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].matched_scope, ScopeType::Global);
        assert_eq!(fused[0].semantic_score, 0.9);
    }
}
