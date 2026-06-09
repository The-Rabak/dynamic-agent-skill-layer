use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use domain::{ScopeDescriptor, ScopeError, ScopeResolver, ScopeType};
use tokio::process::Command;

/// Filesystem-walk project-root resolver.
///
/// Starts at `start_dir` (or the `repo_path` argument when provided) and
/// walks up toward the filesystem root looking for a `.git` directory **or**
/// a path named by the `SKILL_PROJECT_MARKER` environment variable.  The
/// first ancestor directory that contains either marker is returned as the
/// project root.
///
/// This resolver works without a `git` binary in `PATH`, making it suitable
/// for musl-compiled static containers and any other environment where
/// spawning a subprocess is either unavailable or undesirable.
///
/// Fallback: when neither `.git` nor the custom marker is found anywhere up
/// the tree, returns [`ScopeError::ResolverUnavailable`] so callers can fall
/// back to global-scope-only context (unchanged behaviour compared to
/// [`GitRootProjectResolver`] in a git-free environment).
#[derive(Debug, Clone)]
pub struct FsMarkerProjectResolver {
    start_dir: PathBuf,
}

impl FsMarkerProjectResolver {
    pub fn new(start_dir: impl Into<PathBuf>) -> Self {
        Self {
            start_dir: start_dir.into(),
        }
    }

    /// Builds the project [`ScopeDescriptor`] for a resolved root, canonicalizing
    /// the path (best-effort) so it aligns with the canonicalized `source_paths`
    /// the read path matches against via `starts_with`.
    fn descriptor(root: PathBuf) -> ScopeDescriptor {
        let canonical = fs::canonicalize(&root).unwrap_or(root);
        ScopeDescriptor {
            scope_id: "project".to_owned(),
            scope_type: ScopeType::Project,
            paths: vec![canonical],
            config: BTreeMap::from([("resolver".to_owned(), "fs-marker".to_owned())]),
        }
    }

    /// Walks ancestor directories of `start` looking for `.git` or the path
    /// named by `SKILL_PROJECT_MARKER`.  Returns the first matching root.
    fn walk_to_project_root(start: &Path) -> Option<PathBuf> {
        let custom_marker = env::var("SKILL_PROJECT_MARKER").ok();
        let mut current = start;

        loop {
            let has_git = current.join(".git").exists();
            let has_custom = custom_marker
                .as_deref()
                .map(|name| current.join(name).exists())
                .unwrap_or(false);

            if has_git || has_custom {
                return Some(current.to_owned());
            }

            match current.parent() {
                Some(parent) => current = parent,
                None => return None,
            }
        }
    }
}

#[async_trait]
impl ScopeResolver for FsMarkerProjectResolver {
    async fn resolve(&self, repo_path: Option<&str>) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        // 1. A per-request `repo_path` that actually resolves wins (host/dev usage
        //    where the agent knows its cwd): walk up from it for a `.git`/marker.
        //    If it does NOT resolve in this environment (e.g. a host path that does
        //    not exist inside the container), fall through to the configured root
        //    rather than erroring immediately — that fallback is exactly what makes
        //    the container usable when the client sends a host-only path.
        if let Some(repo_path) = repo_path
            && let Some(root) = Self::walk_to_project_root(&PathBuf::from(repo_path))
        {
            return Ok(vec![Self::descriptor(root)]);
        }

        // 2. An explicitly configured `SKILL_PROJECT_ROOT` is an operator
        //    DECLARATION of the project scope root — return it directly, no marker
        //    walk. This is the containerized case (issue #154): the working
        //    directory is `/` with no `.git`/marker, so the walk would always yield
        //    `ResolverUnavailable` → degraded. Setting `SKILL_PROJECT_ROOT` to the
        //    mounted project root makes `compile_context` resolve project scope to
        //    `ok`. Fail loud (named reason) when it is set but missing, rather than
        //    silently falling through to a `/`-walk that always degrades.
        if let Ok(configured) = env::var("SKILL_PROJECT_ROOT") {
            let configured = configured.trim();
            if !configured.is_empty() {
                let root = PathBuf::from(configured);
                if root.is_dir() {
                    return Ok(vec![Self::descriptor(root)]);
                }
                return Err(ScopeError::ResolverUnavailable(format!(
                    "SKILL_PROJECT_ROOT is set to `{}` but that directory does not exist",
                    root.display()
                )));
            }
        }

        // 3. A `repo_path` was supplied but did not resolve and no configured root
        //    is available — report the honest per-request failure.
        if let Some(repo_path) = repo_path {
            return Err(ScopeError::ResolverUnavailable(format!(
                "no .git directory or SKILL_PROJECT_MARKER found walking up from \
                 request repo_path `{repo_path}`, and SKILL_PROJECT_ROOT is not set"
            )));
        }

        // 4. Default (host/dev with no per-request path): walk up from the
        //    configured `start_dir` (typically the process working directory)
        //    looking for a `.git`/marker.
        let root = Self::walk_to_project_root(&self.start_dir).ok_or_else(|| {
            ScopeError::ResolverUnavailable(format!(
                "no .git directory or SKILL_PROJECT_MARKER found walking up from `{}`",
                self.start_dir.display()
            ))
        })?;

        Ok(vec![Self::descriptor(root)])
    }
}

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
    async fn resolve(&self, repo_path: Option<&str>) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        let start_dir = repo_path
            .map(PathBuf::from)
            .unwrap_or_else(|| self.start_dir.clone());
        let output = Command::new("git")
            .arg("-C")
            .arg(start_dir)
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
    async fn resolve(&self, _repo_path: Option<&str>) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        let value = env::var(&self.env_var).map_err(|_| {
            ScopeError::InvalidConfiguration(format!("{} is not set", self.env_var))
        })?;
        let allowed_roots = env::var(&self.allowed_roots_env_var).map_err(|_| {
            ScopeError::InvalidConfiguration(format!("{} is not set", self.allowed_roots_env_var))
        })?;

        let paths: Vec<PathBuf> = split_paths(&value).into_iter().map(PathBuf::from).collect();
        let roots: Vec<PathBuf> = split_paths(&allowed_roots)
            .into_iter()
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

fn split_paths(value: &str) -> Vec<String> {
    value
        .split([':', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Serializes tests that mutate process-global scope env vars
    /// (`SKILL_PROJECT_ROOT`, `SKILL_PROJECT_MARKER`) so parallel test threads in
    /// this binary never observe each other's env mutations.
    ///
    /// Uses `tokio::sync::Mutex` (async-aware) so the guard can be held across
    /// `.await` points without triggering `clippy::await_holding_lock`.
    static SCOPE_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn lock_scope_env() -> tokio::sync::MutexGuard<'static, ()> {
        SCOPE_ENV_LOCK.lock().await
    }

    // --- FsMarkerProjectResolver tests (Red phase: struct does not exist yet) ---

    #[tokio::test]
    async fn fs_marker_resolver_finds_git_dir_walking_up_from_nested_child() {
        let _env = lock_scope_env().await;
        // The repo has a .git at its root. Start from a deeply nested dir and
        // expect the resolver to walk up and return the repo root.
        let nested = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let expected_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root should canonicalize");
        let resolver = FsMarkerProjectResolver::new(nested);

        let scopes = resolver
            .resolve(None)
            .await
            .expect("fs-marker resolver should find .git root");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Project);
        assert_eq!(scopes[0].scope_id, "project");
        assert_eq!(
            scopes[0].config.get("resolver").map(String::as_str),
            Some("fs-marker")
        );
        assert_eq!(
            scopes[0].paths[0]
                .canonicalize()
                .expect("resolved path should canonicalize"),
            expected_root
        );
    }

    #[tokio::test]
    async fn fs_marker_resolver_returns_unavailable_when_no_git_or_marker_exists() {
        let _env = lock_scope_env().await;
        // A temp dir with no .git anywhere up the tree should yield ResolverUnavailable.
        // We create a directory whose parents are all within /tmp, which has no .git.
        let sandbox = std::env::temp_dir().join(format!(
            "fs-marker-no-git-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");

        let resolver = FsMarkerProjectResolver::new(sandbox.clone());

        let error = resolver
            .resolve(None)
            .await
            .expect_err("resolver should return unavailable when no .git or marker found");

        assert!(
            matches!(error, ScopeError::ResolverUnavailable(_)),
            "expected ResolverUnavailable, got {error:?}"
        );

        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
    }

    #[tokio::test]
    async fn fs_marker_resolver_honors_skill_project_marker_env_when_git_absent() {
        let _env = lock_scope_env().await;
        // A sandbox dir with no .git but with a custom marker file named by SKILL_PROJECT_MARKER
        // should resolve to the directory that contains the marker.
        let sandbox = std::env::temp_dir().join(format!(
            "fs-marker-custom-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let nested = sandbox.join("sub").join("deeper");
        let marker_name = ".skillroot";
        std::fs::create_dir_all(&nested).expect("nested sandbox should be creatable");
        // Create the marker at the sandbox root (not in nested).
        std::fs::write(sandbox.join(marker_name), "marker").expect("marker should be writable");

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::set_var("SKILL_PROJECT_MARKER", marker_name);
        }

        let resolver = FsMarkerProjectResolver::new(nested.clone());
        let scopes = resolver
            .resolve(None)
            .await
            .expect("resolver should find custom marker");

        // SAFETY: test-scoped environment cleanup.
        unsafe {
            env::remove_var("SKILL_PROJECT_MARKER");
        }

        std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Project);
        assert_eq!(
            scopes[0].config.get("resolver").map(String::as_str),
            Some("fs-marker")
        );
    }

    /// #154: an explicitly-configured `SKILL_PROJECT_ROOT` resolves project scope
    /// DIRECTLY (no `.git`/marker walk), even when `start_dir` has no marker — the
    /// containerized case where the working directory is `/`. This is what lets
    /// `compile_context` return `ok` for project scope inside the stock container.
    #[tokio::test]
    async fn fs_marker_resolver_returns_configured_project_root_directly() {
        let _env = lock_scope_env().await;

        // A sandbox project root with NO .git and NO marker file.
        let sandbox = std::env::temp_dir().join(format!(
            "fs-marker-configured-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");

        // start_dir deliberately points somewhere with no marker (mirrors `/` in a
        // container); only SKILL_PROJECT_ROOT should make resolution succeed.
        let resolver = FsMarkerProjectResolver::new(std::env::temp_dir());

        // SAFETY: test-scoped env mutation, serialized by SCOPE_ENV_LOCK.
        unsafe {
            env::set_var("SKILL_PROJECT_ROOT", &sandbox);
        }

        let result = resolver.resolve(None).await;

        // SAFETY: cleanup before releasing the lock so the var never leaks.
        unsafe {
            env::remove_var("SKILL_PROJECT_ROOT");
        }

        let scopes = result.expect("configured SKILL_PROJECT_ROOT must resolve project scope");
        std::fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Project);
        assert_eq!(
            scopes[0].paths[0],
            fs::canonicalize(&sandbox).unwrap_or(sandbox),
            "resolved project root must be the configured SKILL_PROJECT_ROOT"
        );
    }

    /// #154: a `SKILL_PROJECT_ROOT` pointing at a non-existent directory fails LOUD
    /// (`ResolverUnavailable` with a named reason) rather than silently falling
    /// through to a `/`-walk that always degrades — per the fail-loud mandate.
    #[tokio::test]
    async fn fs_marker_resolver_fails_loud_when_configured_root_missing() {
        let _env = lock_scope_env().await;

        let missing = std::env::temp_dir().join(format!(
            "fs-marker-missing-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        // Deliberately do NOT create `missing`.

        let resolver = FsMarkerProjectResolver::new(std::env::temp_dir());

        // SAFETY: test-scoped env mutation, serialized by SCOPE_ENV_LOCK.
        unsafe {
            env::set_var("SKILL_PROJECT_ROOT", &missing);
        }

        let result = resolver.resolve(None).await;

        // SAFETY: cleanup before releasing the lock.
        unsafe {
            env::remove_var("SKILL_PROJECT_ROOT");
        }

        let error =
            result.expect_err("a missing SKILL_PROJECT_ROOT must fail loud, not degrade silently");
        match error {
            ScopeError::ResolverUnavailable(message) => assert!(
                message.contains("SKILL_PROJECT_ROOT"),
                "error must name the offending env var, got: {message}"
            ),
            other => {
                panic!("expected ResolverUnavailable naming SKILL_PROJECT_ROOT, got {other:?}")
            }
        }
    }

    /// #154: when a request's `repo_path` does not resolve in THIS environment
    /// (e.g. a host-only path passed into a container), the resolver falls back to
    /// the configured `SKILL_PROJECT_ROOT` instead of degrading — so a client that
    /// always sends a host path still gets project scope in the container.
    #[tokio::test]
    async fn fs_marker_resolver_falls_back_to_configured_root_when_repo_path_unresolvable() {
        let _env = lock_scope_env().await;

        let configured = std::env::temp_dir().join(format!("fs-marker-fallback-{}", {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        }));
        std::fs::create_dir_all(&configured).expect("configured root should be creatable");

        let resolver = FsMarkerProjectResolver::new(std::env::temp_dir());

        // SAFETY: serialized by SCOPE_ENV_LOCK.
        unsafe {
            env::set_var("SKILL_PROJECT_ROOT", &configured);
        }

        // A repo_path that cannot resolve (no .git/marker anywhere up /tmp/<nonce>).
        let bogus = std::env::temp_dir()
            .join("definitely-not-a-repo-12345")
            .display()
            .to_string();
        let result = resolver.resolve(Some(&bogus)).await;

        // SAFETY: cleanup before releasing the lock.
        unsafe {
            env::remove_var("SKILL_PROJECT_ROOT");
        }

        let scopes = result.expect("unresolvable repo_path must fall back to SKILL_PROJECT_ROOT");
        std::fs::remove_dir_all(&configured).expect("cleanup");

        assert_eq!(scopes.len(), 1);
        assert_eq!(
            scopes[0].paths[0],
            fs::canonicalize(&configured).unwrap_or(configured),
            "must fall back to the configured project root, not the bogus repo_path"
        );
    }

    // --- end FsMarkerProjectResolver tests ---

    #[tokio::test]
    async fn git_root_project_resolver_returns_repository_root() {
        let start_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let expected_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root should canonicalize");
        let resolver = GitRootProjectResolver::new(start_dir);

        let scopes = resolver
            .resolve(None)
            .await
            .expect("git root should resolve");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Project);
        assert_eq!(
            scopes[0].paths[0]
                .canonicalize()
                .expect("project path should canonicalize"),
            expected_root
        );
    }

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
            .resolve(None)
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
            .resolve(None)
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
            .resolve(None)
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

    #[tokio::test]
    async fn default_env_resolver_reads_skill_global_paths_variable() {
        let resolver = EnvPathGlobalResolver::default();
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root should canonicalize");
        let docs_path = repo_root.join("docs");
        let scripts_path = repo_root.join("scripts");

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::set_var(
                "SKILL_GLOBAL_PATHS",
                format!("{},{}", docs_path.display(), scripts_path.display()),
            );
            env::set_var(
                "SKILL_GLOBAL_ALLOWED_ROOTS",
                repo_root.display().to_string(),
            );
        }

        let scopes = resolver
            .resolve(None)
            .await
            .expect("default resolver should read SKILL_GLOBAL_PATHS");

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].scope_type, ScopeType::Global);
        assert_eq!(scopes[0].paths.len(), 2);

        // SAFETY: test-scoped environment mutation.
        unsafe {
            env::remove_var("SKILL_GLOBAL_PATHS");
            env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
        }
    }
}
