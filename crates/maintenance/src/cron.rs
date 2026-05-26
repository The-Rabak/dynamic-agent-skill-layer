use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{merge::MergeProposal, retire::RetirementProposal};

/// Result shape returned when a scheduled maintenance pass runs.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintenancePassOutcome {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub merge_proposals: Vec<MergeProposal>,
    pub retirement_proposals: Vec<RetirementProposal>,
}

/// Indicates whether a cron tick executed maintenance work.
#[derive(Debug, Clone, PartialEq)]
pub enum CronDecision {
    SkippedNotDue {
        now: DateTime<Utc>,
        next_due_at: DateTime<Utc>,
    },
    Executed(MaintenancePassOutcome),
}

/// Merge workflow seam for scheduled orchestration.
pub trait MergePassRunner {
    fn run_merge_pass(&mut self, now: DateTime<Utc>) -> Result<Vec<MergeProposal>, CronError>;
}

/// Retirement workflow seam for scheduled orchestration.
pub trait RetirementPassRunner {
    fn run_retirement_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, CronError>;
}

/// Periodic offline maintenance scheduler.
pub struct MaintenanceCron {
    interval: Duration,
    last_run_at: Option<DateTime<Utc>>,
}

impl MaintenanceCron {
    /// Creates a cron scheduler with an explicit interval.
    pub fn new(interval: Duration) -> Result<Self, CronError> {
        if interval.is_zero() {
            return Err(CronError::InvalidInterval(
                "maintenance interval must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            interval,
            last_run_at: None,
        })
    }

    /// Runs merge and retirement passes only when the configured interval has elapsed.
    pub fn tick(
        &mut self,
        now: DateTime<Utc>,
        merge_runner: &mut impl MergePassRunner,
        retirement_runner: &mut impl RetirementPassRunner,
    ) -> Result<CronDecision, CronError> {
        if let Some(last_run_at) = self.last_run_at {
            let elapsed = now
                .signed_duration_since(last_run_at)
                .to_std()
                .map_err(|_| CronError::ClockReversal)?;
            if elapsed < self.interval {
                let remaining = self.interval - elapsed;
                let next_due_at = now
                    + chrono::Duration::from_std(remaining).map_err(|error| {
                        CronError::InvalidInterval(format!(
                            "cannot convert remaining interval into chrono duration: {error}"
                        ))
                    })?;
                return Ok(CronDecision::SkippedNotDue { now, next_due_at });
            }
        }

        let started_at = now;
        let merge_proposals = merge_runner.run_merge_pass(now)?;
        let retirement_proposals = retirement_runner.run_retirement_pass(now)?;
        let completed_at = Utc::now();
        self.last_run_at = Some(now);

        Ok(CronDecision::Executed(MaintenancePassOutcome {
            started_at,
            completed_at,
            merge_proposals,
            retirement_proposals,
        }))
    }
}

#[derive(Debug, Error)]
pub enum CronError {
    #[error("invalid maintenance interval: {0}")]
    InvalidInterval(String),
    #[error("maintenance clock moved backwards between runs")]
    ClockReversal,
    #[error("merge pass failed: {0}")]
    MergePass(String),
    #[error("retirement pass failed: {0}")]
    RetirementPass(String),
}

impl CronError {
    /// Maps scheduler failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidInterval(_) => "maintenance_invalid_interval",
            Self::ClockReversal => "maintenance_clock_reversal",
            Self::MergePass(_) => "maintenance_merge_pass_failed",
            Self::RetirementPass(_) => "maintenance_retirement_pass_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopMergeRunner;
    impl MergePassRunner for NoopMergeRunner {
        fn run_merge_pass(&mut self, _now: DateTime<Utc>) -> Result<Vec<MergeProposal>, CronError> {
            Ok(Vec::new())
        }
    }

    struct NoopRetirementRunner;
    impl RetirementPassRunner for NoopRetirementRunner {
        fn run_retirement_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<RetirementProposal>, CronError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn zero_interval_is_rejected() {
        let result = MaintenanceCron::new(Duration::from_secs(0));
        assert!(matches!(result, Err(CronError::InvalidInterval(_))));
    }

    #[test]
    fn tick_skips_when_not_due() {
        let mut cron = MaintenanceCron::new(Duration::from_secs(60)).expect("cron init");
        let now = Utc::now();
        let mut merge_runner = NoopMergeRunner;
        let mut retirement_runner = NoopRetirementRunner;
        let first = cron
            .tick(now, &mut merge_runner, &mut retirement_runner)
            .expect("first run");
        assert!(matches!(first, CronDecision::Executed(_)));
        let second = cron
            .tick(
                now + chrono::Duration::seconds(10),
                &mut merge_runner,
                &mut retirement_runner,
            )
            .expect("second run");
        assert!(matches!(second, CronDecision::SkippedNotDue { .. }));
    }
}
