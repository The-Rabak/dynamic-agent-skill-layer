/// Container-control helpers for the real-infra fault-injection harness.
///
/// All functions manipulate containers by running `docker compose -f <file> stop/start <svc>`
/// as a child process. They never modify the compose file itself.
///
/// Each function returns a `Result<(), String>` where `Err` carries the captured
/// stderr/exit status so callers can surface meaningful assertions.
use std::{path::Path, process::Command};

/// Stops a single named compose service.
///
/// Equivalent to: `docker compose -f <compose_file> stop <service>`.
/// Returns `Ok(())` when the command exits zero, `Err(stderr)` otherwise.
pub fn compose_stop_service(compose_file: &Path, service: &str) -> Result<(), String> {
    run_compose_command(compose_file, "stop", &[service])
}

/// Starts one or more named compose services.
///
/// Equivalent to: `docker compose -f <compose_file> start <svc1> [<svc2> ...]`.
/// Returns `Ok(())` when the command exits zero, `Err(stderr)` otherwise.
pub fn compose_start_services(compose_file: &Path, services: &[&str]) -> Result<(), String> {
    run_compose_command(compose_file, "start", services)
}

/// RAII guard that restarts a set of compose services when it is dropped —
/// including during a panic unwind.
///
/// Fault-injection tests stop *shared* containers (Postgres, Ollama, …). If such
/// a test panics mid-run it would otherwise leave those containers stopped,
/// breaking every sibling test and any concurrent work on the live stack (#172).
/// Hold one of these for the duration of a chaos test, listing every service the
/// test might stop; on drop it best-effort `docker compose start`s them all so
/// the stack is always restored. `start` on an already-running service is a
/// no-op, so the happy-path drop is harmless.
pub struct ServiceRestoreGuard {
    compose_file: std::path::PathBuf,
    services: Vec<String>,
}

impl ServiceRestoreGuard {
    /// Guards `services`, restarting them all from `compose_file` on drop.
    pub fn new(compose_file: &Path, services: &[&str]) -> Self {
        Self {
            compose_file: compose_file.to_path_buf(),
            services: services.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

impl Drop for ServiceRestoreGuard {
    fn drop(&mut self) {
        // Best-effort: ensure every guarded service is running again, even on a
        // panic. Errors are swallowed — there is nothing useful to do from a Drop,
        // and a failure here must not mask the original test panic.
        let refs: Vec<&str> = self.services.iter().map(String::as_str).collect();
        let _ = compose_start_services(&self.compose_file, &refs);
    }
}

/// Runs `docker compose -f <compose_file> <subcommand> [args...]`.
///
/// Returns `Ok(())` on exit code 0, `Err(<combined stdout+stderr>)` otherwise.
fn run_compose_command(
    compose_file: &Path,
    subcommand: &str,
    service_args: &[&str],
) -> Result<(), String> {
    let mut cmd = Command::new("docker");
    cmd.arg("compose")
        .arg("-f")
        .arg(compose_file)
        .arg(subcommand);
    for svc in service_args {
        cmd.arg(svc);
    }

    let output = cmd
        .output()
        .map_err(|error| format!("failed to spawn docker compose {subcommand}: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "docker compose {subcommand} exited with {}\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        ))
    }
}
