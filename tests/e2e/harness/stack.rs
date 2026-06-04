/// Stack lifecycle management for the real-infra E2E harness.
///
/// Brings the full docker-compose test stack up (or confirms it is already up),
/// waits for all services to be healthy, and provides fault-injection primitives
/// (`kill`, `stop`, `start`, `pause`, `unpause`) that operate on real containers
/// without touching compose files.
///
/// # Cold-start bug (#157)
/// `graph-builder` can crash on a cold `up` with a Qdrant 409 conflict when the
/// `skills` collection already exists from a prior run. `up()` detects an unhealthy
/// `graph-builder` and issues a second `up -d graph-builder` to restart it; by then
/// the collection already exists and the process succeeds.
///
/// # Guardrail
/// NEVER modify `docker-compose.test.yml`, any Dockerfile, `.env`, or migrations.
/// This module only runs `docker compose` CLI commands.
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Canonical path to the test compose file, relative to the repo root.
const COMPOSE_FILE_RELATIVE: &str = "docker-compose.test.yml";

/// Base URL for the `mcp-server` HTTP API.
pub const MCP_SERVER_URL: &str = "http://127.0.0.1:3001";

/// Base URL for the Qdrant REST API (host-mapped port).
pub const QDRANT_URL: &str = "http://127.0.0.1:16333";

/// Redis URL (host-mapped port).
pub const REDIS_URL: &str = "redis://127.0.0.1:16379";

/// Postgres DSN (host-mapped port).
pub const POSTGRES_DSN: &str =
    "postgres://skill_layer:skill_layer@localhost:15432/skill_layer_test";

/// Named Docker volume for global skills.
pub const GLOBAL_SKILLS_VOLUME: &str = "dynamic-agent-skill-layer_test-global-skills";

/// Named Docker volume for project skills.
pub const PROJECT_SKILLS_VOLUME: &str = "dynamic-agent-skill-layer_test-project-skills";

/// All service names managed by the test compose file.
pub const ALL_SERVICES: &[&str] = &[
    "mcp-server",
    "graph-builder",
    "postgres",
    "redis",
    "qdrant",
    "ollama",
];

/// Stack handle returned by [`Stack::up`].
///
/// Callers retain this to issue fault-injection commands and to confirm
/// teardown at the end of a test run.  The stack is intentionally left
/// running after tests finish (per the human-gate guardrail: leave the stack
/// running when done).
pub struct Stack {
    compose_file: PathBuf,
}

impl Stack {
    /// Returns the resolved path to `docker-compose.test.yml`.
    ///
    /// Assumes the test binary's `CARGO_MANIFEST_DIR` is the `mcp-server` crate;
    /// the compose file sits two levels up at the repo root.
    pub fn compose_file_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(COMPOSE_FILE_RELATIVE)
            .canonicalize()
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(COMPOSE_FILE_RELATIVE)
            })
    }

    /// Confirms the full stack is up and healthy, attempting a cold-start if any
    /// service is not running.
    ///
    /// When the stack is already up (as it is for the primary CI/CD path), this
    /// resolves quickly.  For a cold-start it runs `docker compose up -d`, waits
    /// for health, and applies the #157 graph-builder restart if needed.
    ///
    /// Returns a `Stack` handle on success.  Panics with a diagnostic message if
    /// any service fails to become healthy within the timeout window.
    pub async fn up() -> Self {
        let compose_file = Self::compose_file_path();
        let stack = Stack {
            compose_file: compose_file.clone(),
        };

        // If all services are already healthy, skip the bring-up step.
        if stack.all_services_running() {
            return stack;
        }

        // Cold start: bring up all services.
        stack
            .run_compose(&["up", "-d"])
            .expect("docker compose up -d should succeed");

        // Wait for mcp-server and graph-builder health with the #157 cold-start fix.
        stack
            .wait_for_mcp_server_health(Duration::from_secs(120))
            .await;
        stack.apply_graph_builder_cold_start_fix().await;

        stack
    }

    /// Returns `true` when `docker compose ps` shows all services running.
    fn all_services_running(&self) -> bool {
        let output = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&self.compose_file)
            .args(["ps", "--format", "json"])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                // Simple heuristic: all six services should appear as running.
                ALL_SERVICES
                    .iter()
                    .all(|svc| stdout.contains(svc) && stdout.contains("running"))
            }
            _ => false,
        }
    }

    /// Kills a running service with `docker compose kill` (SIGKILL).
    ///
    /// Used for fault-injection scenarios where a hard kill (not graceful stop)
    /// is required.  Returns `Err` with diagnostics if the command fails.
    pub fn kill(&self, service: &str) -> Result<(), String> {
        self.run_compose(&["kill", service])
    }

    /// Stops a service gracefully with `docker compose stop`.
    ///
    /// Returns `Err` with diagnostics if the command fails.
    pub fn stop(&self, service: &str) -> Result<(), String> {
        self.run_compose(&["stop", service])
    }

    /// Starts one or more previously stopped services.
    ///
    /// Returns `Err` with diagnostics if the command fails.
    pub fn start(&self, services: &[&str]) -> Result<(), String> {
        let mut args = vec!["start"];
        args.extend_from_slice(services);
        self.run_compose(&args)
    }

    /// Pauses a running service (SIGSTOP via `docker pause`).
    pub fn pause(&self, service: &str) -> Result<(), String> {
        let container_name = self.container_name(service);
        run_docker(&["pause", &container_name])
    }

    /// Unpauses a previously paused service.
    pub fn unpause(&self, service: &str) -> Result<(), String> {
        let container_name = self.container_name(service);
        run_docker(&["unpause", &container_name])
    }

    /// Brings a stopped or crashed service back up without altering others.
    pub async fn restart(&self, service: &str) {
        self.run_compose(&["up", "-d", service])
            .expect("docker compose up -d <service> should succeed");
    }

    /// Returns the compose-project container name for a service.
    ///
    /// Follows the `docker compose` default naming convention:
    /// `<project>-<service>-1`.
    fn container_name(&self, service: &str) -> String {
        format!("dynamic-agent-skill-layer-{service}-1")
    }

    /// Waits for the `mcp-server` `/health` endpoint to return `{"healthy":true}`.
    ///
    /// Panics if the deadline passes without a healthy response.
    pub async fn wait_for_mcp_server_health(&self, timeout: Duration) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client should build");

        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Ok(resp) = client.get(format!("{MCP_SERVER_URL}/health")).send().await
                && resp.status().is_success()
                    && let Ok(body) = resp.json::<serde_json::Value>().await
                        && body.get("healthy") == Some(&serde_json::Value::Bool(true)) {
                            return;
                        }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "mcp-server did not become healthy within {}s",
                    timeout.as_secs()
                );
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Applies the #157 cold-start fix: if `graph-builder` is unhealthy after
    /// initial `up`, issues a second `up -d graph-builder` to restart it.
    ///
    /// The Qdrant collection already exists by the time of the retry, so the
    /// 409 conflict that caused the first crash does not recur.
    async fn apply_graph_builder_cold_start_fix(&self) {
        let is_healthy = self.service_is_healthy("graph-builder");
        if !is_healthy {
            self.run_compose(&["up", "-d", "graph-builder"])
                .expect("graph-builder cold-start restart should succeed");
            // Give it time to initialize.
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    /// Returns `true` when `docker inspect` shows the container in a healthy state.
    fn service_is_healthy(&self, service: &str) -> bool {
        let container = self.container_name(service);
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{.State.Health.Status}}",
                &container,
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let status = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                status == "healthy"
            }
            _ => false,
        }
    }

    /// Runs a `docker compose -f <file> <args...>` command.
    ///
    /// Returns `Ok(())` on exit 0, `Err(combined stdout+stderr)` otherwise.
    fn run_compose(&self, args: &[&str]) -> Result<(), String> {
        let mut cmd = Command::new("docker");
        cmd.arg("compose").arg("-f").arg(&self.compose_file);
        for arg in args {
            cmd.arg(arg);
        }
        let output = cmd
            .output()
            .map_err(|e| format!("failed to spawn docker compose: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "docker compose {:?} failed ({})\nstdout: {stdout}\nstderr: {stderr}",
                args, output.status
            ))
        }
    }

    /// Returns the path to the compose file for use by other harness modules.
    pub fn compose_file(&self) -> &Path {
        &self.compose_file
    }
}

/// Runs a bare `docker <args...>` command.
///
/// Returns `Ok(())` on exit 0, `Err(combined stdout+stderr)` otherwise.
fn run_docker(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("docker");
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("failed to spawn docker: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "docker {:?} failed ({})\nstdout: {stdout}\nstderr: {stderr}",
            args, output.status
        ))
    }
}
