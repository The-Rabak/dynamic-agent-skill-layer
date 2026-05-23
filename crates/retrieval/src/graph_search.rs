use std::collections::BTreeSet;

use domain::Subunit;

#[derive(Debug, Clone, PartialEq)]
pub struct SubunitProjection {
    pub subunit: Subunit,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphHit {
    pub skill_index: usize,
    pub lexical_score: f32,
    pub projections: Vec<SubunitProjection>,
}

pub fn search_graph(
    prompt: &str,
    skill_text: &[String],
    skill_subunits: &[Vec<Subunit>],
    candidate_indices: &[usize],
    max_subunits_per_skill: usize,
) -> Vec<GraphHit> {
    let prompt_tokens = tokenize(prompt);

    candidate_indices
        .iter()
        .filter_map(|skill_index| {
            let text = skill_text.get(*skill_index)?;
            let subunits = skill_subunits.get(*skill_index)?;

            let lexical_score = token_overlap_score(&prompt_tokens, &tokenize(text));
            let mut projections: Vec<SubunitProjection> = subunits
                .iter()
                .cloned()
                .map(|subunit| {
                    let relevance = token_overlap_score(
                        &prompt_tokens,
                        &tokenize(&format!(
                            "{} {}",
                            subunit.title.to_lowercase(),
                            subunit.content.to_lowercase()
                        )),
                    );
                    SubunitProjection { subunit, relevance }
                })
                .collect();

            projections.sort_by(|left, right| right.relevance.total_cmp(&left.relevance));
            projections.truncate(max_subunits_per_skill);

            Some(GraphHit {
                skill_index: *skill_index,
                lexical_score,
                projections,
            })
        })
        .collect()
}

pub(crate) fn tokenize(input: &str) -> BTreeSet<String> {
    input
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.trim().to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
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
    use domain::{DomainId, LifecycleStatus, SubunitType};

    use super::*;

    #[test]
    fn search_graph_projects_relevant_subunits() {
        let subunit = Subunit {
            id: DomainId::new_unchecked("sub-1"),
            skill_id: DomainId::new_unchecked("skill-1"),
            kind: SubunitType::Procedure,
            title: "Read a file".to_owned(),
            content: "Use std::fs::read_to_string".to_owned(),
            lifecycle: LifecycleStatus::Active,
        };

        let hits = search_graph(
            "how to read file",
            &["read files in rust".to_owned()],
            &[vec![subunit]],
            &[0],
            3,
        );

        assert_eq!(hits.len(), 1);
        assert!(hits[0].lexical_score > 0.0);
        assert!(hits[0].projections[0].relevance > 0.0);
    }
}
