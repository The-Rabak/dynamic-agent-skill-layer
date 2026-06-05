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
    _graph_builder_project_root: Option<EnvVarGuard>,
    _graph_builder_global_root: Option<EnvVarGuard>,
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
    configure_scope_env_with_graph_builder_roots(global_scope, None, None)
}

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

/// Per-run namespace isolation for in-process `LiveServerComponents` tests (#164).
///
/// The live test stack is ONE shared Postgres/Redis/Qdrant that the long-lived
/// containers also use. Without isolation, an in-process test's destructive
/// teardown (`TRUNCATE`, Qdrant `delete_points`, `DEL` the stream) mutates the
/// containers' canonical state out from under them — this is what wedged the
/// mcp-server subscriber in #163.
///
/// This guard gives each run a unique namespace so the adapters built by
/// `from_environment` target sandbox resources only:
/// - **Postgres**: a fresh `test_ns_<runid>` schema, selected via the connection
///   URL's `search_path` (the same mechanism proven in
///   `postgres.rs::live_run_migrations_…`). Migrations create every table there.
/// - **Qdrant**: a `skills_ns_<runid>` collection (`QDRANT_COLLECTION`).
/// - **Redis**: a `skill-layer-events-ns-<runid>` stream + `skill-layer-ns-<runid>`
///   group (`REDIS_STREAM_KEY` / `REDIS_CONSUMER_GROUP`).
///
/// The `ENV_LOCK` is held for the guard's lifetime so the env mutation + the
/// `from_environment` adapter build that reads it cannot race another test in the
/// same binary. Call [`NamespaceGuard::cleanup`] at end of test to drop the
/// sandbox schema/collection/stream; if it is skipped the leftover empty sandbox
/// objects are harmless (they never touch the canonical namespace).
#[allow(dead_code)]
pub(crate) struct NamespaceGuard {
    schema: String,
    base_db_url: String,
    qdrant_url: String,
    qdrant_collection: String,
    redis_url: String,
    stream_key: String,
    /// Prefix for the suppression/context-cache Redis keys (`REDIS_KEY_PREFIX`),
    /// so a sandbox's `suppression:`/`cache:` keys can't collide with the shared
    /// canonical ones (which caused phantom `DuplicateSuppressed` at low versions).
    key_prefix: String,
    _db_url: EnvVarGuard,
    _qdrant_collection: EnvVarGuard,
    _stream_key: EnvVarGuard,
    _consumer_group: EnvVarGuard,
    _key_prefix: EnvVarGuard,
    // Scope env is folded in (rather than a separate `configure_scope_env` guard)
    // because both acquire the non-reentrant `ENV_LOCK` — holding two would
    // deadlock. One guard owns the lock and every env override for the run.
    _allowed_roots: EnvVarGuard,
    _global_paths: EnvVarGuard,
    _lock: MutexGuard<'static, ()>,
}

#[allow(dead_code)]
impl NamespaceGuard {
    /// The sandbox schema name (for assertions / diagnostics).
    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    /// Drops the sandbox schema, Qdrant collection, and Redis stream. Best-effort:
    /// reports nothing on failure beyond a panic-free path, since leftover sandbox
    /// objects cannot contaminate the canonical namespace.
    pub(crate) async fn cleanup(self) {
        // Postgres: DROP SCHEMA … CASCADE via an admin connection on the base URL.
        if let Ok(admin) = sqlx::PgPool::connect(&self.base_db_url).await {
            let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
                .execute(&admin)
                .await;
            admin.close().await;
        }

        // Qdrant: delete the sandbox collection.
        let _ = reqwest::Client::new()
            .delete(format!(
                "{}/collections/{}",
                self.qdrant_url.trim_end_matches('/'),
                self.qdrant_collection
            ))
            .send()
            .await;

        // Redis: delete the sandbox stream (group goes with it) and SCAN+DEL the
        // sandbox suppression/context-cache keys (prefixed with `key_prefix`).
        if let Ok(client) = redis::Client::open(self.redis_url.clone()) {
            if let Ok(mut conn) = client.get_multiplexed_async_connection().await {
                let _: Result<i64, _> = redis::cmd("DEL")
                    .arg(&self.stream_key)
                    .query_async(&mut conn)
                    .await;

                // SCAN+DEL every `{key_prefix}*` key (suppression: + cache:).
                let pattern = format!("{}*", self.key_prefix);
                let mut cursor: u64 = 0;
                loop {
                    let scan: Result<(u64, Vec<String>), _> = redis::cmd("SCAN")
                        .arg(cursor)
                        .arg("MATCH")
                        .arg(&pattern)
                        .arg("COUNT")
                        .arg(200)
                        .query_async(&mut conn)
                        .await;
                    let Ok((next, keys)) = scan else { break };
                    if !keys.is_empty() {
                        let _: Result<i64, _> =
                            redis::cmd("DEL").arg(&keys).query_async(&mut conn).await;
                    }
                    cursor = next;
                    if cursor == 0 {
                        break;
                    }
                }
            }
        }
        // env restored + lock released as `self` drops here.
    }
}

/// Builds an isolated namespace from the live `DATABASE_URL` / `QDRANT_URL` /
/// `REDIS_URL` env (which the e2e run script exports), creates the sandbox PG
/// schema, and overrides the namespace env vars under `ENV_LOCK`.
///
/// The global scope defaults to `<repo>/docs` (matching `configure_scope_env`).
#[allow(dead_code)]
pub(crate) async fn isolated_namespace() -> NamespaceGuard {
    let global_scope = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .join("docs");
    isolated_namespace_with_global_path(global_scope).await
}

/// Like [`isolated_namespace`] but with an explicit global-scope path (mirrors
/// `configure_scope_env_with_global_path`, e.g. a sandbox skills dir).
#[allow(dead_code)]
pub(crate) async fn isolated_namespace_with_global_path(global_scope: PathBuf) -> NamespaceGuard {
    let base_db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set for isolated tests");
    let qdrant_url = env::var("QDRANT_URL").expect("QDRANT_URL must be set for isolated tests");
    let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set for isolated tests");

    let runid = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let schema = format!("test_ns_{runid}");
    let qdrant_collection = format!("skills_ns_{runid}");
    let stream_key = format!("skill-layer-events-ns-{runid}");
    let consumer_group = format!("skill-layer-ns-{runid}");
    let key_prefix = format!("ns_{runid}:");

    // Create the sandbox schema BEFORE taking the lock (independent op on a unique
    // name) so no `await` is held across the lock.
    let admin = sqlx::PgPool::connect(&base_db_url)
        .await
        .expect("admin pool connects for schema creation");
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
        .execute(&admin)
        .await
        .expect("create sandbox schema");
    admin.close().await;

    // Select the sandbox schema via search_path in the connection URL — the same
    // mechanism proven in postgres.rs's live migration test.
    let sep = if base_db_url.contains('?') { '&' } else { '?' };
    let namespaced_db_url = format!("{base_db_url}{sep}options=-csearch_path%3D{schema}");

    // Scope env, mirroring `configure_scope_env` (allowed-roots = repo root,
    // global-paths = the provided scope), so callers need only this one guard.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");

    let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    NamespaceGuard {
        _db_url: EnvVarGuard::set("DATABASE_URL", namespaced_db_url),
        _qdrant_collection: EnvVarGuard::set("QDRANT_COLLECTION", qdrant_collection.clone()),
        _stream_key: EnvVarGuard::set("REDIS_STREAM_KEY", stream_key.clone()),
        _consumer_group: EnvVarGuard::set("REDIS_CONSUMER_GROUP", consumer_group),
        _key_prefix: EnvVarGuard::set("REDIS_KEY_PREFIX", key_prefix.clone()),
        _allowed_roots: EnvVarGuard::set(
            "SKILL_GLOBAL_ALLOWED_ROOTS",
            repo_root.display().to_string(),
        ),
        _global_paths: EnvVarGuard::set("SKILL_GLOBAL_PATHS", global_scope.display().to_string()),
        schema,
        base_db_url,
        qdrant_url,
        qdrant_collection,
        redis_url,
        stream_key,
        key_prefix,
        _lock: lock,
    }
}

#[test]
fn scope_env_guard_restores_previous_values() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    // SAFETY: test-scoped setup.
    unsafe {
        env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", "before-roots");
        env::remove_var("SKILL_GLOBAL_PATHS");
    }

    {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root should resolve");
        let global_scope = repo_root.join("docs");
        let _allowed_roots = EnvVarGuard::set(
            "SKILL_GLOBAL_ALLOWED_ROOTS",
            repo_root.display().to_string(),
        );
        let _global_paths =
            EnvVarGuard::set("SKILL_GLOBAL_PATHS", global_scope.display().to_string());
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
