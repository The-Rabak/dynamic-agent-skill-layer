use chrono::Utc;
use infrastructure::EventEnvelope;
use serde_json::json;
use thiserror::Error;

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

/// Durable state boundary for graph mutations, outbox drain, and graph_version invalidation.
pub trait DurableGraphState {
    fn persist_graph_mutation(
        &mut self,
        mutation: DurableGraphMutation,
    ) -> Result<(), GraphRebuildError>;
    fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError>;
    fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError>;
}

/// Rebuild orchestration: durable writes -> outbox drain -> graph version bump -> graph.rebuilt.
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

    pub fn rebuild_from_changes(
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

        self.durable_state
            .persist_graph_mutation(DurableGraphMutation {
                rebuilt_at: Utc::now(),
                skills: skills.clone(),
                communities: communities.clone(),
                audits,
            })?;
        self.durable_state.mark_outbox_drained()?;
        let graph_version = self.durable_state.bump_graph_version()?;

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

#[derive(Debug, Default)]
pub struct InMemoryDurableGraphState {
    pub operation_log: Vec<String>,
    pub graph_version: i64,
    pub mutations: Vec<DurableGraphMutation>,
}

impl DurableGraphState for InMemoryDurableGraphState {
    fn persist_graph_mutation(
        &mut self,
        mutation: DurableGraphMutation,
    ) -> Result<(), GraphRebuildError> {
        self.operation_log.push("persist_graph_mutation".to_owned());
        self.mutations.push(mutation);
        Ok(())
    }

    fn mark_outbox_drained(&mut self) -> Result<(), GraphRebuildError> {
        self.operation_log.push("mark_outbox_drained".to_owned());
        Ok(())
    }

    fn bump_graph_version(&mut self) -> Result<i64, GraphRebuildError> {
        self.operation_log.push("bump_graph_version".to_owned());
        self.graph_version += 1;
        Ok(self.graph_version)
    }
}
