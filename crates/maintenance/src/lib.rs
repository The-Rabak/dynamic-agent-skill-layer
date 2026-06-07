pub mod audit;
pub mod audit_sink;
pub mod cleanup;
pub mod cron;
pub mod merge;
pub mod merge_verifier;
pub mod promote;
pub mod retire;
pub mod runtime;
pub mod transcript_drain;

#[cfg(any(test, feature = "test-utils"))]
pub use audit::NoopMaintenanceAuditSink;
pub use audit::{
    MaintenanceAuditError, MaintenanceAuditEvent, MaintenanceAuditSink, MergeProposalAuditEvent,
    RetirementProposalAuditEvent,
};
pub use cleanup::{
    CleanupError, MalformedPendingFileDiagnostic, PendingScanReport, PendingWarning,
    PendingWarningScanner,
};
pub use cron::{
    CronDecision, CronError, DemotionPassRunner, MaintenanceCron, MaintenancePassOutcome,
    MergePassRunner, PromotionPassRunner, RetirementPassRunner,
};
pub use merge::{
    MergeCandidate, MergeConfig, MergeError, MergeProposal, MergeProposalWriter,
    MergeSemanticVerifier, ScopeSelectionPolicy, SeededSkillProjection, SkillSnapshot,
};
pub use promote::{
    DemotionProposal, LivePromotionPassRunner, PromotionError, PromotionEvidence,
    PromotionProposal, PromotionProposalWriter, PromotionScopePolicy, PromotionWriterConfig,
    RecurrenceConfig, collect_project_local_identifiers,
    skill_text_contains_project_local_identifier,
};
pub use retire::{
    RetirementConfig, RetirementError, RetirementProposal, RetirementProposalWriter, UsageSample,
};
pub use runtime::{
    LiveMergePassRunner, LiveRetirementPassRunner, MaintenanceRuntimeError,
    MaintenanceWorkerConfig, run_maintenance_worker, run_maintenance_worker_from_environment,
};
#[cfg(any(test, feature = "test-utils"))]
pub use runtime::{NoopMergePassRunner, NoopPromotionPassRunner, NoopRetirementPassRunner};
pub use transcript_drain::{
    DEFAULT_TRANSCRIPT_DRAIN_BATCH, TranscriptDrainError, TranscriptDrainReport,
    TranscriptQueueDrain,
};
