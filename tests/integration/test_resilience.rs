use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use infrastructure::{
    CircuitBreaker, CircuitState, InfrastructureHealthChecker, ResilienceError, RetryPolicy,
    execute_with_resilience, retry_with_backoff,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn spawn_http_dependency(status_line: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should provide address");
    let status_line = status_line.to_owned();
    tokio::spawn(async move {
        let (mut socket, _) = listener
            .accept()
            .await
            .expect("dependency server should accept one request");
        let mut request = vec![0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let payload = "{}";
        let response = format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("dependency server should write response");
    });
    format!("http://{address}/health")
}

#[tokio::test]
async fn retry_with_backoff_stops_after_max_attempts() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_operation = Arc::clone(&attempts);
    let policy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    };

    let result: Result<(), &str> = retry_with_backoff(&policy, move || {
        let attempts_for_operation = Arc::clone(&attempts_for_operation);
        async move {
            attempts_for_operation.fetch_add(1, Ordering::SeqCst);
            Err("still failing")
        }
    })
    .await;

    assert_eq!(result, Err("still failing"));
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn execute_with_resilience_opens_circuit_after_repeated_failures() {
    let breaker = CircuitBreaker::new(2, Duration::from_secs(60));
    let policy = RetryPolicy {
        max_attempts: 1,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
    };

    let first = execute_with_resilience(&breaker, &policy, || async { Err::<(), _>("boom") }).await;
    let second =
        execute_with_resilience(&breaker, &policy, || async { Err::<(), _>("boom-again") }).await;
    let blocked = execute_with_resilience(&breaker, &policy, || async { Ok::<_, &str>(()) }).await;

    assert_eq!(first, Err(ResilienceError::Operation("boom")));
    assert_eq!(second, Err(ResilienceError::Operation("boom-again")));
    assert_eq!(breaker.state().await, CircuitState::Open);
    assert_eq!(blocked, Err(ResilienceError::CircuitOpen));
}

#[tokio::test]
async fn health_checker_reports_dependency_level_statuses() {
    let healthy_endpoint = spawn_http_dependency("200 OK").await;
    let unhealthy_endpoint = spawn_http_dependency("503 Service Unavailable").await;
    let report = InfrastructureHealthChecker::new()
        .with_http_dependency(reqwest::Client::new(), "healthy-http", healthy_endpoint)
        .with_http_dependency(reqwest::Client::new(), "unhealthy-http", unhealthy_endpoint)
        .check()
        .await;

    assert!(!report.healthy);
    assert_eq!(report.components.len(), 2);
    assert!(
        report
            .components
            .iter()
            .any(|component| component.name == "healthy-http" && component.healthy)
    );
    assert!(
        report
            .components
            .iter()
            .any(|component| component.name == "unhealthy-http" && !component.healthy)
    );
}

#[test]
fn compose_dependency_gating_allows_degraded_startup() {
    use serde_yaml::Value;

    let compose_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docker-compose.yml");
    let raw = std::fs::read_to_string(compose_path).expect("compose file should be readable");
    let parsed: Value = serde_yaml::from_str(&raw).expect("compose file should be valid YAML");

    let services = parsed["services"]
        .as_mapping()
        .expect("compose file should have a services mapping");

    let hard_deps: &[(&str, &str)] = &[
        ("postgres", "service_healthy"),
        ("redis", "service_healthy"),
    ];
    let soft_deps: &[(&str, &str)] =
        &[("qdrant", "service_started"), ("ollama", "service_started")];

    for runtime_service in &["mcp-server", "graph-builder", "maintenance-worker"] {
        let depends_on = services[runtime_service]["depends_on"]
            .as_mapping()
            .unwrap_or_else(|| panic!("{runtime_service} should have a depends_on mapping"));

        for (dep_name, expected_condition) in hard_deps {
            let condition = depends_on[dep_name]["condition"]
                .as_str()
                .unwrap_or_else(|| panic!("{runtime_service} should gate on {dep_name}"));
            assert_eq!(
                condition, *expected_condition,
                "{runtime_service} should require {dep_name} to be {expected_condition} before starting"
            );
        }
        for (dep_name, expected_condition) in soft_deps {
            let condition = depends_on[dep_name]["condition"]
                .as_str()
                .unwrap_or_else(|| panic!("{runtime_service} should gate on {dep_name}"));
            assert_eq!(
                condition, *expected_condition,
                "{runtime_service}: {dep_name} should use {expected_condition} so startup proceeds in degraded mode"
            );
        }
    }
}
