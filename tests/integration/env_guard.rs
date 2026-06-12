// Shared across e2e test binaries; #[path]-included per-binary, so any helper a given binary
// doesn't exercise is dead_code only as a per-binary compilation artifact, not a real orphan.
// Review for a genuine orphan before deleting any helper only one non-gate binary uses.
#![allow(dead_code)]

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
/// sandbox schema/collection/stream on the test's own runtime; if the test
/// panics first, the `Drop` impl runs the same teardown synchronously on a
/// dedicated thread so the sandbox is reclaimed either way (#164). As a final
/// backstop, [`isolated_namespace_with_global_path`] sweeps any sandbox older
/// than [`STALE_SANDBOX_NANOS`] left behind by a hard-killed run.
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
    /// Set once the async `cleanup()` has torn the sandbox down, so the `Drop`
    /// fallback (which repeats the teardown synchronously for the panic path)
    /// does not run a second time.
    cleaned: bool,
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

/// The subset of sandbox identifiers needed to tear a sandbox down. Cloned out
/// of the guard so teardown can run either async (happy path, [`NamespaceGuard::cleanup`])
/// or on a dedicated thread from [`NamespaceGuard`]'s `Drop` (panic path).
#[derive(Clone)]
struct SandboxResources {
    schema: String,
    base_db_url: String,
    qdrant_url: String,
    qdrant_collection: String,
    redis_url: String,
    stream_key: String,
    key_prefix: String,
}

/// Drops the sandbox PG schema, Qdrant collection, and Redis stream + prefixed
/// keys. Best-effort and panic-free: every connect is bounded by a short timeout
/// so a down dependency (e.g. the chaos suite stops Postgres) cannot hang
/// teardown, and every error is swallowed since leftover sandbox objects can
/// never touch the canonical namespace.
async fn drop_sandbox(res: SandboxResources) {
    use std::time::Duration;
    const OP_TIMEOUT: Duration = Duration::from_secs(5);

    // Postgres: DROP SCHEMA … CASCADE via an admin connection on the base URL.
    if let Ok(Ok(admin)) =
        tokio::time::timeout(OP_TIMEOUT, sqlx::PgPool::connect(&res.base_db_url)).await
    {
        let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", res.schema))
            .execute(&admin)
            .await;
        admin.close().await;
    }

    // Qdrant: delete the sandbox collection.
    if let Ok(client) = reqwest::Client::builder().timeout(OP_TIMEOUT).build() {
        let _ = client
            .delete(format!(
                "{}/collections/{}",
                res.qdrant_url.trim_end_matches('/'),
                res.qdrant_collection
            ))
            .send()
            .await;
    }

    // Redis: delete the sandbox stream (group goes with it) and SCAN+DEL the
    // sandbox suppression/context-cache keys (prefixed with `key_prefix`).
    if let Ok(redis_client) = redis::Client::open(res.redis_url.clone())
        && let Ok(Ok(mut conn)) =
            tokio::time::timeout(OP_TIMEOUT, redis_client.get_multiplexed_async_connection()).await
    {
        let _: Result<i64, _> = redis::cmd("DEL")
            .arg(&res.stream_key)
            .query_async(&mut conn)
            .await;

        // SCAN+DEL every `{key_prefix}*` key (suppression: + cache:).
        let pattern = format!("{}*", res.key_prefix);
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
                let _: Result<i64, _> = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await;
            }
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
    }
}

#[allow(dead_code)]
impl NamespaceGuard {
    /// The sandbox schema name (for assertions / diagnostics).
    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    fn resources(&self) -> SandboxResources {
        SandboxResources {
            schema: self.schema.clone(),
            base_db_url: self.base_db_url.clone(),
            qdrant_url: self.qdrant_url.clone(),
            qdrant_collection: self.qdrant_collection.clone(),
            redis_url: self.redis_url.clone(),
            stream_key: self.stream_key.clone(),
            key_prefix: self.key_prefix.clone(),
        }
    }

    /// Drops the sandbox schema, Qdrant collection, and Redis stream/keys on the
    /// happy path, on the test's own async runtime. Marks the guard cleaned so
    /// the `Drop` fallback does not repeat the work. Prefer this over relying on
    /// `Drop`: it avoids the thread-spawn + block-on the panic path needs.
    pub(crate) async fn cleanup(mut self) {
        drop_sandbox(self.resources()).await;
        self.cleaned = true;
        // self drops here; the `Drop` impl sees `cleaned == true` and is a no-op.
    }
}

impl Drop for NamespaceGuard {
    fn drop(&mut self) {
        // Happy path already reclaimed the sandbox — nothing to do.
        if self.cleaned {
            return;
        }
        // Panic path (or a test that forgot to call `cleanup().await`): without
        // this, the sandbox would leak its PG schema / Qdrant collection / Redis
        // stream forever. `Drop` is synchronous and we may be unwinding inside the
        // test's own current-thread runtime, so we cannot `.await` here and cannot
        // build a nested runtime on this thread. Run the async teardown on a
        // dedicated thread with its own runtime and block on it — correct even
        // during panic unwinding.
        let res = self.resources();
        if let Ok(handle) = std::thread::Builder::new()
            .name("ns-guard-cleanup".to_owned())
            .spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    rt.block_on(drop_sandbox(res));
                }
            })
        {
            let _ = handle.join();
        }
        // env vars restored + lock released as the remaining fields drop.
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
    let base_db_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set for isolated tests");
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

    // Backstop GC: reclaim any sandbox left behind by a previously hard-killed run
    // (a `SIGKILL` / `cargo test` timeout skips `Drop`). Only touches sandboxes
    // older than STALE_SANDBOX_NANOS, so concurrent sibling sandboxes are safe.
    sweep_stale_namespaces(&base_db_url, &qdrant_url, &redis_url, runid).await;

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
        cleaned: false,
        _lock: lock,
    }
}

/// Sandbox resources older than this (by their embedded `<runid>` nanosecond
/// timestamp) are treated as leaked by a previously hard-killed run and swept on
/// the next [`isolated_namespace_with_global_path`] call. Far longer than any
/// single test, so a sibling test binary's in-flight sandbox (recent runid) is
/// never collected — the sweep is safe to run while other namespaced tests run.
const STALE_SANDBOX_NANOS: u128 = 30 * 60 * 1_000_000_000; // 30 minutes

/// Extracts the `<runid>` (nanoseconds since epoch) embedded in a sandbox object
/// name: `test_ns_<id>`, `skills_ns_<id>`, `skill-layer-events-ns-<id>`, or a
/// `ns_<id>:…` Redis key. Returns `None` for canonical names (no `ns` marker)
/// so they can never be swept.
fn parse_runid(name: &str) -> Option<u128> {
    let after = name
        .split_once("_ns_")
        .or_else(|| name.split_once("-ns-"))
        .map(|(_, rest)| rest)
        .or_else(|| name.strip_prefix("ns_"))?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u128>().ok()
}

/// Best-effort sweep of sandbox resources leaked by a previously panicked or
/// hard-killed run (the `Drop` fallback covers in-process panics, but a `SIGKILL`
/// / `cargo test` timeout skips `Drop` entirely). Only `*ns*` objects whose
/// runid is older than [`STALE_SANDBOX_NANOS`] are dropped; canonical names and
/// recent sandboxes are left untouched. Never panics, never blocks on a down
/// dependency beyond the per-op timeout.
async fn sweep_stale_namespaces(
    base_db_url: &str,
    qdrant_url: &str,
    redis_url: &str,
    now_nanos: u128,
) {
    use std::time::Duration;
    const OP_TIMEOUT: Duration = Duration::from_secs(5);
    let is_stale = |name: &str| {
        parse_runid(name).is_some_and(|id| now_nanos.saturating_sub(id) > STALE_SANDBOX_NANOS)
    };

    // Postgres: drop stale `test_ns_*` schemas.
    if let Ok(Ok(admin)) =
        tokio::time::timeout(OP_TIMEOUT, sqlx::PgPool::connect(base_db_url)).await
    {
        if let Ok(schemas) = sqlx::query_scalar::<_, String>(
            "SELECT nspname FROM pg_namespace WHERE nspname LIKE 'test_ns_%'",
        )
        .fetch_all(&admin)
        .await
        {
            for schema in schemas.iter().filter(|s| is_stale(s)) {
                let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
                    .execute(&admin)
                    .await;
            }
        }
        admin.close().await;
    }

    // Qdrant: delete stale `skills_ns_*` collections.
    if let Ok(client) = reqwest::Client::builder().timeout(OP_TIMEOUT).build() {
        let base = qdrant_url.trim_end_matches('/');
        if let Ok(resp) = client.get(format!("{base}/collections")).send().await
            && let Ok(body) = resp.json::<serde_json::Value>().await
            && let Some(collections) = body
                .get("result")
                .and_then(|r| r.get("collections"))
                .and_then(|c| c.as_array())
        {
            for name in collections
                .iter()
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                .filter(|n| n.starts_with("skills_ns_") && is_stale(n))
            {
                let _ = client
                    .delete(format!("{base}/collections/{name}"))
                    .send()
                    .await;
            }
        }
    }

    // Redis: delete stale sandbox streams + `ns_<id>:` prefixed keys.
    if let Ok(redis_client) = redis::Client::open(redis_url.to_owned())
        && let Ok(Ok(mut conn)) =
            tokio::time::timeout(OP_TIMEOUT, redis_client.get_multiplexed_async_connection()).await
    {
        for pattern in ["skill-layer-events-ns-*", "ns_*"] {
            let mut cursor: u64 = 0;
            loop {
                let scan: Result<(u64, Vec<String>), _> = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(pattern)
                    .arg("COUNT")
                    .arg(200)
                    .query_async(&mut conn)
                    .await;
                let Ok((next, keys)) = scan else { break };
                let stale: Vec<&String> = keys.iter().filter(|k| is_stale(k)).collect();
                if !stale.is_empty() {
                    let _: Result<i64, _> =
                        redis::cmd("DEL").arg(&stale).query_async(&mut conn).await;
                }
                cursor = next;
                if cursor == 0 {
                    break;
                }
            }
        }
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

#[test]
fn parse_runid_extracts_id_from_every_sandbox_object_shape() {
    // Each object name the namespace guard mints embeds the same `<runid>`.
    assert_eq!(
        parse_runid("test_ns_1780641703958278014"),
        Some(1780641703958278014)
    );
    assert_eq!(
        parse_runid("skills_ns_1780641703958278014"),
        Some(1780641703958278014)
    );
    assert_eq!(
        parse_runid("skill-layer-events-ns-1780641703958278014"),
        Some(1780641703958278014)
    );
    // Redis keys carry the runid between the `ns_` prefix and the first `:`.
    assert_eq!(
        parse_runid("ns_1780641703958278014:suppression:sess::abcd"),
        Some(1780641703958278014)
    );
    assert_eq!(
        parse_runid("ns_1780641703958278014:cache:sess:hash:scope"),
        Some(1780641703958278014)
    );
}

#[test]
fn parse_runid_returns_none_for_canonical_names_so_they_are_never_swept() {
    // Canonical resources carry no `ns` marker — the sweep must never match them.
    assert_eq!(parse_runid("skills"), None);
    assert_eq!(parse_runid("skill-layer-events"), None);
    assert_eq!(parse_runid("public"), None);
    assert_eq!(parse_runid("suppression:sess::hash"), None);
    assert_eq!(parse_runid("cache:sess:hash:scope"), None);
}

#[test]
fn staleness_predicate_collects_old_but_spares_recent_and_canonical() {
    // Mirrors the `is_stale` closure used by `sweep_stale_namespaces`.
    let now: u128 = 2_000_000_000_000_000_000; // arbitrary "now" in ns
    let is_stale = |name: &str| {
        parse_runid(name).is_some_and(|id| now.saturating_sub(id) > STALE_SANDBOX_NANOS)
    };

    // Older than the 30-minute threshold → swept.
    let old = now - STALE_SANDBOX_NANOS - 1;
    assert!(is_stale(&format!("test_ns_{old}")));

    // Younger than the threshold (a sibling test binary's live sandbox) → spared.
    let recent = now - 1;
    assert!(!is_stale(&format!("test_ns_{recent}")));

    // A future runid (clock skew) underflows to 0 via saturating_sub → spared.
    let future = now + STALE_SANDBOX_NANOS * 2;
    assert!(!is_stale(&format!("test_ns_{future}")));

    // Canonical names never match.
    assert!(!is_stale("skills"));
    assert!(!is_stale("skill-layer-events"));
}
