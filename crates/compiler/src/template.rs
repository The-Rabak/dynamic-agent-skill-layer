use crate::rescue::CompiledSkillContext;

/// Renders compiled skill contexts into a markdown string for agent injection.
///
/// Each skill section includes:
/// - `### Highlights` — top matching subunit excerpts.
/// - `### Rescue cues` — cross-skill cues ranked by relevance + lexical overlap.
/// - `### Why These Skills` — compact deterministic match-reason (scope + score
///   bucket + retrieval component scores). No LLM involvement; reproducible for
///   the same retrieval output.
pub fn render_markdown(prompt: &str, skills: &[CompiledSkillContext]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut markdown = format!("# Compiled Context\n\nPrompt: `{prompt}`\n");
    for skill in skills {
        markdown.push_str(&format!(
            "\n## Skill: {}\n- Description: {}\n- Score: {}\n",
            skill.name, skill.description, skill.score
        ));

        if !skill.highlights.is_empty() {
            markdown.push_str("### Highlights\n");
            for highlight in &skill.highlights {
                markdown.push_str(highlight);
                markdown.push('\n');
            }
        }

        if !skill.rescue_cues.is_empty() {
            markdown.push_str("### Rescue cues\n");
            for cue in &skill.rescue_cues {
                markdown.push_str(cue);
                markdown.push('\n');
            }
        }

        // Deterministic match-reason: always present so an agent can audit why
        // this skill was injected. Kept compact (one line per skill) to minimise
        // context budget impact. Content is purely derived from retrieval output.
        markdown.push_str(&format!("### Why These Skills\n- {}\n", skill.match_reason));
    }

    markdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_markdown_outputs_sections() {
        let markdown = render_markdown(
            "read file in rust",
            &[CompiledSkillContext {
                name: "rust-file-reading".to_owned(),
                description: "Read files".to_owned(),
                score: "0.900".to_owned(),
                highlights: vec!["- [procedure] Read file — Use fs".to_owned()],
                rescue_cues: vec!["- from `tokio-io`: Async read — Use tokio".to_owned()],
                match_reason: "scope=global | bucket=high | semantic=0.850".to_owned(),
            }],
        );

        assert!(markdown.contains("# Compiled Context"));
        assert!(markdown.contains("### Highlights"));
        assert!(markdown.contains("### Rescue cues"));
        assert!(markdown.contains("### Why These Skills"));
        assert!(markdown.contains("scope=global"));
    }
}
