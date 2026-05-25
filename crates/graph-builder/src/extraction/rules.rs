use std::path::Path;

use domain::SubunitType;

use crate::extraction::ExtractedSubunit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralExtraction {
    pub skill_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<ExtractedSubunit>,
}

/// Applies deterministic markdown rules to extract stable graph subunits.
pub fn extract_structural_subunits(path: &Path, markdown: &str) -> StructuralExtraction {
    let mut skill_name = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("unnamed-skill")
        .to_owned();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut subunits = Vec::new();
    let mut current_kind = SubunitType::Summary;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            if !title.trim().is_empty() {
                skill_name = title.trim().to_owned();
            }
            continue;
        }

        if let Some(tag_line) = trimmed.strip_prefix("tags:") {
            tags.extend(
                tag_line
                    .split(',')
                    .map(str::trim)
                    .filter(|candidate| !candidate.is_empty())
                    .map(str::to_owned),
            );
            continue;
        }

        match trimmed.to_ascii_lowercase().as_str() {
            "## procedures" => current_kind = SubunitType::Procedure,
            "## conventions" => current_kind = SubunitType::Convention,
            "## assets" => current_kind = SubunitType::Asset,
            "## evidence" => current_kind = SubunitType::Evidence,
            "## summary" => current_kind = SubunitType::Summary,
            _ => {
                if description.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
                    description = trimmed.to_owned();
                }
                if let Some(content) = trimmed.strip_prefix("- ") {
                    subunits.push(ExtractedSubunit {
                        kind: current_kind,
                        title: format!("{current_kind:?} note"),
                        content: content.trim().to_owned(),
                    });
                }
            }
        }
    }

    if description.is_empty() {
        description = "No description provided".to_owned();
    }

    StructuralExtraction {
        skill_name,
        description,
        tags,
        subunits,
    }
}
