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
    }

    // Aggregated match-reason section: one heading, one labeled bullet per skill.
    // Emitted after the per-skill blocks so each block stays self-contained while
    // the rationale for the full selection is readable in one place.
    markdown.push_str("### Why These Skills\n");
    for skill in skills {
        markdown.push_str(&format!("- {}: {}\n", skill.name, skill.match_reason));
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
        // Heading must appear exactly once even for a single skill.
        assert_eq!(markdown.matches("### Why These Skills").count(), 1);
        // Bullet must be labeled by skill name.
        assert!(markdown.contains("- rust-file-reading: scope=global"));
    }

    #[test]
    fn render_markdown_why_these_skills_heading_appears_exactly_once_for_multiple_skills() {
        let skills = vec![
            CompiledSkillContext {
                name: "skill-alpha".to_owned(),
                description: "Alpha skill".to_owned(),
                score: "0.900".to_owned(),
                highlights: vec![],
                rescue_cues: vec![],
                match_reason: "scope=global | bucket=high | semantic=0.900".to_owned(),
            },
            CompiledSkillContext {
                name: "skill-beta".to_owned(),
                description: "Beta skill".to_owned(),
                score: "0.750".to_owned(),
                highlights: vec![],
                rescue_cues: vec![],
                match_reason: "scope=local | bucket=medium | semantic=0.750".to_owned(),
            },
        ];
        let markdown = render_markdown("test prompt", &skills);

        // Heading must appear exactly once regardless of skill count.
        assert_eq!(
            markdown.matches("### Why These Skills").count(),
            1,
            "heading appeared more than once:\n{markdown}"
        );
        // One labeled bullet per skill must be present.
        assert!(
            markdown.contains("- skill-alpha: scope=global"),
            "missing skill-alpha bullet:\n{markdown}"
        );
        assert!(
            markdown.contains("- skill-beta: scope=local"),
            "missing skill-beta bullet:\n{markdown}"
        );
    }
}
