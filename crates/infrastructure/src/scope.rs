use std::{collections::BTreeMap, env, fs, path::PathBuf};

use async_trait::async_trait;
use domain::{ScopeDescriptor, ScopeError, ScopeResolver, ScopeType};
use tokio::process::Command;

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
            .await
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
    allowed_roots_env_var: String,
}

impl EnvPathGlobalResolver {
    pub fn new(env_var: impl Into<String>) -> Self {
        Self {
            env_var: env_var.into(),
            allowed_roots_env_var: "SKILL_GLOBAL_ALLOWED_ROOTS".to_owned(),
        }
    }

    pub fn with_allowed_roots_env_var(mut self, allowed_roots_env_var: impl Into<String>) -> Self {
        self.allowed_roots_env_var = allowed_roots_env_var.into();
        self
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
        let allowed_roots = env::var(&self.allowed_roots_env_var).map_err(|_| {
            ScopeError::InvalidConfiguration(format!("{} is not set", self.allowed_roots_env_var))
        })?;

        let paths: Vec<PathBuf> = value
            .split(':')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect();
        let roots: Vec<PathBuf> = allowed_roots
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
        if roots.is_empty() {
            return Err(ScopeError::InvalidConfiguration(format!(
                "{} must include at least one allowed root path",
                self.allowed_roots_env_var
            )));
        }

        let canonical_roots: Vec<PathBuf> = roots
            .into_iter()
            .map(|root| {
                if !root.is_absolute() {
                    return Err(ScopeError::InvalidConfiguration(format!(
                        "allowed root `{}` must be absolute",
                        root.display()
                    )));
                }

                fs::canonicalize(&root).map_err(|error| {
                    ScopeError::InvalidConfiguration(format!(
                        "allowed root `{}` is invalid: {}",
                        root.display(),
                        error
                    ))
                })
            })
            .collect::<Result<_, _>>()?;

        let validated_paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|path| {
                if !path.is_absolute() {
                    return Err(ScopeError::InvalidConfiguration(format!(
                        "global scope path `{}` must be absolute",
                        path.display()
                    )));
                }

                let canonical = fs::canonicalize(&path).map_err(|error| {
                    ScopeError::InvalidConfiguration(format!(
                        "global scope path `{}` is invalid: {}",
                        path.display(),
                        error
                    ))
                })?;
                let in_bounds = canonical_roots
                    .iter()
                    .any(|root| canonical.starts_with(root));

                if !in_bounds {
                    return Err(ScopeError::InvalidConfiguration(format!(
                        "global scope path `{}` is outside allowed roots",
                        canonical.display()
                    )));
                }

                Ok(canonical)
            })
            .collect::<Result<_, _>>()?;

        Ok(vec![ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            paths: validated_paths,
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
        let allowed_roots_var = "INFRA_SCOPE_ALLOWED_ROOTS";
        let resolver =
            EnvPathGlobalResolver::new(env_var).with_allowed_roots_env_var(allowed_roots_var);

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var(env_var);
            env::remove_var(allowed_roots_var);
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
        let allowed_roots_var = "INFRA_SCOPE_ALLOWED_ROOTS_PARSE";
        let resolver =
            EnvPathGlobalResolver::new(env_var).with_allowed_roots_env_var(allowed_roots_var);
        let sandbox = env::temp_dir().join(format!(
            "infra-scope-parse-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let allowed_root = sandbox.join("allowed");
        let global_one = allowed_root.join("global");
        let global_two = allowed_root.join("team");
        std::fs::create_dir_all(&global_one).expect("global one dir should be creatable");
        std::fs::create_dir_all(&global_two).expect("global two dir should be creatable");

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::set_var(
                env_var,
                format!("{}:{}", global_one.display(), global_two.display()),
            );
            env::set_var(allowed_roots_var, allowed_root.display().to_string());
        }

        let scopes = resolver
            .resolve()
            .await
            .expect("resolver should parse paths");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Global);
        assert_eq!(scopes[0].paths.len(), 2);
        assert!(scopes[0].paths[0].starts_with(&allowed_root));
        assert!(scopes[0].paths[1].starts_with(&allowed_root));

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var(env_var);
            env::remove_var(allowed_roots_var);
        }
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[tokio::test]
    async fn env_resolver_rejects_paths_outside_allowed_roots() {
        let env_var = "INFRA_SCOPE_TEST_PATHS_OOB";
        let allowed_roots_var = "INFRA_SCOPE_ALLOWED_ROOTS_OOB";
        let resolver =
            EnvPathGlobalResolver::new(env_var).with_allowed_roots_env_var(allowed_roots_var);
        let sandbox = env::temp_dir().join(format!(
            "infra-scope-oob-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let allowed_root = sandbox.join("allowed");
        let blocked_root = sandbox.join("blocked");
        let blocked_path = blocked_root.join("global");
        std::fs::create_dir_all(&allowed_root).expect("allowed root should be creatable");
        std::fs::create_dir_all(&blocked_path).expect("blocked path should be creatable");

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::set_var(env_var, blocked_path.display().to_string());
            env::set_var(allowed_roots_var, allowed_root.display().to_string());
        }

        let error = resolver
            .resolve()
            .await
            .expect_err("resolver should reject out-of-bounds path");

        assert!(matches!(error, ScopeError::InvalidConfiguration(_)));

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var(env_var);
            env::remove_var(allowed_roots_var);
        }
        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }
}
