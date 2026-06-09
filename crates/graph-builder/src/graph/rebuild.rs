use async_trait::async_trait;
use chrono::Utc;
use infrastructure::{
    EventEnvelope, LiveGraphCommunityRecord, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
    LiveGraphSubunitRecord, OutboxEvent, OutboxRelay, OutboxVectorStore,
    PostgresGraphWriteCoordinator, PostgresRebuildCoordinator, RebuildCoordinator,
    VECTOR_UPSERT_EVENT_TYPE, stable_skill_uuid,
};
use retrieval::{Bm25Index, SkillLexicalFields, build_skill_sparse_vectors, skill_lexical_document};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use domain::{EmbeddingService, HdbscanConfig, ScopeRoot};

use crate::{
    graph::{
        build::{BuiltSkill, GraphBuildError, build_skills_from_scope_roots},
        communities::{CommunityAssignment, assign_communities},
    },
    watcher::SkillFileChange,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub action: String,
    pub entity_id: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableGraphMutation {
    pub rebuilt_at: chrono::DateTime<Utc>,
    pub skills: Vec<BuiltSkill>,
    pub communities: Vec<CommunityAssignment>,
    pub audits: Vec<AuditRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRebuildOutcome {
    pub graph_version: i64,
    pub skills_count: usize,
    pub communities_count: usize,
}

#[derive(Debug, Error)]
pub enum GraphRebuildError {
    #[error("graph build failed: {0}")]
    Build(#[from] GraphBuildError),
    #[error("durable state write failed: {0}")]
    DurableWrite(String),
    #[error("event publication failed: {0}")]
    EventPublication(String),
}

#[async_trait]
pub trait DurableGraphState {
    async fn persist_graph_mutation(
        &mut self,
        mutation: DurableGraphMutation,
    ) -> Result<(), GraphRebuildError>;
    async fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError>;
    async fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError>;
}

pub struct GraphRebuildOrchestrator<'a, T>
where
    T: DurableGraphState,
{
    durable_state: &'a mut T,
    published_events: &'a mut Vec<EventEnvelope>,
    embedding_service: &'a dyn EmbeddingService,
}

impl<'a, T> GraphRebuildOrchestrator<'a, T>
where
    T: DurableGraphState,
{
    pub fn new(
        durable_state: &'a mut T,
        published_events: &'a mut Vec<EventEnvelope>,
        embedding_service: &'a dyn EmbeddingService,
    ) -> Self {
        Self {
            durable_state,
            published_events,
            embedding_service,
        }
    }

    pub async fn rebuild_from_changes(
        &mut self,
        scope_roots: &[ScopeRoot],
        file_changes: &[SkillFileChange],
        hdbscan_config: &HdbscanConfig,
    ) -> Result<GraphRebuildOutcome, GraphRebuildError> {
        let skills = build_skills_from_scope_roots(scope_roots, self.embedding_service).await?;
        let communities =
            assign_communities(&skills, hdbscan_config).map_err(GraphRebuildError::DurableWrite)?;
        let audits = file_changes
            .iter()
            .map(|change| AuditRecord {
                action: "graph.rebuild.file_change".to_owned(),
                entity_id: change.idempotency_key.clone(),
                metadata: json!({
                    "scope": change.scope_id,
                    "path": change.file_path.display().to_string(),
                    "source": format!("{:?}", change.source),
                    "change_type": format!("{:?}", change.kind),
                }),
            })
            .collect::<Vec<_>>();

        let mutation = DurableGraphMutation {
            rebuilt_at: Utc::now(),
            skills: skills.clone(),
            communities: communities.clone(),
            audits,
        };

        self.durable_state.persist_graph_mutation(mutation).await?;
        self.durable_state.mark_outbox_drained().await?;
        let graph_version = self.durable_state.bump_graph_version().await?;

        self.published_events.push(EventEnvelope::new(
            "graph.rebuilt",
            format!("graph.rebuilt:{graph_version}"),
            json!({
                "graph_version": graph_version,
                "skills_count": skills.len(),
                "communities_count": communities.len(),
            }),
        ));

        Ok(GraphRebuildOutcome {
            graph_version,
            skills_count: skills.len(),
            communities_count: communities.len(),
        })
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Default)]
pub struct InMemoryDurableGraphState {
    pub operation_log: Vec<String>,
    pub graph_version: i64,
    pub mutations: Vec<DurableGraphMutation>,
    allow_synthetic_outbox_drain: bool,
}

#[cfg(any(test, feature = "test-utils"))]
impl InMemoryDurableGraphState {
    pub fn with_synthetic_outbox_drain() -> Self {
        Self {
            allow_synthetic_outbox_drain: true,
            ..Self::default()
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl DurableGraphState for InMemoryDurableGraphState {
    async fn persist_graph_mutation(
        &mut self,
        mutation: DurableGraphMutation,
    ) -> Result<(), GraphRebuildError> {
        self.operation_log.push("persist_graph_mutation".to_owned());
        self.mutations.push(mutation);
        Ok(())
    }

    async fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError> {
        if !self.allow_synthetic_outbox_drain {
            return Err(GraphRebuildError::DurableWrite(
                "outbox drain boundary is not wired for this durable state; use a runtime relay-backed durable state or explicitly opt in with InMemoryDurableGraphState::with_synthetic_outbox_drain() for test-only execution".to_owned(),
            ));
        }
        self.operation_log.push("mark_outbox_drained".to_owned());
        Ok(())
    }

    async fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError> {
        self.operation_log.push("bump_graph_version".to_owned());
        self.graph_version += 1;
        Ok(self.graph_version)
    }
}

/// Durable graph state backed by Postgres and a Qdrant vector store.
///
/// When `hybrid_collection` is `Some(name)` (set by `with_hybrid_collection`),
/// the rebuild write path computes real BM25 sparse vectors for each skill and
/// includes them in the outbox payload. The relay then routes those events to
/// `OutboxVectorStore::upsert_hybrid` targeting the named hybrid collection.
///
/// When `hybrid_collection` is `None` (the default), sparse vectors are never
/// computed and the relay follows the existing dense-only path unchanged.
#[derive(Debug)]
pub struct PostgresDurableGraphState<'a, S>
where
    S: OutboxVectorStore,
{
    pub rebuild_coordinator: &'a PostgresRebuildCoordinator,
    pub outbox_coordinator: &'a PostgresGraphWriteCoordinator,
    pub vector_store: &'a S,
    rebuild_correlation_id: Uuid,
    /// When set, sparse BM25 vectors are written into the outbox payload and
    /// the relay routes to `upsert_hybrid` on this collection name.
    hybrid_collection: Option<String>,
}

impl<'a, S> PostgresDurableGraphState<'a, S>
where
    S: OutboxVectorStore,
{
    pub fn new(
        rebuild_coordinator: &'a PostgresRebuildCoordinator,
        outbox_coordinator: &'a PostgresGraphWriteCoordinator,
        vector_store: &'a S,
    ) -> Self {
        Self {
            rebuild_coordinator,
            outbox_coordinator,
            vector_store,
            rebuild_correlation_id: Uuid::now_v7(),
            hybrid_collection: None,
        }
    }

    /// Configures this state for hybrid upserts.
    ///
    /// When called, subsequent calls to `persist_graph_mutation` will compute
    /// real BM25 sparse vectors for each skill and include them in the outbox
    /// payload. The relay will route those events to `upsert_hybrid` on the
    /// supplied collection name.
    ///
    /// Must be called with the model-keyed hybrid collection name produced by
    /// `model_keyed_hybrid_collection_name`.
    pub fn with_hybrid_collection(mut self, collection_name: String) -> Self {
        self.hybrid_collection = Some(collection_name);
        self
    }
}

#[async_trait]
impl<S> DurableGraphState for PostgresDurableGraphState<'_, S>
where
    S: OutboxVectorStore + Send + Sync,
{
    async fn persist_graph_mutation(
        &mut self,
        mutation: DurableGraphMutation,
    ) -> Result<(), GraphRebuildError> {
        let live_skills: Vec<LiveGraphSkillRecord> = mutation
            .skills
            .iter()
            .map(|skill| LiveGraphSkillRecord {
                stable_id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                scope: skill.scope_type,
                tags: skill.tags.clone(),
                // Persist the real SKILL.md path so the retrieval boot adapter
                // can use true provenance instead of the scope-root stand-in.
                source_paths: vec![skill.source_path.display().to_string()],
                subunits: skill
                    .subunits
                    .iter()
                    .map(|subunit| LiveGraphSubunitRecord {
                        kind: subunit.kind,
                        title: subunit.title.clone(),
                        content: subunit.content.clone(),
                    })
                    .collect(),
                // Multi-view WRITE-AHEAD fields: sourced from SKILL.md frontmatter.
                use_when: skill.use_when.clone(),
                avoid_when: skill.avoid_when.clone(),
                artifacts: skill.artifacts.clone(),
                tools: skill.tools.clone(),
                invariants: skill.invariants.clone(),
                requires: skill.requires.clone(),
                produces: skill.produces.clone(),
            })
            .collect();

        let live_communities: Vec<LiveGraphCommunityRecord> = mutation
            .communities
            .iter()
            .map(|community| LiveGraphCommunityRecord {
                stable_id: community.community_name.clone(),
                name: community.community_name.clone(),
                scope: community.scope,
                member_skill_ids: community.skill_ids.clone(),
                source: community.source.as_db_str().to_owned(),
            })
            .collect();

        self.rebuild_coordinator
            .replace_snapshot_and_bump_version(LiveGraphSnapshotMutation {
                rebuilt_at: mutation.rebuilt_at,
                skills: live_skills,
                communities: live_communities,
            })
            .await
            .map_err(|error| GraphRebuildError::DurableWrite(error.to_string()))?;

        // When the qdrant_hybrid backend is active, build real BM25 sparse vectors
        // for each skill and include them in the outbox payload. The relay reads these
        // and routes to `upsert_hybrid_point` on the hybrid collection.
        //
        // Field policy and avoid_when exclusion rationale live in
        // `retrieval::bm25::skill_lexical_document` — the single source of truth shared
        // with the read-side BM25 index path in mcp-server.
        let sparse_vecs: Option<Vec<(Vec<u32>, Vec<f32>)>> = if self.hybrid_collection.is_some() {
            let raw_docs: Vec<(usize, String)> = mutation
                .skills
                .iter()
                .enumerate()
                .map(|(idx, skill)| {
                    let subunit_text: String = skill
                        .subunits
                        .iter()
                        .map(|su| format!("{} {}", su.title, su.content))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let doc = skill_lexical_document(&SkillLexicalFields {
                        name: &skill.name,
                        description: &skill.description,
                        tags: &skill.tags,
                        tools: &skill.tools,
                        artifacts: &skill.artifacts,
                        invariants: &skill.invariants,
                        use_when: &skill.use_when,
                        requires: &skill.requires,
                        produces: &skill.produces,
                        subunit_text: &subunit_text,
                    });
                    (idx, doc)
                })
                .collect();
            let bm25_index = Bm25Index::build(&raw_docs);
            Some(build_skill_sparse_vectors(&raw_docs, &bm25_index))
        } else {
            None
        };

        for (skill_idx, skill) in mutation.skills.iter().enumerate() {
            // The Qdrant `skill_id` must match `skills.id` in Postgres (a UUID),
            // so the qdrant_hybrid arm can join Qdrant query hits against the
            // in-memory snapshot.  `stable_skill_uuid(skill.id)` applies the same
            // blake3→UUID derivation that the PG persistence layer uses when
            // INSERTing the skill row (see `stable_uuid("skill", stable_id)` in
            // `infrastructure::persistence::rebuild`).
            let skill_uuid = stable_skill_uuid(&skill.id).to_string();
            let mut vector_payload = json!({
                "content_hash": skill.id,
                "vector": skill.embedding,
                "payload": {
                    "skill_id": skill_uuid,
                    "name": skill.name,
                    "scope": format!("{:?}", skill.scope_type),
                    "tags": skill.tags,
                }
            });

            // Attach the sparse vector to the payload when operating in hybrid mode.
            // An empty sparse vector (skill with no lexical tokens) is omitted — the
            // relay must never upsert an empty sparse component into Qdrant, and the
            // point will fall back to the dense path for that edge case.
            if let Some(ref vecs) = sparse_vecs {
                let (ref indices, ref values) = vecs[skill_idx];
                if !indices.is_empty() {
                    vector_payload["sparse"] = json!({
                        "indices": indices,
                        "values": values,
                    });
                }
            }

            let outbox_event = OutboxEvent {
                event_id: Uuid::now_v7(),
                event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
                // The correlation_id on a skipped (already-published) event would
                // not match this rebuild, so it only matters for newly-inserted rows.
                correlation_id: self.rebuild_correlation_id,
                // Content-addressed key: same skill content always produces the
                // same key. A key that already exists means the vector is already
                // enqueued/published — skipping is correct and safe.
                idempotency_key: format!("graph.rebuild:vector:{}", skill.id),
                schema_version: 1,
                timestamp: Utc::now(),
                payload: vector_payload,
            };
            self.outbox_coordinator
                .append_outbox_event_idempotent(&outbox_event)
                .await
                .map_err(|error| {
                    GraphRebuildError::DurableWrite(format!(
                        "outbox append for {}: {error}",
                        skill.id,
                    ))
                })?;
        }

        Ok(())
    }

    async fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError> {
        let mut relay = OutboxRelay::new(self.outbox_coordinator, self.vector_store, 10, 0)
            .map_err(|error| GraphRebuildError::DurableWrite(error.to_string()))?;
        // Thread the hybrid collection name through to the relay so it routes
        // sparse-carrying events to `upsert_hybrid` instead of `upsert_vector`.
        if let Some(ref hybrid_col) = self.hybrid_collection {
            relay = relay.with_hybrid_collection(hybrid_col.clone());
        }
        relay
            // Drains to completion — no arbitrary poll cap. The whole corpus's
            // vectors must reach Qdrant, however many cycles that takes; a genuine
            // stall fails loud inside the drain rather than being cut off at a count.
            .drain_correlation_outbox(self.outbox_coordinator, self.rebuild_correlation_id)
            .await
            .map_err(|error| GraphRebuildError::DurableWrite(error.to_string()))
    }

    async fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError> {
        self.rebuild_coordinator
            .bump_graph_version()
            .await
            .map_err(|error| GraphRebuildError::DurableWrite(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use domain::{HdbscanConfig, ScopeRoot, ScopeType};

    use super::*;
    use crate::graph::embeddings::DeterministicEmbeddingService;

    #[tokio::test]
    async fn in_memory_durable_state_fails_closed_without_explicit_synthetic_drain_opt_in() {
        let mut state = InMemoryDurableGraphState::default();

        let result = state.mark_outbox_drained().await;

        assert!(result.is_err());
        let error_text = result
            .expect_err("default in-memory state must not fake outbox drain completion")
            .to_string();
        assert!(
            error_text.contains("not wired"),
            "error should explicitly report missing outbox drain wiring"
        );
        assert!(
            state.operation_log.is_empty(),
            "failed drain should not log synthetic completion"
        );
    }

    /// Proves that `InMemoryDurableGraphState` can run multiple consecutive rebuilds
    /// without erroring.
    ///
    /// The `InMemoryDurableGraphState` does not simulate outbox idempotency conflicts —
    /// it is a boundary test proving the orchestrator flow completes correctly for
    /// repeated rebuilds when the durable state layer is permissive (as it must be
    /// after the idempotent-enqueue fix in `PostgresDurableGraphState`).
    #[tokio::test]
    async fn orchestrator_rebuild_is_idempotent_across_consecutive_cycles() {
        let scope = ScopeRoot::new(
            "project",
            ScopeType::Project,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        );
        let scope_roots = vec![scope];
        let changes: Vec<SkillFileChange> = vec![];
        let embedder = DeterministicEmbeddingService;
        let hdbscan_config = HdbscanConfig::default();

        let mut state = InMemoryDurableGraphState::with_synthetic_outbox_drain();
        let mut published_events: Vec<EventEnvelope> = Vec::new();

        let first_outcome = {
            let mut orchestrator =
                GraphRebuildOrchestrator::new(&mut state, &mut published_events, &embedder);
            orchestrator
                .rebuild_from_changes(&scope_roots, &changes, &hdbscan_config)
                .await
        };
        assert!(
            first_outcome.is_ok(),
            "first rebuild should succeed: {:?}",
            first_outcome
        );
        assert_eq!(
            published_events.len(),
            1,
            "first rebuild should push one graph.rebuilt envelope"
        );

        // Clear published_events to simulate the drain that follows in the real loop.
        published_events.clear();

        // Second rebuild with the same scope (simulates an unchanged skill set).
        let second_outcome = {
            let mut orchestrator =
                GraphRebuildOrchestrator::new(&mut state, &mut published_events, &embedder);
            orchestrator
                .rebuild_from_changes(&scope_roots, &changes, &hdbscan_config)
                .await
        };
        assert!(
            second_outcome.is_ok(),
            "second rebuild on same skill set must not error (idempotency contract): {:?}",
            second_outcome
        );
        assert_eq!(
            published_events.len(),
            1,
            "second rebuild should push one graph.rebuilt envelope"
        );

        // Version must advance on each successful rebuild.
        let first_version = first_outcome.unwrap().graph_version;
        let second_version = second_outcome.unwrap().graph_version;
        assert!(
            second_version > first_version,
            "graph_version must advance on each rebuild: first={first_version}, second={second_version}"
        );
    }
}
