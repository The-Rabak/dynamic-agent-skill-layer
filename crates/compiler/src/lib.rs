pub mod rescue;
pub mod template;

use domain::{ContextCompiler, ScoredSkill};

pub use rescue::{
    CompiledSkillContext, CompilerHighlightInput, CompilerRescueCueInput, CompilerSkillInput,
    attach_rescue_cues,
};
pub use template::render_markdown;

#[derive(Debug, Clone)]
pub struct TemplateOnlyCompiler {
    max_rescue_per_skill: usize,
}

impl Default for TemplateOnlyCompiler {
    fn default() -> Self {
        Self {
            max_rescue_per_skill: 2,
        }
    }
}

impl TemplateOnlyCompiler {
    pub fn new(max_rescue_per_skill: usize) -> Self {
        Self {
            max_rescue_per_skill,
        }
    }

    pub fn compile_with_rescue(
        &self,
        prompt: &str,
        skills: &[CompilerSkillInput],
        rescue_pool: &[CompilerRescueCueInput],
    ) -> String {
        let contexts = attach_rescue_cues(skills, rescue_pool, self.max_rescue_per_skill);
        render_markdown(prompt, &contexts)
    }
}

impl ContextCompiler for TemplateOnlyCompiler {
    fn compile(&self, skills: &[ScoredSkill], prompt: &str) -> String {
        if skills.is_empty() {
            return String::new();
        }

        let mut output = format!("# Compiled Context\n\nPrompt: `{prompt}`\n");
        for skill in skills {
            output.push_str(&format!(
                "\n## Skill: {}\n- Description: {}\n- Score: {:.3}\n",
                skill.skill.name, skill.skill.description, skill.score
            ));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus, SubunitType};

    use super::*;

    #[test]
    fn compile_with_rescue_includes_rescue_section() {
        let skill = CompilerSkillInput {
            name: "rust-file-reading".to_owned(),
            description: "Read files safely".to_owned(),
            score: 0.88,
            highlights: vec![CompilerHighlightInput {
                kind: SubunitType::Procedure,
                title: "Read file".to_owned(),
                content: "Use std::fs::read_to_string".to_owned(),
                relevance: 0.92,
            }],
            matched_scope: "global".to_owned(),
            rationale: vec!["semantic=0.800".to_owned(), "lexical=0.150".to_owned()],
        };
        let rescue_pool = vec![CompilerRescueCueInput {
            source_skill: "tokio-io".to_owned(),
            title: "Async fallback".to_owned(),
            content: "Prefer tokio::fs for async workloads".to_owned(),
            relevance: 0.77,
        }];

        let compiler = TemplateOnlyCompiler::default();
        let markdown = compiler.compile_with_rescue("read file in rust", &[skill], &rescue_pool);

        assert!(markdown.contains("## Skill: rust-file-reading"));
        assert!(markdown.contains("### Rescue cues"));
    }

    #[test]
    fn context_compiler_trait_outputs_summary_markdown() {
        let compiler = TemplateOnlyCompiler::default();
        let markdown = compiler.compile(
            &[ScoredSkill {
                skill: Skill {
                    id: DomainId::new_unchecked("skill-2"),
                    name: "io".to_owned(),
                    description: "I/O".to_owned(),
                    scope: ScopeType::Global,
                    status: SkillStatus::Ready,
                    lifecycle: LifecycleStatus::Active,
                    tags: vec![],
                    subunit_ids: vec![],
                    community_id: None,
                },
                score: 0.4,
                matched_scope: ScopeType::Global,
                rationale: vec![],
            }],
            "prompt",
        );

        assert!(markdown.contains("Prompt: `prompt`"));
    }
}
