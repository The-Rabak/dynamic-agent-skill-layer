use std::{collections::BTreeMap, env, path::PathBuf, process::Command};

use async_trait::async_trait;
use domain::{ScopeDescriptor, ScopeError, ScopeResolver, ScopeType};

#[derive(Debug, Clone)]
pub struct GitRootProjectResolver {
    start_dir: PathBuf,
}

impl GitRootProjectResolver {
    pub fn new(start_dir: impl Into<PathBuf>) -> Self {
        Self {
            start_dir: start_dir.into(),
        }
    }
}

#[async_trait]
impl ScopeResolver for GitRootProjectResolver {
    async fn resolve(&self) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.start_dir)
            .arg("rev-parse")
            .arg("--show-toplevel")
            .output()
            .map_err(|error| ScopeError::ResolverUnavailable(error.to_string()))?;

        if !output.status.success() {
            return Err(ScopeError::ResolverUnavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }

        let root = String::from_utf8(output.stdout)
            .map_err(|error| ScopeError::Unexpected(error.to_string()))?;
        let path = PathBuf::from(root.trim());

        if path.as_os_str().is_empty() {
            return Err(ScopeError::Unexpected(
                "git root resolver returned an empty path".to_owned(),
            ));
        }

        Ok(vec![ScopeDescriptor {
            scope_id: "project".to_owned(),
            scope_type: ScopeType::Project,
            paths: vec![path],
            config: BTreeMap::from([("resolver".to_owned(), "git".to_owned())]),
        }])
    }
}

#[derive(Debug, Clone)]
pub struct EnvPathGlobalResolver {
    env_var: String,
}

impl EnvPathGlobalResolver {
    pub fn new(env_var: impl Into<String>) -> Self {
        Self {
            env_var: env_var.into(),
        }
    }
}

impl Default for EnvPathGlobalResolver {
    fn default() -> Self {
        Self::new("SKILL_GLOBAL_PATHS")
    }
}

#[async_trait]
impl ScopeResolver for EnvPathGlobalResolver {
    async fn resolve(&self) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        let value = env::var(&self.env_var).map_err(|_| {
            ScopeError::InvalidConfiguration(format!("{} is not set", self.env_var))
        })?;

        let paths: Vec<PathBuf> = value
            .split(':')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect();

        if paths.is_empty() {
            return Err(ScopeError::InvalidConfiguration(format!(
                "{} must include at least one path",
                self.env_var
            )));
        }

        Ok(vec![ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            paths,
            config: BTreeMap::from([("resolver".to_owned(), "env".to_owned())]),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn env_resolver_requires_configured_paths() {
        let env_var = "INFRA_SCOPE_TEST_PATHS";
        let resolver = EnvPathGlobalResolver::new(env_var);

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var(env_var);
        }

        let error = resolver
            .resolve()
            .await
            .expect_err("unset global paths should fail");

        assert!(matches!(error, ScopeError::InvalidConfiguration(_)));
    }

    #[tokio::test]
    async fn env_resolver_parses_colon_delimited_paths() {
        let env_var = "INFRA_SCOPE_TEST_PATHS_PARSE";
        let resolver = EnvPathGlobalResolver::new(env_var);

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::set_var(env_var, "/skills/global:/skills/team");
        }

        let scopes = resolver
            .resolve()
            .await
            .expect("resolver should parse paths");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Global);
        assert_eq!(scopes[0].paths.len(), 2);

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var(env_var);
        }
    }
}
