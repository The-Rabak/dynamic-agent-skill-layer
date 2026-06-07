/// Golden-path E2E test — drives the REAL running app end-to-end with full stage logging.
///
/// # What this test proves
/// 1. `McpClient::compile_context` reaches the real `mcp-server` over HTTP `:3001`.
/// 2. `seed::seed_and_approve` writes + approves a `SKILL.md` in the real global-skills volume.
/// 3. `PgObserver`, `QdrantObserver`, `RedisObserver` read real infrastructure values.
/// 4. `StageLogger` writes per-stage JSON + Markdown under `tests/e2e/reports/<run_id>/`.
/// 5. `wait_for_rebuild` confirms the loop CLOSES: after seed+approve, the
///    graph-builder rebuilds and the mcp-server's served `graph_version` advances.
///
/// # Expected outcome
/// The test PASSES: `wait_for_rebuild` observes the served `graph_version` advance
/// past the baseline within the bounded wait, proving the real ingest→rebuild→
/// retrieve loop closes end-to-end on the live stack.
///
/// This is the regression guard for #163 (the mcp-server graph-refresh subscriber
/// silently wedged with `NOGROUP` after the shared stream was deleted out from
/// under it, and never reloaded). The adapter now self-heals the consumer group,
/// so a real `graph.rebuilt` reaches the subscriber and the snapshot swaps.
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
    guard::SeededSkillGuard,
    observe::{InfraSnapshot, PgObserver, QdrantObserver, RedisObserver},
    poll::wait_for_rebuild,
    seed::SkillScope,
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

    // Panic-safe guard: even if this test panics before the explicit `seed::remove`
    // at the end, the seeded skill is removed from the volume on drop.
    let mut seeded_guard = SeededSkillGuard::new();

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

    // Register the skill with the panic-safe guard AFTER a successful approve so
    // the guard removes it from the volume even if a subsequent assertion panics.
    seeded_guard.record(SkillScope::Global, &slug);

    // ── Stage: wait_for_rebuild — the loop must CLOSE within the bounded wait ──
    //
    // graph-builder rebuilds, bumps graph_state.graph_version, and publishes
    // graph.rebuilt to Redis; the mcp-server subscriber consumes it and swaps its
    // snapshot, so the served graph_version advances. We poll for 90 s with a
    // bounded timeout and record the result honestly.
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
            "regression_guard": "#163 — subscriber must self-heal its consumer group and reload on graph.rebuilt",
        }),
        json!({
            "ok": rebuild_ok,
            "detail": rebuild_detail,
            "elapsed_ms": rebuild_elapsed,
            "pg_graph_version_after": post_wait_infra.pg_graph_version,
        }),
        json!(post_wait_infra),
    );

    // Record the loop-closes assertion. This MUST pass: the served graph_version
    // advances after seed+approve once the subscriber self-heals and reloads (#163).
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
            "Regression guard for #163: graph-builder publishes graph.rebuilt, the mcp-server \
             graph-refresh subscriber self-heals its consumer group if needed, consumes the \
             event, and swaps its snapshot so the served graph_version advances.\n\
             prev_version={prev_version}, elapsed_ms={rebuild_elapsed}"
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

    // With the loop closed, status is "ok", the served graph_version has advanced,
    // and the seeded skill appears in additional_context (subject to scope match).
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

    // Assert skill presence — the seeded skill should be retrievable once the loop closes.
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
                     (prev_version={prev_version})"
                ),
            }
        },
        details: format!(
            "Once the loop closes the served snapshot advances and the seeded skill \
             becomes retrievable (subject to scope match).\n\
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

    // Hard-assert skill presence only when the loop is known to have closed (rebuild_ok).
    // This ordering is intentional: if rebuild_ok is false, the assert!(rebuild_ok, …) below
    // fires first with the "loop did not close" message; this assert fires only when the loop
    // DID close but the seeded skill is absent — a distinct regression class.
    //
    // NOTE: whether compile_context must return status="ok" (vs "degraded") is deferred to
    // #154 — the containerised server has no project scope, so "degraded" is expected for now.
    if rebuild_ok {
        assert!(
            skill_present,
            "\n\n\
            === GOLDEN-PATH SKILL NOT SERVED AFTER LOOP CLOSED ===\n\
            The loop closed (graph_version advanced past v{prev_version} to v{served_version}), \
            but the seeded skill '{slug}' is absent from additional_context.\n\
            This is NOT a loop-closure regression (#163) — the version DID advance.\n\
            Likely the embedding/retrieval pipeline returned the wrong snapshot, or the \
            scope filter excluded the seeded skill.\n\
            Inspect:\n\
              • docker logs <skill-builder> | grep -iE 'embed|build|{slug}'\n\
              • docker logs <mcp-server>    | grep -iE 'context|compile|{slug}'\n\
              • verify seed scope matches the compile_context prompt\n\
            Context captured this run:\n\
              • GET /health → {health_code}\n\
              • compile_context status={:?}, served graph_version={served_version}\n\
              • PG baseline v{prev_version}\n\
            =========================================================\n",
            retrieval_result.as_ref().map(|r| &r.status),
        );
    }

    // ── Cleanup: remove the seeded skill from the volume ─────────────────────
    // `seeded_guard.cleanup()` logs failures loudly and marks the guard done
    // so its Drop impl is a no-op. This is the happy-path removal; the guard's
    // Drop handles the panic path automatically.
    let cleanup_result = harness::seed::remove(SkillScope::Global, &slug);
    seeded_guard.cleanup(); // guard is consumed here; Drop is a no-op afterwards.
    logger.log_stage(
        "cleanup",
        json!({"scope": "global", "slug": slug}),
        json!({"ok": cleanup_result.is_ok(), "detail": format!("{:?}", cleanup_result)}),
        json!(null),
    );

    // ── Emit the E2EReport ────────────────────────────────────────────────────
    let report_path = logger.emit_report();
    println!("[golden-path] report written to: {}", report_path.display());

    // ── Final assertion: the loop CLOSES (#163 regression guard) ────────────────
    //
    // assert! here so a regression (served graph_version failing to advance after
    // a real seed+approve) makes the test binary exit non-zero. The stage logs and
    // the E2EReport capture the full evidence either way.
    assert!(
        rebuild_ok,
        "\n\n\
        === GOLDEN-PATH LOOP DID NOT CLOSE (#163 regression) ===\n\
        {rebuild_detail}\n\n\
        The served graph_version did not advance past v{prev_version} within 90s after \
        seed+approve. Likely the mcp-server graph-refresh subscriber is not consuming \
        graph.rebuilt (e.g. consumer group missing and self-heal regressed) or the swap \
        is not applying. Inspect:\n\
          • docker logs <mcp-server> | grep -iE 'reload|swap|NOGROUP|graph refresh'\n\
          • redis-cli XINFO GROUPS skill-layer-events  (group must exist)\n\
        Context captured this run:\n\
          • GET /health → {health_code}\n\
          • compile_context status={:?}, served graph_version={served_version}\n\
          • PG baseline v{prev_version}\n\
        =========================================================\n",
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
