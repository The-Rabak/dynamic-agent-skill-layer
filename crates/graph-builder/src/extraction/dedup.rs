use std::collections::HashSet;

use crate::extraction::ExtractedSubunit;

/// Removes duplicated extracted subunits by normalized title/content identity.
pub fn deduplicate_subunits(subunits: &[ExtractedSubunit]) -> Vec<ExtractedSubunit> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for subunit in subunits {
        let dedup_key = format!(
            "{}:{}:{}",
            format!("{:?}", subunit.kind).to_ascii_lowercase(),
            subunit.title.trim().to_ascii_lowercase(),
            subunit.content.trim().to_ascii_lowercase()
        );
        if seen.insert(dedup_key) {
            unique.push(subunit.clone());
        }
    }

    unique
}
