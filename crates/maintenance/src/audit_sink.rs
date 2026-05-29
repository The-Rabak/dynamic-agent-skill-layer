use crate::audit::{MaintenanceAuditError, MaintenanceAuditEvent, MaintenanceAuditSink};
use infrastructure::PostgresAdapter;
use uuid::Uuid;

#[derive(Debug)]
pub struct PostgresMaintenanceAuditSink {
    pool: sqlx::PgPool,
}

impl PostgresMaintenanceAuditSink {
    pub fn new(adapter: &PostgresAdapter) -> Self {
        Self {
            pool: adapter.pool().clone(),
        }
    }

    pub fn from_pool(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

impl MaintenanceAuditSink for PostgresMaintenanceAuditSink {
    fn emit(&self, event: MaintenanceAuditEvent) -> Result<(), MaintenanceAuditError> {
        let (entity_type, entity_id, action, metadata) = match event {
            MaintenanceAuditEvent::MergeProposalWritten(merge) => {
                let entity_id = blake3::hash(merge.correlation_id.as_bytes());
                let mut id_bytes = [0_u8; 16];
                id_bytes.copy_from_slice(&entity_id.as_bytes()[..16]);
                id_bytes[6] = (id_bytes[6] & 0x0f) | 0x40;
                id_bytes[8] = (id_bytes[8] & 0x3f) | 0x80;
                (
                    "merge_proposal",
                    Uuid::from_bytes(id_bytes),
                    "merge_proposal_written",
                    serde_json::json!({
                        "correlation_id": merge.correlation_id,
                        "happened_at": merge.happened_at.to_rfc3339(),
                        "proposal_path": merge.proposal_path.display().to_string(),
                        "canonical_scope": format!("{:?}", merge.canonical_scope),
                        "merged_from_skill_ids": merge.merged_from_skill_ids,
                        "similarity": merge.similarity,
                    }),
                )
            }
            MaintenanceAuditEvent::RetirementProposalWritten(retire) => {
                let entity_id = blake3::hash(retire.correlation_id.as_bytes());
                let mut id_bytes = [0_u8; 16];
                id_bytes.copy_from_slice(&entity_id.as_bytes()[..16]);
                id_bytes[6] = (id_bytes[6] & 0x0f) | 0x40;
                id_bytes[8] = (id_bytes[8] & 0x3f) | 0x80;
                (
                    "retirement_proposal",
                    Uuid::from_bytes(id_bytes),
                    "retirement_proposal_written",
                    serde_json::json!({
                        "correlation_id": retire.correlation_id,
                        "happened_at": retire.happened_at.to_rfc3339(),
                        "skill_id": retire.skill_id,
                        "source_path": retire.source_path.display().to_string(),
                        "proposal_path": retire.proposal_path.display().to_string(),
                        "usage_score_per_month": retire.usage_score_per_month,
                    }),
                )
            }
        };

        let audit_id = entity_id;
        let actor = "maintenance-worker";
        let rt = tokio::runtime::Handle::try_current().map_err(|_| {
            MaintenanceAuditError::EmitFailure(
                "no async runtime available for audit persistence".to_owned(),
            )
        })?;
        rt.block_on(async {
            sqlx::query(
                r#"
                INSERT INTO audit_log (id, entity_type, entity_id, action, actor, metadata, happened_at)
                VALUES ($1, $2, $3, $4, $5, $6, NOW())
                ON CONFLICT (id) DO NOTHING
                "#,
            )
            .bind(audit_id)
            .bind(entity_type)
            .bind(entity_id)
            .bind(action)
            .bind(actor)
            .bind(&metadata)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                MaintenanceAuditError::EmitFailure(format!(
                    "audit persistence failed: {error}"
                ))
            })
        })?;
        Ok(())
    }
}