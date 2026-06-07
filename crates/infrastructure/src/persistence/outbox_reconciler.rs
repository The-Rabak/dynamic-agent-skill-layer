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
    (candidate.occurred_at, candidate.event.event_id)
        > (current.occurred_at, current.event.event_id)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::persistence::outbox::{OutboxEvent, OutboxRecord, VECTOR_UPSERT_EVENT_TYPE};

    fn make_published_record(content_hash: &str, occurred_offset_ms: i64) -> OutboxRecord {
        let occurred_at = Utc::now() + chrono::Duration::milliseconds(occurred_offset_ms);
        let payload = json!({
            "content_hash": content_hash,
            "vector": [0.1_f32, 0.2_f32],
            "payload": {}
        });
        OutboxRecord {
            event: OutboxEvent {
                event_id: Uuid::now_v7(),
                event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
                correlation_id: Uuid::now_v7(),
                idempotency_key: format!("key:{content_hash}:{occurred_offset_ms}"),
                schema_version: 1,
                timestamp: occurred_at,
                payload,
            },
            attempts: 0,
            stream_id: None,
            last_error: None,
            occurred_at,
            available_at: occurred_at,
        }
    }

    /// Regression test for the DS-005 double-count bug.
    ///
    /// When the reconciler enqueues a repair event for a missing vector, both the
    /// original published event and the repair event end up in `outbox_events` with
    /// `status='published'` and the SAME `content_hash`. A naive `COUNT(*)` would
    /// return 2 for one Qdrant point, creating a phantom mismatch (published=2,
    /// qdrant=1) that falsely looks like drift.
    ///
    /// `expected_points_by_event` must deduplicate: two records with the same
    /// `content_hash` → one entry in the expected map (the newer record wins).
    /// The count of that map equals the true number of Qdrant points expected,
    /// not the number of outbox rows.
    #[test]
    fn expected_points_deduplicates_repair_duplicate_for_same_content_hash() {
        let content_hash = "test-content-hash-abc123";
        // Simulate the DS-005 scenario: original published event + reconciler repair event,
        // both with the same content_hash but different event_ids and idempotency keys.
        let original = make_published_record(content_hash, 0);
        let repair = make_published_record(content_hash, 100); // repair is newer

        let records = vec![original, repair];
        let expected = expected_points_by_event(&records)
            .expect("deduplication must succeed for valid payloads");

        // Two outbox rows with the same content_hash collapse to one point_id in the map.
        assert_eq!(
            expected.len(),
            1,
            "two published events with the same content_hash must map to exactly one expected \
             Qdrant point — counting raw outbox rows would give 2, masking the real count"
        );
    }

    /// Proves that two records with DIFFERENT content_hashes produce two distinct expected
    /// points — the deduplication does not collapse genuinely different vectors.
    #[test]
    fn expected_points_preserves_distinct_content_hashes_as_separate_points() {
        let record_a = make_published_record("hash-alpha", 0);
        let record_b = make_published_record("hash-beta", 0);

        let records = vec![record_a, record_b];
        let expected = expected_points_by_event(&records)
            .expect("two distinct hashes must produce two expected points");

        assert_eq!(
            expected.len(),
            2,
            "distinct content_hashes must each produce their own expected Qdrant point"
        );
    }

    /// Proves the newer-wins deduplication policy: when two events share a content_hash,
    /// the one with the later `occurred_at` is kept as the replay candidate.
    #[test]
    fn expected_points_keeps_newer_record_when_deduplicating() {
        let content_hash = "same-hash";
        let older = make_published_record(content_hash, 0);
        let newer = make_published_record(content_hash, 500);
        let older_event_id = older.event.event_id;
        let newer_event_id = newer.event.event_id;

        // Insert older first, then newer — deduplication must keep the newer one.
        let records = vec![older, newer];
        let expected = expected_points_by_event(&records).expect("deduplication must succeed");

        let kept = expected.values().next().expect("one entry must remain");
        assert_eq!(
            kept.event.event_id, newer_event_id,
            "newer record (event_id={newer_event_id}) must win over older (event_id={older_event_id})"
        );
    }
}
