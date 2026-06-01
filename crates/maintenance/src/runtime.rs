use std::{sync::Arc, time::Duration};

use chrono::Utc;
use infrastructure::{
    PostgresAdapter, PostgresConfig, PostgresGraphSnapshotStore, PostgresUsageSampleStore,
    TranscriptIngestQueue, UsageSampleStore,
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
use crate::merge_verifier::TextOverlapMergeSemanticVerifier;
use crate::retire::{RetirementConfig, RetirementProposal, RetirementProposalWriter, UsageSample};
use crate::transcript_drain::{DEFAULT_TRANSCRIPT_DRAIN_BATCH, TranscriptQueueDrain};

pub const DEFAULT_CRON_INTERVAL_SECS: u64 = 60;
pub const CRON_INTERVAL_ENV: &str = "MAINTENANCE_CRON_INTERVAL_SECS";
pub const RUN_ONCE_ENV: &str = "MAINTENANCE_RUN_ONCE";

/// Rollback flag: set `MAINTENANCE_TRANSCRIPT_DRAIN=off` to disable the durable
/// transcript-ingest queue drain (folds the T07 reconcile flag).
///
// TODO(remove-after-v1.5-green): delete this flag and always run the drain once
// the queue path is proven in deployment. Removal criterion: first green CI on
// `main` with the self-growth loop (SC-B) exercised end-to-end through the queue.
pub const TRANSCRIPT_DRAIN_FLAG: &str = "MAINTENANCE_TRANSCRIPT_DRAIN";

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
}

impl LiveMergePassRunner<PostgresMaintenanceAuditSink> {
    pub fn new(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        audit_adapter: &PostgresAdapter,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            audit_sink: PostgresMaintenanceAuditSink::from_pool(audit_adapter.pool().clone()),
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
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            audit_sink,
        }
    }
}

impl<A> MergePassRunner for LiveMergePassRunner<A>
where
    A: MaintenanceAuditSink,
{
    fn run_merge_pass(
        &mut self,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<MergeProposal>, CronError> {
        let skills = load_skill_snapshots(&self.scope_roots)
            .map_err(|e| CronError::MergePass(e.to_string()))?;
        if skills.len() < 2 {
            return Ok(Vec::new());
        }
        let verifier = TextOverlapMergeSemanticVerifier::default();
        let config = MergeConfig {
            similarity_threshold: 0.85,
            ..MergeConfig::default()
        };
        let writer = MergeProposalWriter::with_audit_sink(config, verifier, &self.audit_sink);
        writer
            .propose(&skills, now)
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
    /// Optional usage store for real retirement scoring (T06).
    ///
    /// When `Some`, `run_retirement_pass` queries real usage aggregates so
    /// recently-used skills are not propose-retired. When `None` (offline tests,
    /// no PG pool), every skill scores zero usage and all are eligible —
    /// the pre-T06 behaviour preserved for deterministic test isolation.
    usage_store: Option<Arc<dyn UsageSampleStore>>,
}

impl LiveRetirementPassRunner<PostgresMaintenanceAuditSink> {
    pub fn new(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        retirement_config: RetirementConfig,
        audit_adapter: &PostgresAdapter,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            retirement_config,
            audit_sink: PostgresMaintenanceAuditSink::from_pool(audit_adapter.pool().clone()),
            usage_store: None,
        }
    }

    /// Creates a runner with a live usage store so retirement scoring is based
    /// on real usage data (T06).
    pub fn new_with_usage_store(
        snapshot_store: PostgresGraphSnapshotStore,
        scope_roots: Vec<std::path::PathBuf>,
        retirement_config: RetirementConfig,
        audit_adapter: &PostgresAdapter,
        usage_store: Arc<dyn UsageSampleStore>,
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            retirement_config,
            audit_sink: PostgresMaintenanceAuditSink::from_pool(audit_adapter.pool().clone()),
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
    ) -> Self {
        Self {
            snapshot_store,
            scope_roots,
            retirement_config,
            audit_sink,
            usage_store: None,
        }
    }
}

impl<A> RetirementPassRunner for LiveRetirementPassRunner<A>
where
    A: MaintenanceAuditSink,
{
    fn run_retirement_pass(
        &mut self,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, CronError> {
        let skills = load_skill_snapshots(&self.scope_roots)
            .map_err(|e| CronError::RetirementPass(e.to_string()))?;
        if skills.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch real usage samples when a store is wired (T06). Uses
        // block_in_place so the sync trait impl can drive the async query
        // without spawning a separate thread.
        let usage_samples = if let Some(store) = &self.usage_store {
            let skill_ids: Vec<String> = skills.iter().map(|s| s.id.clone()).collect();
            let window_days = self.retirement_config.scoring_window_days;
            let summaries = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(store.recent_usage(&skill_ids, window_days))
            })
            .map_err(|e| CronError::RetirementPass(format!("usage query failed: {e}")))?;

            summaries
                .into_iter()
                .flat_map(|summary| {
                    // Emit one UsageSample per usage count unit so the retirement
                    // scorer's recency-weighted sum is accurate. Each sample
                    // represents a single historical selection anchored to
                    // `last_used_at` (best available without the per-event log).
                    // For V1.5 accuracy this is sufficient — the scorer sees
                    // `windowed_count` usages at the last-seen timestamp.
                    if summary.windowed_count == 0 {
                        return Vec::new();
                    }
                    let used_at = summary.last_used_at.unwrap_or(now);
                    (0..summary.windowed_count)
                        .map(|_| UsageSample {
                            skill_id: summary.skill_id.clone(),
                            used_at,
                            usage_count: 1,
                        })
                        .collect::<Vec<_>>()
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

fn load_skill_snapshots(scope_roots: &[std::path::PathBuf]) -> Result<Vec<SkillSnapshot>, String> {
    use domain::{ScopeRoot, ScopeType};
    use graph_builder::{
        graph::build::build_skills_from_scope_roots,
        graph::embeddings::DeterministicEmbeddingGenerator,
    };

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

    let built = build_skills_from_scope_roots(&roots, &DeterministicEmbeddingGenerator)
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

#[derive(Debug, Default)]
pub struct NoopMergePassRunner;

impl MergePassRunner for NoopMergePassRunner {
    fn run_merge_pass(
        &mut self,
        _now: chrono::DateTime<Utc>,
    ) -> Result<Vec<MergeProposal>, CronError> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
pub struct NoopRetirementPassRunner;

impl RetirementPassRunner for NoopRetirementPassRunner {
    fn run_retirement_pass(
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
        run_one_tick(cron, merge_runner, retirement_runner)?;
        return Ok(());
    }

    let mut ticker = tokio::time::interval(config.cron_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if let Err(error) = run_one_tick(cron, merge_runner, retirement_runner) {
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

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://skill_layer:skill_layer@localhost:15432/skill_layer".to_owned()
    });
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

    let mut merge_runner =
        LiveMergePassRunner::new(snapshot_store.clone(), scope_roots.clone(), &pg_adapter);
    let mut retirement_runner = LiveRetirementPassRunner::new_with_usage_store(
        snapshot_store,
        scope_roots,
        RetirementConfig::default(),
        &pg_adapter,
        usage_store,
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

/// Builds the transcript-queue drain, or `None` when disabled or unconstructable.
///
/// Returns `None` (degraded, but the worker still runs merge/retire) when the
/// rollback flag is `off` or when the extractor cannot be built from the
/// environment — e.g. `CLAUDE_TRANSCRIPT_ROOT` / `SKILL_GLOBAL_PATHS` not
/// configured on the maintenance container. The drain only ever uses
/// `transcript_inline`, so the transcript root is never read; it is required
/// solely by `SessionExtractor::from_environment`'s construction contract.
fn build_transcript_drain(pg_adapter: &PostgresAdapter) -> Option<TranscriptQueueDrain> {
    if std::env::var(TRANSCRIPT_DRAIN_FLAG).as_deref() == Ok("off") {
        warn!(
            flag = TRANSCRIPT_DRAIN_FLAG,
            "transcript ingest queue drain disabled by rollback flag"
        );
        return None;
    }

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
    if roots.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
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

fn run_one_tick(
    cron: &mut MaintenanceCron,
    merge_runner: &mut impl MergePassRunner,
    retirement_runner: &mut impl RetirementPassRunner,
) -> Result<(), MaintenanceRuntimeError> {
    let decision = cron.tick(Utc::now(), merge_runner, retirement_runner)?;
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
    use chrono::DateTime;

    #[derive(Default)]
    struct CountingMergeRunner {
        invocations: usize,
    }

    impl MergePassRunner for CountingMergeRunner {
        fn run_merge_pass(&mut self, _now: DateTime<Utc>) -> Result<Vec<MergeProposal>, CronError> {
            self.invocations += 1;
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct CountingRetirementRunner {
        invocations: usize,
    }

    impl RetirementPassRunner for CountingRetirementRunner {
        fn run_retirement_pass(
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
}
