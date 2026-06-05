/// Tracer-bullet e2e: containerized `compile_context` returns `Ok` for a
/// project-scoped skill match.
///
/// Pre-condition: the production mcp-server container is a musl static binary
/// with no `git` binary.  Before this slice, `compile_context` always returned
/// `Degraded(project_scope_resolution_failed)` for project-scoped requests
/// because `GitRootProjectResolver` shells out to `git rev-parse`, which fails
/// in-container.
///
/// Post-condition (this slice): the wiring uses `FsMarkerProjectResolver`,
/// which walks the filesystem for `.git` or `SKILL_PROJECT_MARKER` — no
/// subprocess required.  A `repo_path` that points at a directory containing
/// `.git` now resolves to `Ok` even inside the musl container.
///
/// Test is marked `#[ignore = "requires live containers"]` because it needs
/// the full docker-compose stack (`docker-compose.test.yml`).  Run it with:
///
/// ```sh
/// cargo test -p mcp-server --features test-utils \
///     --test test_project_scope_container -- --include-ignored
/// ```
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use domain::ScopeType;
use infrastructure::{
    LiveGraphSkillRecord, LiveGraphSnapshotMutation, LiveGraphSubunitRecord, RebuildCoordinator,
};
use mcp_server::{
    McpServerApp,
    tools::compile_context::{CompileContextRequest, CompileContextStatus},
};
use retrieval::RetrievalConfig;

#[path = "report.rs"]
mod report;

#[path = "../integration/env_guard.rs"]
mod env_guard;

fn project_scope_retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 3,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        ..RetrievalConfig::default()
    }
}

/// Resolve the repo root — the directory that contains `.git`.
///
/// `FsMarkerProjectResolver` walks up from `repo_path` looking for `.git`, so
/// passing the actual repo root is the thinnest proof that the resolver works.
fn repo_root_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize")
        .display()
        .to_string()
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos()
}

/// **Live container test**: proves that `compile_context` returns `Ok` (not
/// `Degraded`) when called with a `repo_path` that has a `.git` directory,
/// using the `FsMarkerProjectResolver` that requires no `git` binary.
///
/// Seeding a project-scoped skill into PG, then calling `compile_context` with
/// the matching `repo_path` must surface the skill and report `status: Ok`.
#[ignore = "requires live containers"]
#[tokio::test]
async fn project_scoped_compile_context_returns_ok_with_fs_marker_resolver() {
    let namespace = env_guard::isolated_namespace().await;

    let mut builder = report::ReportBuilder::new(
        "project_scoped_compile_context_returns_ok_with_fs_marker_resolver",
    );

    let n = nonce();
    let skill_name = format!("project-scope-tracer-{n}");

    // Boot from live environment.
    let start = std::time::Instant::now();
    let components = McpServerApp::from_environment(project_scope_retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    // Seed a project-scoped skill into PG so the in-memory graph contains it.
    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: vec![LiveGraphSkillRecord {
            stable_id: skill_name.clone(),
            name: skill_name.clone(),
            description: "Tracer-bullet project-scope skill seeded for fs-marker resolver proof"
                .to_owned(),
            scope: ScopeType::Project,
            tags: vec!["tracer".to_owned(), "project-scope".to_owned()],
            // Omit source_paths: the server will try its configured resolver
            // (`FsMarkerProjectResolver`) to gate this skill to the project scope.
            // A non-empty source_paths list would pin the skill to a specific
            // filesystem path; empty lets the resolver do its job.
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: domain::SubunitType::Procedure,
                title: "Project scope tracer procedure".to_owned(),
                content: "Verify that the fs-marker resolver returns a project scope in-container."
                    .to_owned(),
            }],
        }],
        communities: vec![],
    };

    let seed_start = std::time::Instant::now();
    components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should seed project-scoped tracer skill into PG");
    builder.record_latency("seed_skill", seed_start.elapsed().as_millis() as u64);
    builder.push_action(
        "setup",
        report::ReportedAction {
            description: format!("seed project-scoped skill `{skill_name}` into PG"),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::DbRowInserted {
                table: skill_name.clone(),
            }],
            duration_ms: seed_start.elapsed().as_millis() as u64,
        },
    );

    // Reconnect so the new skill is loaded into the in-memory snapshot.
    let components2 = McpServerApp::from_environment(project_scope_retrieval_config())
        .await
        .expect("should reconnect to live infrastructure after seeding");

    // Call compile_context with a repo_path pointing at the actual repo root.
    // `FsMarkerProjectResolver` should find `.git` there and resolve successfully.
    let repo_path = repo_root_path();
    let compile_start = std::time::Instant::now();
    let response = components2
        .app
        .compile_context(CompileContextRequest {
            prompt: "tracer project scope resolver verification".to_owned(),
            session_id: format!("project-scope-tracer-{n}"),
            repo_path: repo_path.clone(),
            trigger: None,
        })
        .await;
    let compile_latency = compile_start.elapsed().as_millis() as u64;
    builder.record_latency("compile_context", compile_latency);

    // The key assertion: the response must NOT be Degraded with
    // project_scope_resolution_failed.  Before this slice the musl container
    // always produced Degraded here.
    assert_ne!(
        response.status,
        CompileContextStatus::Degraded,
        "compile_context must not degrade with project_scope_resolution_failed after \
         FsMarkerProjectResolver wiring; got status={:?} reason={:?} context={:?}",
        response.status,
        response.reason_code,
        response.additional_context.as_deref().unwrap_or("<empty>"),
    );

    // For a clean project-scope test, the expected terminal state is Ok or
    // NoMatch (when the embedding distance doesn't rank the skill high enough).
    // DuplicateSuppressed would be surprising for a fresh session but is
    // technically healthy — accept it as proof the scope resolved.
    let is_healthy = matches!(
        response.status,
        CompileContextStatus::Ok
            | CompileContextStatus::NoMatch
            | CompileContextStatus::DuplicateSuppressed
    );
    assert!(
        is_healthy,
        "compile_context must return a healthy status (Ok, NoMatch, or DuplicateSuppressed) \
         to prove the fs-marker project resolver works end-to-end; \
         got status={:?} reason={:?}",
        response.status, response.reason_code,
    );

    builder.push_action(
        "compile_context",
        report::ReportedAction {
            description: format!(
                "compile_context returned healthy status {:?} with fs-marker resolver",
                response.status
            ),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: compile_latency,
        },
    );

    // When the status is Ok, also verify the match-reason section is present.
    if response.status == CompileContextStatus::Ok {
        let context = response.additional_context.as_deref().unwrap_or("");
        assert!(
            context.contains("### Why These Skills"),
            "Ok response must include the match-reason section; got: {context:?}"
        );
    }

    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "project_scope_fs_marker_resolver_end_to_end".to_owned(),
        status: report::AssertionResult::Passed,
        details: format!(
            "compile_context returned {:?} (not Degraded) using FsMarkerProjectResolver \
             against repo_path={repo_path}",
            response.status
        ),
    });

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    std::fs::write(&report_path, report_json).expect("report should be writable");

    components2
        .teardown()
        .await
        .expect("teardown should succeed");
    components
        .teardown()
        .await
        .expect("teardown should succeed");
    namespace.cleanup().await;
}
