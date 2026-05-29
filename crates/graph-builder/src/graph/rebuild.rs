use async_trait::async_trait;
use chrono::Utc;
use infrastructure::{
    EventEnvelope, GraphWriteCoordinator, LiveGraphCommunityRecord, LiveGraphSkillRecord,
    LiveGraphSnapshotMutation, LiveGraphSubunitRecord, OutboxEvent, OutboxRelay,
    OutboxVectorStore, PostgresGraphWriteCoordinator, PostgresRebuildCoordinator,
    RebuildCoordinator, VECTOR_UPSERT_EVENT_TYPE,
};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    graph::{
        build::{BuiltSkill, GraphBuildError, build_skills_from_scope_roots},
        communities::{CommunityAssignment, assign_communities},
        embeddings::DeterministicEmbeddingGenerator,
    },
    watcher::{ScopeRoot, SkillFileChange},
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
}

impl<'a, T> GraphRebuildOrchestrator<'a, T>
where
    T: DurableGraphState,
{
    pub fn new(durable_state: &'a mut T, published_events: &'a mut Vec<EventEnvelope>) -> Self {
        Self {
            durable_state,
            published_events,
        }
    }

    pub async fn rebuild_from_changes(
        &mut self,
        scope_roots: &[ScopeRoot],
        file_changes: &[SkillFileChange],
    ) -> Result<GraphRebuildOutcome, GraphRebuildError> {
        let skills = build_skills_from_scope_roots(scope_roots, &DeterministicEmbeddingGenerator)?;
        let communities = assign_communities(&skills);
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

        self.durable_state
            .persist_graph_mutation(mutation)
            .await?;
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

#[derive(Debug)]
pub struct InMemoryDurableGraphState {
    pub operation_log: Vec<String>,
    pub graph_version: i64,
    pub mutations: Vec<DurableGraphMutation>,
    allow_synthetic_outbox_drain: bool,
}

impl Default for InMemoryDurableGraphState {
    fn default() -> Self {
        Self {
            operation_log: Vec::new(),
            graph_version: 0,
            mutations: Vec::new(),
            allow_synthetic_outbox_drain: false,
        }
    }
}

impl InMemoryDurableGraphState {
    pub fn with_synthetic_outbox_drain() -> Self {
        Self {
            allow_synthetic_outbox_drain: true,
            ..Self::default()
        }
    }
}

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

#[derive(Debug)]
pub struct PostgresDurableGraphState<'a, S>
where
    S: OutboxVectorStore,
{
    pub rebuild_coordinator: &'a PostgresRebuildCoordinator,
    pub outbox_coordinator: &'a PostgresGraphWriteCoordinator,
    pub vector_store: &'a S,
    rebuild_correlation_id: Uuid,
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
        }
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
                subunits: skill
                    .subunits
                    .iter()
                    .map(|subunit| LiveGraphSubunitRecord {
                        kind: subunit.kind,
                        title: subunit.title.clone(),
                        content: subunit.content.clone(),
                    })
                    .collect(),
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

        for skill in &mutation.skills {
            let vector_payload = json!({
                "content_hash": skill.id,
                "vector": skill.embedding,
                "payload": {
                    "skill_id": skill.id,
                    "name": skill.name,
                    "scope": format!("{:?}", skill.scope_type),
                    "tags": skill.tags,
                }
            });
            let outbox_event = OutboxEvent {
                event_id: Uuid::now_v7(),
                event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
                correlation_id: self.rebuild_correlation_id,
                idempotency_key: format!("graph.rebuild:vector:{}", skill.id),
                schema_version: 1,
                timestamp: Utc::now(),
                payload: vector_payload,
            };
            self.outbox_coordinator
                .append_outbox_event(&outbox_event)
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
        let relay = OutboxRelay::new(self.outbox_coordinator, self.vector_store, 10, 0)
            .map_err(|error| GraphRebuildError::DurableWrite(error.to_string()))?;
        relay
            .drain_correlation_outbox(
                self.outbox_coordinator,
                self.rebuild_correlation_id,
                5,
            )
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
    use super::*;

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
}