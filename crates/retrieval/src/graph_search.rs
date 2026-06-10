use std::collections::BTreeSet;

use domain::Subunit;

use crate::cosine_rank::cosine_similarity;
use crate::orchestrator::SeededSkill;

/// Weight of the semantic (cosine) component in a subunit's displayed relevance.
/// Semantic dominates so a subunit that *means* the same thing as the query ranks
/// above one that merely shares literal tokens; lexical overlap remains as a cheap
/// tiebreaker for near-ties (SkillRAE eq.3 β — issue #172).
const SUBUNIT_SEMANTIC_WEIGHT: f32 = 0.75;
/// Weight of the lexical (token-overlap) component in a subunit's displayed relevance.
const SUBUNIT_LEXICAL_WEIGHT: f32 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct SubunitProjection {
    pub subunit: Subunit,
    /// Blended display relevance: `0.75·cosine(query, subunit) + 0.25·lexical`.
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphHit {
    pub skill_index: usize,
    pub lexical_score: f32,
    /// Aggregate semantic relevance of the skill's subunits to the query: the mean
    /// of the top-`max_subunits_per_skill` per-subunit cosine scores. This is the
    /// real β (`subunit_evidence`) term of SkillRAE eq.3 — it reflects subunit
    /// *meaning*, not skill-name/description token overlap (issue #172).
    pub subunit_evidence: f32,
    pub projections: Vec<SubunitProjection>,
}

/// Projects each candidate skill's subunits, scoring every subunit by a blend of
/// semantic cosine (query embedding vs subunit embedding) and lexical token
/// overlap, and derives the skill's `subunit_evidence` (mean of top-k semantic
/// scores) for the eq.3 β term.
///
/// Reads each candidate skill's name/description/tags and subunits directly from
/// the snapshot `skills` slice (indexed by `candidate_indices`) — it does NOT
/// receive full-corpus projection clones, so a query touches only the ~k candidate
/// rows it actually scores rather than rebuilding N-sized Vecs per request (#254).
/// Each skill's `subunit_embeddings[i]` is the embedding of its i-th subunit (same
/// order); a missing/empty embedding makes that subunit's semantic score 0 and it
/// falls back to lexical only — this never panics on a subunit that was not embedded.
pub fn search_graph(
    prompt: &str,
    prompt_embedding: &[f32],
    skills: &[SeededSkill],
    candidate_indices: &[usize],
    max_subunits_per_skill: usize,
) -> Vec<GraphHit> {
    let prompt_tokens = tokenize(prompt);

    candidate_indices
        .iter()
        .filter_map(|skill_index| {
            let seeded = skills.get(*skill_index)?;
            let text = format!(
                "{} {} {}",
                seeded.skill.name,
                seeded.skill.description,
                seeded.skill.tags.join(" ")
            );
            let subunits = &seeded.subunits;
            let subunit_embeddings = &seeded.subunit_embeddings;

            let lexical_score = token_overlap_score(&prompt_tokens, &tokenize(&text));

            // Score every subunit: semantic (cosine) + lexical (token overlap).
            let mut scored: Vec<(SubunitProjection, f32)> = subunits
                .iter()
                .cloned()
                .enumerate()
                .map(|(position, subunit)| {
                    let semantic = subunit_embeddings
                        .get(position)
                        .map(|embedding| cosine_similarity(prompt_embedding, embedding).max(0.0))
                        .unwrap_or(0.0);
                    let lexical = token_overlap_score(
                        &prompt_tokens,
                        &tokenize(&format!(
                            "{} {}",
                            subunit.title.to_lowercase(),
                            subunit.content.to_lowercase()
                        )),
                    );
                    let relevance =
                        SUBUNIT_SEMANTIC_WEIGHT * semantic + SUBUNIT_LEXICAL_WEIGHT * lexical;
                    (SubunitProjection { subunit, relevance }, semantic)
                })
                .collect();

            // subunit_evidence (β) = mean of the top-k SEMANTIC scores, so it
            // reflects subunit meaning independent of the lexical tiebreaker.
            let mut semantic_scores: Vec<f32> =
                scored.iter().map(|(_, semantic)| *semantic).collect();
            semantic_scores.sort_by(|left, right| right.total_cmp(left));
            let top_k = semantic_scores.iter().take(max_subunits_per_skill);
            let top_k_len = semantic_scores.len().min(max_subunits_per_skill);
            let subunit_evidence = if top_k_len == 0 {
                0.0
            } else {
                top_k.sum::<f32>() / top_k_len as f32
            };

            // Displayed/ranked projections use the blended relevance.
            scored.sort_by(|left, right| right.0.relevance.total_cmp(&left.0.relevance));
            scored.truncate(max_subunits_per_skill);
            let projections: Vec<SubunitProjection> = scored
                .into_iter()
                .map(|(projection, _)| projection)
                .collect();

            Some(GraphHit {
                skill_index: *skill_index,
                lexical_score,
                subunit_evidence,
                projections,
            })
        })
        .collect()
}

/// Deduplicated token set for token-overlap scoring. Thin wrapper over the
/// shared raw tokenizer (#249) — the dedup is the only difference from the
/// TF-preserving callers; the split policy itself lives in one place.
pub(crate) fn tokenize(input: &str) -> BTreeSet<String> {
    crate::text::tokenize_tokens(input).into_iter().collect()
}

pub(crate) fn token_overlap_score(lhs: &BTreeSet<String>, rhs: &BTreeSet<String>) -> f32 {
    if lhs.is_empty() || rhs.is_empty() {
        return 0.0;
    }

    let overlap = lhs.intersection(rhs).count() as f32;
    overlap / lhs.len() as f32
}

#[cfg(test)]
mod tests {
    use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus, SubunitType};

    use super::*;

    fn subunit(id: &str, title: &str, content: &str) -> Subunit {
        Subunit {
            id: DomainId::new_unchecked(id),
            skill_id: DomainId::new_unchecked("skill-1"),
            kind: SubunitType::Procedure,
            title: title.to_owned(),
            content: content.to_owned(),
            lifecycle: LifecycleStatus::Active,
        }
    }

    /// Builds a minimal `SeededSkill` for `search_graph` tests. `text` becomes the
    /// skill name (search_graph derives the lexical document from name+description+
    /// tags); `subunit_embeddings[i]` is the embedding of subunit `i`.
    fn seeded(
        text: &str,
        subunits: Vec<Subunit>,
        subunit_embeddings: Vec<Vec<f32>>,
    ) -> SeededSkill {
        SeededSkill {
            skill: Skill {
                id: DomainId::new_unchecked("skill-1"),
                name: text.to_owned(),
                description: String::new(),
                scope: ScopeType::Global,
                status: SkillStatus::Ready,
                lifecycle: LifecycleStatus::Active,
                tags: vec![],
                subunit_ids: vec![],
                community_id: None,
            },
            scope_id: "global".to_owned(),
            source_paths: vec![],
            embedding: vec![],
            subunits,
            subunit_embeddings,
            prior: 0.0,
            community_boost: 0.0,
            e_task_embedding: Vec::new(),
            e_needs_embedding: Vec::new(),
            e_negative_embedding: Vec::new(),
        }
    }

    #[test]
    fn search_graph_projects_relevant_subunits() {
        let sub = subunit("sub-1", "Read a file", "Use std::fs::read_to_string");

        let skills = vec![seeded(
            "read files in rust",
            vec![sub],
            vec![vec![1.0, 0.0]],
        )];
        let hits = search_graph("how to read file", &[1.0, 0.0], &skills, &[0], 3);

        assert_eq!(hits.len(), 1);
        assert!(hits[0].lexical_score > 0.0);
        assert!(hits[0].projections[0].relevance > 0.0);
        assert!(hits[0].subunit_evidence > 0.0);
    }

    /// THE #172 contract: a subunit that is semantically aligned with the query
    /// but shares ZERO literal tokens with it must still be selected and drive
    /// `subunit_evidence` — where the old lexical-only implementation scored 0.
    #[test]
    fn semantically_aligned_subunit_with_no_lexical_overlap_is_selected() {
        // Query embedding points along axis 0. The "relevant" subunit's embedding
        // also points along axis 0 (cosine = 1) but its TEXT shares no tokens with
        // the query. The "irrelevant" subunit's embedding is orthogonal (cosine = 0)
        // even though it is wordy.
        let prompt = "alpha bravo charlie";
        let prompt_embedding = vec![1.0, 0.0];

        let relevant = subunit("relevant", "zulu yankee", "xray whiskey victor");
        let irrelevant = subunit("irrelevant", "delta echo", "foxtrot golf hotel");

        // No lexical overlap between prompt tokens and either subunit's tokens.
        let skills = vec![seeded(
            "skill text with no query tokens",
            vec![relevant.clone(), irrelevant.clone()],
            vec![
                vec![1.0, 0.0], // relevant: cosine(query)=1
                vec![0.0, 1.0], // irrelevant: cosine(query)=0
            ],
        )];
        let hits = search_graph(prompt, &prompt_embedding, &skills, &[0], 3);

        assert_eq!(hits.len(), 1);
        let hit = &hits[0];

        // Semantic evidence is non-zero despite zero lexical overlap.
        assert!(
            hit.subunit_evidence > 0.0,
            "subunit_evidence must be driven by semantics, not lexical overlap; got {}",
            hit.subunit_evidence
        );

        // The semantically-aligned subunit ranks first with positive relevance,
        // even though it shares no tokens with the query.
        assert_eq!(
            hit.projections[0].subunit.id.as_str(),
            "relevant",
            "the semantically-aligned subunit must rank first"
        );
        assert!(
            hit.projections[0].relevance > 0.0,
            "semantic relevance must be positive with zero lexical overlap"
        );

        // Sanity: a purely lexical implementation would have scored BOTH subunits 0
        // (no token overlap), so this behaviour is only possible with semantics.
        let prompt_tokens = tokenize(prompt);
        for s in [&relevant, &irrelevant] {
            let lex = token_overlap_score(
                &prompt_tokens,
                &tokenize(&format!("{} {}", s.title, s.content)),
            );
            assert_eq!(lex, 0.0, "test fixture must have zero lexical overlap");
        }
    }
}
