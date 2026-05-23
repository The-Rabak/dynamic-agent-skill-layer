#[derive(Debug, Clone, PartialEq)]
pub struct QdrantHit {
    pub skill_index: usize,
    pub semantic_score: f32,
}

pub fn search_qdrant(
    prompt_embedding: &[f32],
    skill_embeddings: &[Vec<f32>],
    limit: usize,
) -> Vec<QdrantHit> {
    let mut hits: Vec<QdrantHit> = skill_embeddings
        .iter()
        .enumerate()
        .map(|(skill_index, embedding)| QdrantHit {
            skill_index,
            semantic_score: cosine_similarity(prompt_embedding, embedding).max(0.0),
        })
        .collect();

    hits.sort_by(|left, right| right.semantic_score.total_cmp(&left.semantic_score));
    hits.truncate(limit);
    hits
}

pub(crate) fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f32 {
    if lhs.len() != rhs.len() || lhs.is_empty() {
        return 0.0;
    }

    let dot = lhs.iter().zip(rhs.iter()).map(|(l, r)| l * r).sum::<f32>();
    let lhs_norm = lhs.iter().map(|value| value * value).sum::<f32>().sqrt();
    let rhs_norm = rhs.iter().map(|value| value * value).sum::<f32>().sqrt();

    if lhs_norm == 0.0 || rhs_norm == 0.0 {
        return 0.0;
    }

    dot / (lhs_norm * rhs_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_qdrant_orders_by_semantic_score() {
        let prompt = vec![1.0, 0.0];
        let embeddings = vec![vec![0.9, 0.0], vec![0.2, 0.8], vec![1.0, 0.0]];

        let hits = search_qdrant(&prompt, &embeddings, 2);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|hit| hit.skill_index == 0));
        assert!(hits.iter().any(|hit| hit.skill_index == 2));
    }
}
