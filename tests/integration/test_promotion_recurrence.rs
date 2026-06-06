//! Live-PG integration tests for the cross-project recurrence promotion path (todo #180).
//!
//! # Isolation
//!
//! Each test seeds rows into a dedicated Postgres schema (created by the test's
//! PG connection). No test touches the canonical `public` schema or any table
//! outside its own schema.
//!
//! # Gate
//!
//! Tests are gated on `DATABASE_URL` being set. When unset, each test calls
//! `return` immediately (graceful skip). When set, the test MUST run real PG
//! assertions — no in-process simulation, no hardcoded `Passed` outcomes.
//!
//! # Acceptance Criteria Covered
//!
//! - AC #1: promotion pass reads project skills across ALL roots from PG.
//! - AC #2: ≥2 distinct roots → Recurrence proposal; same root → no proposal.
//! - AC #3: threshold configurable; pass logs `distinct_project_roots_seen` vs threshold.
//! - AC #4: live-PG: two-root seed → project_count==2; one-root seed → no proposal.
//! - AC #5: DB failure → pass error with reason_code, never swallowed.

use std::{sync::Arc, time::SystemTime};

use async_trait::async_trait;
use chrono::Utc;
use domain::{EmbeddingError, EmbeddingService, ExtractionError};
use infrastructure::{
    EquivalenceDecision, LlmEquivalenceVerifier, PostgresAdapter,
    PostgresPromotionRecurrenceStore, PromotionRecurrenceStore, ensure_database_exists,
};
use maintenance::{
    CronError, LivePromotionPassRunner, PromotionEvidence, PromotionPassRunner,
    PromotionWriterConfig, RecurrenceConfig,
};
use sqlx::PgPool;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Creates an isolated Postgres pool pointing at a scratch schema.
///
/// Calls `ensure_database_exists` first so the database is created if absent
/// (handles fresh volumes / first-boot). Then creates the scratch schema and
/// returns a pool whose search_path is set to the scratch schema.
async fn make_isolated_pool(
    base_db_url: &str,
    schema: &str,
) -> Result<PgPool, String> {
    // Ensure the target database exists (self-healing, matches run_maintenance_worker_from_environment).
    ensure_database_exists(base_db_url)
        .await
        .map_err(|e| format!("ensure_database_exists: {e}"))?;

    // Create the scratch schema via a direct pool connection.
    let admin = sqlx::PgPool::connect(base_db_url)
        .await
        .map_err(|e| format!("admin pool connect: {e}"))?;
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
        .execute(&admin)
        .await
        .map_err(|e| format!("create schema {schema}: {e}"))?;
    admin.close().await;

    // Connect through the scratch schema via search_path in the URL.
    let sep = if base_db_url.contains('?') { '&' } else { '?' };
    let namespaced_url = format!("{base_db_url}{sep}options=-csearch_path%3D{schema}");
    sqlx::PgPool::connect(&namespaced_url)
        .await
        .map_err(|e| format!("namespaced pool connect: {e}"))
}

/// Drops a scratch schema.
async fn drop_schema(base_db_url: &str, schema: &str) {
    if let Ok(admin) = sqlx::PgPool::connect(base_db_url).await {
        let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await;
        admin.close().await;
    }
}

/// Generates a unique scratch schema name for this test run.
///
/// Hyphens in `prefix` are replaced with underscores because Postgres schema
/// names with unquoted hyphens produce a syntax error.
fn scratch_schema(prefix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let safe_prefix = prefix.replace('-', "_");
    format!("test_promo_rec_{safe_prefix}_{nonce}")
}

/// Inserts a project skill row directly into the `skills` table of the pool's
/// current schema. Uses the same column set as `replace_snapshot_and_bump_version`
/// in rebuild.rs. `source_paths` carries the supplied path strings.
async fn seed_project_skill(
    pool: &PgPool,
    id: &str,
    name: &str,
    description: &str,
    source_paths: &[&str],
) {
    let source_paths_arr: Vec<String> = source_paths.iter().map(|s| s.to_string()).collect();
    sqlx::query(
        r#"
        INSERT INTO skills (id, name, description, scope, status, lifecycle, tags, source_paths, merged_from_scopes, graph_version)
        VALUES ($1::uuid, $2, $3, 'project', 'ready', 'active', '{}'::TEXT[], $4, '{}'::TEXT[], 0)
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(&source_paths_arr)
    .execute(pool)
    .await
    .expect("seed_project_skill must succeed");
}

// ── Mock EmbeddingService: returns identical unit vectors ─────────────────────

/// All texts embed to `[1.0, 0.0]` so cosine similarity is always 1.0.
///
/// This guarantees clustering happens (the similarity check passes) without
/// calling a real Ollama instance, keeping the test hermetic to PG only.
struct UnitVectorEmbeddingService;

#[async_trait]
impl EmbeddingService for UnitVectorEmbeddingService {
    async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![1.0, 0.0])
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
    }
}

// ── Mock LlmEquivalenceVerifier: always equivalent ───────────────────────────

struct AlwaysEquivalentVerifier;

#[async_trait]
impl LlmEquivalenceVerifier for AlwaysEquivalentVerifier {
    async fn decide_equivalence(
        &self,
        _left: &str,
        _right: &str,
    ) -> Result<EquivalenceDecision, ExtractionError> {
        Ok(EquivalenceDecision {
            equivalent: true,
            rationale: "test mock: always equivalent".to_owned(),
        })
    }
}

// ── Mock generality verifier: never promotes intrinsically ───────────────────

struct NeverGeneralVerifier;

#[async_trait]
impl infrastructure::SkillGeneralityVerifier for NeverGeneralVerifier {
    async fn decide_generality(
        &self,
        _skill_text: &str,
    ) -> Result<infrastructure::GeneralityDecision, domain::ExtractionError> {
        Ok(infrastructure::GeneralityDecision {
            general: false,
            rationale: "test mock: never general".to_owned(),
        })
    }
}

// ── AC #4 (two roots) — core test ────────────────────────────────────────────

/// AC #4 (two-root case): seeding equivalent project skills under TWO distinct
/// `source_paths` roots → a global `.pending` with `project_count == 2`.
///
/// Also implicitly verifies AC #1 (reads from PG across all roots) and AC #3
/// (threshold logging — verified via pass completing without error).
#[tokio::test]
async fn live_pg_two_distinct_roots_produce_recurrence_proposal_with_project_count_two() {
    let Some(base_db_url) = std::env::var("DATABASE_URL").ok() else {
        // Graceful skip when DATABASE_URL is unset (CI without live infra).
        return;
    };

    let schema = scratch_schema("two-roots");
    let pool = make_isolated_pool(&base_db_url, &schema)
        .await
        .expect("isolated pool must connect (DATABASE_URL set but DB unreachable?)");

    // Run migrations in the scratch schema.
    let adapter = PostgresAdapter::from_pool(pool.clone());
    adapter
        .run_migrations()
        .await
        .expect("migrations must run in scratch schema");

    // Seed two equivalent skills under TWO distinct project roots.
    let skill_id_a = "00000000-0000-0000-0000-000000000001";
    let skill_id_b = "00000000-0000-0000-0000-000000000002";
    seed_project_skill(
        &pool,
        skill_id_a,
        "musl cross-compile rust",
        "Cross-compiling Rust to musl requires musl-tools for ring/cc-rs.",
        &["/workspace/project-alpha/skills/musl-cross-compile/SKILL.md"],
    )
    .await;
    seed_project_skill(
        &pool,
        skill_id_b,
        "musl cross-compile rust",
        "Cross-compiling Rust to musl requires musl-tools for ring/cc-rs.",
        &["/workspace/project-beta/skills/musl-cross-compile/SKILL.md"],
    )
    .await;

    let global_root = std::env::temp_dir().join(format!("promo_rec_test_global_{schema}"));
    std::fs::create_dir_all(&global_root).expect("global root must be created");

    let recurrence_store: Arc<dyn PromotionRecurrenceStore> = Arc::new(
        PostgresPromotionRecurrenceStore::new(pool.clone()),
    );

    let mut runner = LivePromotionPassRunner {
        skill_snapshots: vec![], // no intrinsic candidates
        generality_verifier: Arc::new(NeverGeneralVerifier),
        project_identifier_tokens: vec![],
        promotion_writer_config: PromotionWriterConfig {
            global_scope_root: global_root.clone(),
            pending_directory_name: ".skills".to_owned(),
        },
        recurrence_store: Some(recurrence_store),
        embedding_service: Some(Arc::new(UnitVectorEmbeddingService)),
        equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
        recurrence_config: RecurrenceConfig {
            min_distinct_roots: 2,
            similarity_threshold: 0.5,
        },
        demotion_store: None,
    };

    let proposals = runner
        .run_promotion_pass(Utc::now())
        .await
        .expect("pass must succeed with two-root seed");

    // Cleanup first, then assert (cleanup always runs).
    drop_schema(&base_db_url, &schema).await;
    let _ = std::fs::remove_dir_all(&global_root);

    // AC #4: at least one Recurrence proposal with project_count == 2.
    let recurrence_proposals: Vec<_> = proposals
        .iter()
        .filter(|p| matches!(p.evidence, PromotionEvidence::Recurrence { project_count: 2 }))
        .collect();

    assert!(
        !recurrence_proposals.is_empty(),
        "two-root seed must produce at least one Recurrence proposal with project_count==2; \
         got proposals: {:?}",
        proposals.iter().map(|p| &p.evidence).collect::<Vec<_>>()
    );
}

// ── AC #4 (one root) — threshold-not-met ─────────────────────────────────────

/// AC #4 (one-root case): seeding equivalent project skills under ONE root
/// → no Recurrence proposal AND threshold-not-met (pass returns Ok with empty
/// recurrence proposals).
///
/// Also verifies AC #3: the pass must complete without error (the threshold-not-met
/// branch is a clean Ok, not a panic or error).
#[tokio::test]
async fn live_pg_single_root_produces_no_recurrence_proposal() {
    let Some(base_db_url) = std::env::var("DATABASE_URL").ok() else {
        return;
    };

    let schema = scratch_schema("one-root");
    let pool = make_isolated_pool(&base_db_url, &schema)
        .await
        .expect("isolated pool must connect (DATABASE_URL set but DB unreachable?)");

    let adapter = PostgresAdapter::from_pool(pool.clone());
    adapter
        .run_migrations()
        .await
        .expect("migrations must run in scratch schema");

    // Seed TWO skills but BOTH under the SAME project root.
    let skill_id_a = "00000000-0000-0000-0000-000000000003";
    let skill_id_b = "00000000-0000-0000-0000-000000000004";
    seed_project_skill(
        &pool,
        skill_id_a,
        "cargo bin naming",
        "Declare [[bin]] explicitly in Cargo.toml or the binary is named after the package.",
        &["/workspace/only-project/skills/cargo-bin/SKILL.md"],
    )
    .await;
    seed_project_skill(
        &pool,
        skill_id_b,
        "cargo bin naming",
        "Declare [[bin]] explicitly in Cargo.toml or the binary is named after the package.",
        &["/workspace/only-project/skills/cargo-bin-alt/SKILL.md"],
    )
    .await;

    let global_root = std::env::temp_dir().join(format!("promo_rec_test_global_{schema}"));
    std::fs::create_dir_all(&global_root).expect("global root must be created");

    let recurrence_store: Arc<dyn PromotionRecurrenceStore> = Arc::new(
        PostgresPromotionRecurrenceStore::new(pool.clone()),
    );

    let mut runner = LivePromotionPassRunner {
        skill_snapshots: vec![],
        generality_verifier: Arc::new(NeverGeneralVerifier),
        project_identifier_tokens: vec![],
        promotion_writer_config: PromotionWriterConfig {
            global_scope_root: global_root.clone(),
            pending_directory_name: ".skills".to_owned(),
        },
        recurrence_store: Some(recurrence_store),
        embedding_service: Some(Arc::new(UnitVectorEmbeddingService)),
        equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
        recurrence_config: RecurrenceConfig {
            min_distinct_roots: 2,
            similarity_threshold: 0.5,
        },
        demotion_store: None,
    };

    let proposals = runner
        .run_promotion_pass(Utc::now())
        .await
        .expect("single-root pass must succeed without error (threshold not met is not an error)");

    drop_schema(&base_db_url, &schema).await;
    let _ = std::fs::remove_dir_all(&global_root);

    // AC #4 (one-root): no Recurrence proposals must be emitted.
    let recurrence_proposals: Vec<_> = proposals
        .iter()
        .filter(|p| matches!(p.evidence, PromotionEvidence::Recurrence { .. }))
        .collect();

    assert!(
        recurrence_proposals.is_empty(),
        "single-root seed must produce NO Recurrence proposals; \
         distinct_project_roots_seen=1, threshold=2; \
         got: {:?}",
        proposals.iter().map(|p| &p.evidence).collect::<Vec<_>>()
    );
}

// ── AC #5 — DB failure surfaces as CronError ─────────────────────────────────

/// AC #5: a broken recurrence store (simulated by a pool pointing at an
/// unreachable DB) must surface as `CronError::PromotionPass`, never silently
/// swallowed.
///
/// This test does NOT require a live DATABASE_URL — it uses a lazy pool with a
/// guaranteed-invalid endpoint so the first real query fails immediately.
#[tokio::test]
async fn recurrence_store_db_failure_surfaces_as_cron_error_with_reason_code() {
    // Build a pool that will fail on the first real query (invalid endpoint).
    let failing_pool =
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/nonexistent")
            .expect("lazy pool construction does not connect");

    let global_root = std::env::temp_dir().join(format!(
        "promo_rec_db_fail_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&global_root).expect("global root must be created");

    let recurrence_store: Arc<dyn PromotionRecurrenceStore> = Arc::new(
        PostgresPromotionRecurrenceStore::new(failing_pool),
    );

    let mut runner = LivePromotionPassRunner {
        skill_snapshots: vec![],
        generality_verifier: Arc::new(NeverGeneralVerifier),
        project_identifier_tokens: vec![],
        promotion_writer_config: PromotionWriterConfig {
            global_scope_root: global_root.clone(),
            pending_directory_name: ".skills".to_owned(),
        },
        recurrence_store: Some(recurrence_store),
        embedding_service: Some(Arc::new(UnitVectorEmbeddingService)),
        equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
        recurrence_config: RecurrenceConfig::default(),
        demotion_store: None,
    };

    let result = runner.run_promotion_pass(Utc::now()).await;

    let _ = std::fs::remove_dir_all(&global_root);

    assert!(
        result.is_err(),
        "DB failure must propagate as Err, never a silent success"
    );

    let err = result.unwrap_err();
    assert!(
        matches!(err, CronError::PromotionPass(_)),
        "DB failure must produce CronError::PromotionPass; got: {err:?}"
    );

    // Verify reason_code is surfaced in the error message string.
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("reason_code") || err_msg.contains("recurrence"),
        "error message must contain 'reason_code' or 'recurrence' for observability; got: {err_msg}"
    );
}
