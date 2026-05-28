use std::{path::PathBuf, sync::Arc, time::Duration};

use domain::ScopeType;
use graph_builder::{
    GraphRebuildOrchestrator, InMemoryDurableGraphState, ScopeRoot, SkillFileChange, SkillWatcher,
    WatcherRecovery,
};
use infrastructure::{
    CircuitBreaker, CircuitState, DependencyFactory, EventEnvelope, InfrastructureHealthChecker,
    ResilienceError, RetryPolicy, execute_with_resilience, logging::init_logging,
};
use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::RwLock,
    time::sleep,
};

fn synthetic_outbox_drain_enabled() -> bool {
    matches!(
        std::env::var("GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn build_scope_roots() -> Vec<ScopeRoot> {
    let repo_root = PathBuf::from(std::env::var("GRAPH_BUILDER_PROJECT_ROOT").unwrap_or_else(
        |_| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        },
    ));
    vec![
        ScopeRoot::new("project", ScopeType::Project, repo_root),
        ScopeRoot::new(
            "global",
            ScopeType::Global,
            std::env::var("GRAPH_BUILDER_GLOBAL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("docs")),
        ),
    ]
}

fn polling_interval() -> Duration {
    std::env::var("GRAPH_BUILDER_POLL_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(15))
}

#[derive(Debug, Clone, Default)]
struct GraphBuilderHealthState {
    last_rebuild_error: Option<String>,
    circuit_state: Option<CircuitState>,
}

#[derive(Debug, Serialize)]
struct GraphBuilderHealthResponse {
    healthy: bool,
    detail: String,
    circuit_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_rebuild_error: Option<String>,
    dependencies: infrastructure::HealthReport,
}

fn run_rebuild_cycle(
    watcher: &mut SkillWatcher,
    recovery: &mut WatcherRecovery,
    orchestrator: &mut GraphRebuildOrchestrator<'_, InMemoryDurableGraphState>,
) -> Result<Option<i64>, String> {
    let first_scan = watcher
        .collect_file_changes()
        .map_err(|error| error.to_string())?;
    let recovered = recovery.reconcile(
        &watcher.previous_snapshot(),
        &watcher.current_snapshot(),
        &watcher.scopes(),
    );
    let mut all_changes: Vec<SkillFileChange> = first_scan;
    all_changes.extend(recovered);

    if all_changes.is_empty() {
        return Ok(None);
    }

    let outcome = orchestrator
        .rebuild_from_changes(&watcher.scopes(), &all_changes)
        .map_err(|error| error.to_string())?;
    tracing::info!(
        graph_version = outcome.graph_version,
        skills_count = outcome.skills_count,
        communities_count = outcome.communities_count,
        "graph rebuilt"
    );
    Ok(Some(outcome.graph_version))
}

async fn serve_health_endpoint(
    health_checker: InfrastructureHealthChecker,
    runtime_health_state: Arc<RwLock<GraphBuilderHealthState>>,
) -> std::io::Result<()> {
    let address = std::env::var("GRAPH_BUILDER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse::<std::net::SocketAddr>()
        .map_err(std::io::Error::other)?;
    let listener = TcpListener::bind(address).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0_u8; 2048];
        let _ = socket.read(&mut request).await?;
        let request_text = String::from_utf8_lossy(&request);
        let request_line = request_text.lines().next().unwrap_or_default();

        if !request_line.starts_with("GET /health") {
            socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found",
                )
                .await?;
            continue;
        }

        let dependencies = health_checker.check().await;
        let state = runtime_health_state.read().await.clone();
        let circuit_state = state
            .circuit_state
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "Closed".to_owned());
        let healthy = dependencies.healthy
            && state.last_rebuild_error.is_none()
            && state.circuit_state != Some(CircuitState::Open);
        let detail = if healthy {
            "ok".to_owned()
        } else {
            "degraded (graph_builder_runtime)".to_owned()
        };
        let payload = serde_json::to_string(&GraphBuilderHealthResponse {
            healthy,
            detail,
            circuit_state,
            last_rebuild_error: state.last_rebuild_error,
            dependencies,
        })
        .map_err(std::io::Error::other)?;
        let status_line = if healthy {
            "HTTP/1.1 200 OK"
        } else {
            "HTTP/1.1 503 Service Unavailable"
        };
        let response = format!(
            "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        socket.write_all(response.as_bytes()).await?;
    }
}

/// Runs a durable graph-builder loop with bounded retries and health reporting.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("graph-builder", "info")?;

    if !synthetic_outbox_drain_enabled() {
        return Err("graph-builder runtime durable state has no relay-backed outbox drain wiring yet; refusing to run with synthetic drain disabled (set GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN=1 only for local test/demo runs)".into());
    }

    let scopes = build_scope_roots();
    let mut watcher = SkillWatcher::new(scopes)?;
    let mut recovery = WatcherRecovery::default();
    let mut durable_state = InMemoryDurableGraphState::with_synthetic_outbox_drain();
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events);
    let retry_policy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(2),
    };
    let breaker = CircuitBreaker::new(3, Duration::from_secs(10));
    let runtime_health_state = Arc::new(RwLock::new(GraphBuilderHealthState::default()));
    let health_server_state = Arc::clone(&runtime_health_state);
    tokio::spawn(async move {
        if let Err(error) = serve_health_endpoint(DependencyFactory::build_health_checker_from_environment(), health_server_state).await
        {
            tracing::error!(%error, "graph-builder health endpoint failed");
        }
    });

    loop {
        let result = execute_with_resilience(&breaker, &retry_policy, || {
            let cycle_result = run_rebuild_cycle(&mut watcher, &mut recovery, &mut orchestrator);
            async move { cycle_result }
        })
        .await;
        let mut state = runtime_health_state.write().await;
        state.circuit_state = Some(breaker.state().await);
        match result {
            Ok(_) => state.last_rebuild_error = None,
            Err(ResilienceError::CircuitOpen) => {
                state.last_rebuild_error = Some("rebuild_blocked_by_circuit_breaker".to_owned())
            }
            Err(ResilienceError::Operation(error)) => state.last_rebuild_error = Some(error),
        }
        drop(state);

        sleep(polling_interval()).await;
    }
}
