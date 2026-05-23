use crate::rescue::CompiledSkillContext;

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
            }],
        );

        assert!(markdown.contains("# Compiled Context"));
        assert!(markdown.contains("### Highlights"));
        assert!(markdown.contains("### Rescue cues"));
    }
}
