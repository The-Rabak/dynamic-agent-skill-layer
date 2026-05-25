use std::collections::{HashMap, HashSet};

use chrono::Utc;
use uuid::Uuid;

use crate::persistence::outbox::{
    GraphWriteCoordinator, OutboxError, OutboxInspection, OutboxRecord, OutboxRelayError,
    OutboxVectorStore, VECTOR_UPSERT_EVENT_TYPE, parse_vector_upsert_request,
    qdrant_point_id_from_content_hash,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxReconciliationReport {
    pub scanned: usize,
    pub missing_vectors: usize,
    pub repair_enqueued: usize,
    pub orphaned_vectors_deleted: usize,
}

/// Reconciles PG outbox publication state against the vector store.
pub struct OutboxReconciler<'a, C, S>
where
    C: GraphWriteCoordinator + OutboxInspection,
    S: OutboxVectorStore,
{
    coordinator: &'a C,
    vector_store: &'a S,
    scan_limit: i64,
}

impl<'a, C, S> OutboxReconciler<'a, C, S>
where
    C: GraphWriteCoordinator + OutboxInspection,
    S: OutboxVectorStore,
{
    pub fn new(
        coordinator: &'a C,
        vector_store: &'a S,
        scan_limit: i64,
    ) -> Result<Self, OutboxRelayError> {
        if scan_limit <= 0 {
            return Err(OutboxRelayError::InvalidPayload(
                "scan_limit must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            coordinator,
            vector_store,
            scan_limit,
        })
    }

    pub async fn reconcile_once(&self) -> Result<OutboxReconciliationReport, OutboxRelayError> {
        let published_records = self
            .coordinator
            .list_published_events_by_type(VECTOR_UPSERT_EVENT_TYPE, self.scan_limit)
            .await?;
        let expected_set_is_complete = (published_records.len() as i64) < self.scan_limit;

        let expected_points = expected_points_by_event(&published_records)?;
        let mut missing_records = Vec::new();
        for (point_id, record) in &expected_points {
            let is_present = self
                .vector_store
                .has_vector(*point_id)
                .await
                .map_err(OutboxRelayError::VectorStore)?;
            if !is_present {
                missing_records.push(record.clone());
            }
        }

        let mut repair_enqueued = 0;
        for record in &missing_records {
            let upsert = parse_vector_upsert_request(&record.event.payload)?;
            let repair_event = crate::persistence::outbox::OutboxEvent {
                event_id: Uuid::now_v7(),
                event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
                correlation_id: record.event.correlation_id,
                idempotency_key: format!(
                    "reconcile:{}:{}",
                    record.event.event_id,
                    qdrant_point_id_from_content_hash(&upsert.content_hash)
                ),
                schema_version: record.event.schema_version,
                timestamp: Utc::now(),
                payload: record.event.payload.clone(),
            };

            match self.coordinator.append_outbox_event(&repair_event).await {
                Ok(()) => repair_enqueued += 1,
                Err(OutboxError::IdempotencyConflict { .. }) => {}
                Err(error) => return Err(OutboxRelayError::Coordinator(error)),
            }
        }

        let actual_listing = self
            .vector_store
            .list_point_ids()
            .await
            .map_err(OutboxRelayError::VectorStore)?;
        let can_delete_orphans = expected_set_is_complete && actual_listing.is_complete;
        let orphaned_ids = if can_delete_orphans {
            let expected_ids: HashSet<u64> = expected_points.keys().copied().collect();
            let orphaned = actual_listing
                .point_ids
                .into_iter()
                .filter(|point_id| !expected_ids.contains(point_id))
                .collect::<Vec<_>>();
            if !orphaned.is_empty() {
                self.vector_store
                    .delete_points(&orphaned)
                    .await
                    .map_err(OutboxRelayError::VectorStore)?;
            }
            orphaned
        } else {
            Vec::new()
        };

        Ok(OutboxReconciliationReport {
            scanned: published_records.len(),
            missing_vectors: missing_records.len(),
            repair_enqueued,
            orphaned_vectors_deleted: orphaned_ids.len(),
        })
    }
}

fn expected_points_by_event(
    records: &[OutboxRecord],
) -> Result<HashMap<u64, OutboxRecord>, OutboxRelayError> {
    let mut expected = HashMap::new();
    for record in records {
        let upsert = parse_vector_upsert_request(&record.event.payload)?;
        let point_id = qdrant_point_id_from_content_hash(&upsert.content_hash);
        match expected.get_mut(&point_id) {
            Some(current_record) => {
                if outbox_record_is_newer(record, current_record) {
                    *current_record = record.clone();
                }
            }
            None => {
                expected.insert(point_id, record.clone());
            }
        }
    }
    Ok(expected)
}

/// Compares duplicate outbox records for the same point id and chooses the latest replay candidate.
fn outbox_record_is_newer(candidate: &OutboxRecord, current: &OutboxRecord) -> bool {
    (candidate.occurred_at, candidate.event.event_id) > (current.occurred_at, current.event.event_id)
}
