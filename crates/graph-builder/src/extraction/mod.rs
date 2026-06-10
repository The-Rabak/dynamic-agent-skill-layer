pub mod dedup;
pub mod ollama_fallback;
pub mod rules;

use std::path::Path;

use domain::SubunitType;

use crate::extraction::{
    dedup::deduplicate_subunits,
    ollama_fallback::extract_with_ollama_fallback,
    rules::{StructuralExtraction, extract_structural_subunits},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSubunit {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
}

/// The result of extracting a single SKILL.md file.
///
/// Multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) carry WRITE-AHEAD source data for T04
/// (multi-view dense/BM25 embedding) and T05 (typed-edge proposals).  They are
/// always empty for body-only (no frontmatter) skills.  They never affect the
/// ℓ₁ embedding text (`name + description + tags`) or the subunit list.
///
/// `skill_type` and `evidence` are WRITE-AHEAD for T05 (typed-edge proposals).
/// `None`/empty when the frontmatter contains no `type`/`evidence` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillExtraction {
    pub skill_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<ExtractedSubunit>,
    pub used_ollama_fallback: bool,
    /// Task triggers where this skill applies. Empty for body-only skills.
    pub use_when: Vec<String>,
    /// Situations where this skill should NOT be applied. Empty for body-only skills.
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names the skill applies to. Empty for body-only skills.
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, models, or APIs. Empty for body-only skills.
    pub tools: Vec<String>,
    /// Verifier-critical constraints. Empty for body-only skills.
    pub invariants: Vec<String>,
    /// Prerequisites assumed by this skill. Empty for body-only skills.
    pub requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill. Empty for body-only skills.
    pub produces: Vec<String>,
    /// Taxonomy tag from the `type:` frontmatter key. `None` for body-only skills.
    /// WRITE-AHEAD for T05 typed-edge proposals.
    // TODO(T05): forward skill_type to typed-edge proposals / PersistedGraphSkillRecord
    pub skill_type: Option<String>,
    /// Provenance anchors from the `evidence:` frontmatter key. Empty for body-only skills.
    pub evidence: Vec<String>,
}

/// Extracts deterministic subunits first and only falls back when structural output is thin.
pub fn extract_skill(path: &Path, markdown: &str) -> SkillExtraction {
    let structural = extract_structural_subunits(path, markdown);
    finalize_extraction(structural, markdown, extract_with_ollama_fallback)
}

fn finalize_extraction(
    structural: StructuralExtraction,
    markdown: &str,
    fallback_extractor: impl FnOnce(&str) -> Vec<ExtractedSubunit>,
) -> SkillExtraction {
    let mut subunits = structural.subunits;
    let mut used_ollama_fallback = false;

    if subunits.len() < 2 {
        subunits.extend(fallback_extractor(markdown));
        used_ollama_fallback = true;
    }

    SkillExtraction {
        skill_name: structural.skill_name,
        description: structural.description,
        tags: structural.tags,
        subunits: deduplicate_subunits(&subunits),
        used_ollama_fallback,
        use_when: structural.use_when,
        avoid_when: structural.avoid_when,
        artifacts: structural.artifacts,
        tools: structural.tools,
        invariants: structural.invariants,
        requires: structural.requires,
        produces: structural.produces,
        // Taxonomy tag and provenance anchors — WRITE-AHEAD for T05.
        skill_type: structural.skill_type,
        evidence: structural.evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_extractor_runs_when_structural_output_is_thin() {
        let structural = StructuralExtraction {
            skill_name: "thin-skill".to_owned(),
            description: "thin".to_owned(),
            tags: vec!["tag".to_owned()],
            subunits: vec![ExtractedSubunit {
                kind: SubunitType::Summary,
                title: "Existing summary".to_owned(),
                content: "existing".to_owned(),
            }],
            use_when: vec![],
            avoid_when: vec![],
            artifacts: vec![],
            tools: vec![],
            invariants: vec![],
            requires: vec![],
            produces: vec![],
            skill_type: None,
            evidence: vec![],
        };

        let extraction = finalize_extraction(structural, "markdown", |_| {
            vec![ExtractedSubunit {
                kind: SubunitType::Evidence,
                title: "Fallback provider unavailable".to_owned(),
                content: "Ollama fallback unavailable: test".to_owned(),
            }]
        });

        assert!(extraction.used_ollama_fallback);
        assert_eq!(extraction.subunits.len(), 2);
        assert_eq!(extraction.subunits[1].kind, SubunitType::Evidence);
    }

    #[test]
    fn fallback_extractor_is_skipped_when_structural_output_is_sufficient() {
        let structural = StructuralExtraction {
            skill_name: "full-skill".to_owned(),
            description: "complete".to_owned(),
            tags: vec![],
            subunits: vec![
                ExtractedSubunit {
                    kind: SubunitType::Summary,
                    title: "Summary".to_owned(),
                    content: "summary".to_owned(),
                },
                ExtractedSubunit {
                    kind: SubunitType::Procedure,
                    title: "Procedure".to_owned(),
                    content: "procedure".to_owned(),
                },
            ],
            use_when: vec![],
            avoid_when: vec![],
            artifacts: vec![],
            tools: vec![],
            invariants: vec![],
            requires: vec![],
            produces: vec![],
            skill_type: None,
            evidence: vec![],
        };

        let extraction = finalize_extraction(structural, "markdown", |_| {
            panic!("fallback extractor should not run for sufficient structural extraction")
        });

        assert!(!extraction.used_ollama_fallback);
        assert_eq!(extraction.subunits.len(), 2);
    }
}
