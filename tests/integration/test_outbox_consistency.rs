use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::{HdbscanConfig, ScopeType};
use graph_builder::{
    DurableGraphState, GraphRebuildOrchestrator, ScopeRoot, SkillFileChange, SkillFileChangeKind,
    graph::rebuild::GraphRebuildError, watcher::FileChangeSource,
};
use infrastructure::{
    GraphWriteCoordinator, OutboxEvent, OutboxInspection, OutboxReconciler, OutboxRecord,
    OutboxRelay, OutboxVectorStore, VECTOR_UPSERT_EVENT_TYPE, VectorPointListing,
    qdrant_point_id_from_content_hash,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone)]
struct InMemoryOutboxCoordinator {
    state: std::sync::Arc<Mutex<InMemoryOutboxState>>,
}

#[derive(Clone)]
struct InMemoryOutboxEvent {
    event: OutboxEvent,
    status: String,
    attempts: i32,
    stream_id: Option<String>,
    last_error: Option<String>,
    occurred_at: DateTime<Utc>,
    available_at: DateTime<Utc>,
}

#[derive(Default)]
struct InMemoryOutboxState {
    events: Vec<InMemoryOutboxEvent>,
}

impl InMemoryOutboxCoordinator {
    fn new(seed_events: Vec<InMemoryOutboxEvent>) -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(InMemoryOutboxState {
                events: seed_events,
            })),
        }
    }

    fn event_by_id(&self, event_id: Uuid) -> InMemoryOutboxEvent {
        self.state
            .lock()
            .expect("coordinator lock should not be poisoned")
            .events
            .iter()
            .find(|event| event.event.event_id == event_id)
            .cloned()
            .expect("requested event should exist")
    }

    fn pending_count_for_correlation(&self, correlation_id: Uuid) -> usize {
        self.state
            .lock()
            .expect("coordinator lock should not be poisoned")
            .events
            .iter()
            .filter(|event| {
                event.event.correlation_id == correlation_id
                    && (event.status == "pending" || event.status == "processing")
            })
            .count()
    }
}

#[async_trait]
impl GraphWriteCoordinator for InMemoryOutboxCoordinator {
    async fn begin_outbox_transaction(
        &self,
    ) -> Result<
        sqlx::Transaction<'static, sqlx::Postgres>,
        infrastructure::persistence::outbox::OutboxError,
    > {
        panic!("begin_outbox_transaction is not used by in-memory test coordinator");
    }

    async fn append_outbox_event(
        &self,
        event: &OutboxEvent,
    ) -> Result<(), infrastructure::persistence::outbox::OutboxError> {
        let mut state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        if state
            .events
            .iter()
            .any(|candidate| candidate.event.idempotency_key == event.idempotency_key)
        {
            return Err(
                infrastructure::persistence::outbox::OutboxError::IdempotencyConflict {
                    idempotency_key: event.idempotency_key.clone(),
                },
            );
        }
        state.events.push(InMemoryOutboxEvent {
            event: event.clone(),
            status: "pending".to_owned(),
            attempts: 0,
            stream_id: None,
            last_error: None,
            occurred_at: event.timestamp,
            available_at: Utc::now(),
        });
        Ok(())
    }

    async fn append_outbox_event_in_tx(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _event: &OutboxEvent,
    ) -> Result<(), infrastructure::persistence::outbox::OutboxError> {
        panic!("append_outbox_event_in_tx is not used by in-memory test coordinator");
    }

    async fn claim_pending_outbox(
        &self,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, infrastructure::persistence::outbox::OutboxError> {
        let mut state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        let mut records = Vec::new();
        for event in &mut state.events {
            if records.len() >= limit as usize {
                break;
            }
            if event.status == "pending" && event.available_at <= Utc::now() {
                event.status = "processing".to_owned();
                records.push(OutboxRecord {
                    event: event.event.clone(),
                    attempts: event.attempts,
                    stream_id: event.stream_id.clone(),
                    last_error: event.last_error.clone(),
                    occurred_at: event.occurred_at,
                    available_at: event.available_at,
                });
            }
        }
        Ok(records)
    }

    async fn claim_pending_outbox_for_correlation(
        &self,
        correlation_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, infrastructure::persistence::outbox::OutboxError> {
        let mut state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        let mut records = Vec::new();
        for event in &mut state.events {
            if records.len() >= limit as usize {
                break;
            }
            if event.status == "pending"
                && event.available_at <= Utc::now()
                && event.event.correlation_id == correlation_id
            {
                event.status = "processing".to_owned();
                records.push(OutboxRecord {
                    event: event.event.clone(),
                    attempts: event.attempts,
                    stream_id: event.stream_id.clone(),
                    last_error: event.last_error.clone(),
                    occurred_at: event.occurred_at,
                    available_at: event.available_at,
                });
            }
        }
        Ok(records)
    }

    async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        stream_id: &str,
    ) -> Result<(), infrastructure::persistence::outbox::OutboxError> {
        let mut state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        let event = state
            .events
            .iter_mut()
            .find(|event| event.event.event_id == event_id)
            .expect("event should exist for mark_outbox_published");
        event.status = "published".to_owned();
        event.stream_id = Some(stream_id.to_owned());
        Ok(())
    }

    async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        retry_after_seconds: u64,
    ) -> Result<(), infrastructure::persistence::outbox::OutboxError> {
        let mut state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        let event = state
            .events
            .iter_mut()
            .find(|event| event.event.event_id == event_id)
            .expect("event should exist for mark_outbox_failed");
        event.attempts += 1;
        event.last_error = Some(error_message.to_owned());
        if event.attempts >= infrastructure::persistence::outbox::MAX_OUTBOX_RETRIES {
            event.status = "failed".to_owned();
        } else {
            event.status = "pending".to_owned();
            event.available_at = Utc::now() + Duration::seconds(retry_after_seconds as i64);
        }
        Ok(())
    }
}

#[async_trait]
impl OutboxInspection for InMemoryOutboxCoordinator {
    async fn has_pending_for_correlation(
        &self,
        correlation_id: Uuid,
    ) -> Result<bool, infrastructure::persistence::outbox::OutboxError> {
        Ok(self.pending_count_for_correlation(correlation_id) > 0)
    }

    async fn list_published_events_by_type(
        &self,
        event_type: &str,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, infrastructure::persistence::outbox::OutboxError> {
        let state = self
            .state
            .lock()
            .expect("coordinator lock should not be poisoned");
        let records = state
            .events
            .iter()
            .filter(|event| event.status == "published" && event.event.event_type == event_type)
            .take(limit as usize)
            .map(|event| OutboxRecord {
                event: event.event.clone(),
                attempts: event.attempts,
                stream_id: event.stream_id.clone(),
                last_error: event.last_error.clone(),
                occurred_at: event.occurred_at,
                available_at: event.available_at,
            })
            .collect::<Vec<_>>();
        Ok(records)
    }
}

#[derive(Default)]
struct InMemoryVectorStore {
    points: Mutex<HashMap<u64, (Vec<f32>, Value)>>,
    fail_once: Mutex<HashSet<u64>>,
}

impl InMemoryVectorStore {
    fn with_single_transient_failure(point_id: u64) -> Self {
        let mut fail_once = HashSet::new();
        fail_once.insert(point_id);
        Self {
            points: Mutex::new(HashMap::new()),
            fail_once: Mutex::new(fail_once),
        }
    }
}

#[async_trait]
impl OutboxVectorStore for InMemoryVectorStore {
    async fn upsert_vector(
        &self,
        point_id: u64,
        vector: &[f32],
        payload: &Value,
    ) -> Result<(), String> {
        let mut fail_once = self
            .fail_once
            .lock()
            .expect("vector fail_once lock should not be poisoned");
        if fail_once.remove(&point_id) {
            return Err("transient qdrant outage".to_owned());
        }
        drop(fail_once);
        self.points
            .lock()
            .expect("vector lock should not be poisoned")
            .insert(point_id, (vector.to_vec(), payload.clone()));
        Ok(())
    }

    async fn has_vector(&self, point_id: u64) -> Result<bool, String> {
        Ok(self
            .points
            .lock()
            .expect("vector lock should not be poisoned")
            .contains_key(&point_id))
    }

    async fn list_point_ids(&self) -> Result<VectorPointListing, String> {
        Ok(VectorPointListing {
            point_ids: self
                .points
                .lock()
                .expect("vector lock should not be poisoned")
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            is_complete: true,
        })
    }

    async fn delete_points(&self, point_ids: &[u64]) -> Result<(), String> {
        let mut points = self
            .points
            .lock()
            .expect("vector lock should not be poisoned");
        for point_id in point_ids {
            points.remove(point_id);
        }
        Ok(())
    }
}

fn sample_vector_payload(content_hash: &str) -> Value {
    json!({
        "content_hash": content_hash,
        "vector": [0.1, 0.2, 0.3],
        "payload": {
            "skill_id": "skill-123",
            "scope": "project"
        }
    })
}

fn seed_pending_vector_event(correlation_id: Uuid, content_hash: &str) -> InMemoryOutboxEvent {
    InMemoryOutboxEvent {
        event: OutboxEvent {
            event_id: Uuid::now_v7(),
            event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
            correlation_id,
            idempotency_key: format!("vector-upsert:{content_hash}"),
            schema_version: 1,
            timestamp: Utc::now(),
            payload: sample_vector_payload(content_hash),
        },
        status: "pending".to_owned(),
        attempts: 0,
        stream_id: None,
        last_error: None,
        occurred_at: Utc::now(),
        available_at: Utc::now(),
    }
}

#[tokio::test]
async fn relay_retries_transient_failure_and_marks_event_published_after_recovery() {
    let correlation_id = Uuid::now_v7();
    let pending_event = seed_pending_vector_event(correlation_id, "content-hash-retry");
    let point_id = qdrant_point_id_from_content_hash("content-hash-retry");
    let coordinator = InMemoryOutboxCoordinator::new(vec![pending_event.clone()]);
    let vector_store = InMemoryVectorStore::with_single_transient_failure(point_id);
    let relay = OutboxRelay::new(&coordinator, &vector_store, 10, 0)
        .expect("relay should initialize for valid contract");

    let first_pass = relay
        .relay_once()
        .await
        .expect("first relay pass should run");
    assert_eq!(first_pass.claimed, 1);
    assert_eq!(first_pass.failed, 1);
    assert_eq!(first_pass.published, 0);
    let after_failure = coordinator.event_by_id(pending_event.event.event_id);
    assert_eq!(after_failure.status, "pending");
    assert_eq!(after_failure.attempts, 1);
    assert_eq!(
        after_failure.last_error.as_deref(),
        Some("transient qdrant outage")
    );

    let second_pass = relay
        .relay_once()
        .await
        .expect("second relay pass should run");
    assert_eq!(second_pass.claimed, 1);
    assert_eq!(second_pass.failed, 0);
    assert_eq!(second_pass.published, 1);
    let after_publish = coordinator.event_by_id(pending_event.event.event_id);
    assert_eq!(after_publish.status, "published");
    let expected_stream_id = format!("qdrant:{point_id}");
    assert_eq!(
        after_publish.stream_id.as_deref(),
        Some(expected_stream_id.as_str())
    );
    assert!(
        vector_store
            .has_vector(point_id)
            .await
            .expect("vector lookup should succeed")
    );
}

#[tokio::test]
async fn drain_correlation_outbox_completes_with_unrelated_backlog_present() {
    let target_correlation_id = Uuid::now_v7();
    let unrelated_correlation_id = Uuid::now_v7();
    let unrelated_event_a =
        seed_pending_vector_event(unrelated_correlation_id, "content-hash-unrelated-a");
    let unrelated_event_b =
        seed_pending_vector_event(unrelated_correlation_id, "content-hash-unrelated-b");
    let target_event = seed_pending_vector_event(target_correlation_id, "content-hash-target");
    let coordinator = InMemoryOutboxCoordinator::new(vec![
        unrelated_event_a.clone(),
        unrelated_event_b.clone(),
        target_event.clone(),
    ]);
    let vector_store = InMemoryVectorStore::default();
    let relay = OutboxRelay::new(&coordinator, &vector_store, 1, 0)
        .expect("relay should initialize for valid contract");

    relay
        .drain_correlation_outbox(&coordinator, target_correlation_id, 2)
        .await
        .expect("drain should complete for target correlation");

    let target_after_drain = coordinator.event_by_id(target_event.event.event_id);
    assert_eq!(target_after_drain.status, "published");
    assert_eq!(
        coordinator
            .event_by_id(unrelated_event_a.event.event_id)
            .status,
        "pending"
    );
    assert_eq!(
        coordinator
            .event_by_id(unrelated_event_b.event.event_id)
            .status,
        "pending"
    );
}

#[tokio::test]
async fn reconciler_enqueues_repairs_for_missing_vectors_and_deletes_orphans() {
    let correlation_id = Uuid::now_v7();
    let published_event = {
        let mut event = seed_pending_vector_event(correlation_id, "published-hash");
        event.status = "published".to_owned();
        event
    };
    let expected_point_id = qdrant_point_id_from_content_hash("published-hash");
    let orphan_point_id = qdrant_point_id_from_content_hash("orphan-hash");
    let coordinator = InMemoryOutboxCoordinator::new(vec![published_event.clone()]);
    let vector_store = InMemoryVectorStore::default();
    vector_store
        .upsert_vector(orphan_point_id, &[9.9, 9.9], &json!({"orphan": true}))
        .await
        .expect("orphan vector should seed");
    let reconciler = OutboxReconciler::new(&coordinator, &vector_store, 100)
        .expect("reconciler should initialize for valid contract");

    let report = reconciler
        .reconcile_once()
        .await
        .expect("reconciliation should run");
    assert_eq!(report.scanned, 1);
    assert_eq!(report.missing_vectors, 1);
    assert_eq!(report.repair_enqueued, 1);
    assert_eq!(report.orphaned_vectors_deleted, 1);
    assert!(
        !vector_store
            .has_vector(orphan_point_id)
            .await
            .expect("orphan lookup should succeed")
    );
    assert!(
        coordinator.pending_count_for_correlation(correlation_id) > 0,
        "reconciliation should enqueue a repair event for replay"
    );
    assert!(
        !vector_store
            .has_vector(expected_point_id)
            .await
            .expect("expected vector lookup should succeed"),
        "reconciliation should enqueue repair rather than mutate vectors directly"
    );
}

#[tokio::test]
async fn reconciler_skips_orphan_delete_when_expected_scan_window_is_partial() {
    let correlation_id = Uuid::now_v7();
    let first_published_event = {
        let mut event = seed_pending_vector_event(correlation_id, "partial-window-hash-a");
        event.status = "published".to_owned();
        event
    };
    let second_published_event = {
        let mut event = seed_pending_vector_event(correlation_id, "partial-window-hash-b");
        event.status = "published".to_owned();
        event
    };
    let orphan_point_id = qdrant_point_id_from_content_hash("partial-window-orphan");
    let coordinator = InMemoryOutboxCoordinator::new(vec![
        first_published_event.clone(),
        second_published_event.clone(),
    ]);
    let vector_store = InMemoryVectorStore::default();
    vector_store
        .upsert_vector(orphan_point_id, &[9.9, 9.9], &json!({"orphan": true}))
        .await
        .expect("orphan vector should seed");
    let reconciler = OutboxReconciler::new(&coordinator, &vector_store, 1)
        .expect("reconciler should initialize for valid contract");

    let report = reconciler
        .reconcile_once()
        .await
        .expect("reconciliation should run");

    assert_eq!(report.scanned, 1);
    assert_eq!(report.missing_vectors, 1);
    assert_eq!(report.repair_enqueued, 1);
    assert_eq!(report.orphaned_vectors_deleted, 0);
    assert!(
        vector_store
            .has_vector(orphan_point_id)
            .await
            .expect("orphan lookup should succeed"),
        "reconciler must not delete vectors while expected visibility is partial"
    );
    assert!(
        coordinator.pending_count_for_correlation(correlation_id) > 0,
        "reconciliation should keep enqueuing missing-vector repairs in partial windows"
    );
}

#[tokio::test]
async fn reconciler_enqueues_repair_with_latest_payload_for_duplicate_point_history() {
    let correlation_id = Uuid::now_v7();
    let duplicated_content_hash = "duplicate-point-history";
    let point_id = qdrant_point_id_from_content_hash(duplicated_content_hash);
    let base_time = Utc::now();

    let stale_published_event = {
        let mut event = seed_pending_vector_event(correlation_id, duplicated_content_hash);
        event.status = "published".to_owned();
        event.occurred_at = base_time - Duration::minutes(5);
        event.event.timestamp = event.occurred_at;
        event.event.payload["payload"]["revision"] = json!("stale");
        event
    };
    let latest_published_event = {
        let mut event = seed_pending_vector_event(correlation_id, duplicated_content_hash);
        event.status = "published".to_owned();
        event.occurred_at = base_time;
        event.event.timestamp = event.occurred_at;
        event.event.payload["payload"]["revision"] = json!("latest");
        event
    };

    let coordinator = InMemoryOutboxCoordinator::new(vec![
        latest_published_event.clone(),
        stale_published_event.clone(),
    ]);
    let vector_store = InMemoryVectorStore::default();
    let reconciler = OutboxReconciler::new(&coordinator, &vector_store, 100)
        .expect("reconciler should initialize for valid contract");

    let report = reconciler
        .reconcile_once()
        .await
        .expect("reconciliation should run");

    assert_eq!(report.scanned, 2);
    assert_eq!(report.missing_vectors, 1);
    assert_eq!(report.repair_enqueued, 1);
    assert!(
        !vector_store
            .has_vector(point_id)
            .await
            .expect("vector lookup should succeed"),
        "reconciliation should enqueue repair events without mutating vectors directly"
    );

    let state = coordinator
        .state
        .lock()
        .expect("coordinator lock should not be poisoned");
    let repair_event = state
        .events
        .iter()
        .find(|event| {
            event.status == "pending"
                && event.event.correlation_id == correlation_id
                && event.event.idempotency_key.starts_with("reconcile:")
        })
        .expect("repair event should be queued for missing vector");
    assert_eq!(
        repair_event.event.payload["payload"]["revision"],
        json!("latest")
    );
}

#[derive(Default)]
struct OutboxDrainRequiredState {
    operation_log: Vec<String>,
}

#[async_trait]
impl DurableGraphState for OutboxDrainRequiredState {
    async fn persist_graph_mutation(
        &mut self,
        _mutation: graph_builder::graph::rebuild::DurableGraphMutation,
    ) -> Result<(), GraphRebuildError> {
        self.operation_log.push("persist_graph_mutation".to_owned());
        Ok(())
    }

    async fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError> {
        self.operation_log.push("mark_outbox_drained".to_owned());
        Err(GraphRebuildError::DurableWrite(
            "outbox backlog not drained".to_owned(),
        ))
    }

    async fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError> {
        self.operation_log.push("bump_graph_version".to_owned());
        Ok(1)
    }
}

fn fresh_sandbox() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("outbox-ordering-test-{nonce}"));
    fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

#[tokio::test]
async fn graph_rebuilt_is_not_emitted_when_outbox_drain_fails() {
    let sandbox = fresh_sandbox();
    let skill_dir = sandbox.join("skill-a");
    fs::create_dir_all(&skill_dir).expect("skill directory should be creatable");
    let skill_path = skill_dir.join("SKILL.md");
    fs::write(
        &skill_path,
        "# Skill A\n\ndescription: test\n\n## Procedures\n- step one\n- step two\n",
    )
    .expect("skill fixture should be writable");

    let scope = ScopeRoot::new("project", ScopeType::Project, sandbox.clone());
    let file_change = SkillFileChange {
        scope_id: "project".to_owned(),
        scope_type: ScopeType::Project,
        file_path: skill_path.clone(),
        kind: SkillFileChangeKind::Created,
        source: FileChangeSource::Direct,
        content_hash: "hash-ordering".to_owned(),
        idempotency_key: "ordering:hash-ordering".to_owned(),
    };

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService;
    let mut durable_state = OutboxDrainRequiredState::default();
    let mut published_events = Vec::new();
    let mut orchestrator =
        GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events, &embedder);

    let result = orchestrator
        .rebuild_from_changes(&[scope], &[file_change], &HdbscanConfig::default())
        .await;
    assert!(result.is_err());
    assert!(
        published_events.is_empty(),
        "graph.rebuilt must stay hidden when outbox drain fails"
    );
    assert_eq!(
        durable_state.operation_log,
        vec![
            "persist_graph_mutation".to_owned(),
            "mark_outbox_drained".to_owned()
        ]
    );

    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}

#[derive(Default)]
struct RelayBackedState {
    operation_log: Vec<String>,
    graph_version: i64,
    #[allow(dead_code)]
    outbox_events: Vec<OutboxEvent>,
    outbox_pending_count: usize,
}

#[async_trait]
impl DurableGraphState for RelayBackedState {
    async fn persist_graph_mutation(
        &mut self,
        _mutation: graph_builder::graph::rebuild::DurableGraphMutation,
    ) -> Result<(), GraphRebuildError> {
        self.operation_log.push("persist_graph_mutation".to_owned());
        Ok(())
    }

    async fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError> {
        self.operation_log.push("mark_outbox_drained".to_owned());
        if self.outbox_pending_count > 0 {
            return Err(GraphRebuildError::DurableWrite(
                "outbox still has pending items after relay drain".to_owned(),
            ));
        }
        Ok(())
    }

    async fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError> {
        self.operation_log.push("bump_graph_version".to_owned());
        self.graph_version += 1;
        Ok(self.graph_version)
    }
}

#[tokio::test]
async fn graph_rebuilt_ordering_persist_then_outbox_drain_then_version_then_event() {
    let sandbox = fresh_sandbox();
    let skill_dir = sandbox.join("ordering-skill");
    fs::create_dir_all(&skill_dir).expect("skill directory should be creatable");
    fs::write(
        skill_dir.join("SKILL.md"),
        "# Ordering Skill\n\ndescription: ordering test\n\n## Procedures\n- step one\n",
    )
    .expect("skill fixture should be writable");

    let scope = ScopeRoot::new("project", ScopeType::Project, sandbox.clone());
    let file_change = SkillFileChange {
        scope_id: "project".to_owned(),
        scope_type: ScopeType::Project,
        file_path: skill_dir.join("SKILL.md"),
        kind: SkillFileChangeKind::Created,
        source: FileChangeSource::Direct,
        content_hash: "hash-ordering".to_owned(),
        idempotency_key: "ordering:hash".to_owned(),
    };

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService;
    let mut durable_state = RelayBackedState::default();
    let mut published_events = Vec::new();
    let mut orchestrator =
        GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events, &embedder);

    let _outcome = orchestrator
        .rebuild_from_changes(&[scope], &[file_change], &HdbscanConfig::default())
        .await
        .expect("rebuild should succeed with clean relay");

    assert_eq!(
        durable_state.operation_log,
        vec![
            "persist_graph_mutation".to_owned(),
            "mark_outbox_drained".to_owned(),
            "bump_graph_version".to_owned(),
        ]
    );
    assert_eq!(published_events.len(), 1);
    assert_eq!(published_events[0].event_type, "graph.rebuilt");

    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}

#[tokio::test]
async fn graph_rebuilt_fails_when_outbox_drain_reports_pending_items() {
    let sandbox = fresh_sandbox();
    let skill_dir = sandbox.join("backlog-skill");
    fs::create_dir_all(&skill_dir).expect("skill directory should be creatable");
    fs::write(
        skill_dir.join("SKILL.md"),
        "# Backlog Skill\n\ndescription: drain test\n\n## Procedures\n- step one\n",
    )
    .expect("skill fixture should be writable");

    let scope = ScopeRoot::new("project", ScopeType::Project, sandbox.clone());
    let file_change = SkillFileChange {
        scope_id: "project".to_owned(),
        scope_type: ScopeType::Project,
        file_path: skill_dir.join("SKILL.md"),
        kind: SkillFileChangeKind::Created,
        source: FileChangeSource::Direct,
        content_hash: "hash-backlog".to_owned(),
        idempotency_key: "backlog:hash".to_owned(),
    };

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService;
    let mut durable_state = RelayBackedState {
        operation_log: Vec::new(),
        graph_version: 0,
        outbox_events: Vec::new(),
        outbox_pending_count: 3,
    };
    let mut published_events = Vec::new();
    let mut orchestrator =
        GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events, &embedder);

    let result = orchestrator
        .rebuild_from_changes(&[scope], &[file_change], &HdbscanConfig::default())
        .await;
    assert!(result.is_err());
    assert!(
        published_events.is_empty(),
        "graph.rebuilt must not fire when outbox has pending backlog"
    );
    assert_eq!(
        durable_state.operation_log,
        vec![
            "persist_graph_mutation".to_owned(),
            "mark_outbox_drained".to_owned(),
        ]
    );

    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}

/// Proves the root cause of the 234-skill corpus failure: `drain_correlation_outbox`
/// with `max_polls=5` and `claim_limit=10` errors before draining 60 events.
///
/// 60 events / 10 per cycle = 6 required cycles. With the old cap of 5, the drain
/// stops at 50 events and returns an error, leaving 10 events stuck `pending`.
/// This test documents the failure mode — it must always pass (the error is expected).
#[tokio::test]
async fn drain_correlation_outbox_errors_when_poll_cap_exhausted_before_empty() {
    let target_correlation_id = Uuid::now_v7();
    let seed_events: Vec<InMemoryOutboxEvent> = (0..60)
        .map(|i| {
            seed_pending_vector_event(target_correlation_id, &format!("cap-exceeded-hash-{i}"))
        })
        .collect();
    let coordinator = InMemoryOutboxCoordinator::new(seed_events);
    let vector_store = InMemoryVectorStore::default();
    let relay = OutboxRelay::new(&coordinator, &vector_store, 10, 0)
        .expect("relay should initialize for valid contract");

    let result = relay
        .drain_correlation_outbox(&coordinator, target_correlation_id, 5)
        .await;

    assert!(
        result.is_err(),
        "drain must error when max_polls=5 is exhausted before 60 events drain (needs 6 cycles)"
    );
    let error_text = result.unwrap_err().to_string();
    assert!(
        error_text.contains("did not drain after 5 poll cycles"),
        "error must name the exhausted poll cap: {error_text}"
    );
    // Confirm 10 events are still stuck pending — exactly the last batch.
    assert_eq!(
        coordinator.pending_count_for_correlation(target_correlation_id),
        10,
        "with max_polls=5 at batch=10, the last 10 of 60 events remain stuck pending"
    );
}

/// Proves that `drain_correlation_outbox` completes fully when the event count exceeds
/// one batch, requiring more than the old hardcoded cap of 5 poll cycles.
///
/// This is a direct regression guard for the 234-skill corpus failure where
/// `max_polls=5` at batch=10 only reached 50 events and errored the rebuild.
/// Here we seed 60 events for one correlation at batch=10 → 6 required cycles.
/// Passing `max_polls=1_000` must drain all 60 and succeed.
#[tokio::test]
async fn drain_correlation_outbox_completes_fully_across_more_than_five_poll_cycles() {
    let target_correlation_id = Uuid::now_v7();
    // 60 events at claim_limit=10 requires exactly 6 poll cycles to drain —
    // the old cap of 5 would have errored at event 50, leaving 10 stuck.
    let seed_events: Vec<InMemoryOutboxEvent> = (0..60)
        .map(|i| seed_pending_vector_event(target_correlation_id, &format!("multi-batch-hash-{i}")))
        .collect();
    let event_ids: Vec<Uuid> = seed_events
        .iter()
        .map(|event| event.event.event_id)
        .collect();
    let coordinator = InMemoryOutboxCoordinator::new(seed_events);
    let vector_store = InMemoryVectorStore::default();
    // claim_limit=10 means each relay_once_for_correlation call processes at most 10 events.
    let relay = OutboxRelay::new(&coordinator, &vector_store, 10, 0)
        .expect("relay should initialize for valid contract");

    relay
        .drain_correlation_outbox(&coordinator, target_correlation_id, 1_000)
        .await
        .expect("drain must complete for 60 events across 6 poll cycles");

    for (i, event_id) in event_ids.iter().enumerate() {
        let event = coordinator.event_by_id(*event_id);
        assert_eq!(
            event.status, "published",
            "event {i} (id={event_id}) must be published after full drain"
        );
    }
    assert_eq!(
        coordinator.pending_count_for_correlation(target_correlation_id),
        0,
        "all 60 pending events must be drained — none should remain pending"
    );
}

/// Proves that `relay_all_pending_to_completion` drains orphaned pending events
/// from multiple correlations, regardless of which correlation produced them.
///
/// This guards the startup self-heal path: a previously-failed rebuild leaves
/// `pending` events behind whose correlation_id matches the dead rebuild.
/// `relay_all_pending_to_completion` must relay them all to Qdrant.
#[tokio::test]
async fn relay_all_pending_to_completion_drains_orphaned_events_from_multiple_correlations() {
    let correlation_a = Uuid::now_v7();
    let correlation_b = Uuid::now_v7();
    // Simulate two failed rebuilds leaving orphaned pending events.
    let orphaned_events: Vec<InMemoryOutboxEvent> = (0..15)
        .map(|i| {
            let correlation_id = if i < 8 { correlation_a } else { correlation_b };
            seed_pending_vector_event(correlation_id, &format!("orphan-hash-{i}"))
        })
        .collect();
    let event_ids: Vec<Uuid> = orphaned_events
        .iter()
        .map(|event| event.event.event_id)
        .collect();
    let coordinator = InMemoryOutboxCoordinator::new(orphaned_events);
    let vector_store = InMemoryVectorStore::default();
    let relay = OutboxRelay::new(&coordinator, &vector_store, 10, 0)
        .expect("relay should initialize for valid contract");

    let total_published = relay
        .relay_all_pending_to_completion(1_000)
        .await
        .expect("relay must drain all orphaned pending events");

    assert_eq!(
        total_published, 15,
        "all 15 orphaned events across two correlations must be published"
    );
    for (i, event_id) in event_ids.iter().enumerate() {
        let event = coordinator.event_by_id(*event_id);
        assert_eq!(
            event.status, "published",
            "orphaned event {i} (id={event_id}) must reach published state"
        );
    }
}
