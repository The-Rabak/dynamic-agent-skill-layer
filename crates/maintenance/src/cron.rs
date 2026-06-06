use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{
    merge::MergeProposal,
    promote::{DemotionProposal, PromotionProposal},
    retire::RetirementProposal,
};

/// Result shape returned when a scheduled maintenance pass runs.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintenancePassOutcome {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub merge_proposals: Vec<MergeProposal>,
    pub retirement_proposals: Vec<RetirementProposal>,
    pub promotion_proposals: Vec<PromotionProposal>,
    /// Demotion proposals for global skills found to reference project-local
    /// identifiers. Each proposal is a propose-only `.pending` artifact; the
    /// human gate prevents any auto-mutation of the global scope.
    pub demotion_proposals: Vec<DemotionProposal>,
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
#[async_trait]
pub trait MergePassRunner: Send {
    async fn run_merge_pass(&mut self, now: DateTime<Utc>)
    -> Result<Vec<MergeProposal>, CronError>;
}

/// Retirement workflow seam for scheduled orchestration.
#[async_trait]
pub trait RetirementPassRunner: Send {
    async fn run_retirement_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, CronError>;
}

/// Promotion workflow seam for scheduled orchestration.
///
/// Mirrors `MergePassRunner` and `RetirementPassRunner`. The live implementation
/// queries approved project skills hinted `general` and runs the intrinsic gate
/// before emitting global `.pending` proposals.
#[async_trait]
pub trait PromotionPassRunner: Send {
    /// Runs the promotion pass for the given instant.
    ///
    /// Returns the list of proposals written on this pass. An empty vec means no
    /// promotable skills were found — not an error.
    ///
    /// # Errors
    ///
    /// Returns `CronError::PromotionPass` when the pass encounters a non-recoverable
    /// failure (e.g. verifier provider unavailable). Individual skills that fail the
    /// deterministic veto are silently skipped (not an error).
    async fn run_promotion_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PromotionProposal>, CronError>;
}

/// Demotion workflow seam for scheduled orchestration (todo #182).
///
/// Scans `scope='global'` skills for project-local identifier references.
/// The live implementation is [`crate::promote::LivePromotionPassRunner`], which
/// also implements `PromotionPassRunner` so both directions are colocated.
#[async_trait]
pub trait DemotionPassRunner: Send {
    /// Runs the demotion scan for the given instant.
    ///
    /// Returns demotion proposals for any global skills found to reference
    /// project-local identifiers. An empty vec means no mis-scoped global skills
    /// were found — not an error.
    ///
    /// # Errors
    ///
    /// Returns `CronError::PromotionPass` (reuses the promotion error variant —
    /// demotion is the symmetric inverse of promotion on the same axis) when the
    /// pass encounters a non-recoverable failure (e.g. PG query or write failure).
    async fn run_demotion_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<DemotionProposal>, CronError>;
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

    /// Runs merge, retirement, promotion, and demotion passes only when the configured interval has elapsed.
    ///
    /// The `scope_pass_runner` implements both [`PromotionPassRunner`] and [`DemotionPassRunner`]
    /// because promotion and demotion are symmetric directions on the same axis — both live in
    /// `LivePromotionPassRunner` and share the same project-identifier tokens and writer config.
    pub async fn tick(
        &mut self,
        now: DateTime<Utc>,
        merge_runner: &mut impl MergePassRunner,
        retirement_runner: &mut impl RetirementPassRunner,
        scope_pass_runner: &mut (impl PromotionPassRunner + DemotionPassRunner),
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
        let merge_proposals = merge_runner.run_merge_pass(now).await?;
        let retirement_proposals = retirement_runner.run_retirement_pass(now).await?;
        let promotion_proposals = scope_pass_runner.run_promotion_pass(now).await?;
        let demotion_proposals = scope_pass_runner.run_demotion_pass(now).await?;
        let completed_at = Utc::now();
        self.last_run_at = Some(now);

        Ok(CronDecision::Executed(MaintenancePassOutcome {
            started_at,
            completed_at,
            merge_proposals,
            retirement_proposals,
            promotion_proposals,
            demotion_proposals,
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
    #[error("promotion pass failed: {0}")]
    PromotionPass(String),
}

impl CronError {
    /// Maps scheduler failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidInterval(_) => "maintenance_invalid_interval",
            Self::ClockReversal => "maintenance_clock_reversal",
            Self::MergePass(_) => "maintenance_merge_pass_failed",
            Self::RetirementPass(_) => "maintenance_retirement_pass_failed",
            Self::PromotionPass(_) => "maintenance_promotion_pass_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopMergeRunner;
    #[async_trait]
    impl MergePassRunner for NoopMergeRunner {
        async fn run_merge_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<MergeProposal>, CronError> {
            Ok(Vec::new())
        }
    }

    struct NoopRetirementRunner;
    #[async_trait]
    impl RetirementPassRunner for NoopRetirementRunner {
        async fn run_retirement_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<RetirementProposal>, CronError> {
            Ok(Vec::new())
        }
    }

    /// No-op combined scope-pass runner (implements both PromotionPassRunner and DemotionPassRunner).
    struct NoopScopePassRunner;
    #[async_trait]
    impl DemotionPassRunner for NoopScopePassRunner {
        async fn run_demotion_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<DemotionProposal>, CronError> {
            Ok(Vec::new())
        }
    }
    #[async_trait]
    impl PromotionPassRunner for NoopScopePassRunner {
        async fn run_promotion_pass(
            &mut self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<PromotionProposal>, CronError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn zero_interval_is_rejected() {
        let result = MaintenanceCron::new(Duration::from_secs(0));
        assert!(matches!(result, Err(CronError::InvalidInterval(_))));
    }

    #[tokio::test]
    async fn tick_skips_when_not_due() {
        let mut cron = MaintenanceCron::new(Duration::from_secs(60)).expect("cron init");
        let now = Utc::now();
        let mut merge_runner = NoopMergeRunner;
        let mut retirement_runner = NoopRetirementRunner;
        let mut scope_runner = NoopScopePassRunner;
        let first = cron
            .tick(now, &mut merge_runner, &mut retirement_runner, &mut scope_runner)
            .await
            .expect("first run");
        assert!(matches!(first, CronDecision::Executed(_)));
        let second = cron
            .tick(
                now + chrono::Duration::seconds(10),
                &mut merge_runner,
                &mut retirement_runner,
                &mut scope_runner,
            )
            .await
            .expect("second run");
        assert!(matches!(second, CronDecision::SkippedNotDue { .. }));
    }

    /// Verifies that `MaintenancePassOutcome` carries `promotion_proposals` and `demotion_proposals`.
    #[tokio::test]
    async fn tick_executed_outcome_includes_promotion_and_demotion_proposals_fields() {
        let mut cron = MaintenanceCron::new(Duration::from_secs(60)).expect("cron init");
        let now = Utc::now();
        let mut merge_runner = NoopMergeRunner;
        let mut retirement_runner = NoopRetirementRunner;
        let mut scope_runner = NoopScopePassRunner;

        let decision = cron
            .tick(now, &mut merge_runner, &mut retirement_runner, &mut scope_runner)
            .await
            .expect("tick must succeed");

        match decision {
            CronDecision::Executed(outcome) => {
                assert!(
                    outcome.promotion_proposals.is_empty(),
                    "noop runner must produce zero promotion proposals"
                );
                assert!(
                    outcome.demotion_proposals.is_empty(),
                    "noop runner must produce zero demotion proposals"
                );
            }
            CronDecision::SkippedNotDue { .. } => {
                panic!("expected Executed on first tick");
            }
        }
    }
}
