use std::time::Duration;

use chrono::Utc;
use infrastructure::logging::{ServiceLoggingConfig, init_service_logging};
use thiserror::Error;
use tokio::time::MissedTickBehavior;
use tracing::{error, info};

use crate::cron::{
    CronDecision, CronError, MaintenanceCron, MergePassRunner, RetirementPassRunner,
};

const DEFAULT_CRON_INTERVAL_SECS: u64 = 60;
const CRON_INTERVAL_ENV: &str = "MAINTENANCE_CRON_INTERVAL_SECS";
const RUN_ONCE_ENV: &str = "MAINTENANCE_RUN_ONCE";

/// Runtime configuration for the maintenance worker process.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintenanceWorkerConfig {
    pub cron_interval: Duration,
    pub run_once: bool,
}

impl MaintenanceWorkerConfig {
    /// Loads maintenance worker settings from environment variables.
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

/// Placeholder merge runner used until runtime adapters are connected.
#[derive(Debug, Default)]
pub struct NoopMergePassRunner;

impl MergePassRunner for NoopMergePassRunner {
    fn run_merge_pass(
        &mut self,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::merge::MergeProposal>, CronError> {
        Ok(Vec::new())
    }
}

/// Placeholder retirement runner used until runtime adapters are connected.
#[derive(Debug, Default)]
pub struct NoopRetirementPassRunner;

impl RetirementPassRunner for NoopRetirementPassRunner {
    fn run_retirement_pass(
        &mut self,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::retire::RetirementProposal>, CronError> {
        Ok(Vec::new())
    }
}

/// Runs the maintenance worker loop with explicit dependencies.
pub async fn run_maintenance_worker(
    config: MaintenanceWorkerConfig,
    cron: &mut MaintenanceCron,
    merge_runner: &mut impl MergePassRunner,
    retirement_runner: &mut impl RetirementPassRunner,
) -> Result<(), MaintenanceRuntimeError> {
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
    }
}

/// Builds runtime config and executes the maintenance worker with default runners.
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
    let mut merge_runner = NoopMergePassRunner;
    let mut retirement_runner = NoopRetirementPassRunner;

    run_maintenance_worker(config, &mut cron, &mut merge_runner, &mut retirement_runner).await
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
    /// Maps runtime failures to stable reason codes.
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
    use chrono::{DateTime, Utc};

    #[derive(Default)]
    struct CountingMergeRunner {
        invocations: usize,
    }

    impl MergePassRunner for CountingMergeRunner {
        fn run_merge_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<crate::merge::MergeProposal>, CronError> {
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
        ) -> Result<Vec<crate::retire::RetirementProposal>, CronError> {
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

        run_maintenance_worker(config, &mut cron, &mut merge_runner, &mut retirement_runner)
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
