//! Brutal honesty probes that drive the REAL containerized stack to expose
//! deployment-truth gaps the green suite otherwise tolerates.
//!
//! These are deliberately uncomfortable. They are EXPECTED to fail when the
//! corresponding gap is open, and to pass only once it is genuinely closed.
//! Do not weaken the assertions to make them green; close the gap.
//!
//! - `containerized_project_scope_over_http_returns_ok` — #154. The existing
//!   `test_project_scope_container` constructs the server IN-PROCESS, so it
//!   proves the resolver logic but NOT the deployed container, where the working
//!   dir is `/` and the project root/mount/marker wiring must hold. This probe
//!   drives the real `mcp-server` over HTTP and demands `ok` (not `degraded`).
//! - `builder_crash_does_not_permanently_freeze_the_loop` — #156. Real fault
//!   injection: `docker kill` graph-builder, prove the snapshot does NOT advance
//!   while it is down, then restart it and prove the loop recovers (the seeded
//!   skill becomes retrievable). No in-process shortcut.
//!
//! Run:
//! ```sh
//! cargo test -p mcp-server --features test-utils --test test_brutal_probes -- --ignored
//! ```

#[path = "report.rs"]
mod report;

#[path = "harness/mod.rs"]
mod harness;

use std::time::Duration;

use harness::{
    app::{CompileContextArgs, McpClient},
    guard::SeededSkillGuard,
    observe::PgObserver,
    poll::{wait_for_health, wait_for_rebuild},
    seed::{self, SkillScope},
    stack::Stack,
    stagelog::StageLogger,
};
use report::{AssertionResult, ContractAssertion};
use serde_json::json;

/// Unique slug per run to avoid cross-run dedup.
fn slug(prefix: &str) -> String {
    format!("{prefix}-{}", chrono::Utc::now().timestamp_millis())
}

/// A project-scoped SKILL.md whose H1 the compiler renders as `## Skill: <slug>`.
fn project_skill_md(name: &str) -> String {
    format!(
        "# {name}\n\
         tags: project, scope, deployment\n\
         \n\
         A project-scoped skill seeded into the container's project skills volume \
         to prove that compile_context resolves project scope to ok in the deployed \
         musl container (issue #154).\n\
         \n\
         ## Procedures\n\
         - Resolve the project root from the configured SKILL_PROJECT_ROOT, not the cwd\n\
         - Match project-scoped skills whose source paths live under that root\n\
         \n\
         ## Conventions\n\
         - The containerized server must not degrade project scope when a root is configured\n"
    )
}

/// #154 — the deployed container must resolve project scope to `ok`, over HTTP.
#[tokio::test]
#[ignore = "requires live containers"]
async fn containerized_project_scope_over_http_returns_ok() {
    Stack::up().await;
    wait_for_health(Duration::from_secs(60))
        .await
        .expect("mcp-server healthy");

    let logger = StageLogger::new("brutal-project-scope");
    let client = McpClient::new();
    let pg = PgObserver::connect().await;
    let name = slug("brutal-project");

    // Panic-safe guard: ensures the project-scoped skill is removed from the volume
    // even if an assertion below panics before the explicit `seed::remove` at the end.
    let mut seeded_guard = SeededSkillGuard::new();

    let prev_version = pg.graph_version().await.expect("baseline graph_version");
    seed::write_pending(SkillScope::Project, &name, &project_skill_md(&name))
        .expect("seed project pending");
    seed::approve(SkillScope::Project, &name).expect("approve project skill");
    seeded_guard.record(SkillScope::Project, &name);
    wait_for_rebuild(prev_version, Duration::from_secs(180))
        .await
        .expect("graph must rebuild after seeding a project skill");

    // Query with a project-scope prompt; repo_path points at the container's
    // configured project root (SKILL_PROJECT_ROOT=/skills/project in compose).
    let resp = client
        .compile_context(CompileContextArgs {
            prompt:
                "resolve project scope and match project-scoped skills under the configured root"
                    .to_owned(),
            session_id: format!("brutal-project-{}", chrono::Utc::now().timestamp_millis()),
            repo_path: "/skills/project".to_owned(),
            trigger: None,
        })
        .await
        .expect("compile_context HTTP call");

    let scope_failed = resp.reason_code.as_deref() == Some("project_scope_resolution_failed");
    let is_ok = resp.status == "ok";

    logger.log_stage(
        "project_scope_query",
        json!({"repo_path": "/skills/project", "seeded_skill": name}),
        json!({
            "status": resp.status,
            "reason_code": resp.reason_code,
            "scopes_considered": resp.scopes_considered,
            "graph_version": resp.graph_version,
            "project_scope_resolution_failed": scope_failed,
        }),
        json!(null),
    );
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "deployment::containerized_project_scope_ok".to_owned(),
        status: if is_ok {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: "status == ok".to_owned(),
                actual: format!("status={} reason_code={:?}", resp.status, resp.reason_code),
            }
        },
        details: "the deployed container must resolve project scope, not degrade (#154)".to_owned(),
    });

    seeded_guard.cleanup(); // removes the project skill; Drop is a no-op afterwards.
    let path = logger.emit_report();
    println!(
        "[brutal-project-scope] status={} reason={:?} report={}",
        resp.status,
        resp.reason_code,
        path.display()
    );

    assert!(
        is_ok,
        "\n=== CONTAINERIZED PROJECT SCOPE STILL DEGRADED (#154) ===\n\
         compile_context returned status={} reason_code={:?} for a project-scoped query\n\
         against the REAL container. project_scope_resolution_failed={scope_failed}.\n\
         The in-process test_project_scope_container passes, but the deployed musl\n\
         container (cwd=/, SKILL_PROJECT_ROOT must drive the resolver) does not reach ok.\n\
         A real `docker compose up` user gets degraded on every project query. Report: {}\n",
        resp.status,
        resp.reason_code,
        path.display(),
    );
}

/// #156 — a graph-builder crash must not permanently freeze the served snapshot.
///
/// Proves the dependency honestly (no advance while the builder is dead) and the
/// recovery honestly (advance + retrievability after restart), via real
/// `docker kill`/restart — not an in-process reconstruction.
#[tokio::test]
#[ignore = "requires live containers"]
async fn builder_crash_does_not_permanently_freeze_the_loop() {
    let stack = Stack::up().await;
    wait_for_health(Duration::from_secs(60))
        .await
        .expect("mcp-server healthy");

    let logger = StageLogger::new("brutal-builder-crash");
    let pg = PgObserver::connect().await;
    let name = slug("brutal-crash");

    // Panic-safe guard: ensures the global skill seeded during the crash simulation
    // is removed from the volume even if an assertion panics before explicit cleanup.
    let mut seeded_guard = SeededSkillGuard::new();

    let version_before_kill = pg.graph_version().await.expect("baseline graph_version");

    // ── Kill graph-builder (SIGKILL via docker compose kill) ──────────────────
    stack
        .kill("graph-builder")
        .expect("docker compose kill graph-builder");
    logger.log_stage(
        "builder_killed",
        json!({"service": "graph-builder", "version_before_kill": version_before_kill}),
        json!(null),
        json!(null),
    );

    // ── Seed + approve while the builder is DOWN ──────────────────────────────
    seed::write_pending(SkillScope::Global, &name, &project_skill_md(&name))
        .expect("seed pending while down");
    seed::approve(SkillScope::Global, &name).expect("approve while down");
    seeded_guard.record(SkillScope::Global, &name);

    // ── Prove the snapshot does NOT advance while the builder is dead ──────────
    // Poll for a bounded window; the version must stay put because nothing is
    // rebuilding. If it advances, some other actor is — that is a real surprise.
    let mut advanced_while_down = false;
    let watch_deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < watch_deadline {
        if pg.graph_version().await.unwrap_or(version_before_kill) > version_before_kill {
            advanced_while_down = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "resilience::no_phantom_rebuild_while_builder_down".to_owned(),
        status: if advanced_while_down {
            AssertionResult::Failed {
                expected: "graph_version unchanged while graph-builder is killed".to_owned(),
                actual: "graph_version advanced with no live builder".to_owned(),
            }
        } else {
            AssertionResult::Passed
        },
        details: format!("watched 20s at v{version_before_kill} with builder dead"),
    });

    // ── Restart the builder and prove the loop RECOVERS ───────────────────────
    stack.restart("graph-builder").await;
    let recovered = wait_for_rebuild(version_before_kill, Duration::from_secs(180)).await;
    let recovered_ok = recovered.is_ok();

    // The skill seeded during the outage must now be retrievable (loop caught up).
    let client = McpClient::new();
    let resp = client
        .compile_context(CompileContextArgs {
            prompt: format!("resolve project scope under the configured root {name}"),
            session_id: format!(
                "brutal-crash-retrieve-{}",
                chrono::Utc::now().timestamp_millis()
            ),
            repo_path: "/tmp".to_owned(),
            trigger: None,
        })
        .await;
    let skill_retrievable = resp
        .as_ref()
        .ok()
        .and_then(|r| r.additional_context.clone())
        .map(|ctx| ctx.contains(&name))
        .unwrap_or(false);

    logger.log_stage(
        "builder_recovered",
        json!({"service": "graph-builder"}),
        json!({
            "recovered": recovered_ok,
            "detail": recovered.as_ref().err().cloned(),
            "version_after": pg.graph_version().await.ok(),
            "seeded_skill_retrievable": skill_retrievable,
        }),
        json!(null),
    );
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "resilience::loop_recovers_after_builder_crash".to_owned(),
        status: if recovered_ok {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("snapshot advances past v{version_before_kill} after restart"),
                actual: recovered.clone().err().unwrap_or_default(),
            }
        },
        details:
            "graph-builder restart must re-drive the loop; #156 replay covers a mid-publish crash"
                .to_owned(),
    });

    seeded_guard.cleanup(); // removes the global skill; Drop is a no-op afterwards.
    let path = logger.emit_report();
    println!(
        "[brutal-builder-crash] recovered={recovered_ok} skill_retrievable={skill_retrievable} report={}",
        path.display()
    );

    assert!(
        !advanced_while_down,
        "graph_version advanced while graph-builder was killed — a phantom rebuilder exists"
    );
    assert!(
        recovered_ok,
        "\n=== LOOP DID NOT RECOVER AFTER BUILDER CRASH (#156) ===\n\
         After `docker kill graph-builder`, seeding a skill, and restarting the builder,\n\
         the served snapshot did not advance past v{version_before_kill} within 180s.\n\
         {}\n\
         A builder crash mid-rebuild must not freeze the snapshot forever — the\n\
         replay-safety path (maybe_replay_graph_rebuilt) must re-publish. Report: {}\n",
        recovered.err().unwrap_or_default(),
        path.display(),
    );
    assert!(
        skill_retrievable,
        "loop reported recovered but the skill seeded during the outage is not retrievable — \
         the snapshot advanced without catching up the backlog"
    );
}
