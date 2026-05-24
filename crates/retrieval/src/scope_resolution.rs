use std::sync::Arc;

use domain::{ScopeDescriptor, ScopeResolver, ScopeType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeResolutionOutcome {
    pub project: Option<ScopeDescriptor>,
    pub global: Option<ScopeDescriptor>,
    pub degraded_scopes: Vec<String>,
    pub reason_codes: Vec<String>,
    pub configured_scopes: Vec<String>,
}

impl ScopeResolutionOutcome {
    pub fn resolved_scopes(&self) -> Vec<ScopeDescriptor> {
        let mut scopes = Vec::new();
        if let Some(project) = &self.project {
            scopes.push(project.clone());
        }
        if let Some(global) = &self.global {
            scopes.push(global.clone());
        }
        scopes
    }

    pub fn scopes_considered(&self) -> Vec<String> {
        let mut scopes = Vec::new();
        if self.project.is_some() || self.degraded_scopes.iter().any(|scope| scope == "project") {
            scopes.push("project".to_owned());
        }
        if self.global.is_some() || self.degraded_scopes.iter().any(|scope| scope == "global") {
            scopes.push("global".to_owned());
        }

        if scopes.is_empty() {
            self.configured_scopes.clone()
        } else {
            scopes
        }
    }
}

#[derive(Clone)]
pub struct DualScopeResolver {
    project_resolver: Arc<dyn ScopeResolver>,
    global_resolver: Arc<dyn ScopeResolver>,
}

impl DualScopeResolver {
    pub fn new(
        project_resolver: Arc<dyn ScopeResolver>,
        global_resolver: Arc<dyn ScopeResolver>,
    ) -> Self {
        Self {
            project_resolver,
            global_resolver,
        }
    }

    pub fn configured_scope_ids(&self) -> Vec<String> {
        vec!["project".to_owned(), "global".to_owned()]
    }

    pub async fn resolve(&self, repo_path: Option<&str>) -> ScopeResolutionOutcome {
        let (project_result, global_result) = tokio::join!(
            self.project_resolver.resolve(repo_path),
            self.global_resolver.resolve(None)
        );

        let mut degraded_scopes = Vec::new();
        let mut reason_codes = Vec::new();

        let project = match project_result {
            Ok(scopes) => scopes
                .into_iter()
                .find(|scope| scope.scope_type == ScopeType::Project),
            Err(_) => {
                degraded_scopes.push("project".to_owned());
                reason_codes.push("project_scope_resolution_failed".to_owned());
                None
            }
        };

        if project.is_none() {
            degraded_scopes.push("project".to_owned());
            reason_codes.push("project_scope_unresolved".to_owned());
        }

        let global = match global_result {
            Ok(scopes) => scopes
                .into_iter()
                .find(|scope| scope.scope_type == ScopeType::Global),
            Err(_) => {
                degraded_scopes.push("global".to_owned());
                reason_codes.push("global_scope_resolution_failed".to_owned());
                None
            }
        };

        if global.is_none() {
            degraded_scopes.push("global".to_owned());
            reason_codes.push("global_scope_unresolved".to_owned());
        }

        dedupe_in_place(&mut degraded_scopes);
        dedupe_in_place(&mut reason_codes);

        ScopeResolutionOutcome {
            project,
            global,
            degraded_scopes,
            reason_codes,
            configured_scopes: self.configured_scope_ids(),
        }
    }
}

fn dedupe_in_place(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use async_trait::async_trait;
    use domain::{ScopeError, ScopeResolver};

    use super::*;

    #[derive(Clone)]
    struct StubResolver {
        result: Result<Vec<ScopeDescriptor>, ScopeError>,
    }

    #[async_trait]
    impl ScopeResolver for StubResolver {
        async fn resolve(
            &self,
            _repo_path: Option<&str>,
        ) -> Result<Vec<ScopeDescriptor>, ScopeError> {
            self.result.clone()
        }
    }

    fn scope(scope_id: &str, scope_type: ScopeType) -> ScopeDescriptor {
        ScopeDescriptor {
            scope_id: scope_id.to_owned(),
            scope_type,
            paths: vec![PathBuf::from("/workspace")],
            config: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn resolves_project_and_global_scopes_concurrently() {
        let resolver = DualScopeResolver::new(
            Arc::new(StubResolver {
                result: Ok(vec![scope("project", ScopeType::Project)]),
            }),
            Arc::new(StubResolver {
                result: Ok(vec![scope("global", ScopeType::Global)]),
            }),
        );

        let outcome = resolver.resolve(None).await;

        assert!(outcome.degraded_scopes.is_empty());
        assert!(outcome.reason_codes.is_empty());
        assert_eq!(outcome.scopes_considered(), vec!["project", "global"]);
        assert_eq!(outcome.resolved_scopes().len(), 2);
    }

    #[tokio::test]
    async fn marks_scope_as_degraded_when_resolution_fails() {
        let resolver = DualScopeResolver::new(
            Arc::new(StubResolver {
                result: Ok(vec![scope("project", ScopeType::Project)]),
            }),
            Arc::new(StubResolver {
                result: Err(ScopeError::ResolverUnavailable("env missing".to_owned())),
            }),
        );

        let outcome = resolver.resolve(None).await;

        assert_eq!(outcome.degraded_scopes, vec!["global"]);
        assert!(
            outcome
                .reason_codes
                .iter()
                .any(|code| code == "global_scope_resolution_failed")
        );
        assert_eq!(outcome.scopes_considered(), vec!["project", "global"]);
    }
}
