/// Scope-env guard helpers for mcp-server crate-local integration tests.
///
/// Relocated from `tests/integration/env_guard.rs` — this copy is scoped to
/// the mcp-server crate (compile_context, dual_scope, session_persistence tests).
use std::{
    env,
    ffi::OsString,
    path::PathBuf,
    sync::{LazyLock, Mutex, MutexGuard},
};

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: String) -> Self {
        let previous = env::var_os(key);
        // SAFETY: tests set process env only while holding ENV_LOCK.
        unsafe {
            env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: tests mutate process env only while holding ENV_LOCK.
        unsafe {
            if let Some(value) = &self.previous {
                env::set_var(self.key, value);
            } else {
                env::remove_var(self.key);
            }
        }
    }
}

/// Scope-env guard: holds the `ENV_LOCK` and restores env vars on drop.
pub(crate) struct ScopeEnvGuard {
    _allowed_roots: EnvVarGuard,
    _global_paths: EnvVarGuard,
    _graph_builder_project_root: Option<EnvVarGuard>,
    _graph_builder_global_root: Option<EnvVarGuard>,
    _lock: MutexGuard<'static, ()>,
}

/// Sets `SKILL_GLOBAL_ALLOWED_ROOTS` to the repo root and
/// `SKILL_GLOBAL_PATHS` to `<repo>/docs`, matching the standard test scope.
pub(crate) fn configure_scope_env() -> ScopeEnvGuard {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
    let global_scope = repo_root.join("docs");
    configure_scope_env_with_global_path(global_scope)
}

/// Like `configure_scope_env` but with an explicit global-scope path.
pub(crate) fn configure_scope_env_with_global_path(global_scope: PathBuf) -> ScopeEnvGuard {
    configure_scope_env_with_graph_builder_roots(global_scope, None, None)
}

/// Full scope env setup: sets allowed roots, global paths, and optional
/// graph-builder project/global root overrides.
pub(crate) fn configure_scope_env_with_graph_builder_roots(
    global_scope: PathBuf,
    graph_builder_project_root: Option<PathBuf>,
    graph_builder_global_root: Option<PathBuf>,
) -> ScopeEnvGuard {
    let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");

    ScopeEnvGuard {
        _allowed_roots: EnvVarGuard::set(
            "SKILL_GLOBAL_ALLOWED_ROOTS",
            repo_root.display().to_string(),
        ),
        _global_paths: EnvVarGuard::set("SKILL_GLOBAL_PATHS", global_scope.display().to_string()),
        _graph_builder_project_root: graph_builder_project_root
            .map(|path| EnvVarGuard::set("GRAPH_BUILDER_PROJECT_ROOT", path.display().to_string())),
        _graph_builder_global_root: graph_builder_global_root
            .map(|path| EnvVarGuard::set("GRAPH_BUILDER_GLOBAL_ROOT", path.display().to_string())),
        _lock: lock,
    }
}
