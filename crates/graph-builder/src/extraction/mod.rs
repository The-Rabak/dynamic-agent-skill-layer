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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillExtraction {
    pub skill_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<ExtractedSubunit>,
    pub used_ollama_fallback: bool,
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
        };

        let extraction = finalize_extraction(structural, "markdown", |_| {
            panic!("fallback extractor should not run for sufficient structural extraction")
        });

        assert!(!extraction.used_ollama_fallback);
        assert_eq!(extraction.subunits.len(), 2);
    }
}
