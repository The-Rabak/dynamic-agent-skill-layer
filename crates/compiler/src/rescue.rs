use std::collections::BTreeSet;

use domain::SubunitType;

#[derive(Debug, Clone, PartialEq)]
pub struct CompilerHighlightInput {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

/// Skill input for the compiler, including retrieval provenance for the
/// deterministic "why matched" section emitted by `render_markdown`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompilerSkillInput {
    pub name: String,
    pub description: String,
    pub score: f32,
    pub highlights: Vec<CompilerHighlightInput>,
    /// Scope that this skill matched (e.g. `"global"`, `"project"`).
    pub matched_scope: String,
    /// Rationale tokens from the retrieval engine (e.g. `"rrf=0.012"`,
    /// `"semantic=0.312"`, `"lexical=0.100"`). Empty for legacy callers.
    pub rationale: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompilerRescueCueInput {
    pub source_skill: String,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

/// Fully compiled skill context ready for markdown rendering.
///
/// `match_reason` is the deterministic one-line provenance string emitted in
/// the `### Why These Skills` section. It is built from `matched_scope`, the
/// score bucket, and the top rationale tokens — no LLM involvement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSkillContext {
    pub name: String,
    pub description: String,
    pub score: String,
    pub highlights: Vec<String>,
    pub rescue_cues: Vec<String>,
    /// Deterministic match-reason string, e.g.
    /// `"scope=global | bucket=medium | semantic=0.312 | lexical=0.100"`.
    pub match_reason: String,
}

/// Attaches ranked rescue cues to each skill and computes the deterministic
/// match-reason string for the `### Why These Skills` section.
///
/// The match-reason is built purely from the retrieval output fields already
/// present in `CompilerSkillInput`: scope, score bucket, and rationale tokens.
/// No LLM call is made; the output is reproducible for the same inputs.
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

            let match_reason = build_match_reason(skill);

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
                match_reason,
            }
        })
        .collect()
}

/// Score bucket labels for the deterministic match-reason section.
///
/// Boundaries are aligned with the default `relevance_threshold` (0.450,
/// `retrieval/orchestrator.rs`) so the bucket label honestly reflects whether
/// a skill cleared the relevance floor by a lot (`high`, ≥ 0.40 RRF), a little
/// (`medium`, ≥ 0.20 RRF), or fell below it (`low`). Note that these bucket
/// boundaries are coarser than the 0.450 floor — a "medium" skill may still be
/// admitted by the rescue pool logic; the bucket is a label, not a gate.
/// No LLM involvement — purely arithmetic on the fused RRF score.
fn score_bucket(score: f32) -> &'static str {
    if score >= 0.40 {
        "high"
    } else if score >= 0.20 {
        "medium"
    } else {
        "low"
    }
}

/// Builds a compact deterministic match-reason string from the retrieval fields
/// already present in `CompilerSkillInput`.
///
/// Format: `scope=<scope> | bucket=<high|medium|low> | <rationale tokens>`.
/// Rationale tokens are emitted as-is (e.g. `semantic=0.312 | lexical=0.100`).
/// Unknown/empty rationale is omitted gracefully.
fn build_match_reason(skill: &CompilerSkillInput) -> String {
    let bucket = score_bucket(skill.score);
    let mut parts = vec![
        format!("scope={}", skill.matched_scope),
        format!("bucket={bucket}"),
    ];

    // Include the most informative rationale tokens (semantic + lexical) so
    // the agent can see why this skill scored above threshold. RRF score is
    // less legible than the raw component scores, so prefer semantic/lexical.
    for token in &skill.rationale {
        if token.starts_with("semantic=") || token.starts_with("lexical=") {
            parts.push(token.clone());
        }
    }

    parts.join(" | ")
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
            matched_scope: "global".to_owned(),
            rationale: vec!["semantic=0.850".to_owned(), "lexical=0.200".to_owned()],
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
        // match_reason must carry scope, bucket, and rationale tokens.
        assert!(
            compiled[0].match_reason.contains("scope=global"),
            "match_reason should include scope: {}",
            compiled[0].match_reason
        );
        assert!(
            compiled[0].match_reason.contains("bucket="),
            "match_reason should include score bucket: {}",
            compiled[0].match_reason
        );
        assert!(
            compiled[0].match_reason.contains("semantic=0.850"),
            "match_reason should include semantic score: {}",
            compiled[0].match_reason
        );
    }

    /// Proves the deterministic match-reason score bucket boundaries.
    #[test]
    fn score_bucket_reflects_threshold_alignment() {
        // Score ≥ 0.40 → high
        let high = CompilerSkillInput {
            name: "high-skill".to_owned(),
            description: "desc".to_owned(),
            score: 0.45,
            highlights: vec![],
            matched_scope: "global".to_owned(),
            rationale: vec![],
        };
        // Score [0.20, 0.40) → medium
        let medium = CompilerSkillInput {
            name: "medium-skill".to_owned(),
            description: "desc".to_owned(),
            score: 0.25,
            highlights: vec![],
            matched_scope: "project".to_owned(),
            rationale: vec![],
        };
        // Score < 0.20 → low
        let low = CompilerSkillInput {
            name: "low-skill".to_owned(),
            description: "desc".to_owned(),
            score: 0.10,
            highlights: vec![],
            matched_scope: "global".to_owned(),
            rationale: vec![],
        };

        let compiled = attach_rescue_cues(&[high, medium, low], &[], 0);
        assert!(
            compiled[0].match_reason.contains("bucket=high"),
            "{}",
            compiled[0].match_reason
        );
        assert!(
            compiled[1].match_reason.contains("bucket=medium"),
            "{}",
            compiled[1].match_reason
        );
        assert!(
            compiled[2].match_reason.contains("bucket=low"),
            "{}",
            compiled[2].match_reason
        );
    }
}
