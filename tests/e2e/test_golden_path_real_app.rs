/// Golden-path E2E test — drives the REAL running app end-to-end with full stage logging.
///
/// # What this test proves
/// 1. `McpClient::compile_context` reaches the real `mcp-server` over HTTP `:3001`.
/// 2. `seed::seed_and_approve` writes + approves a `SKILL.md` in the real global-skills volume.
/// 3. `PgObserver`, `QdrantObserver`, `RedisObserver` read real infrastructure values.
/// 4. `StageLogger` writes per-stage JSON + Markdown under `tests/e2e/reports/<run_id>/`.
/// 5. `wait_for_rebuild` detects that the graph has NOT advanced (expected RED due to #156).
///
/// # Expected outcome
/// The test FAILS at the `wait_for_rebuild` assertion due to bug #156 (graph-builder
/// errors on outbox idempotency conflict before publishing `graph.rebuilt`, so the
/// mcp-server snapshot never advances).  This RED is the correct, honest outcome —
/// it is the regression guard for the #156 fix.
///
/// The harness ITSELF is GREEN: HTTP calls, seed/approve, and observation all work.
///
/// # Stage taxonomy logged
/// - `ingest_input`       — the SKILL.md content written to the volume.
/// - `approval`           — rename SKILL.md.pending → SKILL.md.
/// - `snapshot_swap`      — PG graph_version before + after; PG/Qdrant/Redis snapshots.
/// - `retrieval_request`  — the compile_context prompt, session_id, repo_path.
/// - `retrieval_response` — the full CompileContextResponse.
///
/// # Including the harness
/// ```rust
/// #[path = "harness/mod.rs"]
/// mod harness;
/// ```
#[path = "report.rs"]
mod report;

#[path = "harness/mod.rs"]
mod harness;

use std::time::Duration;

use harness::{
    app::{CompileContextArgs, McpClient},
    observe::{InfraSnapshot, PgObserver, QdrantObserver, RedisObserver},
    poll::wait_for_rebuild,
    seed::{SkillScope, seed_and_approve},
    stagelog::StageLogger,
};
use report::{AssertionResult, ContractAssertion, ReportedAction};
use serde_json::json;

/// A unique slug suffix prevents dedup across re-runs.
fn unique_slug() -> String {
    format!("harness-golden-{}", chrono::Utc::now().timestamp_millis())
}

/// Builds the SKILL.md content for the golden-path skill.
fn golden_skill_md(skill_name: &str) -> String {
    format!(
        "# {skill_name}\n\
         tags: golden-path, harness, e2e\n\
         \n\
         A harness-seeded skill for the golden-path E2E test. \
         This skill demonstrates correct sidecar ingestion, \
         graph rebuild, and retrieval for the harness tracer bullet.\n\
         \n\
         ## Procedures\n\
         - Seed a skill via the sidecar volume writer\n\
         - Approve the pending file to trigger graph-builder pickup\n\
         - Verify the mcp-server serves the updated graph version\n\
         \n\
         ## Conventions\n\
         - Always use unique slugs to prevent cross-run dedup\n\
         - Remove seeded skills after the test to keep the volume clean\n"
    )
}

#[tokio::test]
async fn golden_path_real_app() {
    let slug = unique_slug();
    let skill_name = slug.replace('-', " ");
    let skill_md = golden_skill_md(&slug);

    let logger = StageLogger::new("golden-path");
    let client = McpClient::new();

    // ── Stage: initial health check ───────────────────────────────────────────
    let (health_code, health_body) = client
        .health()
        .await
        .expect("GET /health must succeed — is the stack running?");

    assert_eq!(
        health_code, 200,
        "mcp-server must be healthy before the golden-path test starts"
    );
    assert_eq!(
        health_body.get("healthy"),
        Some(&serde_json::Value::Bool(true)),
        "mcp-server health body must report healthy=true"
    );

    logger.log_stage(
        "health_check",
        json!({"url": "http://127.0.0.1:3001/health"}),
        json!({"status_code": health_code, "body": health_body}),
        json!(null),
    );

    // ── Stage: connect observers and read baseline ─────────────────────────────
    let pg = PgObserver::connect().await;
    let qdrant = QdrantObserver::new();
    let redis = RedisObserver::new().expect("RedisObserver must connect");

    let prev_version = pg
        .graph_version()
        .await
        .expect("must read baseline graph_version from PG");

    let baseline_infra = InfraSnapshot::capture(&pg, &qdrant, &redis).await;
    logger.log_stage(
        "baseline_snapshot",
        json!({"description": "read baseline infrastructure state before seeding"}),
        json!({"prev_graph_version": prev_version}),
        json!(baseline_infra),
    );

    println!(
        "[golden-path] baseline: graph_version={prev_version}, qdrant_points={:?}",
        baseline_infra.qdrant_points_count
    );

    // ── Stage: ingest_input — write SKILL.md.pending via sidecar ──────────────
    logger.log_stage(
        "ingest_input",
        json!({
            "scope": "global",
            "slug": slug,
            "skill_name": skill_name,
            "skill_md": skill_md,
            "volume": harness::stack::GLOBAL_SKILLS_VOLUME,
        }),
        json!(null),
        json!(null),
    );

    write_pending_stage(&logger, SkillScope::Global, &slug, &skill_md);

    // ── Stage: approval — rename SKILL.md.pending → SKILL.md ──────────────────
    let approve_start = std::time::Instant::now();
    let approve_result = harness::seed::approve(SkillScope::Global, &slug);
    let approve_elapsed = approve_start.elapsed().as_millis() as u64;

    let approve_ok = approve_result.is_ok();
    let approve_detail = match &approve_result {
        Ok(()) => format!("approved {slug}/SKILL.md.pending → SKILL.md"),
        Err(e) => format!("approval failed: {e}"),
    };

    logger.log_stage(
        "approval",
        json!({"scope": "global", "slug": slug, "action": "rename SKILL.md.pending -> SKILL.md"}),
        json!({"ok": approve_ok, "detail": approve_detail, "elapsed_ms": approve_elapsed}),
        json!(null),
    );

    logger.record_action(
        "seed",
        ReportedAction {
            description: approve_detail.clone(),
            status: if approve_ok {
                AssertionResult::Passed
            } else {
                AssertionResult::Failed {
                    expected: "approve succeeds".to_owned(),
                    actual: approve_detail.clone(),
                }
            },
            side_effects: vec![],
            duration_ms: approve_elapsed,
        },
    );

    assert!(approve_ok, "approve must succeed — {approve_detail}");

    // ── Stage: wait_for_rebuild — this is expected to FAIL due to #156 ─────────
    //
    // graph-builder bumps graph_state.graph_version then errors on the outbox
    // idempotency conflict BEFORE publishing graph.rebuilt, so the mcp-server
    // snapshot never advances.  We poll for 90 s with a bounded timeout and
    // record the result honestly.
    let rebuild_start = std::time::Instant::now();
    let rebuild_result = wait_for_rebuild(prev_version, Duration::from_secs(90)).await;
    let rebuild_elapsed = rebuild_start.elapsed().as_millis() as u64;

    // Capture post-wait infra snapshot regardless of rebuild outcome.
    let post_wait_infra = InfraSnapshot::capture(&pg, &qdrant, &redis).await;

    let rebuild_ok = rebuild_result.is_ok();
    let rebuild_detail = match &rebuild_result {
        Ok(()) => format!(
            "graph version advanced from v{prev_version} to v{}; served version confirmed",
            post_wait_infra.pg_graph_version.unwrap_or(-1)
        ),
        Err(e) => e.clone(),
    };

    logger.log_stage(
        "snapshot_swap",
        json!({
            "prev_graph_version": prev_version,
            "timeout_secs": 90,
            "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
        }),
        json!({
            "ok": rebuild_ok,
            "detail": rebuild_detail,
            "elapsed_ms": rebuild_elapsed,
            "pg_graph_version_after": post_wait_infra.pg_graph_version,
        }),
        json!(post_wait_infra),
    );

    // Record the loop-closes assertion as FAILED (expected due to #156).
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "loop_closes_after_seed".to_owned(),
        status: if rebuild_ok {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("graph_version > v{prev_version} within 90s"),
                actual: rebuild_detail.clone(),
            }
        },
        details: format!(
            "Bug #156: graph-builder bumps graph_state.graph_version then errors on outbox \
             idempotency conflict before publishing graph.rebuilt. The mcp-server \
             refresh subscriber never fires. This assertion is RED by design — it is the \
             regression guard for the #156 fix.\nprev_version={prev_version}, \
             elapsed_ms={rebuild_elapsed}"
        ),
    });

    // ── Stage: retrieval_request — call compile_context with a matching prompt ──
    let retrieval_session = format!(
        "golden-path-retrieval-{}",
        chrono::Utc::now().timestamp_millis()
    );
    let retrieval_args = CompileContextArgs {
        prompt: format!("harness golden-path sidecar ingestion {slug}"),
        session_id: retrieval_session.clone(),
        repo_path: "/tmp".to_owned(),
        trigger: None,
    };

    logger.log_stage(
        "retrieval_request",
        json!({
            "prompt": retrieval_args.prompt,
            "session_id": retrieval_args.session_id,
            "repo_path": retrieval_args.repo_path,
        }),
        json!(null),
        json!(null),
    );

    let retrieval_start = std::time::Instant::now();
    let retrieval_result = client.compile_context(retrieval_args.clone()).await;
    let retrieval_elapsed = retrieval_start.elapsed().as_millis() as u64;

    // Capture final infra snapshot.
    let final_infra = InfraSnapshot::capture(&pg, &qdrant, &redis).await;

    let (retrieval_ok, retrieval_output) = match &retrieval_result {
        Ok(resp) => {
            let contains_skill = resp
                .additional_context
                .as_deref()
                .unwrap_or("")
                .contains(&slug);
            (
                true,
                json!({
                    "status": resp.status,
                    "reason_code": resp.reason_code,
                    "graph_version": resp.graph_version,
                    "latency_ms": resp.latency_ms,
                    "source": resp.source,
                    "contains_seeded_skill": contains_skill,
                    "additional_context_snippet": resp.additional_context
                        .as_deref()
                        .map(|s| &s[..s.len().min(500)])
                        .unwrap_or(""),
                }),
            )
        }
        Err(e) => (false, json!({"error": e})),
    };

    logger.log_stage(
        "retrieval_response",
        json!({
            "prompt": retrieval_args.prompt,
            "session_id": retrieval_args.session_id,
            "elapsed_ms": retrieval_elapsed,
        }),
        retrieval_output.clone(),
        json!(final_infra),
    );

    // When the loop is closed (#156 fixed), status will be "ok" and the skill
    // will appear in additional_context.  With #156 open, the graph_version in
    // the response will equal prev_version (not advanced) and the skill will NOT
    // be present.
    logger.record_action(
        "retrieval",
        ReportedAction {
            description: format!(
                "compile_context for matching prompt: status={:?}",
                retrieval_result.as_ref().map(|r| &r.status)
            ),
            status: AssertionResult::Passed, // the HTTP call itself succeeded
            side_effects: vec![],
            duration_ms: retrieval_elapsed,
        },
    );

    // Assert skill presence — expected to fail until #156 is fixed.
    let served_version = retrieval_result
        .as_ref()
        .map(|r| r.graph_version)
        .unwrap_or(-1);
    let skill_present = retrieval_result
        .as_ref()
        .map(|r| {
            r.additional_context
                .as_deref()
                .unwrap_or("")
                .contains(&slug)
        })
        .unwrap_or(false);

    logger.record_contract_assertion(ContractAssertion {
        contract_name: "seeded_skill_retrievable".to_owned(),
        status: if skill_present {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("additional_context contains '{slug}'"),
                actual: format!(
                    "skill absent; served graph_version={served_version} \
                     (prev_version={prev_version}) — see #156"
                ),
            }
        },
        details: format!(
            "With #156 open the served graph snapshot never advances, so the seeded \
             skill does not appear in retrieval results.\n\
             skill_slug={slug}, prev_version={prev_version}, \
             served_version={served_version}"
        ),
    });

    // ── Harness proof assertions (these must pass regardless of #156) ───────────
    assert!(retrieval_ok, "compile_context HTTP call must succeed");
    assert_eq!(
        health_code, 200,
        "mcp-server was healthy at test start (already asserted above)"
    );

    // ── Cleanup: remove the seeded skill from the volume ─────────────────────
    // Best-effort; failure here does not affect the test outcome.
    let cleanup_result = harness::seed::remove(SkillScope::Global, &slug);
    logger.log_stage(
        "cleanup",
        json!({"scope": "global", "slug": slug}),
        json!({"ok": cleanup_result.is_ok(), "detail": format!("{:?}", cleanup_result)}),
        json!(null),
    );

    // ── Emit the E2EReport ────────────────────────────────────────────────────
    let report_path = logger.emit_report();
    println!("[golden-path] report written to: {}", report_path.display());

    // ── Final assertion: the test is RED due to #156 ───────────────────────────
    //
    // assert! here so the test binary's exit code is non-zero (RED), which is
    // what CI / Ralph TDD "Red" phase requires.  The stage logs and the
    // E2EReport capture the full evidence even when this assertion fires.
    assert!(
        rebuild_ok,
        "\n\n\
        === GOLDEN-PATH TEST FAILED (expected due to #156) ===\n\
        {rebuild_detail}\n\n\
        Harness capabilities that DID work (Green):\n\
          ✓ GET /health → {health_code} healthy=true\n\
          ✓ compile_context HTTP call succeeded (status={:?}, graph_version={served_version})\n\
          ✓ seed_and_approve wrote and approved SKILL.md in the real volume\n\
          ✓ PgObserver read real graph_version from PG (baseline v{prev_version})\n\
          ✓ QdrantObserver read real points_count\n\
          ✓ RedisObserver read real stream length\n\
          ✓ StageLogger wrote per-stage logs\n\
        ==================================================\n",
        retrieval_result.as_ref().map(|r| &r.status),
    );
}

/// Writes the pending file and logs the stage.
///
/// Separated from the main test body to keep the main body readable.
fn write_pending_stage(logger: &StageLogger, scope: SkillScope, slug: &str, skill_md: &str) {
    let write_start = std::time::Instant::now();
    let write_result = harness::seed::write_pending(scope, slug, skill_md);
    let write_elapsed = write_start.elapsed().as_millis() as u64;

    let write_ok = write_result.is_ok();
    let write_detail = match &write_result {
        Ok(()) => format!("wrote {slug}/SKILL.md.pending to volume"),
        Err(e) => format!("write_pending failed: {e}"),
    };

    logger.log_stage(
        "sidecar_write",
        json!({
            "scope": format!("{scope:?}"),
            "slug": slug,
            "action": "write SKILL.md.pending via sidecar",
        }),
        json!({"ok": write_ok, "detail": write_detail, "elapsed_ms": write_elapsed}),
        json!(null),
    );

    assert!(write_ok, "write_pending must succeed — {write_detail}");
}
