use std::collections::HashMap;

use domain::ScopeType;

use crate::graph::build::BuiltSkill;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityAssignment {
    pub community_name: String,
    pub skill_ids: Vec<String>,
    pub scope: ScopeType,
}

/// Assigns deterministic communities so repeated rebuilds remain stable.
pub fn assign_communities(skills: &[BuiltSkill]) -> Vec<CommunityAssignment> {
    let mut grouped: HashMap<(String, String), Vec<String>> = HashMap::new();
    for skill in skills {
        let anchor = skill
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| "untagged".to_owned());
        grouped
            .entry((format!("{:?}", skill.scope_type), anchor))
            .or_default()
            .push(skill.id.clone());
    }

    let mut assignments = grouped
        .into_iter()
        .map(|((scope_key, anchor), mut skill_ids)| {
            skill_ids.sort();
            let scope = match scope_key.as_str() {
                "Project" => ScopeType::Project,
                "Global" => ScopeType::Global,
                _ => ScopeType::Team,
            };
            CommunityAssignment {
                community_name: format!("{scope:?}-{}", anchor.to_ascii_lowercase()),
                skill_ids,
                scope,
            }
        })
        .collect::<Vec<_>>();
    assignments.sort_by(|left, right| left.community_name.cmp(&right.community_name));
    assignments
}
