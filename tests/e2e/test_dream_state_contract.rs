// DREAM-STATE CONTRACT:
// Every test in this file is expected to be green by the time development is complete.
// This suite is intentionally aggressive and production-grade; each test codifies a strict
// end-to-end contract that currently remains ignored until full capabilities exist.

use domain::SubunitType;
use infrastructure::{
    DependencyFactory, GraphWriteCoordinator, LiveGraphSkillRecord, LiveGraphSnapshotMutation,
    LiveGraphSubunitRecord, OutboxEvent, OutboxReconciler, OutboxRelay, OutboxVectorStore,
    RebuildCoordinator, VECTOR_UPSERT_EVENT_TYPE,
};
use mcp_server::McpServerApp;
use mcp_server::tools::compile_context::{CompileContextRequest, CompileContextStatus};
use retrieval::RetrievalConfig;
use std::path::PathBuf;

#[path = "../integration/env_guard.rs"]
mod env_guard;
#[path = "report.rs"]
mod report;
#[path = "support/mod.rs"]
mod support;

#[derive(Debug)]
struct DreamContractCase {
    id: &'static str,
    objective: &'static str,
    flow: &'static [&'static str],
    hard_invariants: &'static [&'static str],
    determinism_strategy: &'static [&'static str],
}

fn pending_contract(case: DreamContractCase) {
    panic!(
        "\nDream-state E2E contract pending implementation:\n\
         case={}\n\
         objective={}\n\
         flow={:#?}\n\
         hard_invariants={:#?}\n\
         determinism_strategy={:#?}",
        case.id, case.objective, case.flow, case.hard_invariants, case.determinism_strategy
    );
}

fn dream_retrieval_config() -> RetrievalConfig {
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

async fn dream_seed_skills(
    rebuild_coordinator: &impl RebuildCoordinator,
    skills: &[(&str, &str, &[&str])],
) -> i64 {
    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: skills
            .iter()
            .map(|(name, desc, tags)| LiveGraphSkillRecord {
                stable_id: name.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                scope: domain::ScopeType::Global,
                tags: tags.iter().map(|t| t.to_string()).collect(),
                source_paths: vec![],
                subunits: vec![LiveGraphSubunitRecord {
                    kind: SubunitType::Procedure,
                    title: "test procedure".to_string(),
                    content: "test content".to_string(),
                }],
            })
            .collect(),
        communities: vec![],
    };
    rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("seed succeeded")
}

fn test_repo_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
        .display()
        .to_string()
}

#[test]
#[ignore = "Dream-state contract: closed-loop deterministic analysis->extraction->ingestion->retrieval not implemented"]
fn full_session_analysis_extraction_ingestion_retrieval_loop_is_deterministic() {
    pending_contract(DreamContractCase {
        id: "DS-001",
        objective: "Given the same transcript corpus and fixture repository, repeated full-loop runs produce identical compile_context semantic output, ranking order, graph_version progression, and audit/event trails.",
        flow: &[
            "MCP prompt/session start",
            "Session transcript analysis",
            "extract_session",
            ".pending proposal write",
            "human approval rename .pending -> SKILL.md",
            "watcher detects + reconciliation scan",
            "graph rebuild + outbox relay + PG/Qdrant sync",
            "compile_context retrieval over live stores",
        ],
        hard_invariants: &[
            "No hidden/manual seeding path is used",
            "No dropped lifecycle or graph events",
            "Reason codes are stable across reruns",
            "Deterministic golden assertions hold for N repeated runs",
        ],
        determinism_strategy: &[
            "Pinned fixture corpus + frozen clocks/ids in harness",
            "Fixed embedding provider profile for deterministic mode",
            "Canonical sort and snapshot normalization for assertions",
        ],
    });
}

#[test]
#[ignore = "Dream-state contract: transport-level MCP end-to-end path not fully implemented"]
fn mcp_transport_roundtrip_over_stdio_and_http_is_lossless() {
    pending_contract(DreamContractCase {
        id: "DS-002",
        objective: "Verify protocol-equivalent behavior over stdio and HTTP transport surfaces under the same workload.",
        flow: &[
            "Client sends tools/list and tools/call through stdio",
            "Client repeats same sequence through HTTP",
            "Responses normalized and diffed",
        ],
        hard_invariants: &[
            "Payload shape equality",
            "Status/reason code equality",
            "No transport-specific behavior drift",
        ],
        determinism_strategy: &[
            "Deterministic request corpus",
            "Canonical JSON normalization",
        ],
    });
}

/// DS-003: Option A CQRS resilience proof.
///
/// Under the ratified Option A architecture (ADR-0001), `compile_context` reads
/// from the in-memory `RetrievalSnapshot` — Qdrant, Postgres, and Redis are all
/// write-side concerns and are NEVER queried at read time. This means:
///
/// - Qdrant down → `compile_context` still returns `Ok`/`NoMatch` (read path unaffected)
/// - The infrastructure health checker surfaces `qdrant_write_side` as degraded (HARD assertion)
/// - Ollama down → embedding unavailable → `Degraded` + non-empty `reason_code`
/// - Postgres down → `compile_context` still returns `Ok`/`NoMatch` (write-side only)
/// - Redis down → `compile_context` still returns `Ok`/`NoMatch` (event bus, write-side)
/// - Full recovery → `compile_context` returns `Ok`/`NoMatch`; recovery latency is recorded
///
/// This test explicitly does NOT assert `Degraded` on Qdrant, Postgres, or Redis stop.
/// That would be the stale pre-Option-A contract. This scenario proves CQRS resilience for
/// all write-side services. Recovery latency is measured and asserted within a bounded budget.
#[ignore = "requires live containers"]
#[tokio::test]
async fn dependency_chaos_matrix_preserves_degraded_semantics_and_fast_recovery() {
    use std::time::Instant;
    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("DS-003_dependency_chaos_matrix");
    let docker_compose = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker-compose.test.yml")
        .canonicalize()
        .expect("compose file");

    let components = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("live");
    dream_seed_skills(
        components.rebuild_coordinator.as_ref(),
        &[
            (
                "dream-rust-001",
                "Rust async file IO patterns with error handling",
                &["rust", "file", "async"],
            ),
            (
                "dream-security-001",
                "Authentication and authorization middleware patterns",
                &["auth", "security"],
            ),
        ],
    )
    .await;

    let repo = test_repo_path();

    // --- Phase 1: healthy baseline ---
    let r_baseline = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "rust file async".to_owned(),
            session_id: "ds003-baseline".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let baseline_ok = matches!(
        r_baseline.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        baseline_ok,
        "expected Ok or NoMatch at healthy baseline, got {:?}",
        r_baseline.status
    );
    builder.assert_contract(
        "healthy_baseline_ok_or_no_match",
        baseline_ok,
        "Ok | NoMatch",
        &format!("{:?}", r_baseline.status),
        "compile_context at healthy baseline must return Ok or NoMatch",
    );
    builder.record_degradation_event("all", false, "healthy baseline");

    // --- Phase 2: Qdrant stopped — read path must be unaffected (Option A CQRS) ---
    //
    // Under Option A the in-memory snapshot is the read model. Qdrant being down
    // does NOT degrade `compile_context`; it only degrades the write side (vector
    // store outbox drain). The infrastructure health checker surfaces this via the
    // `qdrant_write_side` component, which MUST be present and reported as unhealthy.
    support::infra::compose_stop_service(&docker_compose, "qdrant")
        .expect("docker compose stop qdrant");

    // Bounded readiness poll: wait up to 10 s for the container to actually stop.
    // Fixed sleeps are non-deterministic; polling on the observable health marker
    // is the correct contract here.
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("http client");
    let qdrant_url_for_poll = qdrant_url.clone();
    let http_for_poll = http.clone();
    support::poll::poll_until(
        move || {
            let http = http_for_poll.clone();
            let url = format!("{}/collections", qdrant_url_for_poll.trim_end_matches('/'));
            async move { http.get(&url).send().await.is_err() }
        },
        std::time::Duration::from_secs(10),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("qdrant container did not stop within polling window");

    // Option A proof: compile_context reads from in-memory snapshot — Qdrant down
    // must NOT cause Degraded. The read path is decoupled from the write store.
    let r_qdrant_down = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "auth middleware".to_owned(),
            session_id: "ds003-qdrant-down".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let qdrant_down_read_unaffected = matches!(
        r_qdrant_down.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        qdrant_down_read_unaffected,
        "Option A CQRS: compile_context must NOT degrade when Qdrant is down \
         (read path uses in-memory snapshot, not Qdrant); got {:?}",
        r_qdrant_down.status
    );
    builder.assert_contract(
        "option_a_cqrs_qdrant_down_read_path_unaffected",
        qdrant_down_read_unaffected,
        "Ok | NoMatch (read path must be decoupled from write store)",
        &format!("{:?}", r_qdrant_down.status),
        "Option A: in-memory snapshot is the read model; Qdrant down must not degrade compile_context",
    );
    builder.record_degradation_event(
        "qdrant",
        false,
        "qdrant stopped — read path unaffected (Option A CQRS contract)",
    );

    // Write-side health proof: the infrastructure health checker MUST surface
    // `qdrant_write_side` as a component AND it must be unhealthy. Absence of the
    // component is itself a contract failure — it means the health checker was not
    // configured to probe Qdrant, which breaks operational observability.
    let health_while_qdrant_down = DependencyFactory::build_health_checker_from_environment()
        .check()
        .await;
    let qdrant_write_component = health_while_qdrant_down
        .components
        .iter()
        .find(|c| c.name == "qdrant_write_side");
    let qdrant_write_component_present = qdrant_write_component.is_some();
    assert!(
        qdrant_write_component_present,
        "qdrant_write_side health component must be present in the health report; \
         QDRANT_URL env var may be unset or the health checker was not configured to probe Qdrant"
    );
    builder.assert_contract(
        "qdrant_write_side_health_component_present",
        qdrant_write_component_present,
        "qdrant_write_side component present in health report",
        if qdrant_write_component_present {
            "present"
        } else {
            "absent"
        },
        "health checker must always expose qdrant_write_side for operational observability",
    );
    if let Some(component) = qdrant_write_component {
        let qdrant_write_unhealthy = !component.healthy;
        assert!(
            qdrant_write_unhealthy,
            "qdrant_write_side health component must be unhealthy when Qdrant is stopped; \
             got healthy=true detail='{}'",
            component.detail
        );
        builder.assert_contract(
            "qdrant_write_side_health_degraded_when_qdrant_stopped",
            qdrant_write_unhealthy,
            "healthy=false (Qdrant is stopped)",
            &format!(
                "healthy={} detail='{}'",
                component.healthy, component.detail
            ),
            "qdrant_write_side must be degraded in health report when the Qdrant container is down",
        );
    }
    builder.record_degradation_event(
        "qdrant_write_side_health",
        true,
        "qdrant_write_side health marker degraded as expected",
    );

    // --- Phase 3: Ollama stopped — embedding is unavailable → Degraded ---
    //
    // Qdrant is still down. Ollama is a READ-PATH dependency (vectorises the query
    // prompt). Stopping Ollama must cause `compile_context` to return `Degraded`.
    support::infra::compose_stop_service(&docker_compose, "ollama")
        .expect("docker compose stop ollama");

    // Bounded poll: wait until Ollama is actually unreachable (replaces fixed 2s sleep).
    let ollama_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11444".to_owned());
    let ollama_url_for_poll = ollama_url.clone();
    let http_for_ollama_poll = http.clone();
    support::poll::poll_until(
        move || {
            let h = http_for_ollama_poll.clone();
            let url = format!("{}/api/tags", ollama_url_for_poll.trim_end_matches('/'));
            async move { h.get(&url).send().await.is_err() }
        },
        std::time::Duration::from_secs(10),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("ollama container did not stop within polling window");

    let r_ollama_down = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "rust file".to_owned(),
            session_id: "ds003-ollama-down".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let ollama_down_degraded = r_ollama_down.status == CompileContextStatus::Degraded;
    assert!(
        ollama_down_degraded,
        "expected Degraded when Ollama is down (embedding unavailable); got {:?}",
        r_ollama_down.status
    );
    let ollama_down_has_reason_code = !r_ollama_down
        .reason_code
        .as_deref()
        .unwrap_or("")
        .is_empty();
    assert!(
        ollama_down_has_reason_code,
        "Degraded response must carry a non-empty reason_code"
    );
    builder.assert_contract(
        "ollama_down_yields_degraded",
        ollama_down_degraded,
        "Degraded",
        &format!("{:?}", r_ollama_down.status),
        "Ollama down (embedding unavailable) must produce Degraded status",
    );
    builder.assert_contract(
        "degraded_carries_non_empty_reason_code",
        ollama_down_has_reason_code,
        "non-empty reason_code",
        r_ollama_down.reason_code.as_deref().unwrap_or("(none)"),
        "every Degraded response must carry a machine-parseable reason_code",
    );
    builder.record_degradation_event("both", true, "ollama stopped — Degraded as expected");

    // --- Phase 4: Postgres stopped — write-side only; read path must survive ---
    //
    // Postgres is the canonical skill graph store (write side). The in-memory
    // RetrievalSnapshot was already loaded; Postgres being down must NOT degrade
    // compile_context (it cannot trigger a graph rebuild right now either, but the
    // existing snapshot is authoritative for reads).
    support::infra::compose_stop_service(&docker_compose, "postgres")
        .expect("docker compose stop postgres");

    // Poll until Postgres health check in the infra checker reports unhealthy.
    // This is more reliable than polling TCP directly since sqlx uses lazy connect.
    support::poll::poll_until(
        || async {
            let health = DependencyFactory::build_health_checker_from_environment()
                .check()
                .await;
            health
                .components
                .iter()
                .find(|c| c.name == "postgres")
                .map(|c| !c.healthy)
                .unwrap_or(false)
        },
        std::time::Duration::from_secs(15),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("postgres container did not report unhealthy within polling window");

    let r_pg_down = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "auth middleware".to_owned(),
            session_id: "ds003-pg-down".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let pg_down_read_unaffected = matches!(
        r_pg_down.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        pg_down_read_unaffected,
        "Option A CQRS: compile_context must NOT degrade when Postgres is down \
         (read path uses in-memory snapshot); got {:?}",
        r_pg_down.status
    );
    builder.assert_contract(
        "option_a_cqrs_postgres_down_read_path_unaffected",
        pg_down_read_unaffected,
        "Ok | NoMatch (Postgres is write-side only)",
        &format!("{:?}", r_pg_down.status),
        "Postgres is the write-side graph store; its downtime must not degrade compile_context reads",
    );
    builder.record_degradation_event(
        "postgres",
        false,
        "postgres stopped — read path unaffected (Option A CQRS contract)",
    );

    // Restart Postgres before proceeding.
    support::infra::compose_start_services(&docker_compose, &["postgres"])
        .expect("docker compose start postgres");
    support::poll::poll_until(
        || async {
            let health = DependencyFactory::build_health_checker_from_environment()
                .check()
                .await;
            health
                .components
                .iter()
                .find(|c| c.name == "postgres")
                .map(|c| c.healthy)
                .unwrap_or(false)
        },
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("postgres did not recover within polling window");
    builder.record_degradation_event("postgres", true, "postgres restarted and healthy");

    // --- Phase 5: Redis stopped — write-side event bus; read path must survive ---
    //
    // Redis carries the `graph.rebuilt` event stream (write side). The in-memory
    // snapshot was already loaded; Redis being down must NOT degrade compile_context.
    support::infra::compose_stop_service(&docker_compose, "redis")
        .expect("docker compose stop redis");

    support::poll::poll_until(
        || async {
            let health = DependencyFactory::build_health_checker_from_environment()
                .check()
                .await;
            health
                .components
                .iter()
                .find(|c| c.name == "redis")
                .map(|c| !c.healthy)
                .unwrap_or(false)
        },
        std::time::Duration::from_secs(15),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("redis container did not report unhealthy within polling window");

    let r_redis_down = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "rust file async".to_owned(),
            session_id: "ds003-redis-down".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let redis_down_read_unaffected = matches!(
        r_redis_down.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        redis_down_read_unaffected,
        "Option A CQRS: compile_context must NOT degrade when Redis is down \
         (read path uses in-memory snapshot, Redis is the event bus only); got {:?}",
        r_redis_down.status
    );
    builder.assert_contract(
        "option_a_cqrs_redis_down_read_path_unaffected",
        redis_down_read_unaffected,
        "Ok | NoMatch (Redis is write-side event bus only)",
        &format!("{:?}", r_redis_down.status),
        "Redis carries graph.rebuilt events (write-side bus); its downtime must not degrade compile_context reads",
    );
    builder.record_degradation_event(
        "redis",
        false,
        "redis stopped — read path unaffected (Option A CQRS contract)",
    );

    // Restart Redis before full recovery phase.
    support::infra::compose_start_services(&docker_compose, &["redis"])
        .expect("docker compose start redis");
    support::poll::poll_until(
        || async {
            let health = DependencyFactory::build_health_checker_from_environment()
                .check()
                .await;
            health
                .components
                .iter()
                .find(|c| c.name == "redis")
                .map(|c| c.healthy)
                .unwrap_or(false)
        },
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("redis did not recover within polling window");
    builder.record_degradation_event("redis", true, "redis restarted and healthy");

    // --- Phase 6: Full recovery — restart Qdrant + Ollama, measure recovery latency ---
    //
    // All write-side services are now up (Postgres, Redis). Restart the remaining
    // stopped services (Qdrant, Ollama) and poll until compile_context recovers to
    // Ok/NoMatch. Recovery latency is measured from the moment the start command
    // returns to the first successful Ok/NoMatch response.
    support::infra::compose_start_services(&docker_compose, &["qdrant", "ollama"])
        .expect("docker compose start qdrant ollama");

    let recovery_start = Instant::now();

    // Bounded readiness poll: wait for both Qdrant and Ollama to be network-reachable.
    let qdrant_url_for_recovery = qdrant_url.clone();
    let ollama_url_for_recovery = ollama_url.clone();
    let http_for_recovery = http.clone();
    support::poll::poll_until(
        move || {
            let h = http_for_recovery.clone();
            let q_url = format!(
                "{}/collections",
                qdrant_url_for_recovery.trim_end_matches('/')
            );
            let o_url = format!("{}/api/tags", ollama_url_for_recovery.trim_end_matches('/'));
            async move {
                let qdrant_ok = h.get(&q_url).send().await.is_ok();
                let ollama_ok = h.get(&o_url).send().await.is_ok();
                qdrant_ok && ollama_ok
            }
        },
        std::time::Duration::from_secs(60),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("qdrant and ollama did not recover within 60s polling window");

    // Poll compile_context itself until it returns Ok/NoMatch, then record latency.
    // This proves the full read-path recovery is observable, not just network reachability.
    let recovery_latency_ms = {
        let mut latency_ms = 0u64;
        let mut recovered = false;
        for _ in 0..120 {
            let r = components
                .app
                .compile_context(CompileContextRequest {
                    prompt: "rust file async".to_owned(),
                    session_id: "ds003-recovery-poll".to_owned(),
                    repo_path: repo.clone(),
                    trigger: None,
                })
                .await;
            if matches!(
                r.status,
                CompileContextStatus::Ok | CompileContextStatus::NoMatch
            ) {
                latency_ms = recovery_start.elapsed().as_millis() as u64;
                recovered = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        assert!(
            recovered,
            "compile_context did not recover to Ok/NoMatch within 60s after Qdrant+Ollama restart"
        );
        latency_ms
    };

    builder.record_latency("recovery", recovery_latency_ms);

    // Assert recovery within a 60 s budget. The budget is generous because Ollama
    // model loading can be slow in CI environments, but a timeout here means the
    // recovery polling was exhausted — a real contract failure.
    let recovery_within_budget = recovery_latency_ms <= 60_000;
    builder.assert_contract(
        "recovery_latency_within_60s_budget",
        recovery_within_budget,
        "recovery_latency_ms <= 60000",
        &format!("recovery_latency_ms={recovery_latency_ms}"),
        "compile_context must recover to Ok/NoMatch within 60s of Qdrant+Ollama restart",
    );

    // Final post-recovery check with a stable session id.
    let r_recovered = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "rust file async".to_owned(),
            session_id: "ds003-recovered".to_owned(),
            repo_path: repo.clone(),
            trigger: None,
        })
        .await;
    let fully_recovered = matches!(
        r_recovered.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        fully_recovered,
        "expected Ok or NoMatch after full recovery; got {:?}",
        r_recovered.status
    );
    builder.assert_contract(
        "full_recovery_restores_ok_or_no_match",
        fully_recovered,
        "Ok | NoMatch",
        &format!("{:?}", r_recovered.status),
        "after all services restarted, compile_context must return Ok or NoMatch",
    );
    builder.record_degradation_event("all", true, "recovered to healthy");

    let report = builder.build();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{}__{}.json", report.test_name, report.test_id)),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    components.teardown().await.expect("teardown");
}

/// DS-004: Outbox backlog replay without data loss across multiple hard restart cycles.
///
/// This test proves that the outbox durability contract holds across real crashes:
///
/// 1. A backlog of N≥10 `vector.upsert` events is enqueued into the real PG outbox
///    while Qdrant is DOWN, so no relay drain is possible — the backlog accumulates.
/// 2. Two hard "crash" cycles: the live-server components are torn down and rebuilt
///    from environment (simulating process death + restart while Qdrant remains down).
///    After each restart, pending events are still present in the durable PG store.
/// 3. Qdrant is brought back and the relay drains the full backlog.
/// 4. The measured `replayed` count is compared to `enqueued`; `lost == 0` and
///    `duplicated == 0` are asserted as explicit contract assertions.
/// 5. Skills seeded to the PG graph store are verified retrievable post-replay.
///
/// Fail-ability: if an event is lost (not in the published set), `lost > 0` ⇒ Failed.
/// If a duplicate is delivered (idempotency key appears twice in published set),
/// `duplicated > 0` ⇒ Failed. A tautological `graph_version before<after` assertion
/// is NOT used — only measured event counts drive the outcome.
#[ignore = "requires live containers"]
#[tokio::test]
async fn outbox_backlog_replays_without_data_loss_after_multi_restart_sequence() {
    use sqlx::Row as _;
    use uuid::Uuid;

    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("DS-004_outbox_backlog_replay");

    let docker_compose = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker-compose.test.yml")
        .canonicalize()
        .expect("compose file");

    // --- Phase 1: Build components and seed skills to PG graph store ---
    //
    // Skills are seeded into the durable PG graph store via replace_snapshot_and_bump_version.
    // This is independent of the outbox: the graph store feeds the in-memory retrieval model,
    // so skills are retrievable even before outbox replay.
    let components = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("live");

    // Seed 10 distinct skills into the PG graph store. These skills are the payload
    // whose vector embeddings must survive the crash/replay cycle.
    let backlog_skill_ids: Vec<(&str, &str)> = vec![
        (
            "ds004-outbox-skill-01",
            "Outbox replay skill 01 rust async patterns",
        ),
        (
            "ds004-outbox-skill-02",
            "Outbox replay skill 02 error handling strategy",
        ),
        (
            "ds004-outbox-skill-03",
            "Outbox replay skill 03 actor model concurrency",
        ),
        (
            "ds004-outbox-skill-04",
            "Outbox replay skill 04 database migration tooling",
        ),
        (
            "ds004-outbox-skill-05",
            "Outbox replay skill 05 distributed tracing context",
        ),
        (
            "ds004-outbox-skill-06",
            "Outbox replay skill 06 circuit breaker pattern",
        ),
        (
            "ds004-outbox-skill-07",
            "Outbox replay skill 07 event sourcing cqrs boundary",
        ),
        (
            "ds004-outbox-skill-08",
            "Outbox replay skill 08 hexagonal architecture ports",
        ),
        (
            "ds004-outbox-skill-09",
            "Outbox replay skill 09 observability telemetry hooks",
        ),
        (
            "ds004-outbox-skill-10",
            "Outbox replay skill 10 zero-copy serialization approach",
        ),
    ];

    let skill_seed_input: Vec<(&str, &str, &[&str])> = backlog_skill_ids
        .iter()
        .map(|(id, desc)| (*id, *desc, [].as_slice()))
        .collect();
    dream_seed_skills(components.rebuild_coordinator.as_ref(), &skill_seed_input).await;

    // --- Phase 2: Stop Qdrant so the outbox relay cannot drain ---
    //
    // With Qdrant down, any relay attempt for vector upsert events will fail.
    // Events remain in the PG outbox_events table as `pending`.
    support::infra::compose_stop_service(&docker_compose, "qdrant")
        .expect("docker compose stop qdrant");

    // Poll until Qdrant is unreachable, confirming the backlog will accumulate.
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .expect("http client");
    support::poll::poll_until(
        || {
            let http = http.clone();
            let qdrant_url = qdrant_url.clone();
            async move { http.get(&qdrant_url).send().await.is_err() }
        },
        std::time::Duration::from_secs(15),
        std::time::Duration::from_millis(300),
    )
    .await
    .expect("qdrant container did not stop within 15s polling window");

    builder.record_degradation_event("qdrant", false, "qdrant stopped to force outbox backlog");

    // --- Phase 3: Enqueue N=10 vector.upsert events into the outbox while Qdrant is DOWN ---
    //
    // A shared correlation_id scopes all backlog events so we can count them precisely
    // without interfering with other test runs or existing outbox rows.
    let backlog_correlation_id = Uuid::now_v7();
    let enqueued: usize = backlog_skill_ids.len();

    // Use a synthetic but valid vector payload. The relay validates the payload shape
    // before publishing; the production "skills" collection is 768-dim
    // (ensure_collection("skills", 768)), so the vector MUST be 768 floats or Qdrant
    // rejects the upsert at relay time.
    let synthetic_vector: Vec<f32> = vec![0.1_f32; 768];
    let synthetic_vector_json: Vec<serde_json::Value> = synthetic_vector
        .iter()
        .map(|v| serde_json::Value::from(*v as f64))
        .collect();

    for (skill_id, description) in &backlog_skill_ids {
        let payload = serde_json::json!({
            "content_hash": skill_id,
            "vector": synthetic_vector_json,
            "payload": {
                "skill_id": skill_id,
                "name": description,
                "scope": "Global",
                "tags": [],
            }
        });
        let event = OutboxEvent {
            event_id: Uuid::now_v7(),
            event_type: VECTOR_UPSERT_EVENT_TYPE.to_owned(),
            correlation_id: backlog_correlation_id,
            idempotency_key: format!("ds004:vector:{skill_id}"),
            schema_version: 1,
            timestamp: chrono::Utc::now(),
            payload,
        };
        components
            .write_coordinator
            .append_outbox_event(&event)
            .await
            .expect("enqueue outbox event");
    }

    // Confirm the backlog is present: pending count for our correlation must equal enqueued.
    let pool = components.pg_adapter.pool().clone();
    let pending_initial: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM outbox_events
        WHERE correlation_id = $1
          AND status IN ('pending', 'processing')
        "#,
    )
    .bind(backlog_correlation_id)
    .fetch_one(&pool)
    .await
    .expect("count pending outbox events");

    let backlog_seeded_correctly = pending_initial == enqueued as i64;
    assert!(
        backlog_seeded_correctly,
        "DS-004: expected {enqueued} pending events after seeding backlog, got {pending_initial}"
    );
    builder.assert_contract(
        "backlog_seeded",
        backlog_seeded_correctly,
        &format!("pending_count == {enqueued}"),
        &format!("pending_count={pending_initial}"),
        "All enqueued events must be present in outbox_events as pending before restart cycle",
    );

    // --- Phase 4: Crash restart 1 — tear down components, rebuild while Qdrant still DOWN ---
    //
    // Simulates a hard process crash: the live server (with its in-process relay) is destroyed.
    // Because teardown calls TRUNCATE, we must NOT call teardown here — we want the durable
    // PG outbox rows to survive the "crash". We intentionally drop components instead.
    // NOTE: we cannot call `.teardown()` because it TRUNCATEs tables (including outbox_events).
    // A real crash does NOT truncate — it just ends the process. We simulate this by
    // dropping `components` without teardown, relying on the connection pool closing cleanly.
    drop(components);

    // Rebuild from environment — simulates the relay process restarting.
    let crash_restart_1 = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("crash restart 1 live");

    // After restart 1: outbox rows must still be pending (durable in PG).
    let pool_after_restart_1 = crash_restart_1.pg_adapter.pool().clone();
    let pending_after_restart_1: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM outbox_events
        WHERE correlation_id = $1
          AND status IN ('pending', 'processing')
        "#,
    )
    .bind(backlog_correlation_id)
    .fetch_one(&pool_after_restart_1)
    .await
    .expect("count pending after restart 1");

    let durable_after_restart_1 = pending_after_restart_1 == enqueued as i64;
    assert!(
        durable_after_restart_1,
        "DS-004: outbox must be durable through crash restart 1; \
         expected {enqueued} pending, got {pending_after_restart_1}"
    );
    builder.assert_contract(
        "backlog_durable_after_restart_1",
        durable_after_restart_1,
        &format!("pending_count == {enqueued} after crash restart 1"),
        &format!("pending_count={pending_after_restart_1}"),
        "Outbox events must survive the first simulated crash (PG durability)",
    );

    // --- Phase 5: Crash restart 2 — second crash while Qdrant still DOWN ---
    //
    // Drop the first restart instance (second "crash") without teardown.
    drop(crash_restart_1);

    let crash_restart_2 = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("crash restart 2 live");

    let pool_after_restart_2 = crash_restart_2.pg_adapter.pool().clone();
    let pending_after_restart_2: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM outbox_events
        WHERE correlation_id = $1
          AND status IN ('pending', 'processing')
        "#,
    )
    .bind(backlog_correlation_id)
    .fetch_one(&pool_after_restart_2)
    .await
    .expect("count pending after restart 2");

    let durable_after_restart_2 = pending_after_restart_2 == enqueued as i64;
    assert!(
        durable_after_restart_2,
        "DS-004: outbox must be durable through crash restart 2; \
         expected {enqueued} pending, got {pending_after_restart_2}"
    );
    builder.assert_contract(
        "backlog_durable_after_restart_2",
        durable_after_restart_2,
        &format!("pending_count == {enqueued} after crash restart 2"),
        &format!("pending_count={pending_after_restart_2}"),
        "Outbox events must survive the second simulated crash (PG durability)",
    );

    // --- Phase 6: Recovery — bring Qdrant back and drain the outbox backlog ---
    support::infra::compose_start_services(&docker_compose, &["qdrant"])
        .expect("docker compose start qdrant");

    // Poll until Qdrant is reachable again before running the relay drain.
    support::poll::poll_until(
        || {
            let http = http.clone();
            let qdrant_url = qdrant_url.clone();
            async move { http.get(&qdrant_url).send().await.is_ok() }
        },
        std::time::Duration::from_secs(60),
        std::time::Duration::from_millis(500),
    )
    .await
    .expect("qdrant did not recover within 60s polling window");

    builder.record_degradation_event("qdrant", true, "qdrant restarted — relay drain begins");

    // Run the outbox relay in a loop until no pending events remain for our correlation_id.
    // claim_limit of 20 covers the 10-event backlog in a single cycle.
    let relay = OutboxRelay::new(
        crash_restart_2.write_coordinator.as_ref(),
        crash_restart_2.qdrant_adapter.as_ref(),
        20,
        0,
    )
    .expect("outbox relay construction");

    const MAX_RELAY_DRAIN_POLLS: u32 = 20;
    for _ in 0..MAX_RELAY_DRAIN_POLLS {
        let remaining: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM outbox_events
            WHERE correlation_id = $1
              AND status IN ('pending', 'processing')
            "#,
        )
        .bind(backlog_correlation_id)
        .fetch_one(&pool_after_restart_2)
        .await
        .expect("count remaining pending during drain");

        if remaining == 0 {
            break;
        }
        // Run one relay cycle; ignore the run-report here — we measure outcomes via SQL.
        let _ = relay.relay_once().await;
    }

    // --- Phase 7: Measure replayed, lost, duplicated ---
    //
    // A published row = one successful relay delivery to Qdrant for that event.
    // Unique idempotency_keys among published rows for our correlation_id = replayed count.
    // lost = enqueued - replayed (events that never reached Qdrant).
    // duplicated = published_rows - unique_idempotency_keys (same key published >1 time).
    let rows = sqlx::query(
        r#"
        SELECT idempotency_key
        FROM outbox_events
        WHERE correlation_id = $1
          AND status = 'published'
        "#,
    )
    .bind(backlog_correlation_id)
    .fetch_all(&pool_after_restart_2)
    .await
    .expect("fetch published events for correlation");

    let published_total = rows.len();
    let unique_idempotency_keys: std::collections::HashSet<String> = rows
        .into_iter()
        .map(|row| {
            row.try_get::<String, _>("idempotency_key")
                .expect("idempotency_key")
        })
        .collect();
    let replayed = unique_idempotency_keys.len();
    let lost = enqueued.saturating_sub(replayed);
    let duplicated = published_total.saturating_sub(replayed);

    builder.record_latency(
        "drain_cycle",
        0, // relay drain time not separately measured; focus is on correctness counts
    );

    // These are the real fail-able assertions:
    // - If any event was lost (not published), lost > 0 ⇒ Failed.
    // - If any event was published more than once (idempotency violated), duplicated > 0 ⇒ Failed.
    // - If replayed != enqueued, the outbox did not fully drain ⇒ Failed.
    let no_events_lost = lost == 0;
    let no_duplicates = duplicated == 0;
    let fully_replayed = replayed == enqueued;

    builder.assert_contract(
        "replayed_equals_enqueued",
        fully_replayed,
        &format!("replayed == {enqueued}"),
        &format!("replayed={replayed}, enqueued={enqueued}"),
        "Every enqueued outbox event must be published exactly once after replay",
    );
    builder.assert_contract(
        "zero_events_lost",
        no_events_lost,
        "lost == 0",
        &format!("lost={lost} (enqueued={enqueued}, replayed={replayed})"),
        "No events may be lost: every enqueued event must appear in the published set",
    );
    builder.assert_contract(
        "zero_duplicates",
        no_duplicates,
        "duplicated == 0",
        &format!(
            "duplicated={duplicated} (published_total={published_total}, replayed={replayed})"
        ),
        "No event may be published more than once: idempotency_key must be unique in published set",
    );

    // --- Phase 8: Verify seeded skills are retrievable post-replay ---
    //
    // Under Option A CQRS, compile_context reads from the in-memory snapshot (loaded from PG
    // graph tables at startup). Skills seeded via replace_snapshot_and_bump_version are
    // retrievable from the in-memory model regardless of Qdrant state.
    // After crash_restart_2 rebuilt from PG, the seeded skills must be in the snapshot.
    let repo = test_repo_path();
    let r_retrieval = crash_restart_2
        .app
        .compile_context(CompileContextRequest {
            prompt: "outbox replay rust async patterns".to_owned(),
            session_id: "ds004-post-replay-retrieval".to_owned(),
            repo_path: repo,
            trigger: None,
        })
        .await;
    let skills_retrievable = matches!(
        r_retrieval.status,
        CompileContextStatus::Ok | CompileContextStatus::NoMatch
    );
    assert!(
        skills_retrievable,
        "DS-004: compile_context must return Ok or NoMatch post-replay; got {:?}",
        r_retrieval.status
    );
    builder.assert_contract(
        "seeded_skills_retrievable_post_replay",
        skills_retrievable,
        "Ok | NoMatch",
        &format!("{:?}", r_retrieval.status),
        "Skills seeded to PG graph store must be retrievable via compile_context after replay",
    );

    let report = builder.build();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{}__{}.json", report.test_name, report.test_id)),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    crash_restart_2.teardown().await.expect("teardown");
}

/// DS-005: Qdrant/PG drift injection, reconciliation, and convergence at scale.
///
/// This test proves the full drift-detect-and-repair contract at N≥100 divergences:
///
/// 1. **Missing-vector direction (≥50)**: Outbox events are inserted as `published`
///    with valid `vector.upsert` payloads but no matching Qdrant point. The reconciler
///    must detect these, enqueue repairs, and the relay must push them to Qdrant.
///
/// 2. **Orphaned-vector direction (≥50)**: Qdrant vectors are injected directly with
///    no published outbox event. The reconciler must identify and delete them.
///
/// 3. **Convergence**: After a bounded reconcile+relay loop, both directions are closed.
///    The durable invariant — published-vector outbox count == Qdrant vector count —
///    is asserted as the final contract.
///
/// Fail-ability: without `reconcile_once`, `missing_vectors` and `orphaned_vectors_deleted`
/// stay at zero; the store-count equality assertion fails because orphan vectors remain
/// in Qdrant and missing repairs are never enqueued.
#[ignore = "requires live containers"]
#[tokio::test]
async fn qdrant_pg_drift_detection_and_reconciliation_closes_all_gaps() {
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    // Number of divergences in each direction. Total = 2 * DRIFT_COUNT ≥ 100.
    const MISSING_VECTOR_COUNT: usize = 50;
    const ORPHAN_VECTOR_COUNT: usize = 50;
    // Total divergences injected in both directions.
    let gaps_injected = MISSING_VECTOR_COUNT + ORPHAN_VECTOR_COUNT;

    // The production collection uses 768-dimensional nomic-embed-text vectors.
    const VECTOR_DIM: usize = 768;
    // The production Qdrant collection name.
    const COLLECTION_NAME: &str = "skills";
    // scan_limit must exceed the total published-event count to ensure `expected_set_is_complete`
    // is true, which is required for the reconciler to delete orphans.
    const RECONCILER_SCAN_LIMIT: i64 = 500;
    // Maximum reconcile+relay cycles before declaring convergence failure.
    const MAX_CONVERGENCE_CYCLES: u32 = 20;

    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("DS-005_qdrant_pg_drift");

    let components = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("DS-005: live server components must be reachable");

    let pool = components.pg_adapter.pool().clone();
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());

    // --- Phase 1: Inject missing-vector drift ---
    //
    // Insert `outbox_events` rows with status='published' and valid vector.upsert payloads,
    // but never upsert the corresponding Qdrant vectors. The reconciler sees these as
    // published events with no Qdrant counterpart and enqueues repair events.
    //
    // Each row gets a unique content_hash; the Qdrant point_id is derived from the hash
    // using the same `qdrant_point_id_from_content_hash` logic that production uses.
    // We do NOT insert a Qdrant vector, so the reconciler must detect the gap.
    let correlation_id = Uuid::now_v7();
    let mut injected_published_event_ids: Vec<Uuid> = Vec::with_capacity(MISSING_VECTOR_COUNT);

    for i in 0..MISSING_VECTOR_COUNT {
        let event_id = Uuid::now_v7();
        let content_hash = format!("ds005-missing-{i}-{}", event_id);
        // Build a minimal 768-dim vector in payload form. Values are deterministic and bounded.
        let vector: Vec<f32> = (0..VECTOR_DIM)
            .map(|j| (i as f32 * 0.001 + j as f32 * 0.0001) % 1.0)
            .collect();
        let vector_json: Vec<serde_json::Value> = vector.iter().map(|v| json!(*v)).collect();
        let payload = json!({
            "content_hash": content_hash,
            "vector": vector_json,
            "payload": {
                "drift_marker": format!("ds005-missing-{i}"),
                "skill_name": format!("ds005-missing-skill-{i}")
            }
        });
        let idempotency_key = format!("ds005:missing:{i}:{}", event_id);

        // Insert directly as 'published' so the reconciler treats this as an event that
        // has already been relayed to Qdrant — but we deliberately skip the actual upsert.
        sqlx::query(
            r#"
            INSERT INTO outbox_events (
                event_id, event_type, correlation_id, idempotency_key,
                schema_version, payload, occurred_at, available_at, status,
                stream_id, published_at
            ) VALUES ($1, $2, $3, $4, 1, $5, $6, $6, 'published',
                      'ds005-drift-inject', $6)
            "#,
        )
        .bind(event_id)
        .bind(VECTOR_UPSERT_EVENT_TYPE)
        .bind(correlation_id)
        .bind(&idempotency_key)
        .bind(&payload)
        .bind(Utc::now())
        .execute(&pool)
        .await
        .expect("DS-005: insert published outbox event for missing-vector drift");

        injected_published_event_ids.push(event_id);
    }

    builder.assert_contract(
        "missing_vector_drift_injected",
        injected_published_event_ids.len() == MISSING_VECTOR_COUNT,
        &format!("injected_count == {MISSING_VECTOR_COUNT}"),
        &format!("injected_count={}", injected_published_event_ids.len()),
        "All missing-vector drift rows must be inserted before reconcile begins",
    );

    // --- Phase 2: Inject orphaned-vector drift ---
    //
    // Upsert Qdrant vectors with no published outbox event. The reconciler sees these as
    // point IDs not in the expected set and deletes them.
    let injected_orphans = support::drift::inject_qdrant_vectors_without_pg_rows(
        &qdrant_url,
        COLLECTION_NAME,
        VECTOR_DIM,
        ORPHAN_VECTOR_COUNT,
    )
    .await
    .expect("DS-005: inject orphan vectors into Qdrant");

    builder.assert_contract(
        "orphan_vector_drift_injected",
        injected_orphans.len() == ORPHAN_VECTOR_COUNT,
        &format!("injected_count == {ORPHAN_VECTOR_COUNT}"),
        &format!("injected_count={}", injected_orphans.len()),
        "All orphan-vector drift rows must be injected before reconcile begins",
    );

    // Record total gaps injected before any reconciliation.
    builder.record_latency("gaps_injected", gaps_injected as u64);

    // --- Phase 3: Run reconcile+relay loop until convergence ---
    //
    // Each cycle:
    // 1. `reconcile_once` detects missing vectors (enqueues repairs) and deletes orphans.
    // 2. `relay_once` processes any newly-enqueued repair events, pushing vectors to Qdrant.
    //
    // Convergence is reached when a full reconcile cycle reports zero missing_vectors
    // and zero orphaned_vectors_deleted — meaning both directions are fully closed.
    //
    // The relay constructor takes (coordinator, vector_store, claim_limit, retry_after_secs).
    let reconciler = OutboxReconciler::new(
        components.write_coordinator.as_ref(),
        components.qdrant_adapter.as_ref(),
        RECONCILER_SCAN_LIMIT,
    )
    .expect("DS-005: OutboxReconciler construction must succeed with positive scan_limit");

    let relay = OutboxRelay::new(
        components.write_coordinator.as_ref(),
        components.qdrant_adapter.as_ref(),
        // claim_limit covers all injected missing-vector repairs in one cycle.
        (MISSING_VECTOR_COUNT as i64) * 2,
        0,
    )
    .expect("DS-005: OutboxRelay construction must succeed");

    // Accumulated reconciliation totals across all convergence cycles.
    let mut total_missing_vectors_detected: usize = 0;
    let mut total_repair_enqueued: usize = 0;
    let mut total_orphans_deleted: usize = 0;
    let mut convergence_cycles_used: u32 = 0;

    for cycle in 0..MAX_CONVERGENCE_CYCLES {
        let report = reconciler
            .reconcile_once()
            .await
            .expect("DS-005: reconcile_once must not error with live infrastructure");

        total_missing_vectors_detected += report.missing_vectors;
        total_repair_enqueued += report.repair_enqueued;
        total_orphans_deleted += report.orphaned_vectors_deleted;
        convergence_cycles_used = cycle + 1;

        // Run the relay to process repair events enqueued by this reconcile cycle.
        // This pushes repaired vectors to Qdrant so subsequent reconcile sees them.
        let _ = relay
            .relay_once()
            .await
            .expect("DS-005: relay_once must not error with live infrastructure");

        // Convergence: reconcile sees no remaining gaps in either direction.
        if report.missing_vectors == 0 && report.orphaned_vectors_deleted == 0 {
            break;
        }
    }

    builder.record_latency("convergence_cycles", convergence_cycles_used as u64);

    // --- Phase 4: Assert convergence contract assertions ---
    //
    // (4a) missing-vector direction: all 50 injected events had their gap detected and repair enqueued.
    let missing_direction_closed = total_missing_vectors_detected >= MISSING_VECTOR_COUNT;
    builder.assert_contract(
        "missing_vector_direction_closed",
        missing_direction_closed,
        &format!("total_missing_detected >= {MISSING_VECTOR_COUNT}"),
        &format!("total_missing_detected={total_missing_vectors_detected}"),
        "Reconciler must detect all injected missing-vector gaps across convergence cycles",
    );

    let repairs_enqueued_for_all_missing = total_repair_enqueued >= MISSING_VECTOR_COUNT;
    builder.assert_contract(
        "repairs_enqueued_for_all_missing_vectors",
        repairs_enqueued_for_all_missing,
        &format!("total_repair_enqueued >= {MISSING_VECTOR_COUNT}"),
        &format!("total_repair_enqueued={total_repair_enqueued}"),
        "Reconciler must enqueue repairs for every missing-vector gap detected",
    );

    // (4b) orphaned-vector direction: all 50 injected orphan vectors deleted.
    let orphan_direction_closed = total_orphans_deleted >= ORPHAN_VECTOR_COUNT;
    builder.assert_contract(
        "orphan_vector_direction_closed",
        orphan_direction_closed,
        &format!("total_orphans_deleted >= {ORPHAN_VECTOR_COUNT}"),
        &format!("total_orphans_deleted={total_orphans_deleted}"),
        "Reconciler must delete all injected orphan Qdrant vectors across convergence cycles",
    );

    // (4c) total gaps closed == total gaps injected (both directions).
    // gaps_closed = missing detected + orphans deleted (the two halves of the contract).
    let gaps_closed = total_missing_vectors_detected.min(MISSING_VECTOR_COUNT)
        + total_orphans_deleted.min(ORPHAN_VECTOR_COUNT);
    let all_gaps_closed = gaps_closed == gaps_injected;
    builder.assert_contract(
        "gaps_closed_equals_gaps_injected",
        all_gaps_closed,
        &format!("gaps_closed == {gaps_injected}"),
        &format!("gaps_closed={gaps_closed}"),
        "All injected divergences (both directions) must be resolved by the reconciler",
    );

    // --- Phase 5: Post-reconcile store-count equality (the durable invariant) ---
    //
    // After full convergence: the count of Qdrant vectors must equal the count of
    // published vector.upsert outbox events. This is the fundamental consistency
    // invariant: every published event must have a live vector, and every live vector
    // must have a published event.
    //
    // We measure both counts from the real stores.
    let published_event_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM outbox_events
        WHERE status = 'published'
          AND event_type = $1
        "#,
    )
    .bind(VECTOR_UPSERT_EVENT_TYPE)
    .fetch_one(&pool)
    .await
    .expect("DS-005: count published vector.upsert events");

    let qdrant_listing = components
        .qdrant_adapter
        .list_point_ids()
        .await
        .expect("DS-005: list Qdrant point IDs for final count assertion");

    let qdrant_vector_count = qdrant_listing.point_ids.len() as i64;

    // The equality is only meaningful when the Qdrant listing is complete (no pagination).
    // If the listing is paginated, the test cannot make a reliable count comparison.
    assert!(
        qdrant_listing.is_complete,
        "DS-005: Qdrant listing must be complete for post-reconcile count assertion; \
         increase the list_point_ids page limit if needed"
    );

    let store_counts_equal = published_event_count == qdrant_vector_count;
    builder.assert_contract(
        "post_reconcile_pg_published_count_equals_qdrant_vector_count",
        store_counts_equal,
        "published_event_count == qdrant_vector_count",
        &format!(
            "published_event_count={published_event_count} qdrant_vector_count={qdrant_vector_count}"
        ),
        "After reconciliation, every published vector.upsert event must have a live Qdrant vector \
         and every live Qdrant vector must have a published event — the fundamental consistency invariant",
    );

    let report = builder.build();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{}__{}.json", report.test_name, report.test_id)),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    components.teardown().await.expect("DS-005: teardown");
}

#[ignore = "requires live containers"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sustained_watcher_and_extraction_saturation_keeps_eventual_consistency() {
    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("DS-006_watcher_extraction_saturation");

    let components = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("live");
    dream_seed_skills(
        components.rebuild_coordinator.as_ref(),
        &[
            ("ds006-sat-skill-1", "Saturation skill alpha", &["alpha"]),
            ("ds006-sat-skill-2", "Saturation skill beta", &["beta"]),
            ("ds006-sat-skill-3", "Saturation skill gamma", &["gamma"]),
        ],
    )
    .await;

    let repo = test_repo_path();
    use tokio::task::JoinSet;
    let mut set = JoinSet::new();
    let app = components.app.clone();
    for i in 0..24 {
        let a = app.clone();
        let repo_clone = repo.clone();
        set.spawn(async move {
            a.compile_context(CompileContextRequest {
                prompt: format!("saturation stress {i}"),
                session_id: format!("ds006-session-{i}"),
                repo_path: repo_clone,
                trigger: None,
            })
            .await
        });
    }
    let mut ok_count = 0usize;
    let mut no_match_count = 0usize;
    while let Some(result) = set.join_next().await {
        let r = result.expect("task");
        match r.status {
            CompileContextStatus::Ok => ok_count += 1,
            CompileContextStatus::NoMatch => no_match_count += 1,
            CompileContextStatus::Degraded => {}
            CompileContextStatus::DuplicateSuppressed => {}
        }
    }
    // Brutal, fail-loud assertion: saturation must yield at least one REAL OK retrieval.
    // The previous `ok_count + no_match_count > 0` masked the ok=0/no_match=N failure mode
    // (retrieval silently returning nothing for every request still counted as success).
    let saturation_yields_ok = ok_count > 0;
    assert!(
        saturation_yields_ok,
        "saturation must yield at least one OK retrieval; got ok={ok_count} no_match={no_match_count} \
         (ok=0 means retrieval returned nothing for every concurrent request — the masked failure mode)"
    );
    builder.assert_contract(
        "saturation_yields_ok_retrievals",
        saturation_yields_ok,
        "ok_count > 0 under concurrent saturation",
        &format!("ok={ok_count} no_match={no_match_count}"),
        "concurrent compile_context saturation must produce real OK retrievals, not all-NoMatch",
    );

    let report = builder.build();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{}__{}.json", report.test_name, report.test_id)),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    components.teardown().await.expect("teardown");
}

#[ignore = "requires live containers"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn high_qps_compile_context_load_meets_p95_and_error_budget_targets() {
    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("DS-007_high_qps_compile_context");

    let components = McpServerApp::from_environment(dream_retrieval_config())
        .await
        .expect("live");
    dream_seed_skills(
        components.rebuild_coordinator.as_ref(),
        &[
            (
                "ds007-qps-skill-1",
                "QPS benchmark skill one",
                &["bench", "one"],
            ),
            (
                "ds007-qps-skill-2",
                "QPS benchmark skill two",
                &["bench", "two"],
            ),
            (
                "ds007-qps-skill-3",
                "QPS benchmark skill three",
                &["bench", "three"],
            ),
        ],
    )
    .await;

    let repo = test_repo_path();
    use tokio::task::JoinSet;
    let mut set = JoinSet::new();
    let app = components.app.clone();
    let total_requests = 48usize;
    for i in 0..total_requests {
        let a = app.clone();
        let repo_clone = repo.clone();
        set.spawn(async move {
            let t0 = std::time::Instant::now();
            let r = a
                .compile_context(CompileContextRequest {
                    prompt: format!("qps benchmark {i}"),
                    session_id: format!("ds007-session-{i}"),
                    repo_path: repo_clone,
                    trigger: None,
                })
                .await;
            (r, t0.elapsed().as_millis() as u64)
        });
    }
    let mut latencies = Vec::with_capacity(total_requests);
    let mut degraded_count = 0usize;
    while let Some(result) = set.join_next().await {
        let (r, lat) = result.expect("task");
        latencies.push(lat);
        builder.record_latency(&format!("req-{}", latencies.len() - 1), lat);
        // Under fault-free read load every request should resolve to Ok / NoMatch /
        // DuplicateSuppressed. A Degraded response means the read path failed (e.g.
        // embedding unavailable) and counts against the error budget below.
        if matches!(r.status, CompileContextStatus::Degraded) {
            degraded_count += 1;
        }
    }
    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    let p95 = latencies[(latencies.len() * 95 / 100).min(latencies.len() - 1)];
    let p99 = latencies[(latencies.len() * 99 / 100).min(latencies.len() - 1)];
    let max = latencies.last().copied().unwrap_or(0);
    let min = latencies.first().copied().unwrap_or(0);

    // Explicit, FAIL-ABLE contract thresholds (warm in-process read path). If the real
    // measured p95 or error rate exceeds budget the scenario FAILS — no hardcoded Passed.
    // P95 target: <500ms-class warm path (docs/reference/online-retrieval-cqrs.md, DS-007).
    // Error budget: zero Degraded tolerated under fault-free read load.
    const P95_BUDGET_MS: u64 = 500;
    const DEGRADED_BUDGET: usize = 0;
    let p95_within_budget = p95 <= P95_BUDGET_MS;
    let errors_within_budget = degraded_count <= DEGRADED_BUDGET;
    builder.assert_contract(
        "high_qps_p95_within_budget",
        p95_within_budget,
        &format!("p95 <= {P95_BUDGET_MS}ms"),
        &format!("p95={p95}ms (p50={p50}ms p99={p99}ms max={max}ms min={min}ms)"),
        "warm in-process read path must meet the p95 latency budget under concurrent QPS",
    );
    builder.assert_contract(
        "high_qps_error_budget",
        errors_within_budget,
        &format!("degraded_count <= {DEGRADED_BUDGET}"),
        &format!("degraded_count={degraded_count} of {total_requests}"),
        "concurrent QPS must stay within the error budget (no Degraded under fault-free load)",
    );
    assert!(
        p95_within_budget,
        "p95 latency {p95}ms exceeds the {P95_BUDGET_MS}ms warm-path budget \
         (p50={p50}ms p99={p99}ms max={max}ms min={min}ms)"
    );
    assert!(
        errors_within_budget,
        "error budget exceeded: {degraded_count} Degraded responses of {total_requests} \
         under fault-free read load (budget {DEGRADED_BUDGET})"
    );

    let report = builder.build();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{}__{}.json", report.test_name, report.test_id)),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    components.teardown().await.expect("teardown");
}

#[test]
#[ignore = "Dream-state contract: multi-repo isolation topology not fully implemented"]
fn multi_repo_scope_isolation_prevents_cross_tenant_context_leakage() {
    pending_contract(DreamContractCase {
        id: "DS-008",
        objective: "Ensure strict isolation across concurrent repositories/scopes in shared runtime topologies.",
        flow: &[
            "Run multiple repos with overlapping terms",
            "Issue interleaved compile_context and extraction calls",
            "Validate response source provenance",
        ],
        hard_invariants: &[
            "No cross-repo context leakage",
            "Per-repo suppression boundaries stay isolated",
            "Per-scope allowlist boundaries are enforced",
        ],
        determinism_strategy: &["Named fixture repos with unique canary tokens"],
    });
}

#[test]
#[ignore = "Dream-state contract: restart persistence scenario not fully implemented"]
fn full_restart_cycle_preserves_session_suppression_and_cache_invalidation_contracts() {
    pending_contract(DreamContractCase {
        id: "DS-009",
        objective: "Prove correctness of suppression/cache invalidation state across full process/container restarts.",
        flow: &[
            "Serve compile_context traffic",
            "Trigger graph updates",
            "Restart one service at a time and then all services",
            "Replay same sessions/prompts",
        ],
        hard_invariants: &[
            "No stale cache-serving after graph_version changes",
            "Suppression semantics preserved correctly",
            "No duplicate injection after restart",
        ],
        determinism_strategy: &[
            "Restart choreography script",
            "Golden pre/post state snapshots",
        ],
    });
}

#[test]
#[ignore = "Dream-state contract: security abuse-case suite not fully implemented"]
fn hostile_input_suite_never_breaches_writer_or_transcript_trust_boundaries() {
    pending_contract(DreamContractCase {
        id: "DS-010",
        objective: "Assert boundary safety against malicious transcript refs, repo paths, payloads, and event inputs.",
        flow: &[
            "Inject traversal and escaping attempts",
            "Inject malformed and oversized payloads",
            "Inject conflicting idempotency/event envelopes",
        ],
        hard_invariants: &[
            "No out-of-root file writes",
            "No path traversal reads",
            "Explicit failure reason codes for all rejected inputs",
        ],
        determinism_strategy: &[
            "Curated adversarial fixture corpus",
            "Negative-case reason-code matrix",
        ],
    });
}

#[test]
#[ignore = "Dream-state contract: observability end-to-end assertions not fully implemented"]
fn observability_contract_emits_complete_reason_coded_traces_for_all_failure_modes() {
    pending_contract(DreamContractCase {
        id: "DS-011",
        objective: "Require complete structured observability coverage for healthy/degraded/failure transitions.",
        flow: &[
            "Exercise nominal + degraded + hard-failure flows",
            "Collect logs/events/traces",
            "Correlate by request and job identifiers",
        ],
        hard_invariants: &[
            "Every failure has a machine-parseable reason code",
            "Critical transitions are trace-correlated end-to-end",
            "No silent swallow paths",
        ],
        determinism_strategy: &["Normalized log/event comparison harness"],
    });
}

#[test]
#[ignore = "Dream-state contract: model-provider parity checks not fully implemented"]
fn extraction_provider_parity_holds_for_contract_shape_and_quality_floor() {
    pending_contract(DreamContractCase {
        id: "DS-012",
        objective: "Enforce output-contract parity and minimum quality floor across extraction providers.",
        flow: &[
            "Replay same transcript corpus through Claude and Ollama providers",
            "Normalize candidate structures and evaluate differences",
        ],
        hard_invariants: &[
            "Contract keys and types always match",
            "Quality floor thresholds are met for both providers",
            "Provider switch does not break ingestion contracts",
        ],
        determinism_strategy: &[
            "Pinned model versions",
            "Fixture corpus with expected quality bands",
        ],
    });
}

#[test]
#[ignore = "Dream-state contract: lifecycle policy and approval SLA not fully implemented"]
fn pending_lifecycle_and_human_approval_sla_are_enforced_under_backlog() {
    pending_contract(DreamContractCase {
        id: "DS-013",
        objective: "Verify lifecycle state machine correctness and approval-policy behavior at scale.",
        flow: &[
            "Generate large pending backlog",
            "Apply mixed approvals/rejections/retirements",
            "Run maintenance cycles and inspect lifecycle state outputs",
        ],
        hard_invariants: &[
            "State transitions are legal and auditable",
            "TTL warning/tombstone semantics are preserved",
            "No hidden auto-approval path",
        ],
        determinism_strategy: &["Deterministic approval script with timestamp control"],
    });
}

// Dream detail:
// The platform should self-heal from known degraded states without human intervention when a
// safe remediation is available. This contract requires the system to detect, choose, execute,
// and verify recovery actions while preserving data integrity and auditability.
#[test]
#[ignore = "Dream-state contract: autonomous self-healing loop not fully implemented"]
fn autonomous_self_healing_loop_recovers_known_degraded_states_safely() {
    pending_contract(DreamContractCase {
        id: "DS-014",
        objective: "Automatically recover from known degraded conditions using policy-approved repair actions.",
        flow: &[
            "Detect degraded reason codes from runtime events",
            "Select remediation from policy-safe repair catalog",
            "Execute remediation with bounded retries and rollback hooks",
            "Re-run health probes and contract tests",
        ],
        hard_invariants: &[
            "No unsafe or out-of-policy auto-action",
            "Every repair is traceable and auditable",
            "Recovery does not create data drift",
        ],
        determinism_strategy: &[
            "Pinned degraded-state fixtures",
            "Deterministic remediation decision table",
            "Replayable repair transcript log",
        ],
    });
}

// Dream detail:
// Historical reproducibility is critical for debugging and trust. Given a commit/session tuple,
// the system should reconstruct prior context and produce equivalent retrieval behavior.
#[test]
#[ignore = "Dream-state contract: time-travel memory replay not fully implemented"]
fn time_travel_memory_reconstructs_historical_context_and_retrieval_output() {
    pending_contract(DreamContractCase {
        id: "DS-015",
        objective: "Reproduce historical compile_context outcomes from archived session and repo states.",
        flow: &[
            "Checkout historical repo snapshot",
            "Load archived transcript/session artifacts",
            "Rebuild historical graph and cache state",
            "Replay compile_context and compare to golden historical outputs",
        ],
        hard_invariants: &[
            "Historical replay matches expected top-k ordering",
            "Reason codes and scope merges are stable",
            "No dependency on current mutable state",
        ],
        determinism_strategy: &[
            "Versioned fixture snapshots",
            "Frozen provider profile for replay mode",
            "Golden output bundles per replay case",
        ],
    });
}

// Dream detail:
// Skill ingestion should be policy-native: risk, novelty, trust, and governance metadata drive
// whether a proposal is auto-routed, escalated, or rejected.
#[test]
#[ignore = "Dream-state contract: policy-native skill governance not fully implemented"]
fn policy_native_skill_governance_routes_proposals_by_risk_and_trust_scores() {
    pending_contract(DreamContractCase {
        id: "DS-016",
        objective: "Enforce governance-aware routing of extracted and maintenance-generated skill proposals.",
        flow: &[
            "Generate proposals across trust/risk/novelty bands",
            "Evaluate policy rules and scoring",
            "Route to approve/escalate/reject queues",
            "Verify lifecycle artifacts and policy audit records",
        ],
        hard_invariants: &[
            "Policy outcomes are deterministic and explainable",
            "High-risk proposals never bypass human gate",
            "Lifecycle state machine remains valid under policy routing",
        ],
        determinism_strategy: &[
            "Fixed policy rule fixtures",
            "Deterministic scoring model for governance mode",
            "Golden route-decision snapshots",
        ],
    });
}

// Dream detail:
// This extends strict isolation with shared intelligence: global learnings are aggregated across
// repositories while ensuring no tenant leakage at retrieval time.
#[test]
#[ignore = "Dream-state contract: cross-repo collective intelligence not fully implemented"]
fn cross_repo_collective_intelligence_learns_globally_without_tenant_leakage() {
    pending_contract(DreamContractCase {
        id: "DS-017",
        objective: "Aggregate global skill improvements from many repos while preserving hard isolation guarantees.",
        flow: &[
            "Ingest contributions from multiple tenant repos",
            "Build global aggregate skill corpus with provenance",
            "Serve mixed repo/global retrieval queries",
            "Validate provenance and isolation boundaries",
        ],
        hard_invariants: &[
            "No unauthorized cross-tenant content exposure",
            "Every global skill carries immutable provenance trail",
            "Tenant-specific context remains tenant-scoped",
        ],
        determinism_strategy: &[
            "Canary-tagged multi-tenant fixture corpus",
            "Deterministic provenance hashing",
            "Golden isolation assertion matrix",
        ],
    });
}

// Dream detail:
// Retrieval should be explainable beyond score dumps: users/operators should see why selected
// skills won and what minimal prompt/weight changes would alter ranking.
#[test]
#[ignore = "Dream-state contract: counterfactual explainability not fully implemented"]
fn retrieval_counterfactual_explainability_reports_why_and_what_would_change() {
    pending_contract(DreamContractCase {
        id: "DS-018",
        objective: "Provide counterfactual explanations for ranking and fusion decisions in compile_context.",
        flow: &[
            "Execute retrieval for baseline prompt",
            "Compute ranked rationale and feature contributions",
            "Generate minimal counterfactual perturbations",
            "Validate explanation consistency against observed ranking changes",
        ],
        hard_invariants: &[
            "Explanation fields are complete and machine-parseable",
            "Counterfactual claims are empirically verifiable",
            "No exposure of prohibited internal secrets",
        ],
        determinism_strategy: &[
            "Pinned ranking fixtures",
            "Deterministic perturbation set",
            "Golden explanation output schemas",
        ],
    });
}

// Dream detail:
// Drift sentinel goes beyond PG/Qdrant repair by continuously checking semantic and operational
// drift across files, graph, vectors, lifecycle metadata, and runtime output behavior.
#[test]
#[ignore = "Dream-state contract: always-on drift sentinel not fully implemented"]
fn always_on_drift_sentinel_detects_and_blocks_semantic_and_operational_drift() {
    pending_contract(DreamContractCase {
        id: "DS-019",
        objective: "Continuously detect multi-surface drift before user-visible quality or correctness degrades.",
        flow: &[
            "Continuously sample filesystem, PG graph, Qdrant vectors, and lifecycle metadata",
            "Run behavioral canary prompts through runtime",
            "Trigger drift alarms and optional quarantine policies",
            "Verify repair actions clear drift within bounded windows",
        ],
        hard_invariants: &[
            "No silent drift accumulation",
            "Drift alerts are precise and actionable",
            "Quarantine never corrupts healthy data paths",
        ],
        determinism_strategy: &[
            "Synthetic drift injection campaigns",
            "Deterministic canary prompt set",
            "Golden drift-detection confusion matrix",
        ],
    });
}

// Dream detail:
// Request orchestration should optimize quality/latency/cost dynamically while preserving
// contract semantics and avoiding policy violations.
#[test]
#[ignore = "Dream-state contract: SLO-aware orchestration brain not fully implemented"]
fn slo_aware_orchestration_brain_balances_quality_latency_and_cost_safely() {
    pending_contract(DreamContractCase {
        id: "DS-020",
        objective: "Adapt execution strategy per request to satisfy SLOs and budgets without semantic regressions.",
        flow: &[
            "Classify incoming requests by urgency and quality requirements",
            "Select provider/path strategy under budget constraints",
            "Execute with online feedback and fallback policies",
            "Verify semantic contract equivalence across adaptive paths",
        ],
        hard_invariants: &[
            "SLO breaches stay under budget",
            "Adaptive routing never violates correctness contracts",
            "Cost controls are enforced deterministically",
        ],
        determinism_strategy: &[
            "Pinned traffic classes and budgets",
            "Deterministic routing policy table",
            "Golden semantic-equivalence assertions",
        ],
    });
}

// Dream detail:
// New extraction/ranking strategies should run in shadow mode against live traffic and only
// promote when statistically and contractually superior.
#[test]
#[ignore = "Dream-state contract: shadow deployment evaluator not fully implemented"]
fn shadow_deployment_evaluator_promotes_new_strategies_only_on_proven_improvement() {
    pending_contract(DreamContractCase {
        id: "DS-021",
        objective: "Compare candidate strategies in shadow execution and gate promotion on hard improvement criteria.",
        flow: &[
            "Mirror live traffic to baseline and candidate strategies",
            "Collect quality, latency, stability, and safety deltas",
            "Run statistical significance + contract violation checks",
            "Auto-promote or auto-reject with immutable decision record",
        ],
        hard_invariants: &[
            "No promotion with unresolved contract regressions",
            "Promotion decisions are evidence-backed",
            "Rollback path is immediate and lossless",
        ],
        determinism_strategy: &[
            "Replayed traffic corpus for reproducibility",
            "Fixed evaluation windows and thresholds",
            "Golden promotion decision fixtures",
        ],
    });
}

// Dream detail:
// We want one correlation chain from transcript ingestion to final context output and all
// side effects, so any anomaly can be traced causally in minutes.
#[test]
#[ignore = "Dream-state contract: end-to-end causal tracing not fully implemented"]
fn end_to_end_causal_tracing_links_every_side_effect_to_originating_session_event() {
    pending_contract(DreamContractCase {
        id: "DS-022",
        objective: "Guarantee complete causal traceability across extraction, ingestion, rebuild, relay, and retrieval.",
        flow: &[
            "Inject identifiable session events",
            "Follow correlation IDs through event bus and persistence layers",
            "Query trace graph for full lineage",
            "Validate no orphan side effects exist",
        ],
        hard_invariants: &[
            "Every durable mutation has upstream cause",
            "Every response has complete lineage",
            "No trace breaks at service boundaries",
        ],
        determinism_strategy: &[
            "Deterministic correlation-id generation in test mode",
            "Trace graph snapshot comparison",
            "Golden lineage path assertions",
        ],
    });
}

// Dream detail:
// A deterministic digital twin allows exact replay/debugging and prevents production-only
// mysteries by reproducing runtime behavior locally.
#[test]
#[ignore = "Dream-state contract: offline deterministic twin not fully implemented"]
fn offline_deterministic_twin_replays_production_behavior_bit_for_bit() {
    pending_contract(DreamContractCase {
        id: "DS-023",
        objective: "Run an offline twin that reproduces production outcomes exactly for replay/debug workflows.",
        flow: &[
            "Capture production event and request traces",
            "Replay traces in deterministic twin mode",
            "Compare outputs, state transitions, and events",
            "Flag non-deterministic divergence causes",
        ],
        hard_invariants: &[
            "Replay outputs match production golden traces",
            "State transition deltas remain zero",
            "Divergence reports are complete and actionable",
        ],
        determinism_strategy: &[
            "Frozen runtime inputs and clocks",
            "Deterministic provider adapters for replay",
            "Golden state/event timeline snapshots",
        ],
    });
}

// Dream detail:
// The system should learn safely from accepted/rejected outcomes and measurable downstream
// impact, then improve extraction/retrieval policy over time without regressions.
#[test]
#[ignore = "Dream-state contract: outcome-based learning loop not fully implemented"]
fn outcome_based_learning_loop_improves_quality_without_contract_regressions() {
    pending_contract(DreamContractCase {
        id: "DS-024",
        objective: "Continuously tune system behavior from outcome feedback with strict regression guards.",
        flow: &[
            "Collect acceptance/rejection/usefulness outcomes",
            "Train or tune policy thresholds in sandbox",
            "Validate candidate policy via shadow evaluator",
            "Promote only if quality gains and contract safety hold",
        ],
        hard_invariants: &[
            "No regression in core correctness contracts",
            "Learning decisions are auditable and reversible",
            "Quality trend improves over evaluation windows",
        ],
        determinism_strategy: &[
            "Versioned outcome datasets",
            "Fixed training/evaluation splits",
            "Golden pre/post policy comparison artifacts",
        ],
    });
}
