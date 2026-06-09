use std::str::FromStr;
use std::time::Duration;

use sqlx::{
    Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use thiserror::Error;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial_schema.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_transcript_ingest_queue.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_usage_fields.sql");
const MIGRATION_004: &str = include_str!("../../migrations/004_session_logs_status_check.sql");
/// Migration 005: adds `skills.source_paths TEXT[] NOT NULL DEFAULT '{}'`.
/// Per-skill SKILL.md provenance so the retrieval boot adapter uses true paths
/// instead of the scope-root stand-in. Pre-migration rows get an empty array
/// and fall back to the scope-root behavior in `build_graph_from_pg`.
const MIGRATION_005: &str = include_str!("../../migrations/005_skill_source_paths.sql");
/// Migration 006: adds `community_skills.source TEXT NOT NULL DEFAULT 'tag'` and
/// widens the PK to `(community_id, skill_id, source)` so a skill can belong to
/// both an HDBSCAN cluster community and a tag community simultaneously (dual
/// membership per CONTEXT.md §2.2).
const MIGRATION_006: &str = include_str!("../../migrations/006_community_skills_source.sql");
/// Migration 007: adds advisory scope-generality hints (`generality`,
/// `generality_rationale`) to `skills` so extraction can signal whether a skill
/// is project-local or globally promotable.
const MIGRATION_007: &str = include_str!("../../migrations/007_skill_generality.sql");
/// Migration 008: creates the `embedding_model_metadata` table so graph rebuilds
/// record which embedding model + dimension + collection produced the current
/// Qdrant vectors (V1.7 multi-arm observability).
const MIGRATION_008: &str = include_str!("../../migrations/008_embedding_model_metadata.sql");

/// Ordered migration set: each entry is `(stable_id, sql)`.
///
/// `stable_id` is the migration filename stem (e.g. `"001_initial_schema"`) and
/// serves as the primary key in the `schema_migrations` tracking table.  Ids are
/// stable — they must never change once a migration has shipped.
///
/// Ordering matters because later migrations depend on objects created by earlier
/// ones (002 reuses the trigger function from 001; 003 adds columns to tables from
/// 001; 004 adds a constraint to session_logs; 005 adds a column to skills;
/// 006 widens community_skills for dual membership; 007 adds generality columns;
/// 008 adds embedding_model_metadata table).
///
/// Individual migrations remain idempotent (`IF NOT EXISTS` / `ADD COLUMN IF NOT
/// EXISTS`) as a belt-and-braces safety net, but the tracking table is the primary
/// guard against re-execution.
const MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial_schema", MIGRATION_001),
    ("002_transcript_ingest_queue", MIGRATION_002),
    ("003_usage_fields", MIGRATION_003),
    ("004_session_logs_status_check", MIGRATION_004),
    ("005_skill_source_paths", MIGRATION_005),
    ("006_community_skills_source", MIGRATION_006),
    ("007_skill_generality", MIGRATION_007), // ratified alongside 008 on 2026-06-09 (owner triage #233): dormant write-ahead schema, no live reader
    ("008_embedding_model_metadata", MIGRATION_008),
];

/// SQL executed by `truncate_all_tables`.
///
/// Defined as a named constant so the `truncate_all_tables_sql_includes_usage_tables`
/// unit test can assert against the exact string the runtime sends to Postgres,
/// turning the test into a real guard against table-omission drift rather than a
/// check against a separate hardcoded copy.
///
/// Must be kept in sync with every table created by the migration set.  Any table
/// omitted here causes cross-suite contamination when the table has live data.
#[cfg(any(test, feature = "test-utils"))]
const TRUNCATE_ALL_TABLES_SQL: &str =
    "TRUNCATE TABLE community_skills, skill_subunits, communities, subunits, skills, \
     outbox_events, rebuild_locks, transcript_ingest_queue, \
     session_logs, skill_usage, embedding_model_metadata CASCADE";

#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub database_url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub acquire_timeout_secs: u64,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 20,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 3,
        }
    }
}

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("invalid postgres configuration: {0}")]
    InvalidConfiguration(String),
    #[error("postgres connection failure: {0}")]
    Connection(#[from] sqlx::Error),
    #[error("postgres migration failure: {0}")]
    Migration(String),
}

/// Ensures the application database named in `database_url` exists, creating it
/// if absent.
///
/// Postgres only runs its `POSTGRES_DB` bootstrap on a first-boot EMPTY data
/// directory. A reused or stale volume — or one first initialized under a
/// different `POSTGRES_DB` (e.g. the test database) — therefore leaves the
/// application database missing, and every service then crash-loops on
/// `database "X" does not exist`. Calling this at service boot makes the stack
/// self-heal regardless of how the volume was initialized.
///
/// It connects to the always-present `postgres` maintenance database, checks
/// `pg_database`, and issues `CREATE DATABASE` only when needed. Idempotent and
/// safe to call from every service concurrently: the loser of a `CREATE` race
/// re-checks and treats the now-present database as success.
///
/// Fails loud (returns `Err`) when the maintenance database is unreachable or the
/// role lacks `CREATEDB` — it never silently proceeds against a missing database.
pub async fn ensure_database_exists(database_url: &str) -> Result<(), PostgresError> {
    let options = PgConnectOptions::from_str(database_url)?;
    let database_name = options.get_database().unwrap_or_default().to_owned();
    if database_name.trim().is_empty() {
        return Err(PostgresError::InvalidConfiguration(
            "database_url must name a database".to_owned(),
        ));
    }

    // Connect to the maintenance database that always exists in a Postgres
    // cluster so we can interrogate/create the application database.
    let admin_options = options.clone().database("postgres");
    let mut admin = PgConnection::connect_with(&admin_options).await?;

    if database_exists(&mut admin, &database_name).await? {
        admin.close().await.ok();
        return Ok(());
    }

    // CREATE DATABASE cannot run inside a transaction nor be parameterized. The
    // name comes from our own trusted config; we still quote-identify it so an
    // unusual database name can never break the statement. A concurrent booter
    // may win the race — on error, re-check and accept an already-created db.
    let quoted = database_name.replace('"', "\"\"");
    if let Err(create_error) = sqlx::query(&format!("CREATE DATABASE \"{quoted}\""))
        .execute(&mut admin)
        .await
    {
        if database_exists(&mut admin, &database_name).await? {
            tracing::info!(
                database = %database_name,
                "application database was created concurrently by another booter"
            );
        } else {
            admin.close().await.ok();
            return Err(PostgresError::Connection(create_error));
        }
    } else {
        tracing::info!(database = %database_name, "created missing application database");
    }

    admin.close().await.ok();
    Ok(())
}

/// Returns whether a database with `database_name` is present in the cluster.
async fn database_exists(
    admin: &mut PgConnection,
    database_name: &str,
) -> Result<bool, PostgresError> {
    let found: Option<i32> = sqlx::query_scalar("SELECT 1 FROM pg_database WHERE datname = $1")
        .bind(database_name)
        .fetch_optional(&mut *admin)
        .await?;
    Ok(found.is_some())
}

#[derive(Debug, Clone)]
pub struct PostgresAdapter {
    pool: PgPool,
}

impl PostgresAdapter {
    pub async fn connect(config: &PostgresConfig) -> Result<Self, PostgresError> {
        if config.database_url.trim().is_empty() {
            return Err(PostgresError::InvalidConfiguration(
                "database_url must not be blank".to_owned(),
            ));
        }

        if config.max_connections == 0
            || config.acquire_timeout_secs == 0
            || config.connect_timeout_secs == 0
        {
            return Err(PostgresError::InvalidConfiguration(
                "pool and timeout values must be greater than zero".to_owned(),
            ));
        }

        let pool = tokio::time::timeout(
            Duration::from_secs(config.connect_timeout_secs),
            PgPoolOptions::new()
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
                // Test connections before checkout. This evicts connections that are in
                // an "idle in transaction (aborted)" state — e.g. left dirty after a
                // failed multi-statement migration run. Without this guard, the pool
                // can recycle a dirty connection and every subsequent query fails with
                // "current transaction is aborted".
                .test_before_acquire(true)
                .connect(&config.database_url),
        )
        .await
        .map_err(|_| PostgresError::Connection(sqlx::Error::PoolTimedOut))??;

        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn ping(&self) -> Result<(), PostgresError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Applies any unapplied migrations in `MIGRATIONS` order and records each
    /// one in the `schema_migrations` tracking table.
    ///
    /// Bootstrap: creates `schema_migrations` idempotently before the loop so the
    /// table is always present when the loop queries it.
    ///
    /// Per-migration transactionality: each migration file is expected to be wrapped
    /// in `BEGIN;` / `COMMIT;`; the runner strips those outer statements and executes
    /// the inner DDL body together with `INSERT INTO schema_migrations` as one
    /// multi-statement batch, which Postgres runs in a single implicit transaction
    /// (see `apply_and_record`). DDL and id record commit together — a failure rolls
    /// the whole batch back, leaving no partial state or unrecorded id.
    ///
    /// This is genuinely atomic because the migration body never issues its own
    /// `COMMIT;` or `BEGIN;`.  Dollar-quoted `BEGIN … END` blocks (PL/pgSQL) are
    /// NOT transaction-control and are left untouched.
    ///
    /// A migration file whose SQL does not begin with `BEGIN;` and end with
    /// `COMMIT;` is rejected immediately with `PostgresError::Migration` — the
    /// runner refuses to proceed rather than silently lose the atomicity guarantee.
    ///
    /// Skip logic: migrations whose `id` is already present in `schema_migrations`
    /// are skipped entirely.  The idempotency guards (`IF NOT EXISTS` etc.) in each
    /// SQL file remain as a belt-and-braces safety net but are not the primary gate.
    ///
    /// Failures surface as `PostgresError::Migration` — no errors are swallowed.
    pub async fn run_migrations(&self) -> Result<(), PostgresError> {
        // Serialize concurrent migrators ACROSS PROCESSES. mcp-server and
        // graph-builder boot at the same time against the same database and both call
        // `run_migrations`; without a cross-process gate they race on the
        // `schema_migrations` ledger and one crashes with a duplicate-key violation
        // on `schema_migrations_pkey` (observed on a fresh DB). A session-level
        // Postgres advisory lock, held on a dedicated connection for the whole
        // sequence, guarantees only one process runs the loop at a time; the others
        // block here, then find every migration already applied and skip.
        //
        // Stable arbitrary lock key (must be identical across all processes).
        const MIGRATION_LOCK_KEY: i64 = 0x5C17_1A4E_4D16_2017;

        let mut lock_conn = self.pool.acquire().await.map_err(|err| {
            PostgresError::Migration(format!("acquire migration lock connection: {err}"))
        })?;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *lock_conn)
            .await
            .map_err(|err| {
                PostgresError::Migration(format!("acquire advisory migration lock: {err}"))
            })?;

        // Run under the lock; always release it (even on error) before returning.
        let result = self.run_migrations_locked().await;

        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *lock_conn)
            .await;
        drop(lock_conn); // dropping the session also releases the lock as a backstop

        result
    }

    /// The actual migration sequence, run while the caller holds the advisory lock.
    async fn run_migrations_locked(&self) -> Result<(), PostgresError> {
        // Bootstrap the tracking table. Created outside of any application
        // migration transaction so it is always available for the loop query.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                id         TEXT        PRIMARY KEY,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|err| PostgresError::Migration(format!("bootstrap schema_migrations: {err}")))?;

        // Collect the ids that have already been applied so we only hit the DB once.
        let applied_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM schema_migrations")
            .fetch_all(&self.pool)
            .await
            .map_err(|err| PostgresError::Migration(format!("query schema_migrations: {err}")))?;

        for (migration_id, migration_sql) in MIGRATIONS {
            if applied_ids.iter().any(|applied| applied == migration_id) {
                continue;
            }

            self.apply_and_record(migration_id, migration_sql).await?;
        }

        Ok(())
    }

    /// Strips the `BEGIN;` / `COMMIT;` wrapper from a migration SQL file and
    /// executes the inner DDL body together with an `INSERT INTO schema_migrations`
    /// as one multi-statement `raw_sql` batch on the pool.
    ///
    /// Atomicity comes from Postgres's *implicit* transaction for a multi-statement
    /// simple-query message: when several statements are sent in a single query
    /// string with NO explicit transaction control, Postgres wraps them in one
    /// implicit transaction. If any statement fails, the entire batch rolls back —
    /// no partial DDL is committed and no id is recorded — and the connection is
    /// left clean (verified live: an aborted implicit transaction does not leave
    /// the connection in the `25P02` "current transaction is aborted" state, unlike
    /// an explicit `BEGIN` whose `COMMIT` is never reached).
    ///
    /// This is why the migration's own `BEGIN;`/`COMMIT;` wrapper is stripped first:
    /// an explicit `COMMIT` mid-batch would commit early (defeating atomicity of the
    /// id record), and an explicit `BEGIN` left open on error would strand the
    /// connection in an aborted-transaction state.
    ///
    /// Why a pool batch rather than a held `sqlx::Transaction`: executing via
    /// `&mut *tx` (`&mut PgConnection: Executor`) introduces a higher-ranked
    /// obligation that tips rustc's trait solver into "Send is not general enough"
    /// for downstream crates that hold `&PostgresAdapter` across `.await` (e.g.
    /// `admin`'s snapshot reader). The pool batch keeps the same atomicity guarantee
    /// without that obligation.
    ///
    /// `migration_id` is a trusted compile-time constant (the `MIGRATIONS` filename
    /// stem, `[a-z0-9_]` only) — never user input — so interpolating it into the
    /// `INSERT` is safe.
    ///
    /// Returns `PostgresError::Migration` if the SQL does not match the expected
    /// wrapper convention, or if the batch fails.
    async fn apply_and_record(
        &self,
        migration_id: &str,
        migration_sql: &str,
    ) -> Result<(), PostgresError> {
        let inner_body = strip_begin_commit_wrapper(migration_id, migration_sql)?;

        // One simple-query message → one implicit transaction over both the DDL and
        // the id record. No explicit BEGIN/COMMIT (see the doc block above).
        //
        // `ON CONFLICT (id) DO NOTHING`: even though `run_migrations` holds a
        // cross-process advisory lock, this makes the ledger insert idempotent as a
        // belt-and-braces guard — a concurrent or repeated apply can never crash with
        // a duplicate-key violation on `schema_migrations_pkey`.
        let batch = format!(
            "{inner_body}\nINSERT INTO schema_migrations (id) VALUES ('{migration_id}') \
             ON CONFLICT (id) DO NOTHING;"
        );

        sqlx::raw_sql(&batch)
            .execute(&self.pool)
            .await
            .map_err(|err| PostgresError::Migration(format!("apply {migration_id}: {err}")))?;

        Ok(())
    }

    /// Persists the active embedding model identity after a successful graph rebuild.
    ///
    /// Writes exactly one row to `embedding_model_metadata` with `key = 'active'`,
    /// replacing any previous value via `ON CONFLICT DO UPDATE`.  The table's
    /// `CHECK (key = 'active')` constraint enforces the single-row invariant at the
    /// database level; the UPSERT satisfies it on every rebuild without a PK
    /// violation.
    ///
    /// `model_digest` is optional — `None` when Ollama does not expose a digest via
    /// the embed response (the column is `TEXT` nullable in migration 008).
    ///
    /// Fails loud (`PostgresError::Connection`) on any database error; callers must
    /// decide whether to abort or log-and-continue.
    pub async fn persist_embedding_model_metadata(
        &self,
        model_name: &str,
        dimension: i32,
        collection: &str,
        model_digest: Option<&str>,
    ) -> Result<(), PostgresError> {
        sqlx::query(
            "INSERT INTO embedding_model_metadata \
             (key, model_name, dimension, collection, model_digest, updated_at) \
             VALUES ('active', $1, $2, $3, $4, NOW()) \
             ON CONFLICT (key) DO UPDATE SET \
               model_name = EXCLUDED.model_name, \
               dimension = EXCLUDED.dimension, \
               collection = EXCLUDED.collection, \
               model_digest = EXCLUDED.model_digest, \
               updated_at = EXCLUDED.updated_at",
        )
        .bind(model_name)
        .bind(dimension)
        .bind(collection)
        .bind(model_digest)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Truncates all application tables. Intended for test teardown only.
    ///
    /// Includes `session_logs` and `skill_usage` so E2E tests run with a clean
    /// usage slate and do not leak usage rows across runs (T06).
    ///
    /// Includes `embedding_model_metadata` so E2E suites started after a graph
    /// rebuild do not see a stale active-model row from a previous run.
    ///
    /// The SQL is stored in `TRUNCATE_ALL_TABLES_SQL` so the companion unit test
    /// asserts against the same string that actually executes, preventing silent
    /// table-omission drift.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn truncate_all_tables(&self) -> Result<(), PostgresError> {
        sqlx::query(TRUNCATE_ALL_TABLES_SQL)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Strips the mandatory `BEGIN;` / `COMMIT;` wrapper from a migration SQL file,
/// returning only the inner body for execution inside a runner-owned transaction.
///
/// # Convention
///
/// Every migration file is expected to start with the literal text `BEGIN;` (as
/// the first non-whitespace content) and end with the literal text `COMMIT;` (as
/// the last non-whitespace content).  This mirrors the format of the five bundled
/// migration files.
///
/// Dollar-quoted `BEGIN … END` blocks (e.g. PL/pgSQL `DO $$` / function bodies)
/// are NOT transaction-control statements and are not affected — only the
/// outermost `BEGIN;` line and `COMMIT;` line are removed.
///
/// # Errors
///
/// Returns `PostgresError::Migration` if:
/// - The SQL does not start with `BEGIN;` after trimming leading whitespace.
/// - The SQL does not end with `COMMIT;` after trimming trailing whitespace.
///
/// The migration id is included in the error message so the caller can surface
/// exactly which file violated the convention.
fn strip_begin_commit_wrapper(migration_id: &str, sql: &str) -> Result<String, PostgresError> {
    let trimmed = sql.trim();

    // Validate and strip the leading `BEGIN;`.
    let after_begin = trimmed.strip_prefix("BEGIN;").ok_or_else(|| {
        PostgresError::Migration(format!(
            "migration {migration_id} is not wrapped in BEGIN;/COMMIT; — \
                 cannot guarantee atomic apply+record (expected first token: BEGIN;)"
        ))
    })?;

    // Validate and strip the trailing `COMMIT;`.
    let inner = after_begin
        .trim_end()
        .strip_suffix("COMMIT;")
        .ok_or_else(|| {
            PostgresError::Migration(format!(
                "migration {migration_id} is not wrapped in BEGIN;/COMMIT; — \
                 cannot guarantee atomic apply+record (expected last token: COMMIT;)"
            ))
        })?;

    Ok(inner.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_contains_required_contract_tables() {
        for table in [
            "skills",
            "subunits",
            "communities",
            "skill_subunits",
            "community_skills",
            "session_logs",
            "skill_usage",
            "audit_log",
            "outbox_events",
            "rebuild_locks",
        ] {
            assert!(
                MIGRATION_001.contains(table),
                "migration should declare {table}"
            );
        }
    }

    #[test]
    fn migration_002_declares_transcript_ingest_queue() {
        assert!(
            MIGRATION_002.contains("transcript_ingest_queue"),
            "migration 002 should declare the transcript ingest queue table"
        );
        assert!(
            MIGRATION_002.contains("content_hash TEXT NOT NULL UNIQUE"),
            "dedup is keyed on a UNIQUE content_hash"
        );
    }

    #[test]
    fn migration_set_is_ordered_001_through_008() {
        // MIGRATIONS is now &[(&str, &str)] — (stable_id, sql). Assert that
        // the ids and sql content appear in the correct 001..008 order.
        let ids: Vec<&str> = MIGRATIONS.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            &[
                "001_initial_schema",
                "002_transcript_ingest_queue",
                "003_usage_fields",
                "004_session_logs_status_check",
                "005_skill_source_paths",
                "006_community_skills_source",
                "007_skill_generality",
                "008_embedding_model_metadata",
            ],
            "migration ids must appear in 001..008 order"
        );

        let sqls: Vec<&str> = MIGRATIONS.iter().map(|(_, sql)| *sql).collect();
        assert_eq!(
            sqls,
            &[
                MIGRATION_001,
                MIGRATION_002,
                MIGRATION_003,
                MIGRATION_004,
                MIGRATION_005,
                MIGRATION_006,
                MIGRATION_007,
                MIGRATION_008,
            ],
            "migration sql bodies must match the include_str! constants in 001..008 order"
        );
    }

    /// Live Postgres: proves that `run_migrations` applies all eight migrations on
    /// a fresh schema and records them in `schema_migrations`, then proves that a
    /// second call skips all eight by asserting `applied_at` timestamps are UNCHANGED.
    ///
    /// A re-applied migration would re-INSERT or UPDATE the row (changing the
    /// timestamp). A truly skipped migration leaves the row exactly as it was.
    ///
    /// Isolation: runs in a dedicated scratch schema (`test_schema_migrations_<ts>`)
    /// that is dropped on completion, so the shared `skill_layer_test` schema is
    /// never touched.
    #[tokio::test]
    #[ignore = "requires live postgres"]
    async fn live_run_migrations_applies_then_skips_on_second_boot() {
        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        // Connect to the default database to create and drop the scratch schema.
        let admin_pool = sqlx::PgPool::connect(&db_url)
            .await
            .expect("admin pool connect");

        let scratch_schema = format!(
            "test_schema_migrations_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        // Create isolated scratch schema.
        sqlx::query(&format!("CREATE SCHEMA {scratch_schema}"))
            .execute(&admin_pool)
            .await
            .expect("create scratch schema");

        // Point the adapter at the scratch schema via search_path.
        let scratch_url = format!("{db_url}?options=-csearch_path%3D{scratch_schema}");
        let config = PostgresConfig {
            database_url: scratch_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("scratch adapter connect");

        // ---- First boot: all eight migrations must be applied ----
        adapter
            .run_migrations()
            .await
            .expect("first run_migrations must succeed");

        let first_run_rows: Vec<(String, chrono::DateTime<chrono::Utc>)> =
            sqlx::query_as("SELECT id, applied_at FROM schema_migrations ORDER BY id")
                .fetch_all(adapter.pool())
                .await
                .expect("query schema_migrations after first run");

        let first_run_ids: Vec<&str> = first_run_rows.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            first_run_ids,
            &[
                "001_initial_schema",
                "002_transcript_ingest_queue",
                "003_usage_fields",
                "004_session_logs_status_check",
                "005_skill_source_paths",
                "006_community_skills_source",
                "007_skill_generality",
                "008_embedding_model_metadata",
            ],
            "first boot must record all eight migration ids"
        );

        let first_applied_ats: Vec<chrono::DateTime<chrono::Utc>> =
            first_run_rows.iter().map(|(_, ts)| *ts).collect();

        // ---- Second boot: all eight must be SKIPPED (applied_at unchanged) ----
        adapter
            .run_migrations()
            .await
            .expect("second run_migrations must succeed");

        let second_run_rows: Vec<(String, chrono::DateTime<chrono::Utc>)> =
            sqlx::query_as("SELECT id, applied_at FROM schema_migrations ORDER BY id")
                .fetch_all(adapter.pool())
                .await
                .expect("query schema_migrations after second run");

        let second_applied_ats: Vec<chrono::DateTime<chrono::Utc>> =
            second_run_rows.iter().map(|(_, ts)| *ts).collect();

        assert_eq!(
            first_applied_ats, second_applied_ats,
            "applied_at timestamps must be UNCHANGED on second boot — \
             any difference proves a migration was re-applied rather than skipped"
        );

        // Cleanup: drop the scratch schema.
        sqlx::query(&format!("DROP SCHEMA {scratch_schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .expect("drop scratch schema");

        admin_pool.close().await;
    }

    #[test]
    fn migration_003_adds_typed_usage_columns() {
        assert!(
            MIGRATION_003.contains("prompt_hash"),
            "migration 003 should add session_logs.prompt_hash"
        );
        assert!(
            MIGRATION_003.contains("latency_ms"),
            "migration 003 should add session_logs.latency_ms"
        );
        assert!(
            MIGRATION_003.contains("relevance_score"),
            "migration 003 should add skill_usage.relevance_score"
        );
        assert!(
            MIGRATION_003.contains("ADD COLUMN IF NOT EXISTS"),
            "migration 003 must use ADD COLUMN IF NOT EXISTS (non-rewriting)"
        );
    }

    #[test]
    fn truncate_all_tables_sql_includes_usage_tables() {
        // Guard: asserts against `TRUNCATE_ALL_TABLES_SQL` — the same string that
        // `truncate_all_tables` actually sends to Postgres — so any table omitted
        // from the runtime function causes this test to fail rather than passing
        // silently against a stale hardcoded copy.
        assert!(
            TRUNCATE_ALL_TABLES_SQL.contains("session_logs"),
            "truncate SQL must include session_logs to prevent usage-row leakage"
        );
        assert!(
            TRUNCATE_ALL_TABLES_SQL.contains("skill_usage"),
            "truncate SQL must include skill_usage to prevent usage-row leakage"
        );
        assert!(
            TRUNCATE_ALL_TABLES_SQL.contains("embedding_model_metadata"),
            "truncate SQL must include embedding_model_metadata to prevent stale active-model row leakage"
        );
    }

    #[test]
    fn strip_begin_commit_wrapper_removes_outer_transaction_control() {
        // Valid wrapped SQL: stripped body should contain neither the leading
        // BEGIN; nor the trailing COMMIT;.
        let sql = "BEGIN;\n\nCREATE TABLE foo(id INT);\n\nCOMMIT;\n";
        let body = strip_begin_commit_wrapper("test_migration", sql).unwrap();
        assert!(!body.starts_with("BEGIN"), "body must not start with BEGIN");
        assert!(
            !body.trim_end().ends_with("COMMIT;"),
            "body must not end with COMMIT;"
        );
        assert!(
            body.contains("CREATE TABLE foo(id INT);"),
            "body must retain inner DDL"
        );
    }

    #[test]
    fn strip_begin_commit_wrapper_preserves_dollar_quoted_begin_end() {
        // PL/pgSQL BEGIN…END inside $$…$$ must not be touched.
        let sql = "BEGIN;\n\nDO $$\nBEGIN\n  RAISE NOTICE 'hi';\nEND\n$$;\n\nCOMMIT;\n";
        let body = strip_begin_commit_wrapper("004", sql).unwrap();
        assert!(
            body.contains("DO $$\nBEGIN\n  RAISE NOTICE 'hi';\nEND\n$$;"),
            "dollar-quoted BEGIN…END block must be preserved verbatim"
        );
        assert!(
            !body.trim_start().starts_with("BEGIN"),
            "outer BEGIN; must be stripped"
        );
    }

    #[test]
    fn strip_begin_commit_wrapper_rejects_unwrapped_sql() {
        // SQL that doesn't start with BEGIN; must be rejected with a loud error.
        let sql = "CREATE TABLE foo(id INT);\n";
        let result = strip_begin_commit_wrapper("999_bad", sql);
        assert!(result.is_err(), "unwrapped SQL must produce an error");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("999_bad") && msg.contains("BEGIN;"),
            "error must name the migration id and mention the expected wrapper: {msg}"
        );
    }

    #[test]
    fn strip_begin_commit_wrapper_rejects_missing_commit() {
        // SQL that starts with BEGIN; but has no trailing COMMIT; must be rejected.
        let sql = "BEGIN;\n\nCREATE TABLE foo(id INT);\n";
        let result = strip_begin_commit_wrapper("999_no_commit", sql);
        assert!(
            result.is_err(),
            "SQL missing trailing COMMIT; must produce an error"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("999_no_commit") && msg.contains("COMMIT;"),
            "error must name the migration id and mention the expected wrapper: {msg}"
        );
    }

    /// Live Postgres: proves that a failing migration body is rolled back atomically.
    ///
    /// A migration that creates a table and then hits an intentional error must
    /// leave (a) no row in `schema_migrations`, (b) the created table absent from
    /// the schema — proving the apply-DDL and record-id steps are genuinely atomic.
    ///
    /// This test is the RED/GREEN gate for the wrapper-stripping atomicity fix.
    #[tokio::test]
    #[ignore = "requires live postgres"]
    async fn live_failing_migration_rolls_back_atomically() {
        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        let admin_pool = sqlx::PgPool::connect(&db_url)
            .await
            .expect("admin pool connect");

        let scratch_schema = format!(
            "test_rollback_atomicity_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        sqlx::query(&format!("CREATE SCHEMA {scratch_schema}"))
            .execute(&admin_pool)
            .await
            .expect("create scratch schema");

        let scratch_url = format!("{db_url}?options=-csearch_path%3D{scratch_schema}");
        let config = PostgresConfig {
            database_url: scratch_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("scratch adapter connect");

        // Bootstrap schema_migrations so the adapter can record (or fail to record).
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                id         TEXT        PRIMARY KEY,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )",
        )
        .execute(adapter.pool())
        .await
        .expect("bootstrap schema_migrations");

        // A migration body that creates a table, then hits a syntax error.
        // The table creation must be rolled back along with the failed record-id step.
        let failing_sql = "BEGIN;\n\nCREATE TABLE atomic_probe_table (id INT);\n\
                           SELECT boom_intentional_error_column_does_not_exist;\n\nCOMMIT;\n";
        let migration_id = "test_atomic_rollback";

        // Call the internal helper directly to test the atomic apply-and-record.
        let result = adapter.apply_and_record(migration_id, failing_sql).await;

        // (a) apply_and_record must return Err.
        assert!(
            result.is_err(),
            "a failing migration must return Err; got Ok instead"
        );

        // (b) schema_migrations must have NO row for this id.
        let recorded: Option<String> =
            sqlx::query_scalar("SELECT id FROM schema_migrations WHERE id = $1")
                .bind(migration_id)
                .fetch_optional(adapter.pool())
                .await
                .expect("query schema_migrations");
        assert!(
            recorded.is_none(),
            "schema_migrations must NOT record a failed migration id; found: {recorded:?}"
        );

        // (c) the partial DDL (table creation) must have been rolled back.
        let table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = current_schema()
                  AND table_name = 'atomic_probe_table'
             )",
        )
        .fetch_one(adapter.pool())
        .await
        .expect("check table existence");
        assert!(
            !table_exists,
            "atomic_probe_table must NOT exist after a rolled-back migration"
        );

        // Cleanup.
        sqlx::query(&format!("DROP SCHEMA {scratch_schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .expect("drop scratch schema");
        admin_pool.close().await;
    }

    /// Live Postgres: proves that `persist_embedding_model_metadata` inserts on
    /// first call and UPSERTs (no PK violation, `updated_at` changes) on second
    /// call.
    ///
    /// Two writes with the same sentinel `key = 'active'` must not produce a
    /// duplicate-key error. The second write must overwrite all fields, including
    /// `updated_at`, which is forced to advance via `pg_sleep(0.01)`.
    ///
    /// Isolation: uses a scratch schema that is dropped on completion.
    #[tokio::test]
    #[ignore = "requires live postgres"]
    async fn live_persist_embedding_model_metadata_inserts_then_upserts() {
        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        let admin_pool = sqlx::PgPool::connect(&db_url)
            .await
            .expect("admin pool connect");

        let scratch_schema = format!(
            "test_embedding_metadata_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        sqlx::query(&format!("CREATE SCHEMA {scratch_schema}"))
            .execute(&admin_pool)
            .await
            .expect("create scratch schema");

        let scratch_url = format!("{db_url}?options=-csearch_path%3D{scratch_schema}");
        let config = PostgresConfig {
            database_url: scratch_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("scratch adapter connect");

        // Apply migrations so `embedding_model_metadata` table exists.
        adapter
            .run_migrations()
            .await
            .expect("run_migrations must succeed");

        // ---- First write: must insert one row with key='active' ----
        adapter
            .persist_embedding_model_metadata(
                "nomic-embed-text",
                768,
                "skills__nomic_embed_text",
                None,
            )
            .await
            .expect("first persist must succeed");

        let row: (String, String, i32, String, Option<String>, chrono::DateTime<chrono::Utc>) =
            sqlx::query_as(
                "SELECT key, model_name, dimension, collection, model_digest, updated_at \
                 FROM embedding_model_metadata WHERE key = 'active'",
            )
            .fetch_one(adapter.pool())
            .await
            .expect("row must exist after first write");

        assert_eq!(row.0, "active", "key must be 'active'");
        assert_eq!(row.1, "nomic-embed-text");
        assert_eq!(row.2, 768);
        assert_eq!(row.3, "skills__nomic_embed_text");
        assert!(row.4.is_none(), "model_digest must be NULL when None passed");
        let first_updated_at = row.5;

        // Advance wall clock enough that NOW() differs from first write.
        sqlx::query("SELECT pg_sleep(0.02)")
            .execute(adapter.pool())
            .await
            .expect("pg_sleep");

        // ---- Second write: must UPSERT (no PK violation), update all fields ----
        adapter
            .persist_embedding_model_metadata(
                "qwen3-embedding:4b",
                2560,
                "skills__qwen3_embedding_4b",
                Some("sha256:abc123"),
            )
            .await
            .expect("second persist must succeed without PK violation");

        let row2: (String, String, i32, String, Option<String>, chrono::DateTime<chrono::Utc>) =
            sqlx::query_as(
                "SELECT key, model_name, dimension, collection, model_digest, updated_at \
                 FROM embedding_model_metadata WHERE key = 'active'",
            )
            .fetch_one(adapter.pool())
            .await
            .expect("row must still exist after second write");

        let row_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM embedding_model_metadata")
                .fetch_one(adapter.pool())
                .await
                .expect("count rows");

        assert_eq!(row_count, 1, "UPSERT must keep exactly one row in the table");
        assert_eq!(row2.1, "qwen3-embedding:4b", "model_name must be updated");
        assert_eq!(row2.2, 2560, "dimension must be updated");
        assert_eq!(row2.3, "skills__qwen3_embedding_4b", "collection must be updated");
        assert_eq!(
            row2.4.as_deref(),
            Some("sha256:abc123"),
            "model_digest must be updated"
        );
        assert!(
            row2.5 > first_updated_at,
            "updated_at must advance on second write (was {first_updated_at}, now {})",
            row2.5
        );

        // Cleanup.
        sqlx::query(&format!("DROP SCHEMA {scratch_schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .expect("drop scratch schema");
        admin_pool.close().await;
    }
}
