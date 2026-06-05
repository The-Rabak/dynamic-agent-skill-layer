use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use domain::EmbeddingService;
use infrastructure::{
    ClaudeMergeVerifier, ClaudeMergeVerifierConfig, LlmEquivalenceVerifier, OllamaEmbeddingConfig,
    OllamaEmbeddingService, OllamaMergeVerifier, OllamaMergeVerifierConfig, PostgresAdapter,
    PostgresConfig, PostgresGraphSnapshotStore, PostgresUsageSampleStore, TranscriptIngestQueue,
    UsageSampleStore,
    logging::{ServiceLoggingConfig, init_service_logging},
};
use session_extractor::SessionExtractor;
use thiserror::Error;
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};

use crate::audit::MaintenanceAuditSink;
use crate::audit_sink::PostgresMaintenanceAuditSink;
use crate::cron::{
    CronDecision, CronError, MaintenanceCron, MergePassRunner, RetirementPassRunner,
};
use crate::merge::{MergeConfig, MergeProposal, MergeProposalWriter, SkillSnapshot};
use crate::merge_verifier::LlmMergeSemanticVerifier;
use crate::retire::{RetirementConfig, RetirementProposal, RetirementProposalWriter, UsageSample};
use crate::transcript_drain::{DEFAULT_TRANSCRIPT_DRAIN_BATCH, TranscriptQueueDrain};

/// Environment variable selecting the merge-verifier LLM provider.
///
/// Accepted values:
/// - unset / blank / `"ollama"` → Ollama `/api/generate` (local-first default)
/// - `"claude"` → Anthropic Messages API (requires `ANTHROPIC_API_KEY`)
pub const MERGE_VERIFIER_PROVIDER_ENV: &str = "MERGE_VERIFIER_PROVIDER";

/// Ollama model used by the merge-verifier generate path.
///
/// Overridable via `MERGE_VERIFIER_MODEL`. Defaults to `gemma4:e4b` (same model
/// as extraction) so a single local Ollama instance covers both workloads.
pub const MERGE_VERIFIER_MODEL_ENV: &str = "MERGE_VERIFIER_MODEL";

pub const DEFAULT_CRON_INTERVAL_SECS: u64 = 60;
pub const CRON_INTERVAL_ENV: &str = "MAINTENANCE_CRON_INTERVAL_SECS";
pub const RUN_ONCE_ENV: &str = "MAINTENANCE_RUN_ONCE";

/// Returns the value of `name` from the environment, or an error message suitable
/// for boot-time failure. Required env vars must be present; there is no default.
fn env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} must be set"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaintenanceWorkerConfig {
    pub cron_interval: Duration,
    pub run_once: bool,
}

impl MaintenanceWorkerConfig {
    pub fn from_environment() -> Result<Self, MaintenanceRuntimeError> {
        let interval_seconds = match std::env::var(CRON_INTERVAL_ENV) {
            Ok(raw) => parse_positive_seconds(CRON_INTERVAL_ENV, &raw)?,
            Err(_) => DEFAULT_CRON_INTERVAL_SECS,
        };
        let run_once = match std::env::var(RUN_ONCE_ENV) {
            Ok(raw) => parse_boolean_switch(RUN_ONCE_ENV, &raw)?,
            Err(_) => false,
        };
        Ok(Self {
            cron_interval: Duration::from_secs(interval_seconds),
            run_once,
        })
    }
}

pub struct LiveMergePassRunner<A = PostgresMaintenanceAuditSink>
where
    A: MaintenanceAuditSink,
{
    pub snapshot_store: PostgresGraphSnapshotStore,
    pub scope_roots: Vec<std::path::PathBuf>,
    audit_sink: A,
    embedding_service: Arc<dyn EmbeddingService>,
}

impl LiveMergePassRunner<PostgresMaintenanceAuditSink> {
    pub fn new(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        audit_adapter: &PostgresAdapter,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            audit_sink: PostgresMaintenanceAuditSink::from_pool(audit_adapter.pool().clone()),
            embedding_service,
        }
    }
}

impl<A> LiveMergePassRunner<A>
where
    A: MaintenanceAuditSink,
{
    pub fn with_audit_sink(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        audit_sink: A,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            audit_sink,
            embedding_service,
        }
    }
}

#[async_trait]
impl<A> MergePassRunner for LiveMergePassRunner<A>
where
    A: MaintenanceAuditSink + Send,
{
    async fn run_merge_pass(
        &mut self,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<MergeProposal>, CronError> {
        let skills =
            load_skill_snapshots(&self.scope_roots, self.embedding_service.as_ref())
                .await
                .map_err(|e| CronError::MergePass(e.to_string()))?;
        if skills.len() < 2 {
            return Ok(Vec::new());
        }
        let llm = build_merge_verifier_from_environment()
            .map_err(|e| CronError::MergePass(e.to_string()))?;
        let verifier = LlmMergeSemanticVerifier::new(llm);
        let config = MergeConfig {
            similarity_threshold: 0.85,
            ..MergeConfig::default()
        };
        let writer = MergeProposalWriter::with_audit_sink(config, verifier, &self.audit_sink);
        writer
            .propose(&skills, now)
            .await
            .map_err(|e| CronError::MergePass(e.to_string()))
    }
}

pub struct LiveRetirementPassRunner<A = PostgresMaintenanceAuditSink>
where
    A: MaintenanceAuditSink,
{
    pub snapshot_store: PostgresGraphSnapshotStore,
    pub scope_roots: Vec<std::path::PathBuf>,
    pub retirement_config: RetirementConfig,
    audit_sink: A,
    embedding_service: Arc<dyn EmbeddingService>,
    /// Optional usage store for real retirement scoring (T06).
    ///
    /// When `Some`, `run_retirement_pass` queries real usage aggregates so
    /// recently-used skills are not propose-retired. When `None` (offline tests,
    /// no PG pool), every skill scores zero usage and all are eligible —
    /// the pre-T06 behaviour preserved for deterministic test isolation.
    usage_store: Option<Arc<dyn UsageSampleStore>>,
}

impl LiveRetirementPassRunner<PostgresMaintenanceAuditSink> {
    /// Creates a runner with a live usage store so retirement scoring is based
    /// on real usage data (T06).
    pub fn new_with_usage_store(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        retirement_config: RetirementConfig,
        audit_adapter: &PostgresAdapter,
        usage_store: Arc<dyn UsageSampleStore>,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            retirement_config,
            audit_sink: PostgresMaintenanceAuditSink::from_pool(audit_adapter.pool().clone()),
            embedding_service,
            usage_store: Some(usage_store),
        }
    }
}

impl<A> LiveRetirementPassRunner<A>
where
    A: MaintenanceAuditSink,
{
    pub fn with_audit_sink(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        retirement_config: RetirementConfig,
        audit_sink: A,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            retirement_config,
            audit_sink,
            embedding_service,
            usage_store: None,
        }
    }

    /// Wires a usage store into an audit-sink-constructed runner.
    ///
    /// Used in unit tests to attach a stub store without a live Postgres pool.
    #[cfg(test)]
    fn with_usage_store(mut self, usage_store: Arc<dyn UsageSampleStore>) -> Self {
        self.usage_store = Some(usage_store);
        self
    }
}

#[async_trait]
impl<A> RetirementPassRunner for LiveRetirementPassRunner<A>
where
    A: MaintenanceAuditSink + Send,
{
    async fn run_retirement_pass(
        &mut self,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, CronError> {
        let skills =
            load_skill_snapshots(&self.scope_roots, self.embedding_service.as_ref())
                .await
                .map_err(|e| CronError::RetirementPass(e.to_string()))?;
        if skills.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch real usage samples when a store is wired (T06). The async trait
        // lets us await the query directly without any blocking bridge.
        let usage_samples = if let Some(store) = &self.usage_store {
            let skill_ids: Vec<String> = skills.iter().map(|s| s.id.clone()).collect();
            let window_days = self.retirement_config.scoring_window_days;
            let summaries = store
                .recent_usage(&skill_ids, window_days)
                .await
                .map_err(|e| CronError::RetirementPass(format!("usage query failed: {e}")))?;

            summaries
                .into_iter()
                .filter_map(|summary| {
                    if summary.windowed_count == 0 {
                        return None;
                    }
                    // Emit ONE sample with `usage_count = windowed_count` rather
                    // than fanning out into `windowed_count` unit-count clones.
                    // The retirement scorer sums `sample.usage_count * recency_weight`
                    // per sample, so one sample of N produces the same weighted sum
                    // as N samples of 1 — same math, O(1) allocations per skill
                    // instead of O(windowed_count).
                    let used_at = summary.last_used_at.unwrap_or(now);
                    Some(UsageSample {
                        skill_id: summary.skill_id,
                        used_at,
                        usage_count: summary.windowed_count,
                    })
                })
                .collect::<Vec<UsageSample>>()
        } else {
            Vec::new()
        };

        let writer = RetirementProposalWriter::with_audit_sink(
            self.retirement_config.clone(),
            &self.audit_sink,
        );
        writer
            .propose(&skills, &usage_samples, now)
            .map_err(|e| CronError::RetirementPass(e.to_string()))
    }
}

/// Loads skill snapshots by walking the given scope-root paths and embedding their text.
///
/// Returns an empty slice immediately when `scope_roots` is empty, avoiding
/// an unnecessary `embed_batch` call (which the real embedder rejects for empty inputs).
async fn load_skill_snapshots(
    scope_roots: &[std::path::PathBuf],
    embedding_service: &dyn EmbeddingService,
) -> Result<Vec<SkillSnapshot>, String> {
    use domain::{ScopeRoot, ScopeType};
    use graph_builder::graph::build::build_skills_from_scope_roots;

    if scope_roots.is_empty() {
        return Ok(Vec::new());
    }

    let roots: Vec<ScopeRoot> = scope_roots
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let scope_id = format!("scope-{i}");
            let scope_type = if path.to_str().is_some_and(|s| s.contains("global")) {
                ScopeType::Global
            } else {
                ScopeType::Project
            };
            ScopeRoot::new(&scope_id, scope_type, path.clone())
        })
        .collect();

    let built = build_skills_from_scope_roots(&roots, embedding_service)
        .await
        .map_err(|e| e.to_string())?;

    Ok(built
        .into_iter()
        .map(|skill| SkillSnapshot {
            id: skill.id,
            name: skill.name,
            description: skill.description,
            scope: skill.scope_type,
            source_path: skill.source_path,
            tags: skill.tags,
            subunits: skill.subunits.into_iter().map(|s| s.content).collect(),
            embedding: skill.embedding,
        })
        .collect())
}

/// No-op merge runner for tests that need a `MergePassRunner` but no real merge work.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Default)]
pub struct NoopMergePassRunner;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl MergePassRunner for NoopMergePassRunner {
    async fn run_merge_pass(
        &mut self,
        _now: chrono::DateTime<Utc>,
    ) -> Result<Vec<MergeProposal>, CronError> {
        Ok(Vec::new())
    }
}

/// No-op retirement runner for tests that need a `RetirementPassRunner` but no real retirement work.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Default)]
pub struct NoopRetirementPassRunner;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl RetirementPassRunner for NoopRetirementPassRunner {
    async fn run_retirement_pass(
        &mut self,
        _now: chrono::DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, CronError> {
        Ok(Vec::new())
    }
}

pub async fn run_maintenance_worker(
    config: MaintenanceWorkerConfig,
    cron: &mut MaintenanceCron,
    merge_runner: &mut impl MergePassRunner,
    retirement_runner: &mut impl RetirementPassRunner,
    transcript_drain: Option<&TranscriptQueueDrain>,
) -> Result<(), MaintenanceRuntimeError> {
    // Startup catch-up sweep: a laptop that was closed (no hook fired cleanly)
    // still gets its queued transcripts drained as soon as the worker boots.
    drain_transcripts(transcript_drain).await;

    if config.run_once {
        run_one_tick(cron, merge_runner, retirement_runner).await?;
        return Ok(());
    }

    let mut ticker = tokio::time::interval(config.cron_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if let Err(error) = run_one_tick(cron, merge_runner, retirement_runner).await {
            error!(
                reason_code = error.reason_code(),
                error = %error,
                "maintenance cron tick failed"
            );
            return Err(error);
        }
        // The drain shares the cron cadence but never aborts the worker: a
        // transient PG/extractor error is logged and retried next sweep (rows
        // stay `pending`), so steady-state maintenance keeps running.
        drain_transcripts(transcript_drain).await;
    }
}

/// Runs one transcript-queue drain sweep, logging (never propagating) failures.
async fn drain_transcripts(transcript_drain: Option<&TranscriptQueueDrain>) {
    let Some(drain) = transcript_drain else {
        return;
    };
    match drain.drain_once().await {
        Ok(report) => {
            if report.claimed > 0 {
                info!(
                    claimed = report.claimed,
                    processed = report.processed,
                    failed = report.failed,
                    "transcript ingest queue drained"
                );
            }
        }
        Err(error) => {
            error!(error = %error, "transcript ingest queue drain failed; will retry next sweep");
        }
    }
}

pub async fn run_maintenance_worker_from_environment() -> Result<(), MaintenanceRuntimeError> {
    let environment = std::env::var("APP_ENV")
        .or_else(|_| std::env::var("ENVIRONMENT"))
        .unwrap_or_else(|_| "local".to_owned());
    init_service_logging(ServiceLoggingConfig::new(
        "maintenance-worker",
        env!("CARGO_PKG_VERSION"),
        environment,
        "info",
    ))
    .map_err(|error| MaintenanceRuntimeError::Logging(error.to_string()))?;

    let config = MaintenanceWorkerConfig::from_environment()?;
    let mut cron = MaintenanceCron::new(config.cron_interval)?;

    let db_url = env_var("DATABASE_URL")
        .map_err(|error| MaintenanceRuntimeError::InvalidConfiguration(error))?;
    let pg_adapter = PostgresAdapter::connect(&PostgresConfig {
        database_url: db_url,
        ..PostgresConfig::default()
    })
    .await
    .map_err(|error| MaintenanceRuntimeError::InvalidConfiguration(error.to_string()))?;
    let snapshot_store = PostgresGraphSnapshotStore::new(pg_adapter.pool().clone());

    let scope_roots = build_scope_roots_from_environment();

    let usage_store: Arc<dyn UsageSampleStore> =
        Arc::new(PostgresUsageSampleStore::new(pg_adapter.pool().clone()));

    let embedding_service = build_embedding_service_from_environment()
        .map_err(|e| MaintenanceRuntimeError::InvalidConfiguration(e.to_string()))?;

    let mut merge_runner = LiveMergePassRunner::new(
        snapshot_store.clone(),
        scope_roots.clone(),
        &pg_adapter,
        Arc::clone(&embedding_service),
    );
    let mut retirement_runner = LiveRetirementPassRunner::new_with_usage_store(
        snapshot_store,
        scope_roots,
        RetirementConfig::default(),
        &pg_adapter,
        usage_store,
        embedding_service,
    );

    let transcript_drain = build_transcript_drain(&pg_adapter);

    run_maintenance_worker(
        config,
        &mut cron,
        &mut merge_runner,
        &mut retirement_runner,
        transcript_drain.as_ref(),
    )
    .await
}

/// Builds the transcript-queue drain, or `None` when unconstructable.
///
/// Returns `None` (degraded, but the worker still runs merge/retire) when the
/// extractor cannot be built from the environment — e.g. `CLAUDE_TRANSCRIPT_ROOT`
/// / `SKILL_GLOBAL_PATHS` not configured on the maintenance container. The drain
/// only ever uses `transcript_inline`, so the transcript root is never read; it is
/// required solely by `SessionExtractor::from_environment`'s construction contract.
fn build_transcript_drain(pg_adapter: &PostgresAdapter) -> Option<TranscriptQueueDrain> {
    let extractor = match SessionExtractor::from_environment() {
        Ok(extractor) => extractor,
        Err(error) => {
            warn!(
                error = %error,
                "transcript queue drain unavailable: could not build session extractor \
                 from environment; SessionEnd hook path still functions"
            );
            return None;
        }
    };

    let queue = TranscriptIngestQueue::new(pg_adapter.pool().clone());
    info!("transcript ingest queue drain enabled");
    Some(TranscriptQueueDrain::new(
        queue,
        extractor,
        DEFAULT_TRANSCRIPT_DRAIN_BATCH,
    ))
}

/// Builds a real Ollama embedding service from `OLLAMA_URL`.
///
/// Returns an error (causing the worker to fail at boot) when `OLLAMA_URL` is unset.
/// There is no silent fallback — missing configuration must surface loudly.
fn build_embedding_service_from_environment() -> Result<Arc<dyn EmbeddingService>, String> {
    let base_url =
        std::env::var("OLLAMA_URL").map_err(|_| "OLLAMA_URL must be set".to_owned())?;
    let config = OllamaEmbeddingConfig {
        base_url,
        model: "nomic-embed-text".to_owned(),
        timeout_ms: 5_000,
        batch_timeout_ms: 10_000,
        max_concurrency: 4,
    };
    let service = OllamaEmbeddingService::from_config(config)
        .map_err(|e| format!("OllamaEmbeddingService init failed: {e}"))?;
    Ok(Arc::new(service) as Arc<dyn EmbeddingService>)
}

/// Builds the merge-verifier LLM provider from `MERGE_VERIFIER_PROVIDER`.
///
/// Provider routing:
/// - unset / blank / `"ollama"` → Ollama `/api/generate` using `OLLAMA_URL` + `MERGE_VERIFIER_MODEL`
/// - `"claude"` → Anthropic Messages API using `ANTHROPIC_API_KEY`
///
/// Missing required configuration (e.g. `OLLAMA_URL` for Ollama, or `ANTHROPIC_API_KEY`
/// for Claude) returns `Err` and the merge pass fails loudly at startup — there is no
/// silent fallback.
fn build_merge_verifier_from_environment() -> Result<Arc<dyn LlmEquivalenceVerifier>, String> {
    let provider_raw = std::env::var(MERGE_VERIFIER_PROVIDER_ENV).unwrap_or_default();
    match provider_raw.trim().to_ascii_lowercase().as_str() {
        "" | "ollama" => {
            let base_url = env_var("OLLAMA_URL")
                .map_err(|e| format!("merge verifier (Ollama): {e}"))?;
            let model = std::env::var(MERGE_VERIFIER_MODEL_ENV)
                .unwrap_or_else(|_| "gemma4:e4b".to_owned());
            let endpoint = format!("{}/api/generate", base_url.trim_end_matches('/'));
            let config = OllamaMergeVerifierConfig {
                endpoint,
                model,
                timeout_ms: 60_000,
            };
            let verifier = OllamaMergeVerifier::from_config(config)
                .map_err(|e| format!("OllamaMergeVerifier init failed: {e}"))?;
            Ok(Arc::new(verifier) as Arc<dyn LlmEquivalenceVerifier>)
        }
        "claude" => {
            let api_key = env_var("ANTHROPIC_API_KEY")
                .map_err(|e| format!("merge verifier (Claude): {e}"))?;
            let mut config = ClaudeMergeVerifierConfig::default();
            config.api_key = api_key;
            if let Ok(base_url) = std::env::var("ANTHROPIC_BASE_URL")
                && !base_url.trim().is_empty()
            {
                config.base_url = base_url;
            }
            let verifier = ClaudeMergeVerifier::from_config(config)
                .map_err(|e| format!("ClaudeMergeVerifier init failed: {e}"))?;
            Ok(Arc::new(verifier) as Arc<dyn LlmEquivalenceVerifier>)
        }
        other => Err(format!(
            "{MERGE_VERIFIER_PROVIDER_ENV} must be one of [ollama, claude], got `{other}`"
        )),
    }
}

fn build_scope_roots_from_environment() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Ok(project_root) = std::env::var("GRAPH_BUILDER_PROJECT_ROOT") {
        let path = std::path::PathBuf::from(project_root);
        if path.is_dir() {
            roots.push(path);
        }
    }
    if let Ok(global_root) = std::env::var("GRAPH_BUILDER_GLOBAL_ROOT") {
        let path = std::path::PathBuf::from(global_root);
        if path.is_dir() {
            roots.push(path);
        }
    }
    if roots.is_empty()
        && let Ok(cwd) = std::env::current_dir()
    {
        roots.push(cwd);
    }
    roots
}

#[derive(Debug, Error)]
pub enum MaintenanceRuntimeError {
    #[error("invalid maintenance runtime configuration: {0}")]
    InvalidConfiguration(String),
    #[error("maintenance cron runtime failed: {0}")]
    Cron(#[from] CronError),
    #[error("maintenance runtime logging setup failed: {0}")]
    Logging(String),
}

impl MaintenanceRuntimeError {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration(_) => "maintenance_runtime_invalid_configuration",
            Self::Cron(error) => error.reason_code(),
            Self::Logging(_) => "maintenance_runtime_logging_failed",
        }
    }
}

async fn run_one_tick(
    cron: &mut MaintenanceCron,
    merge_runner: &mut impl MergePassRunner,
    retirement_runner: &mut impl RetirementPassRunner,
) -> Result<(), MaintenanceRuntimeError> {
    let decision = cron
        .tick(Utc::now(), merge_runner, retirement_runner)
        .await?;
    match decision {
        CronDecision::SkippedNotDue { now, next_due_at } => {
            info!(
                status = "skipped_not_due",
                now = %now,
                next_due_at = %next_due_at,
                "maintenance cron tick skipped"
            );
        }
        CronDecision::Executed(outcome) => {
            info!(
                status = "executed",
                started_at = %outcome.started_at,
                completed_at = %outcome.completed_at,
                merge_proposals = outcome.merge_proposals.len(),
                retirement_proposals = outcome.retirement_proposals.len(),
                "maintenance cron tick executed"
            );
        }
    }
    Ok(())
}

fn parse_positive_seconds(name: &str, raw_value: &str) -> Result<u64, MaintenanceRuntimeError> {
    let parsed = raw_value.parse::<u64>().map_err(|error| {
        MaintenanceRuntimeError::InvalidConfiguration(format!(
            "{name} must be a positive integer, got `{raw_value}`: {error}"
        ))
    })?;
    if parsed == 0 {
        return Err(MaintenanceRuntimeError::InvalidConfiguration(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn parse_boolean_switch(name: &str, raw_value: &str) -> Result<bool, MaintenanceRuntimeError> {
    match raw_value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(MaintenanceRuntimeError::InvalidConfiguration(format!(
            "{name} must be one of [1,true,yes,0,false,no], got `{raw_value}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::DateTime;
    use infrastructure::{SkillUsageSummary, UsagePersistenceError};
    use sqlx::postgres::PgPoolOptions;

    #[derive(Default)]
    struct CountingMergeRunner {
        invocations: usize,
    }

    #[async_trait]
    impl MergePassRunner for CountingMergeRunner {
        async fn run_merge_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<MergeProposal>, CronError> {
            self.invocations += 1;
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct CountingRetirementRunner {
        invocations: usize,
    }

    #[async_trait]
    impl RetirementPassRunner for CountingRetirementRunner {
        async fn run_retirement_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<RetirementProposal>, CronError> {
            self.invocations += 1;
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn run_once_executes_one_maintenance_tick() {
        let config = MaintenanceWorkerConfig {
            cron_interval: Duration::from_secs(60),
            run_once: true,
        };
        let mut cron = MaintenanceCron::new(Duration::from_secs(60)).expect("cron init");
        let mut merge_runner = CountingMergeRunner::default();
        let mut retirement_runner = CountingRetirementRunner::default();

        run_maintenance_worker(
            config,
            &mut cron,
            &mut merge_runner,
            &mut retirement_runner,
            None,
        )
        .await
        .expect("runtime should execute once");

        assert_eq!(merge_runner.invocations, 1);
        assert_eq!(retirement_runner.invocations, 1);
    }

    #[test]
    fn parse_positive_seconds_rejects_zero() {
        let error = parse_positive_seconds("MAINTENANCE_CRON_INTERVAL_SECS", "0")
            .expect_err("zero interval should fail");
        assert!(matches!(
            error,
            MaintenanceRuntimeError::InvalidConfiguration(_)
        ));
    }

    #[test]
    fn parse_boolean_switch_accepts_expected_values() {
        assert!(parse_boolean_switch("MAINTENANCE_RUN_ONCE", "true").expect("true"));
        assert!(!parse_boolean_switch("MAINTENANCE_RUN_ONCE", "0").expect("0"));
    }

    /// Stub store that returns empty usage results immediately.
    ///
    /// Used to prove the async retirement path on a current-thread runtime
    /// without a live Postgres connection.
    struct EmptyUsageSampleStore;

    #[async_trait]
    impl UsageSampleStore for EmptyUsageSampleStore {
        async fn recent_usage(
            &self,
            _skill_ids: &[String],
            _window_days: i64,
        ) -> Result<Vec<SkillUsageSummary>, UsagePersistenceError> {
            Ok(Vec::new())
        }
    }

    /// Proves that `LiveRetirementPassRunner` with a usage store wired can be
    /// driven on a current-thread `#[tokio::test]` runtime without panicking.
    ///
    /// Previously, the `block_in_place` bridge would panic with "can only
    /// block_in_place on a multi-threaded runtime" on this default runtime.
    #[tokio::test]
    async fn live_retirement_runner_with_usage_store_does_not_panic_on_current_thread_runtime() {
        // Build a lazy pool that never actually connects — the runner's
        // `run_retirement_pass` returns early (empty scope_roots -> empty skills)
        // before touching the pool, so no real DB is needed.
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://unused:unused@localhost/unused")
            .expect("lazy pool construction should not connect");
        let snapshot_store = PostgresGraphSnapshotStore::new(pool);

        let usage_store: Arc<dyn UsageSampleStore> = Arc::new(EmptyUsageSampleStore);
        let embedding_service: Arc<dyn EmbeddingService> = Arc::new(
            graph_builder::graph::embeddings::DeterministicEmbeddingService,
        );
        let mut runner = LiveRetirementPassRunner::with_audit_sink(
            snapshot_store,
            Vec::new(), // empty scope_roots: load_skill_snapshots returns [] -> early return
            RetirementConfig::default(),
            crate::audit::NoopMaintenanceAuditSink,
            embedding_service,
        )
        .with_usage_store(usage_store);

        let result = runner.run_retirement_pass(Utc::now()).await;
        assert!(
            result.is_ok(),
            "retirement pass with usage store must succeed on current-thread runtime"
        );
        assert!(
            result.unwrap().is_empty(),
            "no skills loaded -> no proposals expected"
        );
    }

    #[test]
    fn env_var_returns_err_with_must_be_set_message_when_unset() {
        // Use a name that cannot exist in the environment during tests.
        let result = env_var("MAINTENANCE_TEST_DEFINITELY_UNSET_VAR_167");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "MAINTENANCE_TEST_DEFINITELY_UNSET_VAR_167 must be set"
        );
    }

    #[test]
    fn env_var_returns_value_when_set() {
        // SAFETY: single-threaded test; no other thread reads this var concurrently.
        unsafe {
            std::env::set_var("MAINTENANCE_TEST_SET_VAR_167", "postgres://localhost/test");
        }
        let result = env_var("MAINTENANCE_TEST_SET_VAR_167");
        // SAFETY: single-threaded test; no other thread reads this var concurrently.
        unsafe {
            std::env::remove_var("MAINTENANCE_TEST_SET_VAR_167");
        }
        assert_eq!(result.unwrap(), "postgres://localhost/test");
    }
}
