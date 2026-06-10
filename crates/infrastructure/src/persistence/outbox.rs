use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEvent {
    pub event_id: Uuid,
    pub event_type: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxRecord {
    pub event: OutboxEvent,
    pub attempts: i32,
    pub stream_id: Option<String>,
    pub last_error: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub available_at: DateTime<Utc>,
}

pub const MAX_OUTBOX_RETRIES: i32 = 3;
pub const VECTOR_UPSERT_EVENT_TYPE: &str = "vector.upsert";

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("invalid outbox contract: {0}")]
    InvalidContract(String),
    #[error("outbox idempotency conflict for key `{idempotency_key}`")]
    IdempotencyConflict { idempotency_key: String },
    #[error("outbox event `{event_id}` was not found")]
    EventNotFound { event_id: Uuid },
    #[error("outbox transition requires `{expected}` state for `{event_id}`, got `{actual}`")]
    IllegalTransition {
        event_id: Uuid,
        expected: &'static str,
        actual: String,
    },
    #[error("outbox persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum OutboxRelayError {
    #[error("invalid vector payload contract: {0}")]
    InvalidPayload(String),
    #[error("outbox coordinator failure: {0}")]
    Coordinator(#[from] OutboxError),
    #[error("vector store failure: {0}")]
    VectorStore(String),
}

/// Sparse vector carried in a `VectorUpsertRequest` for hybrid upserts.
///
/// When present, the relay routes the event to `upsert_hybrid` on the vector
/// store (targeting the hybrid collection) instead of the plain `upsert_vector`
/// path. Absent means dense-only — the existing path is unchanged.
///
/// `indices` and `values` must be the same length. Zero-length sparse vectors
/// are rejected by `parse_vector_upsert_request` (an empty sparse component
/// carries no information and indicates a construction bug in the writer).
#[derive(Debug, Clone, PartialEq)]
pub struct SparseVectorPayload {
    /// FNV-1a term indices (same mapping as `retrieval::sparse::term_to_sparse_index`).
    pub indices: Vec<u32>,
    /// BM25 TF-saturation weights for each corresponding index.
    pub values: Vec<f32>,
}

/// A parsed outbox event payload for a `vector.upsert` event.
///
/// `sparse` is present only for events emitted under `RETRIEVAL_BACKEND=qdrant_hybrid`.
/// Absent `sparse` means the event targets the dense-only collection via
/// `upsert_vector`; present `sparse` targets the hybrid collection via
/// `upsert_hybrid`. This is backward-compatible: old events without a `sparse`
/// field parse with `sparse: None` and follow the existing relay path unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorUpsertRequest {
    pub content_hash: String,
    pub vector: Vec<f32>,
    /// Optional sparse BM25 vector. Present only for `RETRIEVAL_BACKEND=qdrant_hybrid`.
    pub sparse: Option<SparseVectorPayload>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRelayRunReport {
    pub claimed: usize,
    pub published: usize,
    pub failed: usize,
}

#[async_trait]
pub trait GraphWriteCoordinator: Send + Sync {
    async fn begin_outbox_transaction(&self)
    -> Result<Transaction<'static, Postgres>, OutboxError>;
    async fn append_outbox_event(&self, event: &OutboxEvent) -> Result<(), OutboxError>;
    async fn append_outbox_event_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        event: &OutboxEvent,
    ) -> Result<(), OutboxError>;
    async fn claim_pending_outbox(&self, limit: i64) -> Result<Vec<OutboxRecord>, OutboxError>;
    async fn claim_pending_outbox_for_correlation(
        &self,
        correlation_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, OutboxError>;
    async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        stream_id: &str,
    ) -> Result<(), OutboxError>;
    async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        retry_after_seconds: u64,
    ) -> Result<(), OutboxError>;
}

#[async_trait]
pub trait OutboxInspection: Send + Sync {
    async fn has_pending_for_correlation(&self, correlation_id: Uuid) -> Result<bool, OutboxError>;
    async fn list_published_events_by_type(
        &self,
        event_type: &str,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, OutboxError>;
}

#[async_trait]
pub trait OutboxVectorStore: Send + Sync {
    async fn upsert_vector(
        &self,
        point_id: u64,
        vector: &[f32],
        payload: &Value,
    ) -> Result<(), String>;
    async fn has_vector(&self, point_id: u64) -> Result<bool, String>;
    async fn list_point_ids(&self) -> Result<VectorPointListing, String>;
    async fn delete_points(&self, point_ids: &[u64]) -> Result<(), String>;

    /// Upserts a point with both dense and sparse vectors into the hybrid collection.
    ///
    /// Called by the relay when the outbox payload carries a `SparseVectorPayload`
    /// and the relay is configured with a hybrid collection name (i.e.
    /// `RETRIEVAL_BACKEND=qdrant_hybrid` at relay construction time).
    ///
    /// # Default implementation
    ///
    /// The default implementation fails loudly. Any `OutboxVectorStore` that is
    /// wired in a `qdrant_hybrid` configuration MUST override this method with a
    /// real implementation — a silent no-op would lose sparse vectors without any
    /// indication of failure, violating the no-stubs mandate.
    ///
    /// Stores that only support the dense path (e.g. test doubles or the plain
    /// `QdrantAdapter` in non-hybrid deployments) do not override this; the relay
    /// will never call it when `hybrid_collection` is `None`.
    async fn upsert_hybrid(
        &self,
        hybrid_collection: &str,
        point_id: u64,
        _dense: &[f32],
        _sparse_indices: &[u32],
        _sparse_values: &[f32],
        _payload: &Value,
    ) -> Result<(), String> {
        Err(format!(
            "upsert_hybrid called on a vector store that does not implement it \
             (collection={hybrid_collection}, point_id={point_id}); \
             configure RETRIEVAL_BACKEND=qdrant_hybrid only with a QdrantAdapter"
        ))
    }
}

/// Captures the visible point id set and whether the listing is complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorPointListing {
    pub point_ids: Vec<u64>,
    pub is_complete: bool,
}

/// Converts a content hash into the canonical Qdrant point id used for idempotent replay.
pub fn qdrant_point_id_from_content_hash(content_hash: &str) -> u64 {
    let digest = blake3::hash(content_hash.as_bytes());
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_be_bytes(bytes)
}

/// Parses outbox payload into the vector upsert contract.
///
/// The `sparse` field is optional and backward-compatible: payloads written
/// before `RETRIEVAL_BACKEND=qdrant_hybrid` support was added (i.e. without a
/// `sparse` key) parse cleanly with `sparse: None`, following the existing
/// dense-only relay path unchanged.
///
/// When `sparse` is present it must be a well-formed object with non-empty
/// `indices` and `values` arrays of equal length. An empty or malformed sparse
/// field is rejected so a construction bug in the writer surfaces immediately
/// rather than silently producing a point with no sparse component.
pub fn parse_vector_upsert_request(
    payload: &Value,
) -> Result<VectorUpsertRequest, OutboxRelayError> {
    let content_hash = payload
        .get("content_hash")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            OutboxRelayError::InvalidPayload(
                "payload.content_hash must be a non-empty string".to_owned(),
            )
        })?
        .to_owned();

    let vector_values = payload
        .get("vector")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            OutboxRelayError::InvalidPayload(
                "payload.vector must be a non-empty numeric array".to_owned(),
            )
        })?;

    if vector_values.is_empty() {
        return Err(OutboxRelayError::InvalidPayload(
            "payload.vector must be a non-empty numeric array".to_owned(),
        ));
    }

    let mut vector = Vec::with_capacity(vector_values.len());
    for item in vector_values {
        let value = item.as_f64().ok_or_else(|| {
            OutboxRelayError::InvalidPayload(
                "payload.vector must contain only numeric values".to_owned(),
            )
        })?;
        vector.push(value as f32);
    }

    // Parse the optional sparse component. Absent key → None (backward-compatible).
    // Present key must be a well-formed object with matching-length arrays.
    let sparse = if let Some(sparse_val) = payload.get("sparse") {
        let indices_raw = sparse_val
            .get("indices")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                OutboxRelayError::InvalidPayload(
                    "payload.sparse.indices must be a numeric array".to_owned(),
                )
            })?;
        let values_raw = sparse_val
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                OutboxRelayError::InvalidPayload(
                    "payload.sparse.values must be a numeric array".to_owned(),
                )
            })?;
        if indices_raw.len() != values_raw.len() {
            return Err(OutboxRelayError::InvalidPayload(format!(
                "payload.sparse.indices length ({}) must equal payload.sparse.values length ({})",
                indices_raw.len(),
                values_raw.len(),
            )));
        }
        if indices_raw.is_empty() {
            return Err(OutboxRelayError::InvalidPayload(
                "payload.sparse must not be empty; omit the key entirely for dense-only upserts"
                    .to_owned(),
            ));
        }
        let mut indices = Vec::with_capacity(indices_raw.len());
        for item in indices_raw {
            let raw_u64 = item.as_u64().ok_or_else(|| {
                OutboxRelayError::InvalidPayload(
                    "payload.sparse.indices must contain only non-negative integers".to_owned(),
                )
            })?;
            let idx = u32::try_from(raw_u64).map_err(|_| {
                OutboxRelayError::InvalidPayload(format!(
                    "payload.sparse.indices value {raw_u64} exceeds u32 range (max {})",
                    u32::MAX
                ))
            })?;
            indices.push(idx);
        }
        let mut values = Vec::with_capacity(values_raw.len());
        for item in values_raw {
            let val = item.as_f64().ok_or_else(|| {
                OutboxRelayError::InvalidPayload(
                    "payload.sparse.values must contain only numeric values".to_owned(),
                )
            })? as f32;
            values.push(val);
        }
        Some(SparseVectorPayload { indices, values })
    } else {
        None
    };

    let payload_body = payload.get("payload").cloned().unwrap_or(Value::Null);
    Ok(VectorUpsertRequest {
        content_hash,
        vector,
        sparse,
        payload: payload_body,
    })
}

/// Drives outbox -> Qdrant relay transitions for one polling cycle.
///
/// When `hybrid_collection` is `Some(name)`, events carrying a sparse vector
/// component are routed to `vector_store.upsert_hybrid(name, …)` instead of
/// `vector_store.upsert_vector`. This is activated by setting
/// `RETRIEVAL_BACKEND=qdrant_hybrid` at relay construction time and passing
/// the model-keyed hybrid collection name.
///
/// Events without a sparse component always follow the dense-only path via
/// `upsert_vector`, even when `hybrid_collection` is set. This preserves
/// backward compatibility for any legacy events in the outbox.
pub struct OutboxRelay<'a, C, S>
where
    C: GraphWriteCoordinator,
    S: OutboxVectorStore,
{
    coordinator: &'a C,
    vector_store: &'a S,
    claim_limit: i64,
    retry_after_seconds: u64,
    /// When `Some`, sparse-carrying events are routed to `upsert_hybrid` on
    /// this collection. `None` means dense-only relay for all events.
    hybrid_collection: Option<String>,
}

impl<'a, C, S> OutboxRelay<'a, C, S>
where
    C: GraphWriteCoordinator,
    S: OutboxVectorStore,
{
    pub fn new(
        coordinator: &'a C,
        vector_store: &'a S,
        claim_limit: i64,
        retry_after_seconds: u64,
    ) -> Result<Self, OutboxRelayError> {
        if claim_limit <= 0 {
            return Err(OutboxRelayError::InvalidPayload(
                "claim_limit must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            coordinator,
            vector_store,
            claim_limit,
            retry_after_seconds,
            hybrid_collection: None,
        })
    }

    /// Returns a new relay configured for the hybrid collection.
    ///
    /// When `hybrid_collection` is set, outbox events that carry a sparse vector
    /// payload are forwarded to `OutboxVectorStore::upsert_hybrid` instead of
    /// `upsert_vector`. The hybrid collection name must be the exact name passed
    /// to `QdrantAdapter::ensure_hybrid_collection` at boot, typically derived via
    /// `model_keyed_hybrid_collection_name`.
    ///
    /// Dense-only events (no sparse payload) are always forwarded via `upsert_vector`
    /// regardless of this setting, preserving backward compatibility.
    pub fn with_hybrid_collection(mut self, collection_name: String) -> Self {
        self.hybrid_collection = Some(collection_name);
        self
    }

    pub async fn relay_once(&self) -> Result<OutboxRelayRunReport, OutboxRelayError> {
        let claimed_records = self
            .coordinator
            .claim_pending_outbox(self.claim_limit)
            .await?;
        self.process_claimed_records(claimed_records).await
    }

    /// Relays one cycle of outbox records for a specific correlation id only.
    pub async fn relay_once_for_correlation(
        &self,
        correlation_id: Uuid,
    ) -> Result<OutboxRelayRunReport, OutboxRelayError> {
        let claimed_records = self
            .coordinator
            .claim_pending_outbox_for_correlation(correlation_id, self.claim_limit)
            .await?;
        self.process_claimed_records(claimed_records).await
    }

    async fn process_claimed_records(
        &self,
        claimed_records: Vec<OutboxRecord>,
    ) -> Result<OutboxRelayRunReport, OutboxRelayError> {
        let mut report = OutboxRelayRunReport {
            claimed: claimed_records.len(),
            published: 0,
            failed: 0,
        };

        for record in claimed_records {
            if record.event.event_type != VECTOR_UPSERT_EVENT_TYPE {
                report.failed += 1;
                self.coordinator
                    .mark_outbox_failed(
                        record.event.event_id,
                        &format!(
                            "unsupported outbox event type `{}`",
                            record.event.event_type
                        ),
                        self.retry_after_seconds,
                    )
                    .await?;
                continue;
            }

            let upsert = match parse_vector_upsert_request(&record.event.payload) {
                Ok(request) => request,
                Err(error) => {
                    report.failed += 1;
                    self.coordinator
                        .mark_outbox_failed(
                            record.event.event_id,
                            &error.to_string(),
                            self.retry_after_seconds,
                        )
                        .await?;
                    continue;
                }
            };

            let point_id = qdrant_point_id_from_content_hash(&upsert.content_hash);
            // Route to hybrid upsert when the relay is configured for a hybrid
            // collection AND this event carries a real sparse vector. Fall back to
            // the dense path for events without a sparse component (backward compat).
            let write_result = match (&self.hybrid_collection, &upsert.sparse) {
                (Some(hybrid_col), Some(sparse)) => {
                    self.vector_store
                        .upsert_hybrid(
                            hybrid_col,
                            point_id,
                            &upsert.vector,
                            &sparse.indices,
                            &sparse.values,
                            &upsert.payload,
                        )
                        .await
                }
                _ => {
                    self.vector_store
                        .upsert_vector(point_id, &upsert.vector, &upsert.payload)
                        .await
                }
            };
            match write_result {
                Ok(()) => {
                    self.coordinator
                        .mark_outbox_published(record.event.event_id, &format!("qdrant:{point_id}"))
                        .await?;
                    report.published += 1;
                }
                Err(error) => {
                    report.failed += 1;
                    self.coordinator
                        .mark_outbox_failed(record.event.event_id, &error, self.retry_after_seconds)
                        .await?;
                }
            }
        }

        Ok(report)
    }

    /// Drains outbox entries scoped to one correlation id before rebuild visibility changes.
    ///
    /// Drains **until the correlation's outbox is empty** — there is no arbitrary
    /// poll-cycle cap. A durable queue must be drained to completion, never cut
    /// off at a magic cycle count (that is the foot-gun that silently lost vectors
    /// at scale). The loop terminates on exactly two conditions:
    /// - the outbox is empty → `Ok`;
    /// - a relay pass claims nothing while events are still pending → a genuine
    ///   stall (events stuck in retry backoff or unprocessable). That is surfaced
    ///   LOUDLY as an error rather than spinning forever — a real stuck state,
    ///   derived from actual progress, not an arbitrary limit.
    pub async fn drain_correlation_outbox<I: OutboxInspection>(
        &self,
        inspection: &I,
        correlation_id: Uuid,
    ) -> Result<(), OutboxRelayError> {
        loop {
            if !inspection
                .has_pending_for_correlation(correlation_id)
                .await?
            {
                return Ok(());
            }
            let report = self.relay_once_for_correlation(correlation_id).await?;
            if report.claimed == 0 {
                return Err(OutboxRelayError::InvalidPayload(format!(
                    "outbox for correlation `{correlation_id}` has pending events that could not \
                     be claimed or relayed (stuck or in retry backoff); drain made no progress"
                )));
            }
        }
    }

    /// Relays all globally pending outbox events, regardless of correlation id.
    ///
    /// Intended for startup self-heal: if a previous rebuild failed mid-drain and
    /// left orphaned `pending` events behind, this method drains them before the
    /// next rebuild cycle runs. Drains **to completion** — no arbitrary cycle cap;
    /// it loops until a `relay_once` pass claims nothing (queue drained / nothing
    /// currently claimable).
    ///
    /// Returns the total number of events published across all cycles.
    pub async fn relay_all_pending_to_completion(&self) -> Result<usize, OutboxRelayError> {
        let mut total_published: usize = 0;
        loop {
            let report = self.relay_once().await?;
            total_published = total_published.saturating_add(report.published);
            if report.claimed == 0 {
                return Ok(total_published);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresGraphWriteCoordinator {
    pool: PgPool,
}

impl PostgresGraphWriteCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn transition_error(
        &self,
        event_id: Uuid,
        expected: &'static str,
    ) -> Result<OutboxError, OutboxError> {
        let maybe_row = sqlx::query(
            r#"
            SELECT status
            FROM outbox_events
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match maybe_row {
            Some(row) => OutboxError::IllegalTransition {
                event_id,
                expected,
                actual: row.try_get("status")?,
            },
            None => OutboxError::EventNotFound { event_id },
        })
    }
}

fn validate_outbox_event(event: &OutboxEvent) -> Result<i32, OutboxError> {
    if event.event_type.trim().is_empty() || event.idempotency_key.trim().is_empty() {
        return Err(OutboxError::InvalidContract(
            "event_type and idempotency_key must not be blank".to_owned(),
        ));
    }

    i32::try_from(event.schema_version).map_err(|_| {
        OutboxError::InvalidContract("schema_version exceeds i32 storage limit".to_owned())
    })
}

fn validate_claim_limit(limit: i64) -> Result<(), OutboxError> {
    if limit <= 0 {
        return Err(OutboxError::InvalidContract(
            "claim limit must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn map_outbox_records(rows: Vec<PgRow>) -> Result<Vec<OutboxRecord>, OutboxError> {
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(OutboxRecord {
            event: OutboxEvent {
                event_id: row.try_get("event_id")?,
                event_type: row.try_get("event_type")?,
                correlation_id: row.try_get("correlation_id")?,
                idempotency_key: row.try_get("idempotency_key")?,
                schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                timestamp: row.try_get("occurred_at")?,
                payload: row.try_get("payload")?,
            },
            attempts: row.try_get("attempts")?,
            stream_id: row.try_get("stream_id")?,
            last_error: row.try_get("last_error")?,
            occurred_at: row.try_get("occurred_at")?,
            available_at: row.try_get("available_at")?,
        });
    }
    Ok(records)
}

async fn insert_outbox_event(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    event: &OutboxEvent,
    schema_version: i32,
) -> Result<(), OutboxError> {
    let insert_result = sqlx::query(
        r#"
        INSERT INTO outbox_events (
            event_id,
            event_type,
            correlation_id,
            idempotency_key,
            schema_version,
            payload,
            occurred_at,
            available_at,
            status
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), 'pending')
        "#,
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(event.correlation_id)
    .bind(&event.idempotency_key)
    .bind(schema_version)
    .bind(&event.payload)
    .bind(event.timestamp)
    .execute(executor)
    .await;

    match insert_result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            let constraint = error.constraint();
            if constraint == Some("outbox_events_idempotency_key_key") {
                return Err(OutboxError::IdempotencyConflict {
                    idempotency_key: event.idempotency_key.clone(),
                });
            }

            Err(OutboxError::InvalidContract(format!(
                "outbox event already exists for event_id {}",
                event.event_id
            )))
        }
        Err(error) => Err(OutboxError::Persistence(error)),
    }
}

/// Inserts an outbox event using `ON CONFLICT (idempotency_key) DO NOTHING`.
///
/// A content-addressed key that already exists in `outbox_events` means the
/// vector is either enqueued or already published — skipping is correct.
/// This variant exists ONLY for paths where replaying the same content-addressed
/// event is safe by design (e.g. rebuild vector emission). Do NOT replace
/// `insert_outbox_event` with this for the general case: the strict
/// `IdempotencyConflict` error is intentional for duplicate-detection elsewhere.
async fn insert_outbox_event_idempotent(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    event: &OutboxEvent,
    schema_version: i32,
) -> Result<bool, OutboxError> {
    let result = sqlx::query(
        r#"
        INSERT INTO outbox_events (
            event_id,
            event_type,
            correlation_id,
            idempotency_key,
            schema_version,
            payload,
            occurred_at,
            available_at,
            status
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), 'pending')
        ON CONFLICT (idempotency_key) DO NOTHING
        "#,
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(event.correlation_id)
    .bind(&event.idempotency_key)
    .bind(schema_version)
    .bind(&event.payload)
    .bind(event.timestamp)
    .execute(executor)
    .await
    .map_err(OutboxError::Persistence)?;

    // rows_affected == 0 means the key already existed and was skipped (benign).
    Ok(result.rows_affected() > 0)
}

#[async_trait]
impl GraphWriteCoordinator for PostgresGraphWriteCoordinator {
    async fn begin_outbox_transaction(
        &self,
    ) -> Result<Transaction<'static, Postgres>, OutboxError> {
        Ok(self.pool.begin().await?)
    }

    async fn append_outbox_event(&self, event: &OutboxEvent) -> Result<(), OutboxError> {
        let mut tx = self.begin_outbox_transaction().await?;
        self.append_outbox_event_in_tx(&mut tx, event).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn append_outbox_event_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        event: &OutboxEvent,
    ) -> Result<(), OutboxError> {
        let schema_version = validate_outbox_event(event)?;
        insert_outbox_event(&mut **tx, event, schema_version).await
    }

    async fn claim_pending_outbox(&self, limit: i64) -> Result<Vec<OutboxRecord>, OutboxError> {
        validate_claim_limit(limit)?;

        let rows = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = 'processing', updated_at = NOW()
            WHERE event_id IN (
                SELECT event_id
                FROM outbox_events
                WHERE status = 'pending' AND available_at <= NOW()
                ORDER BY available_at ASC, occurred_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING
                event_id,
                event_type,
                correlation_id,
                idempotency_key,
                schema_version,
                payload,
                attempts,
                stream_id,
                last_error,
                occurred_at,
                available_at
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        map_outbox_records(rows)
    }

    async fn claim_pending_outbox_for_correlation(
        &self,
        correlation_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, OutboxError> {
        validate_claim_limit(limit)?;

        let rows = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = 'processing', updated_at = NOW()
            WHERE event_id IN (
                SELECT event_id
                FROM outbox_events
                WHERE status = 'pending'
                  AND available_at <= NOW()
                  AND correlation_id = $1
                ORDER BY available_at ASC, occurred_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            RETURNING
                event_id,
                event_type,
                correlation_id,
                idempotency_key,
                schema_version,
                payload,
                attempts,
                stream_id,
                last_error,
                occurred_at,
                available_at
            "#,
        )
        .bind(correlation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        map_outbox_records(rows)
    }

    async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        stream_id: &str,
    ) -> Result<(), OutboxError> {
        if stream_id.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "stream_id must not be blank".to_owned(),
            ));
        }

        let update_result = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = 'published',
                stream_id = $2,
                published_at = NOW(),
                updated_at = NOW()
            WHERE event_id = $1
              AND status = 'processing'
            "#,
        )
        .bind(event_id)
        .bind(stream_id)
        .execute(&self.pool)
        .await?;

        if update_result.rows_affected() == 1 {
            return Ok(());
        }

        Err(self.transition_error(event_id, "processing").await?)
    }

    async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        retry_after_seconds: u64,
    ) -> Result<(), OutboxError> {
        if error_message.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "error_message must not be blank".to_owned(),
            ));
        }

        let retry_seconds = i64::try_from(retry_after_seconds).map_err(|_| {
            OutboxError::InvalidContract(
                "retry_after_seconds exceeds i64 range for scheduling".to_owned(),
            )
        })?;
        let available_at = Utc::now() + Duration::seconds(retry_seconds);

        let update_result = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = CASE
                    WHEN attempts + 1 >= $4 THEN 'failed'
                    ELSE 'pending'
                END,
                attempts = attempts + 1,
                last_error = $2,
                available_at = CASE
                    WHEN attempts + 1 >= $4 THEN available_at
                    ELSE $3
                END,
                updated_at = NOW()
            WHERE event_id = $1
              AND status = 'processing'
            "#,
        )
        .bind(event_id)
        .bind(error_message)
        .bind(available_at)
        .bind(MAX_OUTBOX_RETRIES)
        .execute(&self.pool)
        .await?;

        if update_result.rows_affected() == 1 {
            return Ok(());
        }

        Err(self.transition_error(event_id, "processing").await?)
    }
}

#[async_trait]
impl OutboxInspection for PostgresGraphWriteCoordinator {
    async fn has_pending_for_correlation(&self, correlation_id: Uuid) -> Result<bool, OutboxError> {
        let (exists,): (bool,) = sqlx::query_as(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM outbox_events
                WHERE correlation_id = $1
                  AND status IN ('pending', 'processing')
            )
            "#,
        )
        .bind(correlation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    async fn list_published_events_by_type(
        &self,
        event_type: &str,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, OutboxError> {
        if event_type.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "event_type must not be blank".to_owned(),
            ));
        }
        if limit <= 0 {
            return Err(OutboxError::InvalidContract(
                "limit must be greater than zero".to_owned(),
            ));
        }

        let rows = sqlx::query(
            r#"
            SELECT
                event_id,
                event_type,
                correlation_id,
                idempotency_key,
                schema_version,
                payload,
                attempts,
                stream_id,
                last_error,
                occurred_at,
                available_at
            FROM outbox_events
            WHERE status = 'published'
              AND event_type = $1
            ORDER BY occurred_at DESC
            LIMIT $2
            "#,
        )
        .bind(event_type)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(OutboxRecord {
                event: OutboxEvent {
                    event_id: row.try_get("event_id")?,
                    event_type: row.try_get("event_type")?,
                    correlation_id: row.try_get("correlation_id")?,
                    idempotency_key: row.try_get("idempotency_key")?,
                    schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                    timestamp: row.try_get("occurred_at")?,
                    payload: row.try_get("payload")?,
                },
                attempts: row.try_get("attempts")?,
                stream_id: row.try_get("stream_id")?,
                last_error: row.try_get("last_error")?,
                occurred_at: row.try_get("occurred_at")?,
                available_at: row.try_get("available_at")?,
            });
        }

        Ok(records)
    }
}

impl PostgresGraphWriteCoordinator {
    /// Appends an outbox event, silently skipping if the idempotency key already exists.
    ///
    /// Returns `true` if the row was newly inserted, `false` if a row with the
    /// same `idempotency_key` already existed and the insert was skipped.
    ///
    /// Use this ONLY for content-addressed events where replaying the same key is
    /// safe by design (e.g. rebuild vector emission). The standard
    /// `append_outbox_event` preserves strict exactly-once semantics and must
    /// NOT be replaced by this method for the general outbox case.
    pub async fn append_outbox_event_idempotent(
        &self,
        event: &OutboxEvent,
    ) -> Result<bool, OutboxError> {
        let schema_version = validate_outbox_event(event)?;
        let mut tx = self.begin_outbox_transaction().await?;
        let inserted = insert_outbox_event_idempotent(&mut *tx, event, schema_version).await?;
        tx.commit().await?;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn outbox_retry_limit_matches_event_contract() {
        assert_eq!(MAX_OUTBOX_RETRIES, 3);
    }

    #[test]
    fn qdrant_point_id_is_deterministic_for_same_content_hash() {
        let first = qdrant_point_id_from_content_hash("abc123");
        let second = qdrant_point_id_from_content_hash("abc123");
        let different = qdrant_point_id_from_content_hash("xyz789");

        assert_eq!(first, second);
        assert_ne!(first, different);
    }

    #[test]
    fn parse_vector_upsert_request_rejects_invalid_contract() {
        let error = parse_vector_upsert_request(&json!({"vector":[1.0]}))
            .expect_err("missing content_hash should fail");
        assert!(error.to_string().contains("content_hash"));
    }

    /// Absent `sparse` key parses to `sparse: None` — backward-compatible with
    /// dense-only outbox events written before hybrid support was added.
    #[test]
    fn parse_vector_upsert_request_absent_sparse_parses_to_none() {
        let payload = json!({
            "content_hash": "abc123",
            "vector": [0.1_f32, 0.2_f32],
            "payload": {}
        });
        let req = parse_vector_upsert_request(&payload)
            .expect("dense-only payload must parse without error");
        assert_eq!(req.content_hash, "abc123");
        assert_eq!(req.vector.len(), 2);
        assert!(
            req.sparse.is_none(),
            "absent 'sparse' key must parse to sparse: None"
        );
    }

    /// A well-formed `sparse` field round-trips through `parse_vector_upsert_request`.
    #[test]
    fn parse_vector_upsert_request_sparse_round_trips_correctly() {
        let payload = json!({
            "content_hash": "def456",
            "vector": [0.5_f32, 0.6_f32],
            "sparse": {
                "indices": [100_u32, 200_u32, 300_u32],
                "values": [1.5_f32, 0.8_f32, 2.1_f32]
            },
            "payload": { "skill_id": "s1" }
        });
        let req = parse_vector_upsert_request(&payload)
            .expect("payload with sparse must parse without error");
        let sparse = req.sparse.expect("sparse must be Some for hybrid payload");
        assert_eq!(sparse.indices, vec![100_u32, 200_u32, 300_u32]);
        assert_eq!(sparse.values.len(), 3);
        assert!(
            (sparse.values[0] - 1.5_f32).abs() < 1e-5,
            "sparse value[0] must round-trip: {}",
            sparse.values[0]
        );
    }

    /// An empty `sparse` object (indices=[], values=[]) is explicitly rejected —
    /// it indicates a construction bug and must never silently produce a point
    /// with an empty sparse component.
    #[test]
    fn parse_vector_upsert_request_rejects_empty_sparse() {
        let payload = json!({
            "content_hash": "gh789",
            "vector": [0.1_f32],
            "sparse": {
                "indices": [],
                "values": []
            }
        });
        let err = parse_vector_upsert_request(&payload).expect_err("empty sparse must be rejected");
        assert!(
            err.to_string().contains("sparse"),
            "error must mention 'sparse': {err}"
        );
    }

    /// Mismatched indices/values lengths are explicitly rejected.
    #[test]
    fn parse_vector_upsert_request_rejects_sparse_length_mismatch() {
        let payload = json!({
            "content_hash": "ij012",
            "vector": [0.1_f32],
            "sparse": {
                "indices": [1_u32, 2_u32],
                "values": [0.5_f32]
            }
        });
        let err = parse_vector_upsert_request(&payload)
            .expect_err("mismatched sparse lengths must be rejected");
        assert!(
            err.to_string().contains("length"),
            "error must mention 'length': {err}"
        );
    }
}
