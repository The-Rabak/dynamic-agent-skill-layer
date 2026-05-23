use std::collections::BTreeSet;

use domain::SubunitType;

#[derive(Debug, Clone, PartialEq)]
pub struct CompilerHighlightInput {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompilerSkillInput {
    pub name: String,
    pub description: String,
    pub score: f32,
    pub highlights: Vec<CompilerHighlightInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompilerRescueCueInput {
    pub source_skill: String,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSkillContext {
    pub name: String,
    pub description: String,
    pub score: String,
    pub highlights: Vec<String>,
    pub rescue_cues: Vec<String>,
}

pub fn attach_rescue_cues(
    skills: &[CompilerSkillInput],
    rescue_pool: &[CompilerRescueCueInput],
    max_rescue_per_skill: usize,
) -> Vec<CompiledSkillContext> {
    skills
        .iter()
        .map(|skill| {
            let skill_tokens = tokenize(&format!("{} {}", skill.name, skill.description));

            let mut ranked_rescue: Vec<(&CompilerRescueCueInput, f32)> = rescue_pool
                .iter()
                .filter(|cue| cue.source_skill != skill.name)
                .map(|cue| {
                    let cue_tokens = tokenize(&format!("{} {}", cue.title, cue.content));
                    let lexical = token_overlap_score(&skill_tokens, &cue_tokens);
                    let composite = 0.6 * cue.relevance + 0.4 * lexical;
                    (cue, composite)
                })
                .filter(|(_, score)| *score > 0.0)
                .collect();

            ranked_rescue.sort_by(|left, right| right.1.total_cmp(&left.1));
            ranked_rescue.truncate(max_rescue_per_skill);

            CompiledSkillContext {
                name: skill.name.clone(),
                description: skill.description.clone(),
                score: format!("{:.3}", skill.score),
                highlights: skill
                    .highlights
                    .iter()
                    .map(|highlight| {
                        format!(
                            "- [{}] {} — {}",
                            format!("{:?}", highlight.kind).to_lowercase(),
                            highlight.title,
                            highlight.content
                        )
                    })
                    .collect(),
                rescue_cues: ranked_rescue
                    .into_iter()
                    .map(|(cue, _)| {
                        format!(
                            "- from `{}`: {} — {}",
                            cue.source_skill, cue.title, cue.content
                        )
                    })
                    .collect(),
            }
        })
        .collect()
}

fn tokenize(input: &str) -> BTreeSet<String> {
    input
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.trim().to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

fn token_overlap_score(lhs: &BTreeSet<String>, rhs: &BTreeSet<String>) -> f32 {
    if lhs.is_empty() || rhs.is_empty() {
        return 0.0;
    }

    lhs.intersection(rhs).count() as f32 / lhs.len() as f32
}

#[cfg(test)]
mod tests {
    use domain::SubunitType;

    use super::*;

    #[test]
    fn attach_rescue_cues_joins_rescue_with_primary_skill() {
        let skills = vec![CompilerSkillInput {
            name: "rust-file-reading".to_owned(),
            description: "Read files from disk".to_owned(),
            score: 0.91,
            highlights: vec![CompilerHighlightInput {
                kind: SubunitType::Procedure,
                title: "Use std::fs::read_to_string".to_owned(),
                content: "Reads complete file contents".to_owned(),
                relevance: 0.9,
            }],
        }];

        let rescue_pool = vec![CompilerRescueCueInput {
            source_skill: "tokio-io".to_owned(),
            title: "Async file read".to_owned(),
            content: "Use tokio::fs::read_to_string for async tasks".to_owned(),
            relevance: 0.75,
        }];

        let compiled = attach_rescue_cues(&skills, &rescue_pool, 2);
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].rescue_cues.len(), 1);
    }
}
