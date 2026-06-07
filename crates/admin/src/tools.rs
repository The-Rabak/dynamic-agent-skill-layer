use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use domain::{EmbeddingService, HdbscanConfig, ScopeType};
use graph_builder::{
    ScopeRoot,
    graph::{build::build_skills_from_scope_roots, communities::assign_communities},
};
use infrastructure::{
    LiveGraphCommunityRecord, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
    LiveGraphSubunitRecord, PostgresAdapter, PostgresConfig, PostgresGraphSnapshotStore,
    PostgresRebuildCoordinator, RebuildCoordinator,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

/// Error contract for admin tool operations.
#[derive(Debug, Error)]
pub enum AdminToolError {
    #[error("operation unavailable: {0}")]
    Unavailable(String),
    #[error("operation failed: {0}")]
    Failed(String),
}

/// Rebuild result shape used by the rebuild trigger boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRebuildSnapshot {
    pub graph_version: i64,
    pub skills_count: usize,
    pub communities_count: usize,
}

/// Rebuild trigger boundary used by the admin tool.
#[async_trait]
pub trait GraphRebuildTrigger: Send + Sync {
    async fn trigger_full_rebuild(&self) -> Result<GraphRebuildSnapshot, AdminToolError>;
}

/// Skill-level read model for admin inspection tools.
///
/// `community_ids` holds all community memberships for this skill across both
/// `hdbscan` and `tag` sources (dual membership introduced in migration 006).
/// Empty when the skill has no memberships; callers must NOT treat an empty
/// list as a single-membership field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// All community IDs this skill belongs to (any source). Empty when no memberships.
    pub community_ids: Vec<String>,
    pub subunits: Vec<SubunitSnapshot>,
}

/// Subunit-level read model for inspection payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubunitSnapshot {
    pub kind: String,
    pub title: String,
    pub content: String,
}

/// Community-level read model for admin listing and inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunitySnapshot {
    pub community_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_skill_ids: Vec<String>,
}

/// Read boundary that provides graph snapshots for inspection.
#[async_trait]
pub trait GraphSnapshotReader: Send + Sync {
    async fn list_skills(&self) -> Result<Vec<SkillSnapshot>, AdminToolError>;
    async fn list_communities(&self) -> Result<Vec<CommunitySnapshot>, AdminToolError>;
}

/// Deterministic static reader for seeded tests and in-memory graphs.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Default)]
pub struct StaticGraphSnapshotReader {
    skills: Vec<SkillSnapshot>,
    communities: Vec<CommunitySnapshot>,
}

#[cfg(any(test, feature = "test-utils"))]
impl StaticGraphSnapshotReader {
    pub fn new(skills: Vec<SkillSnapshot>, communities: Vec<CommunitySnapshot>) -> Self {
        Self {
            skills,
            communities,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl GraphSnapshotReader for StaticGraphSnapshotReader {
    async fn list_skills(&self) -> Result<Vec<SkillSnapshot>, AdminToolError> {
        Ok(self.skills.clone())
    }

    async fn list_communities(&self) -> Result<Vec<CommunitySnapshot>, AdminToolError> {
        Ok(self.communities.clone())
    }
}

/// Postgres-backed reader that resolves admin inspection state from durable graph tables.
#[derive(Debug, Clone)]
pub struct PostgresGraphSnapshotReader {
    database_url_env: String,
}

impl PostgresGraphSnapshotReader {
    pub fn new(database_url_env: impl Into<String>) -> Self {
        Self {
            database_url_env: database_url_env.into(),
        }
    }

    pub fn with_default_database_env() -> Self {
        Self::new("DATABASE_URL")
    }

    async fn load_store(&self) -> Result<PostgresGraphSnapshotStore, AdminToolError> {
        let database_url = std::env::var(&self.database_url_env).map_err(|_| {
            AdminToolError::Unavailable(format!(
                "live graph reads require `{}` to point at the Postgres graph state",
                self.database_url_env
            ))
        })?;
        let postgres = PostgresAdapter::connect(&PostgresConfig {
            database_url,
            ..PostgresConfig::default()
        })
        .await
        .map_err(|error| AdminToolError::Failed(error.to_string()))?;
        postgres
            .run_migrations()
            .await
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;

        Ok(PostgresGraphSnapshotStore::new(postgres.pool().clone()))
    }
}

#[async_trait]
impl GraphSnapshotReader for PostgresGraphSnapshotReader {
    async fn list_skills(&self) -> Result<Vec<SkillSnapshot>, AdminToolError> {
        let store = self.load_store().await?;
        let records = store
            .list_skills()
            .await
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;
        Ok(records
            .into_iter()
            .map(|record| SkillSnapshot {
                skill_id: record.skill_id,
                name: record.name,
                description: record.description,
                tags: record.tags,
                community_ids: record.community_ids,
                subunits: record
                    .subunits
                    .into_iter()
                    .map(|subunit| SubunitSnapshot {
                        kind: subunit.kind,
                        title: subunit.title,
                        content: subunit.content,
                    })
                    .collect(),
            })
            .collect())
    }

    async fn list_communities(&self) -> Result<Vec<CommunitySnapshot>, AdminToolError> {
        let store = self.load_store().await?;
        let records = store
            .list_communities()
            .await
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;
        Ok(records
            .into_iter()
            .map(|record| CommunitySnapshot {
                community_id: record.community_id,
                name: record.name,
                scope: record.scope,
                member_skill_ids: record.member_skill_ids,
            })
            .collect())
    }
}

/// Fail-closed rebuild trigger for tests where rebuild wiring is not needed.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Default)]
pub struct NoopGraphRebuildTrigger;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl GraphRebuildTrigger for NoopGraphRebuildTrigger {
    async fn trigger_full_rebuild(&self) -> Result<GraphRebuildSnapshot, AdminToolError> {
        Err(AdminToolError::Unavailable(
            "rebuild trigger is not configured".to_owned(),
        ))
    }
}

/// Runtime rebuild trigger that executes the existing graph-builder orchestration workflow.
#[derive(Clone)]
pub struct FilesystemGraphRebuildTrigger {
    scope_roots: Vec<ScopeRoot>,
    database_url_env: String,
    rebuild_coordinator: Option<Arc<dyn RebuildCoordinator>>,
    embedding_service: Arc<dyn EmbeddingService>,
}

impl FilesystemGraphRebuildTrigger {
    pub fn new(scope_roots: Vec<ScopeRoot>, embedding_service: Arc<dyn EmbeddingService>) -> Self {
        Self {
            scope_roots,
            database_url_env: "DATABASE_URL".to_owned(),
            rebuild_coordinator: None,
            embedding_service,
        }
    }

    pub fn with_database_url_env(
        scope_roots: Vec<ScopeRoot>,
        database_url_env: impl Into<String>,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            scope_roots,
            database_url_env: database_url_env.into(),
            rebuild_coordinator: None,
            embedding_service,
        }
    }

    pub fn with_rebuild_coordinator(
        scope_roots: Vec<ScopeRoot>,
        rebuild_coordinator: Arc<dyn RebuildCoordinator>,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            scope_roots,
            database_url_env: "DATABASE_URL".to_owned(),
            rebuild_coordinator: Some(rebuild_coordinator),
            embedding_service,
        }
    }
}

#[async_trait]
impl GraphRebuildTrigger for FilesystemGraphRebuildTrigger {
    async fn trigger_full_rebuild(&self) -> Result<GraphRebuildSnapshot, AdminToolError> {
        let skills =
            build_skills_from_scope_roots(&self.scope_roots, self.embedding_service.as_ref())
                .await
                .map_err(|error| AdminToolError::Failed(error.to_string()))?;
        // Use default HDBSCAN config. A future enhancement can expose this via the
        // admin tool config surface — for now the defaults match the spec.
        let hdbscan_config = HdbscanConfig::default();
        let communities = assign_communities(&skills, &hdbscan_config)
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;
        let skills_count = skills.len();
        let communities_count = communities.len();

        let mutation = LiveGraphSnapshotMutation {
            rebuilt_at: chrono::Utc::now(),
            skills: skills
                .into_iter()
                .map(|skill| LiveGraphSkillRecord {
                    stable_id: skill.id,
                    name: skill.name,
                    description: skill.description,
                    scope: skill.scope_type,
                    tags: skill.tags,
                    // Persist the real SKILL.md path so the retrieval boot
                    // adapter uses true provenance, not the scope-root stand-in.
                    source_paths: vec![skill.source_path.display().to_string()],
                    subunits: skill
                        .subunits
                        .into_iter()
                        .map(|subunit| LiveGraphSubunitRecord {
                            kind: subunit.kind,
                            title: subunit.title,
                            content: subunit.content,
                        })
                        .collect(),
                })
                .collect(),
            communities: communities
                .into_iter()
                .map(|community| LiveGraphCommunityRecord {
                    stable_id: community.community_name.clone(),
                    name: community.community_name,
                    scope: community.scope,
                    member_skill_ids: community.skill_ids,
                    source: community.source.as_db_str().to_owned(),
                })
                .collect(),
        };
        let coordinator = if let Some(coordinator) = &self.rebuild_coordinator {
            coordinator.clone()
        } else {
            let database_url = std::env::var(&self.database_url_env).map_err(|_| {
                AdminToolError::Unavailable(format!(
                    "live rebuild requires `{}` to point at the Postgres graph state",
                    self.database_url_env
                ))
            })?;
            let postgres = PostgresAdapter::connect(&PostgresConfig {
                database_url,
                ..PostgresConfig::default()
            })
            .await
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;
            postgres
                .run_migrations()
                .await
                .map_err(|error| AdminToolError::Failed(error.to_string()))?;
            Arc::new(PostgresRebuildCoordinator::new(postgres.pool().clone()))
        };
        let graph_version = coordinator
            .replace_snapshot_and_bump_version(mutation)
            .await
            .map_err(|error| AdminToolError::Failed(error.to_string()))?;

        Ok(GraphRebuildSnapshot {
            graph_version,
            skills_count,
            communities_count,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RebuildGraphRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildGraphResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub job_id: Option<String>,
    pub queue_position: Option<usize>,
    pub graph_version: Option<i64>,
    pub skills_count: Option<usize>,
    pub communities_count: Option<usize>,
}

/// Status-query request for a specific rebuild job id.
#[derive(Debug, Clone, Deserialize)]
pub struct RebuildGraphStatusRequest {
    pub job_id: String,
}

/// Rebuild job status payload returned by the status-query boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildGraphStatusResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub job: Option<RebuildJobStatus>,
}

/// Current lifecycle state and result snapshot for a rebuild job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildJobStatus {
    pub job_id: String,
    pub lifecycle_status: String,
    pub reason_code: Option<String>,
    pub graph_version: Option<i64>,
    pub skills_count: Option<usize>,
    pub communities_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InspectSkillRequest {
    pub skill_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectSkillResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub skill: Option<InspectedSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectedSkill {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<SubunitSnapshot>,
    pub community: Option<CommunityContext>,
    pub neighborhood: Vec<NeighborSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityContext {
    pub community_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborSkill {
    pub skill_id: String,
    pub name: String,
    pub shared_tags: Vec<String>,
    pub same_community: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListCommunitiesRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListCommunitiesResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub communities: Vec<CommunitySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunitySummary {
    pub community_id: String,
    pub name: String,
    pub scope: ScopeType,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RebuildLifecycleStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl RebuildLifecycleStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct RebuildJobRecord {
    job_id: String,
    lifecycle_status: RebuildLifecycleStatus,
    reason_code: Option<String>,
    graph_version: Option<i64>,
    skills_count: Option<usize>,
    communities_count: Option<usize>,
}

impl RebuildJobRecord {
    fn queued(job_id: String) -> Self {
        Self {
            job_id,
            lifecycle_status: RebuildLifecycleStatus::Queued,
            reason_code: None,
            graph_version: None,
            skills_count: None,
            communities_count: None,
        }
    }

    fn to_status_payload(&self) -> RebuildJobStatus {
        RebuildJobStatus {
            job_id: self.job_id.clone(),
            lifecycle_status: self.lifecycle_status.as_str().to_owned(),
            reason_code: self.reason_code.clone(),
            graph_version: self.graph_version,
            skills_count: self.skills_count,
            communities_count: self.communities_count,
        }
    }
}

#[derive(Debug, Default)]
struct RebuildQueueState {
    jobs: HashMap<String, RebuildJobRecord>,
    queue: VecDeque<String>,
    runner_active: bool,
}

#[derive(Clone)]
struct RebuildJobQueue {
    trigger: Arc<dyn GraphRebuildTrigger>,
    next_job_sequence: Arc<AtomicU64>,
    state: Arc<Mutex<RebuildQueueState>>,
}

impl RebuildJobQueue {
    fn new(trigger: Arc<dyn GraphRebuildTrigger>) -> Self {
        Self {
            trigger,
            next_job_sequence: Arc::new(AtomicU64::new(1)),
            state: Arc::new(Mutex::new(RebuildQueueState::default())),
        }
    }

    async fn enqueue_rebuild(&self) -> (String, usize) {
        let job_id = format!(
            "rebuild-job-{}",
            self.next_job_sequence.fetch_add(1, Ordering::Relaxed)
        );
        let mut state = self.state.lock().await;
        let queue_position = state.queue.len();
        let queued_job = RebuildJobRecord::queued(job_id.clone());
        state.jobs.insert(job_id.clone(), queued_job);
        state.queue.push_back(job_id.clone());
        if !state.runner_active {
            state.runner_active = true;
            self.spawn_runner();
        }
        (job_id, queue_position)
    }

    async fn status_for(&self, job_id: &str) -> Option<RebuildJobStatus> {
        let state = self.state.lock().await;
        state
            .jobs
            .get(job_id)
            .map(RebuildJobRecord::to_status_payload)
    }

    fn spawn_runner(&self) {
        let queue = self.clone();
        tokio::spawn(async move {
            queue.run_jobs_until_empty().await;
        });
    }

    async fn run_jobs_until_empty(&self) {
        loop {
            let maybe_job_id = {
                let mut state = self.state.lock().await;
                let Some(job_id) = state.queue.pop_front() else {
                    state.runner_active = false;
                    return;
                };
                if let Some(job) = state.jobs.get_mut(&job_id) {
                    job.lifecycle_status = RebuildLifecycleStatus::Running;
                    job.reason_code = None;
                }
                Some(job_id)
            };
            let Some(job_id) = maybe_job_id else {
                continue;
            };

            let result = self.trigger.trigger_full_rebuild().await;
            let mut state = self.state.lock().await;
            if let Some(job) = state.jobs.get_mut(&job_id) {
                match result {
                    Ok(snapshot) => {
                        job.lifecycle_status = RebuildLifecycleStatus::Succeeded;
                        job.reason_code = None;
                        job.graph_version = Some(snapshot.graph_version);
                        job.skills_count = Some(snapshot.skills_count);
                        job.communities_count = Some(snapshot.communities_count);
                    }
                    Err(AdminToolError::Unavailable(_)) => {
                        job.lifecycle_status = RebuildLifecycleStatus::Failed;
                        job.reason_code = Some("rebuild_unavailable".to_owned());
                    }
                    Err(AdminToolError::Failed(_)) => {
                        job.lifecycle_status = RebuildLifecycleStatus::Failed;
                        job.reason_code = Some("rebuild_failed".to_owned());
                    }
                }
            }
        }
    }
}

/// Thin orchestration surface for admin MCP tools.
#[derive(Clone)]
pub struct AdminTools {
    rebuild_jobs: RebuildJobQueue,
    graph_reader: Arc<dyn GraphSnapshotReader>,
}

impl AdminTools {
    pub fn new(
        rebuild_trigger: Arc<dyn GraphRebuildTrigger>,
        graph_reader: Arc<dyn GraphSnapshotReader>,
    ) -> Self {
        Self {
            rebuild_jobs: RebuildJobQueue::new(rebuild_trigger),
            graph_reader,
        }
    }

    pub async fn rebuild_graph(&self, _request: RebuildGraphRequest) -> RebuildGraphResponse {
        let (job_id, queue_position) = self.rebuild_jobs.enqueue_rebuild().await;
        RebuildGraphResponse {
            status: "accepted".to_owned(),
            reason_code: None,
            job_id: Some(job_id),
            queue_position: Some(queue_position),
            graph_version: None,
            skills_count: None,
            communities_count: None,
        }
    }

    pub async fn rebuild_graph_status(
        &self,
        request: RebuildGraphStatusRequest,
    ) -> RebuildGraphStatusResponse {
        let Some(job) = self.rebuild_jobs.status_for(&request.job_id).await else {
            return RebuildGraphStatusResponse {
                status: "no_match".to_owned(),
                reason_code: Some("job_not_found".to_owned()),
                job: None,
            };
        };

        RebuildGraphStatusResponse {
            status: "ok".to_owned(),
            reason_code: None,
            job: Some(job),
        }
    }

    pub async fn inspect_skill(&self, request: InspectSkillRequest) -> InspectSkillResponse {
        let skills = match self.graph_reader.list_skills().await {
            Ok(skills) => skills,
            Err(AdminToolError::Unavailable(_)) => {
                return InspectSkillResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_unavailable".to_owned()),
                    skill: None,
                };
            }
            Err(AdminToolError::Failed(_)) => {
                return InspectSkillResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_failed".to_owned()),
                    skill: None,
                };
            }
        };
        let communities = match self.graph_reader.list_communities().await {
            Ok(communities) => communities,
            Err(AdminToolError::Unavailable(_)) => {
                return InspectSkillResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_unavailable".to_owned()),
                    skill: None,
                };
            }
            Err(AdminToolError::Failed(_)) => {
                return InspectSkillResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_failed".to_owned()),
                    skill: None,
                };
            }
        };

        let Some(target_skill) = skills
            .iter()
            .find(|skill| skill.skill_id == request.skill_id)
            .cloned()
        else {
            return InspectSkillResponse {
                status: "no_match".to_owned(),
                reason_code: Some("skill_not_found".to_owned()),
                skill: None,
            };
        };

        // For inspect, show the first community the skill belongs to (lowest ID for
        // determinism).  A future iteration can surface all memberships, but the
        // inspect response shape keeps a single optional `community` field for now.
        let community = {
            let mut matching_ids = target_skill.community_ids.clone();
            matching_ids.sort();
            matching_ids.first().and_then(|community_id| {
                communities
                    .iter()
                    .find(|candidate| candidate.community_id == *community_id)
                    .map(|candidate| CommunityContext {
                        community_id: candidate.community_id.clone(),
                        name: candidate.name.clone(),
                        scope: candidate.scope,
                        member_count: candidate.member_skill_ids.len(),
                    })
            })
        };

        let mut neighborhood = skills
            .iter()
            .filter(|candidate| candidate.skill_id != target_skill.skill_id)
            .filter_map(|candidate| {
                // Skills are in the same community if they share any community ID.
                let same_community = target_skill
                    .community_ids
                    .iter()
                    .any(|id| candidate.community_ids.contains(id));
                let shared_tags = intersect_tags(&target_skill.tags, &candidate.tags);

                if !same_community && shared_tags.is_empty() {
                    return None;
                }

                Some(NeighborSkill {
                    skill_id: candidate.skill_id.clone(),
                    name: candidate.name.clone(),
                    shared_tags,
                    same_community,
                })
            })
            .collect::<Vec<_>>();
        neighborhood.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));

        InspectSkillResponse {
            status: "ok".to_owned(),
            reason_code: None,
            skill: Some(InspectedSkill {
                skill_id: target_skill.skill_id,
                name: target_skill.name,
                description: target_skill.description,
                tags: target_skill.tags,
                subunits: target_skill.subunits,
                community,
                neighborhood,
            }),
        }
    }

    pub async fn list_communities(
        &self,
        _request: ListCommunitiesRequest,
    ) -> ListCommunitiesResponse {
        let communities = match self.graph_reader.list_communities().await {
            Ok(communities) => communities,
            Err(AdminToolError::Unavailable(_)) => {
                return ListCommunitiesResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_unavailable".to_owned()),
                    communities: Vec::new(),
                };
            }
            Err(AdminToolError::Failed(_)) => {
                return ListCommunitiesResponse {
                    status: "failed".to_owned(),
                    reason_code: Some("graph_read_failed".to_owned()),
                    communities: Vec::new(),
                };
            }
        };
        let mut communities = communities
            .into_iter()
            .map(|community| CommunitySummary {
                community_id: community.community_id,
                name: community.name,
                scope: community.scope,
                member_count: community.member_skill_ids.len(),
            })
            .collect::<Vec<_>>();
        communities.sort_by(|left, right| left.community_id.cmp(&right.community_id));

        ListCommunitiesResponse {
            status: "ok".to_owned(),
            reason_code: None,
            communities,
        }
    }
}

fn intersect_tags(left: &[String], right: &[String]) -> Vec<String> {
    let mut shared = left
        .iter()
        .filter(|tag| right.contains(tag))
        .cloned()
        .collect::<Vec<_>>();
    shared.sort();
    shared.dedup();
    shared
}
