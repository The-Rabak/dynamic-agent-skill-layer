use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use admin::tools::{
    CommunitySnapshot, FilesystemGraphRebuildTrigger, GraphSnapshotReader, InspectSkillRequest,
    ListCommunitiesRequest, NoopGraphRebuildTrigger, PostgresGraphSnapshotReader,
    RebuildGraphRequest, RebuildGraphStatusRequest, SkillSnapshot, StaticGraphSnapshotReader,
    SubunitSnapshot,
};
use async_trait::async_trait;
use chrono::Utc;
use domain::ScopeType;
use graph_builder::ScopeRoot;
use infrastructure::{
    LiveGraphSnapshotMutation, PostgresAdapter, PostgresConfig, RebuildCoordinator, RebuildError,
};
use mcp_server::{McpServerApp, protocol::JsonRpcRequest};
use retrieval::{RetrievalOutcome, SkillRetriever};
use serde_json::json;
use sqlx::Executor;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone, Default)]
struct EmptyRetriever;

#[async_trait]
impl SkillRetriever for EmptyRetriever {
    async fn retrieve(&self, _prompt: &str, _repo_path: Option<&str>) -> RetrievalOutcome {
        RetrievalOutcome {
            skills: Vec::new(),
            rescue_pool: Vec::new(),
            degraded_scopes: Vec::new(),
            reason_codes: Vec::new(),
            health: BTreeMap::new(),
            scopes_considered: vec!["global".to_owned()],
            graph_version: 1,
            latency_ms: 0,
        }
    }

    fn current_graph_version(&self) -> i64 {
        1
    }

    fn configured_scopes(&self) -> Vec<String> {
        vec!["global".to_owned()]
    }
}

fn build_admin_app(
    rebuild_trigger: Arc<dyn admin::tools::GraphRebuildTrigger>,
    graph_reader: Arc<dyn GraphSnapshotReader>,
) -> McpServerApp {
    McpServerApp::new_with_admin(
        Arc::new(EmptyRetriever),
        rebuild_trigger,
        graph_reader,
        None,
    )
}

fn seeded_reader() -> StaticGraphSnapshotReader {
    StaticGraphSnapshotReader::new(
        vec![
            SkillSnapshot {
                skill_id: "skill-alpha".to_owned(),
                name: "skill-alpha".to_owned(),
                description: "alpha skill".to_owned(),
                tags: vec!["rust".to_owned(), "io".to_owned()],
                community_id: Some("community-rust".to_owned()),
                subunits: vec![SubunitSnapshot {
                    kind: "procedure".to_owned(),
                    title: "Read files".to_owned(),
                    content: "Use read_to_string".to_owned(),
                }],
            },
            SkillSnapshot {
                skill_id: "skill-beta".to_owned(),
                name: "skill-beta".to_owned(),
                description: "beta skill".to_owned(),
                tags: vec!["rust".to_owned()],
                community_id: Some("community-rust".to_owned()),
                subunits: vec![SubunitSnapshot {
                    kind: "convention".to_owned(),
                    title: "Return Result".to_owned(),
                    content: "Never panic".to_owned(),
                }],
            },
        ],
        vec![CommunitySnapshot {
            community_id: "community-rust".to_owned(),
            name: "community-rust".to_owned(),
            scope: ScopeType::Global,
            member_skill_ids: vec!["skill-alpha".to_owned(), "skill-beta".to_owned()],
        }],
    )
}

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

fn write_skill_file(root: &PathBuf, slug: &str, title: &str) {
    let skill_dir = root.join(slug);
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be creatable");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            r#"# {title}

## Description
Reusable capability for {title}.

## Tags
- rust
- admin

## Procedure
1. Validate input.
2. Return deterministic output.
"#
        ),
    )
    .expect("skill file should be writable");
}

#[derive(Default)]
struct RecordingRebuildCoordinatorState {
    latest_mutation: Option<LiveGraphSnapshotMutation>,
    graph_version: i64,
}

#[derive(Default)]
struct RecordingRebuildCoordinator {
    state: Mutex<RecordingRebuildCoordinatorState>,
}

#[async_trait]
impl RebuildCoordinator for RecordingRebuildCoordinator {
    async fn try_acquire_lock(
        &self,
        _lock_name: &str,
        _owner_id: Uuid,
        _lease_duration: std::time::Duration,
    ) -> Result<bool, RebuildError> {
        Ok(true)
    }

    async fn renew_lock(
        &self,
        _lock_name: &str,
        _owner_id: Uuid,
        _lease_duration: std::time::Duration,
    ) -> Result<bool, RebuildError> {
        Ok(true)
    }

    async fn release_lock(&self, _lock_name: &str, _owner_id: Uuid) -> Result<(), RebuildError> {
        Ok(())
    }

    async fn current_graph_version(&self) -> Result<i64, RebuildError> {
        Ok(self
            .state
            .lock()
            .expect("recording coordinator lock should not be poisoned")
            .graph_version)
    }

    async fn bump_graph_version(&self) -> Result<i64, RebuildError> {
        let mut state = self
            .state
            .lock()
            .expect("recording coordinator lock should not be poisoned");
        state.graph_version += 1;
        Ok(state.graph_version)
    }

    async fn replace_snapshot_and_bump_version(
        &self,
        mutation: LiveGraphSnapshotMutation,
    ) -> Result<i64, RebuildError> {
        let mut state = self
            .state
            .lock()
            .expect("recording coordinator lock should not be poisoned");
        state.latest_mutation = Some(mutation);
        state.graph_version += 1;
        Ok(state.graph_version)
    }
}

#[derive(Clone)]
struct GatedRebuildTrigger {
    permits: Arc<Semaphore>,
    running: Arc<AtomicUsize>,
    max_running: Arc<AtomicUsize>,
}

impl GatedRebuildTrigger {
    fn new() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(0)),
            running: Arc::new(AtomicUsize::new(0)),
            max_running: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn release_one(&self) {
        self.permits.add_permits(1);
    }

    fn running_count(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    fn max_running_count(&self) -> usize {
        self.max_running.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl admin::tools::GraphRebuildTrigger for GatedRebuildTrigger {
    async fn trigger_full_rebuild(
        &self,
    ) -> Result<admin::tools::GraphRebuildSnapshot, admin::tools::AdminToolError> {
        let running_now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_running.fetch_max(running_now, Ordering::SeqCst);
        self.permits
            .acquire()
            .await
            .expect("test gate should stay open")
            .forget();
        self.running.fetch_sub(1, Ordering::SeqCst);
        Ok(admin::tools::GraphRebuildSnapshot {
            graph_version: 11,
            skills_count: 3,
            communities_count: 1,
        })
    }
}

#[tokio::test]
async fn json_rpc_exposes_inspect_skill_and_list_communities_payloads() {
    let app = build_admin_app(
        Arc::new(NoopGraphRebuildTrigger),
        Arc::new(seeded_reader()) as Arc<dyn GraphSnapshotReader>,
    );

    let inspect = app
        .handle_json_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "inspect_skill",
                "arguments": { "skill_id": "skill-alpha" }
            }),
        })
        .await;
    assert_eq!(
        inspect
            .result
            .as_ref()
            .and_then(|result| result.get("status"))
            .and_then(|status| status.as_str()),
        Some("ok")
    );
    assert_eq!(
        inspect
            .result
            .as_ref()
            .and_then(|result| result.pointer("/skill/neighborhood/0/skill_id"))
            .and_then(|id| id.as_str()),
        Some("skill-beta")
    );

    let communities = app
        .handle_json_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "list_communities",
                "arguments": {}
            }),
        })
        .await;
    assert_eq!(
        communities
            .result
            .as_ref()
            .and_then(|result| result.get("status"))
            .and_then(|status| status.as_str()),
        Some("ok")
    );
    assert_eq!(
        communities
            .result
            .as_ref()
            .and_then(|result| result.pointer("/communities/0/member_count"))
            .and_then(|count| count.as_u64()),
        Some(2)
    );

    let status = app
        .handle_json_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "rebuild_graph_status",
                "arguments": { "job_id": "missing-job" }
            }),
        })
        .await;
    assert_eq!(
        status
            .result
            .as_ref()
            .and_then(|result| result.get("status"))
            .and_then(|value| value.as_str()),
        Some("no_match")
    );
}

#[tokio::test]
async fn rebuild_graph_uses_graph_builder_full_rebuild_workflow() {
    let sandbox = fresh_sandbox("admin-rebuild");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    write_skill_file(&project_root, "project-skill", "Project Skill");
    write_skill_file(&global_root, "global-skill", "Global Skill");

    let coordinator = Arc::new(RecordingRebuildCoordinator::default());
    let rebuild_trigger = FilesystemGraphRebuildTrigger::with_rebuild_coordinator(
        vec![
            ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
            ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
        ],
        coordinator.clone(),
    );
    let app = build_admin_app(
        Arc::new(rebuild_trigger),
        Arc::new(StaticGraphSnapshotReader::default()) as Arc<dyn GraphSnapshotReader>,
    );

    let response = app.rebuild_graph(RebuildGraphRequest::default()).await;
    assert_eq!(response.status, "accepted");
    let job_id = response
        .job_id
        .clone()
        .expect("accepted rebuild should include job id");
    let status = wait_for_rebuild_terminal_status(&app, job_id).await;
    let job = status
        .job
        .as_ref()
        .expect("terminal status should include job payload");
    assert_eq!(job.lifecycle_status, "succeeded");
    assert_eq!(job.graph_version, Some(1));
    assert!(job.skills_count.unwrap_or_default() >= 2);
    assert!(job.communities_count.unwrap_or_default() >= 1);
    let persisted = coordinator
        .state
        .lock()
        .expect("recording coordinator lock should not be poisoned")
        .latest_mutation
        .clone()
        .expect("rebuild should persist a live graph mutation");
    assert!(persisted.rebuilt_at <= Utc::now());
    assert_eq!(persisted.skills.len(), job.skills_count.unwrap_or_default());
    assert_eq!(
        persisted.communities.len(),
        job.communities_count.unwrap_or_default()
    );
    assert!(
        persisted
            .skills
            .iter()
            .any(|skill| skill.name == "Project Skill"),
        "persisted live mutation should include project skill"
    );

    let communities = app
        .list_communities(ListCommunitiesRequest::default())
        .await;
    assert_eq!(communities.status, "ok");
}

#[tokio::test]
async fn inspect_skill_and_list_communities_read_live_postgres_state_after_rebuild() {
    let Some(database_url) = std::env::var("DATABASE_URL").ok() else {
        return;
    };
    let postgres = PostgresAdapter::connect(&PostgresConfig {
        database_url,
        ..PostgresConfig::default()
    })
    .await
    .expect("postgres should connect for live admin read test");
    postgres
        .run_migrations()
        .await
        .expect("migrations should run for live admin read test");
    postgres
        .pool()
        .execute("DELETE FROM community_skills")
        .await
        .expect("community_skills should clear");
    postgres
        .pool()
        .execute("DELETE FROM skill_subunits")
        .await
        .expect("skill_subunits should clear");
    postgres
        .pool()
        .execute("DELETE FROM communities")
        .await
        .expect("communities should clear");
    postgres
        .pool()
        .execute("DELETE FROM subunits")
        .await
        .expect("subunits should clear");
    postgres
        .pool()
        .execute("DELETE FROM skills")
        .await
        .expect("skills should clear");

    let sandbox = fresh_sandbox("admin-live-reader");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    write_skill_file(&project_root, "project-skill", "Project Skill");
    write_skill_file(&global_root, "global-skill", "Global Skill");
    let project_skill_id = persisted_skill_id(project_root.join("project-skill/SKILL.md"));

    let app = build_admin_app(
        Arc::new(FilesystemGraphRebuildTrigger::new(vec![
            ScopeRoot::new("project", ScopeType::Project, project_root),
            ScopeRoot::new("global", ScopeType::Global, global_root),
        ])),
        Arc::new(PostgresGraphSnapshotReader::with_default_database_env()),
    );

    let rebuild = app.rebuild_graph(RebuildGraphRequest::default()).await;
    assert_eq!(rebuild.status, "accepted");
    let job_id = rebuild
        .job_id
        .clone()
        .expect("accepted rebuild should include job id");
    let status = wait_for_rebuild_terminal_status(&app, job_id).await;
    assert_eq!(
        status.job.as_ref().map(|job| job.lifecycle_status.as_str()),
        Some("succeeded")
    );
    assert_eq!(
        status.job.as_ref().and_then(|job| job.skills_count),
        Some(2)
    );
    assert_eq!(
        status.job.as_ref().and_then(|job| job.communities_count),
        Some(2)
    );

    let communities = app
        .list_communities(ListCommunitiesRequest::default())
        .await;
    assert_eq!(communities.status, "ok");
    assert_eq!(communities.reason_code, None);
    assert_eq!(communities.communities.len(), 2);
    assert!(
        communities
            .communities
            .iter()
            .any(|community| community.member_count == 1),
        "each rebuilt community should report live member counts"
    );

    let inspect = app
        .inspect_skill(InspectSkillRequest {
            skill_id: project_skill_id.clone(),
        })
        .await;
    assert_eq!(inspect.status, "ok");
    let inspected = inspect
        .skill
        .expect("skill should be present from live state");
    assert_eq!(inspected.skill_id, project_skill_id);
    assert_eq!(inspected.name, "Project Skill");
    assert!(inspected.subunits.len() >= 1);
    assert!(inspected.community.is_some());
}

#[tokio::test]
async fn rebuild_graph_enqueues_job_and_reports_status_transitions() {
    let gated_trigger = Arc::new(GatedRebuildTrigger::new());
    let app = build_admin_app(
        gated_trigger.clone(),
        Arc::new(StaticGraphSnapshotReader::default()) as Arc<dyn GraphSnapshotReader>,
    );

    let accepted = app.rebuild_graph(RebuildGraphRequest::default()).await;
    assert_eq!(accepted.status, "accepted");
    let job_id = accepted
        .job_id
        .clone()
        .expect("accepted rebuild should include job id");

    let queued_or_running = app
        .rebuild_graph_status(RebuildGraphStatusRequest {
            job_id: job_id.clone(),
        })
        .await;
    let lifecycle = queued_or_running
        .job
        .as_ref()
        .map(|job| job.lifecycle_status.as_str());
    assert!(
        lifecycle == Some("queued") || lifecycle == Some("running"),
        "job should be queued or running immediately after enqueue"
    );
    assert_eq!(
        queued_or_running
            .job
            .as_ref()
            .and_then(|job| job.graph_version),
        None
    );

    gated_trigger.release_one();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let finished = app
        .rebuild_graph_status(RebuildGraphStatusRequest { job_id })
        .await;
    assert_eq!(finished.status, "ok");
    assert_eq!(
        finished
            .job
            .as_ref()
            .map(|job| job.lifecycle_status.as_str()),
        Some("succeeded")
    );
    assert_eq!(
        finished.job.as_ref().and_then(|job| job.graph_version),
        Some(11)
    );
}

#[tokio::test]
async fn rebuild_graph_processes_jobs_singleflight_with_queueing() {
    let gated_trigger = Arc::new(GatedRebuildTrigger::new());
    let app = build_admin_app(
        gated_trigger.clone(),
        Arc::new(StaticGraphSnapshotReader::default()) as Arc<dyn GraphSnapshotReader>,
    );

    let first = app.rebuild_graph(RebuildGraphRequest::default()).await;
    let second = app.rebuild_graph(RebuildGraphRequest::default()).await;
    assert_eq!(first.status, "accepted");
    assert_eq!(second.status, "accepted");
    assert_eq!(first.queue_position, Some(0));
    assert_eq!(second.queue_position, Some(1));

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(gated_trigger.running_count(), 1);

    let second_job_id = second
        .job_id
        .clone()
        .expect("second request should return job id");
    let second_status_before = app
        .rebuild_graph_status(RebuildGraphStatusRequest {
            job_id: second_job_id.clone(),
        })
        .await;
    assert_eq!(
        second_status_before
            .job
            .as_ref()
            .map(|job| job.lifecycle_status.as_str()),
        Some("queued")
    );

    gated_trigger.release_one();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(gated_trigger.max_running_count(), 1);
    assert_eq!(gated_trigger.running_count(), 1);

    gated_trigger.release_one();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(gated_trigger.running_count(), 0);
    assert_eq!(gated_trigger.max_running_count(), 1);

    let second_status_after = app
        .rebuild_graph_status(RebuildGraphStatusRequest {
            job_id: second_job_id,
        })
        .await;
    assert_eq!(
        second_status_after
            .job
            .as_ref()
            .map(|job| job.lifecycle_status.as_str()),
        Some("succeeded")
    );
}

#[test]
fn mcp_server_transport_keeps_admin_wiring_in_internal_module() {
    let transport_source = include_str!("../../crates/mcp-server/src/lib.rs");
    assert!(
        transport_source.contains("admin_wiring::live_admin_runtime_dependencies()"),
        "transport entrypoints should delegate admin assembly to internal wiring module"
    );
    assert!(
        !transport_source.contains("fn default_scope_roots()"),
        "scope root assembly should not live in mcp-server transport path"
    );

    let wiring_source = include_str!("../../crates/mcp-server/src/admin_wiring.rs");
    assert!(
        wiring_source.contains("fn default_scope_roots()"),
        "admin wiring module should own graph scope root defaults"
    );
}

async fn wait_for_rebuild_terminal_status(
    app: &McpServerApp,
    job_id: String,
) -> admin::tools::RebuildGraphStatusResponse {
    for _attempt in 0..50 {
        let status = app
            .rebuild_graph_status(RebuildGraphStatusRequest {
                job_id: job_id.clone(),
            })
            .await;
        let lifecycle = status
            .job
            .as_ref()
            .map(|job| job.lifecycle_status.as_str())
            .unwrap_or_default();
        if lifecycle == "succeeded" || lifecycle == "failed" {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("rebuild job should reach a terminal status");
}

fn persisted_skill_id(skill_file_path: PathBuf) -> String {
    let stable_id = blake3::hash(skill_file_path.display().to_string().as_bytes())
        .to_hex()
        .to_string();
    deterministic_entity_uuid("skill", &stable_id).to_string()
}

fn deterministic_entity_uuid(entity_kind: &str, stable_id: &str) -> Uuid {
    let digest = blake3::hash(format!("{entity_kind}:{stable_id}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
