//! Thin live shim: shells out to `retrieval_sweep.py --gate` and asserts exit 0.
//!
//! # Why this test exists
//! The validated T11 instrument is the Python gate (`scripts/retrieval_sweep.py
//! --gate`), which drives the REAL running mcp-server over HTTP on the 262-skill
//! corpus fixture.  This shim surfaces that gate in `cargo test --ignored` so it
//! is visible alongside the other live e2e tests.  It is intentionally thin:
//! no Rust re-implementation of gate logic, just exit-code assertion.
//!
//! # Running
//! ```sh
//! cargo test -p mcp-server --features test-utils --test test_retrieval_quality_gate -- --ignored
//! ```
//!
//! Requires:
//!   - live containers (mcp-server at 127.0.0.1:3001)
//!   - `python3` on PATH
//!   - `tests/fixtures/retrieval_quality_262_corpus_labeled.json` present
//!   - 262-skill corpus seeded in the running server

/// Runs the Python retrieval gate against the live server and asserts exit 0.
///
/// On non-zero exit, the test fails loudly with the gate's stderr so the
/// failing floor or crater message is surfaced directly in the test output.
/// Never swallows a gate failure — no silent fallback.
#[test]
#[ignore = "requires live containers"]
fn retrieval_quality_gate_passes_on_live_stack() {
    let run_id = format!(
        "ci-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX_EPOCH")
            .as_secs()
    );

    // The test binary's CWD is the crate manifest dir (crates/mcp-server), two
    // levels below the repo root — resolve the script and run from the repo root
    // so the gate's relative paths (fixture, reports/) anchor correctly.
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = repo_root.join("scripts/retrieval_sweep.py");

    let output = std::process::Command::new("python3")
        .current_dir(&repo_root)
        .args([
            script.to_str().expect("script path is valid UTF-8"),
            "--gate",
            "--run-id",
            &run_id,
        ])
        .output()
        .expect(
            "failed to spawn python3 scripts/retrieval_sweep.py --gate. \
             Ensure python3 is on PATH and the scripts/ directory is present.",
        );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "retrieval gate FAILED (exit={}).\n\
             --- gate stdout ---\n{}\n\
             --- gate stderr ---\n{}\n\
             Investigate the failing floor or crater assertion above.\n\
             Do NOT lower thresholds — fix retrieval.",
            output.status,
            stdout.trim(),
            stderr.trim(),
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("GATE: PASS"),
        "gate exited 0 but stdout does not contain 'GATE: PASS' — unexpected output:\n{stdout}",
    );

    println!("[retrieval-gate] PASS  run-id={run_id}");
}
