/// Watcher-churn and concurrent-load helpers for the real-infra E2E harness.
///
/// Two capabilities are provided:
///
/// - **`write_skill_files_to_sandbox`**: writes N `SKILL.md` files into a
///   caller-supplied sandbox directory to drive the filesystem watcher and
///   extraction-worker pool. Files are placed in distinct sub-directories to
///   match the `<dir>/SKILL.md` layout the watcher recognises. The caller
///   controls sandbox creation and cleanup; this function only creates files.
///   To simulate approval through the human gate the caller may rename any
///   written `.pending` file to `SKILL.md` — this helper writes final SKILL.md
///   files directly into the sandbox, which is acceptable because the sandbox
///   is isolated from the real skill directory.
///
/// - **`fire_concurrent_compile_context`**: spawns K `compile_context` calls in
///   parallel via `tokio::spawn` + `JoinSet`, returning one [`CallSample`] per
///   call with its latency. Callers can verify genuine concurrency by checking
///   that the wall-clock time is less than the sum of sequential latencies.
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use mcp_server::{
    McpServerApp,
    tools::compile_context::{CompileContextRequest, CompileContextStatus},
};
use tokio::task::JoinSet;

/// A single observed compile_context call outcome with its wall-clock latency.
#[derive(Debug, Clone)]
pub struct CallSample {
    /// The response status returned by `compile_context`.
    pub status: CompileContextStatus,
    /// Wall-clock duration from call start to first-byte response, in milliseconds.
    pub duration_ms: u64,
}

/// Writes `count` SKILL.md fixture files into `sandbox_dir`, each in its own
/// numbered sub-directory, and returns the list of written file paths.
///
/// Each file uses a minimal but valid SKILL.md format so the watcher and
/// extraction worker can process them without errors. The sandbox directory
/// must already exist; this function does not create it.
///
/// # Human-gate note
/// The watcher requires `.pending` → `SKILL.md` rename for normal approval
/// flow. This helper writes `SKILL.md` directly because the sandbox dir is
/// test-controlled and isolated from the production skill tree. Tests that
/// want to exercise the approval rename should create `.pending` files and
/// rename them within the sandbox.
pub fn write_skill_files_to_sandbox(
    sandbox_dir: &Path,
    count: usize,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut written = Vec::with_capacity(count);

    for i in 0..count {
        let skill_dir = sandbox_dir.join(format!("harness-skill-{i:04}"));
        std::fs::create_dir_all(&skill_dir)?;

        let skill_path = skill_dir.join("SKILL.md");
        let content = format!(
            "# Harness Skill {i}\n\
             \n\
             tags: harness, load-test, SKILL.md\n\
             \n\
             Load-test fixture written by the fault-injection harness (Slice 1.3).\n\
             \n\
             ## Procedures\n\
             \n\
             1. Step one for skill {i}.\n\
             2. Step two for skill {i}.\n"
        );
        std::fs::write(&skill_path, content)?;
        written.push(skill_path);
    }

    Ok(written)
}

/// Fires `concurrency` compile_context calls in genuine parallel and returns
/// one [`CallSample`] per call.
///
/// All calls use the same `repo_path`; session IDs are suffixed with the call
/// index so suppression does not collapse multiple calls into a single response.
/// Latency is measured wall-clock from spawn to `await` completion.
///
/// # Concurrency proof
/// The returned samples allow callers to verify genuine concurrency by comparing
/// `wall_clock_elapsed < sum(duration_ms)`. With real parallelism the wall-clock
/// will be substantially less than the sequential sum.
pub async fn fire_concurrent_compile_context(
    app: McpServerApp,
    repo_path: String,
    session_prefix: &str,
    concurrency: usize,
) -> Vec<CallSample> {
    let mut set: JoinSet<CallSample> = JoinSet::new();

    for i in 0..concurrency {
        let app_clone = app.clone();
        let repo = repo_path.clone();
        let session_id = format!("{session_prefix}-{i}");

        set.spawn(async move {
            let start = Instant::now();
            let response = app_clone
                .compile_context(CompileContextRequest {
                    prompt: format!("concurrent load test prompt {i}"),
                    session_id,
                    repo_path: repo,
                    trigger: None,
                })
                .await;
            CallSample {
                status: response.status,
                duration_ms: start.elapsed().as_millis() as u64,
            }
        });
    }

    let mut samples = Vec::with_capacity(concurrency);
    while let Some(result) = set.join_next().await {
        samples.push(result.expect("concurrent task must not panic"));
    }
    samples
}
