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

pub(crate) struct ScopeEnvGuard {
    _allowed_roots: EnvVarGuard,
    _global_paths: EnvVarGuard,
    _lock: MutexGuard<'static, ()>,
}

pub(crate) fn configure_scope_env() -> ScopeEnvGuard {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
    let global_scope = repo_root.join("docs");
    configure_scope_env_with_global_path(global_scope)
}

pub(crate) fn configure_scope_env_with_global_path(global_scope: PathBuf) -> ScopeEnvGuard {
    let lock = ENV_LOCK.lock().expect("env lock poisoned");
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
        _lock: lock,
    }
}

#[test]
fn scope_env_guard_restores_previous_values() {
    // SAFETY: test-scoped setup.
    unsafe {
        env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", "before-roots");
        env::remove_var("SKILL_GLOBAL_PATHS");
    }

    {
        let _guard = configure_scope_env();
        assert_ne!(
            env::var("SKILL_GLOBAL_ALLOWED_ROOTS").ok().as_deref(),
            Some("before-roots")
        );
        assert!(env::var("SKILL_GLOBAL_PATHS").is_ok());
    }

    assert_eq!(
        env::var("SKILL_GLOBAL_ALLOWED_ROOTS").ok().as_deref(),
        Some("before-roots")
    );
    assert!(env::var("SKILL_GLOBAL_PATHS").is_err());
}
