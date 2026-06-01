use crate::merge::{MergeError, MergeSemanticVerifier, SkillSnapshot};

#[derive(Debug, Clone, Copy, Default)]
pub struct TextOverlapMergeSemanticVerifier {
    pub threshold: f32,
}

impl TextOverlapMergeSemanticVerifier {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl MergeSemanticVerifier for TextOverlapMergeSemanticVerifier {
    fn are_equivalent(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
    ) -> Result<bool, MergeError> {
        let left_text = left.semantic_text();
        let right_text = right.semantic_text();
        if left_text.trim().is_empty() || right_text.trim().is_empty() {
            return Ok(false);
        }
        let left_tokens: std::collections::HashSet<&str> = left_text.split_whitespace().collect();
        let right_tokens: std::collections::HashSet<&str> = right_text.split_whitespace().collect();
        if left_tokens.is_empty() || right_tokens.is_empty() {
            return Ok(false);
        }
        let intersection = left_tokens.intersection(&right_tokens).count();
        let union = left_tokens.union(&right_tokens).count();
        let jaccard = intersection as f32 / union as f32;
        Ok(jaccard >= self.threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn snapshot(name: &str, desc: &str, subunits: Vec<&str>) -> SkillSnapshot {
        SkillSnapshot {
            id: name.to_owned(),
            name: name.to_owned(),
            description: desc.to_owned(),
            scope: domain::ScopeType::Global,
            source_path: PathBuf::from("/tmp/SKILL.md"),
            tags: vec![],
            subunits: subunits.into_iter().map(String::from).collect(),
            embedding: vec![],
        }
    }

    #[test]
    fn identical_snapshots_pass() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let s = snapshot(
            "rust-auth",
            "Rust authentication flow",
            vec!["verify JWT", "check scope"],
        );
        assert!(verifier.are_equivalent(&s, &s).unwrap());
    }

    #[test]
    fn disjoint_snapshots_fail() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let a = snapshot("rust-auth", "Rust authentication flow", vec!["verify JWT"]);
        let b = snapshot(
            "py-http",
            "Python HTTP client patterns",
            vec!["use requests"],
        );
        assert!(!verifier.are_equivalent(&a, &b).unwrap());
    }

    #[test]
    fn empty_inputs_return_false() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let a = snapshot("a", "", vec![]);
        let b = snapshot("b", "something", vec![]);
        assert!(!verifier.are_equivalent(&a, &b).unwrap());
        assert!(!verifier.are_equivalent(&b, &a).unwrap());
    }
}
