/// A single ranked result from an in-memory cosine similarity pass.
///
/// Embeddings originate from the graph-builder→Qdrant write-side pipeline
/// but this ranking is purely in-memory against the pre-loaded `RetrievalSnapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct CosineHit {
    pub skill_index: usize,
    pub semantic_score: f32,
}

/// Ranks `skill_embeddings` against `prompt_embedding` by cosine similarity and
/// returns the top-`limit` hits as `CosineHit` values.
///
/// **This function is purely in-memory.** It does not contact Qdrant or any network
/// service. The name reflects the historical source of the embeddings (they are built
/// by the graph-builder and stored durably in Qdrant), but the retrieval step
/// operates entirely on the pre-loaded `RetrievalSnapshot` (the CQRS read model).
///
/// Under Option A (ADR-0001) the read path never queries Qdrant at request time;
/// Qdrant is the durable write-side store only. Option B (V2) would replace this
/// call with a live Qdrant query behind the unchanged `SkillRetriever` trait.
pub fn rank_by_cosine(
    prompt_embedding: &[f32],
    skill_embeddings: &[&[f32]],
    limit: usize,
) -> Vec<CosineHit> {
    let mut hits: Vec<CosineHit> = skill_embeddings
        .iter()
        .enumerate()
        .map(|(skill_index, &embedding)| CosineHit {
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
    fn rank_by_cosine_orders_by_semantic_score() {
        let prompt = vec![1.0, 0.0];
        let embeddings = [vec![0.9, 0.0_f32], vec![0.2, 0.8], vec![1.0, 0.0]];
        let embedding_refs: Vec<&[f32]> = embeddings.iter().map(Vec::as_slice).collect();

        let hits = rank_by_cosine(&prompt, &embedding_refs, 2);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|hit| hit.skill_index == 0));
        assert!(hits.iter().any(|hit| hit.skill_index == 2));
    }
}
