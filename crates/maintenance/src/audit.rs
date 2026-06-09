use std::path::PathBuf;

use chrono::{DateTime, Utc};
use domain::ScopeType;
use thiserror::Error;

/// Contract for durable audit emission from maintenance proposal workflows.
pub trait MaintenanceAuditSink: Send + Sync {
    /// Emits one maintenance audit event.
    fn emit(&self, event: MaintenanceAuditEvent) -> Result<(), MaintenanceAuditError>;
}

/// No-op audit sink for tests that deliberately skip audit emission.
///
/// Test-support only (gated): production proposal writers must be constructed with a
/// real sink via `with_audit_sink` — there is no Noop-default constructor, so a forgotten
/// sink is a compile error, not a silent no-op. (#161 foot-gun closure.)
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Default)]
pub struct NoopMaintenanceAuditSink;

#[cfg(any(test, feature = "test-utils"))]
impl MaintenanceAuditSink for NoopMaintenanceAuditSink {
    fn emit(&self, _event: MaintenanceAuditEvent) -> Result<(), MaintenanceAuditError> {
        Ok(())
    }
}

/// Explicit audit sink failure surfaced by maintenance proposal writers.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MaintenanceAuditError {
    #[error("maintenance audit sink emit failed: {0}")]
    EmitFailure(String),
}

/// Typed maintenance audit events emitted for proposal writes.
#[derive(Debug, Clone, PartialEq)]
pub enum MaintenanceAuditEvent {
    MergeProposalWritten(MergeProposalAuditEvent),
    RetirementProposalWritten(RetirementProposalAuditEvent),
}

/// Audit payload emitted after writing a merge `.pending` proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeProposalAuditEvent {
    pub correlation_id: String,
    pub happened_at: DateTime<Utc>,
    pub proposal_path: PathBuf,
    pub canonical_scope: ScopeType,
    pub merged_from_skill_ids: Vec<String>,
    pub merged_from_scopes: Vec<ScopeType>,
    pub merged_from_paths: Vec<PathBuf>,
    pub similarity: f32,
}

/// Audit payload emitted after writing a retirement `.retired` proposal marker.
#[derive(Debug, Clone, PartialEq)]
pub struct RetirementProposalAuditEvent {
    pub correlation_id: String,
    pub happened_at: DateTime<Utc>,
    pub skill_id: String,
    pub source_path: PathBuf,
    pub proposal_path: PathBuf,
    pub usage_score_per_month: f32,
}
