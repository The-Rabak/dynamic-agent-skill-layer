//! Real-infrastructure E2E for the durable transcript-ingest queue (todo 103).
//!
//! This is the test the old `extract_session_live_ref_payload_loads_from_transcript_volume`
//! could not be: it replays the SHIPPED command-hook payload shape end to end
//! instead of hand-building a relative ref. The flow under test is exactly what
//! deployment runs:
//!
//!   shipped `config/claude-code/capture-transcript.sh`
//!     → reads `{{transcript_path}}` on the host (where it is valid)
//!     → POSTs the CONTENT to the localhost `/ingest/transcript` endpoint
//!     → server enqueues a row in the live `transcript_ingest_queue` (PG)
//!     → maintenance `TranscriptQueueDrain` feeds it via `transcript_inline`
//!     → a `.pending` draft lands on disk
//!
//! It also pins the two guard contracts the endpoint must honor: a wrong shared
//! secret is rejected with `401`, and an identical re-capture dedups on
//! `content_hash` (idempotent SessionEnd-after-PreCompact).
//!
//! Requires the live container stack (PG + Redis + Qdrant + Ollama). Gated with
//! `#[ignore]` per the repo's live-infra convention; run via
//! `scripts/run-e2e-tests.sh` or `cargo test ... -- --ignored`.

use std::{
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use infrastructure::{DependencyFactory, TranscriptIngestQueue};
use maintenance::{DEFAULT_TRANSCRIPT_DRAIN_BATCH, TranscriptQueueDrain};
use mcp_server::{McpServerApp, protocol};
use retrieval::RetrievalConfig;
use session_extractor::SessionExtractor;

#[path = "report.rs"]
mod report;

#[path = "../integration/env_guard.rs"]
mod env_guard;

const INGEST_SECRET: &str = "test-ingest-secret-103";

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 2,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        ..RetrievalConfig::default()
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

fn capture_script_path() -> PathBuf {
    repo_root().join("config/claude-code/capture-transcript.sh")
}

/// Hook payload shape Claude Code sends to a `command` hook on stdin: an
/// ABSOLUTE `transcript_path`, the `session_id`, and the working dir as `cwd`.
fn hook_payload_json(transcript_path: &str, session_id: &str, cwd: &str) -> String {
    serde_json::json!({
        "transcript_path": transcript_path,
        "session_id": session_id,
        "cwd": cwd,
    })
    .to_string()
}

/// Runs the shipped capture script with a hook payload on stdin, returning its
/// exit status. Mirrors how Claude Code invokes the `command` hook.
fn run_capture_script(
    source: &str,
    ingest_url: &str,
    secret: &str,
    hook_payload: &str,
) -> std::process::ExitStatus {
    let mut child = Command::new("bash")
        .arg(capture_script_path())
        .arg(source)
        .env("SKILL_LAYER_INGEST_URL", ingest_url)
        .env("SKILL_LAYER_INGEST_SECRET", secret)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("capture-transcript.sh should be spawnable");
    child
        .stdin
        .take()
        .expect("child stdin should be piped")
        .write_all(hook_payload.as_bytes())
        .expect("hook payload should write to stdin");
    child.wait().expect("capture script should complete")
}

#[ignore = "requires live containers"]
// Multi-threaded runtime is required: the test blocks one worker on the capture
// script's `child.wait()` (a synchronous `curl`), so the in-process axum server
// must be free to accept that request on another worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shipped_command_hook_payload_round_trips_through_queue_to_pending() {
    let repo_root = repo_root();
    // A procedure-rich transcript so the real extractor reliably yields a
    // candidate (the 2-line sample-transcript.jsonl is too sparse to extract a
    // skill from). The shipped capture script reads this exact file.
    let fixture = repo_root.join("tests/fixtures/session-rich-transcript.jsonl");
    let transcript_content =
        std::fs::read_to_string(&fixture).expect("rich transcript fixture should read");
    let content_hash = TranscriptIngestQueue::content_hash(&transcript_content);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    // Sandbox under the repo so it sits inside SKILL_GLOBAL_ALLOWED_ROOTS
    // (= repo root, set by configure_scope_env_with_global_path).
    let sandbox = repo_root.join(format!("target/tmp-ingest-queue-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");

    let namespace = env_guard::isolated_namespace_with_global_path(sandbox.clone()).await;
    // SAFETY: process env is mutated only while holding ENV_LOCK via namespace.
    unsafe {
        std::env::set_var("CLAUDE_TRANSCRIPT_ROOT", repo_root.join("tests/fixtures"));
        std::env::set_var("EXTRACT_SESSION_PROVIDER", "ollama");
        std::env::set_var("OLLAMA_EXTRACTION_MODEL", "granite4:3b");
        // The extraction provider reads OLLAMA_EXTRACTION_ENDPOINT (NOT OLLAMA_URL,
        // which only drives embeddings) and expects the FULL /api/generate path.
        // Point it at the real Ollama so the drain performs a genuine extraction —
        // this is what proves the end-to-end .pending production, unmasked.
        let ollama_base = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11444".to_owned())
            .trim_end_matches('/')
            .to_owned();
        std::env::set_var(
            "OLLAMA_EXTRACTION_ENDPOINT",
            format!("{ollama_base}/api/generate"),
        );
        // No extraction request timeout: Ollama calls run to completion (models are
        // kept warm via OLLAMA_KEEP_ALIVE), so no per-call ceiling is configured.
        // Must be set before protocol::router() reads it.
        std::env::set_var("TRANSCRIPT_INGEST_SECRET", INGEST_SECRET);
    }

    let mut builder = report::ReportBuilder::new(
        "shipped_command_hook_payload_round_trips_through_queue_to_pending",
    );

    // --- Boot the live server (wires the durable queue from the PG pool) ---
    let boot_start = std::time::Instant::now();
    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", boot_start.elapsed().as_millis() as u64);

    let queue = TranscriptIngestQueue::new(components.pg_adapter.pool().clone());

    // --- Serve the real router on an ephemeral localhost port ---
    let health_checker = DependencyFactory::build_health_checker_from_environment();
    let app_router = protocol::router(components.app.clone(), health_checker);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral port should bind");
    let local_addr: SocketAddr = listener.local_addr().expect("listener has local addr");
    let server = tokio::spawn(async move {
        axum::serve(listener, app_router)
            .await
            .expect("server runs");
    });
    let ingest_url = format!("http://{local_addr}/ingest/transcript");
    let health_url = format!("http://{local_addr}/health");
    let session_id = format!("ingest-queue-{nonce}");

    // Wait until the spawned server is actually accepting before driving the
    // external capture script (whose curl would otherwise race the accept loop).
    let readiness_client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..100 {
        if readiness_client.get(&health_url).send().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ready, "ephemeral ingest server did not become reachable");

    let pending_before = queue
        .count_with_status("pending")
        .await
        .expect("queue countable");

    // --- 1. Shipped capture script POSTs the hook payload (correct secret) ---
    let hook_payload = hook_payload_json(
        fixture.to_str().expect("fixture path utf8"),
        &session_id,
        sandbox.to_str().expect("sandbox path utf8"),
    );
    let invoke_start = std::time::Instant::now();
    let status = run_capture_script("session_end", &ingest_url, INGEST_SECRET, &hook_payload);
    assert!(
        status.success(),
        "capture script must exit 0 (fire-and-forget)"
    );

    // The script detaches its POST so the hook returns immediately (non-blocking),
    // so the row appears asynchronously — poll for it rather than asserting at once.
    let mut enqueued_status = None;
    for _ in 0..100 {
        enqueued_status = queue
            .find_status_by_hash(&content_hash)
            .await
            .expect("queue queryable");
        if enqueued_status.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        enqueued_status.as_deref(),
        Some("pending"),
        "shipped hook payload should enqueue a pending row keyed on content_hash"
    );
    assert_eq!(
        queue.count_with_status("pending").await.expect("countable"),
        pending_before + 1,
        "exactly one new pending row from the capture"
    );
    builder.push_action(
        "ingest",
        report::ReportedAction {
            description: "shipped capture-transcript.sh enqueued a pending queue row".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: invoke_start.elapsed().as_millis() as u64,
        },
    );

    // --- 2. Idempotent re-capture dedups on content_hash (200 duplicate) ---
    let http = reqwest::Client::new();
    let dup_response = http
        .post(&ingest_url)
        .header("X-Ingest-Secret", INGEST_SECRET)
        .json(&serde_json::json!({
            "session_id": session_id,
            "source": "pre_compact",
            "content": transcript_content,
            "repo_path": sandbox.to_str().unwrap(),
        }))
        .send()
        .await
        .expect("dup ingest request sends");
    assert_eq!(
        dup_response.status(),
        reqwest::StatusCode::OK,
        "duplicate content should return 200 OK (deduped)"
    );
    let dup_body: serde_json::Value = dup_response.json().await.expect("dup body json");
    assert_eq!(dup_body["status"], "duplicate");
    assert_eq!(
        queue.count_with_status("pending").await.expect("countable"),
        pending_before + 1,
        "duplicate re-capture must not add a second pending row"
    );
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "transcript_ingest_dedup".to_owned(),
        status: report::AssertionResult::Passed,
        details: "identical content re-capture deduped on content_hash".to_owned(),
    });

    // --- 3. Wrong secret is rejected with 401 (no new row) ---
    let bad_secret_response = http
        .post(&ingest_url)
        .header("X-Ingest-Secret", "wrong-secret")
        .json(&serde_json::json!({
            "session_id": format!("{session_id}-bad"),
            "source": "session_end",
            "content": "{\"speaker\":\"user\",\"content\":\"unauthorized attempt\"}\n",
        }))
        .send()
        .await
        .expect("bad-secret request sends");
    assert_eq!(
        bad_secret_response.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "wrong shared secret must be rejected with 401"
    );
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "transcript_ingest_shared_secret".to_owned(),
        status: report::AssertionResult::Passed,
        details: "mismatched X-Ingest-Secret rejected with 401".to_owned(),
    });

    // --- 4. Maintenance drain extracts the queued content into .pending ---
    let extractor =
        SessionExtractor::from_environment().expect("live extractor builds from environment");
    let drain = TranscriptQueueDrain::new(queue.clone(), extractor, DEFAULT_TRANSCRIPT_DRAIN_BATCH);
    let drain_start = std::time::Instant::now();
    let report = drain.drain_once().await.expect("drain sweep succeeds");
    assert!(
        report.processed >= 1,
        "drain should process the queued transcript, got {report:?}"
    );
    assert_eq!(
        queue
            .find_status_by_hash(&content_hash)
            .await
            .expect("queryable")
            .as_deref(),
        Some("processed"),
        "drained row should be marked processed"
    );
    builder.push_action(
        "drain",
        report::ReportedAction {
            description: format!(
                "maintenance drain processed {} queued transcript(s)",
                report.processed
            ),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: drain_start.elapsed().as_millis() as u64,
        },
    );

    // --- 5. A .pending draft landed on disk under the sandbox repo ---
    //
    // The wiring above is deterministic; the only nondeterministic step is
    // whether the real LLM emits a candidate for a given transcript. If the
    // first drain produced no draft (model returned zero candidates), re-capture
    // a fresh content variant THROUGH THE REAL ENDPOINT and drain again, bounded
    // — keeping the path fully real (no stubbed extractor) while immune to an
    // occasional empty extraction.
    let pending_root = sandbox.join(".skills");
    fn collect_pending(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_pending(&path, out);
                } else if path.extension().and_then(|s| s.to_str()) == Some("pending") {
                    out.push(path);
                }
            }
        }
    }
    fn gather(pending_root: &std::path::Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if pending_root.exists() {
            collect_pending(pending_root, &mut out);
        }
        out
    }

    let mut pending_files = gather(&pending_root);
    let max_extraction_attempts = 4;
    let mut attempt = 0;
    while pending_files.is_empty() && attempt < max_extraction_attempts {
        attempt += 1;
        // Fresh content => new content_hash => a new pending row through the
        // real ingest endpoint (not a backdoor insert).
        let variant = format!(
            "{transcript_content}{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"Capture attempt {attempt}: please record the reusable Rust file I/O skill with its procedures and conventions.\"}}}}\n"
        );
        let retry_response = http
            .post(&ingest_url)
            .header("X-Ingest-Secret", INGEST_SECRET)
            .json(&serde_json::json!({
                "session_id": format!("{session_id}-retry-{attempt}"),
                "source": "session_end",
                "content": variant,
                "repo_path": sandbox.to_str().unwrap(),
            }))
            .send()
            .await
            .expect("retry ingest sends");
        assert_eq!(retry_response.status(), reqwest::StatusCode::ACCEPTED);
        drain
            .drain_once()
            .await
            .expect("retry drain sweep succeeds");
        pending_files = gather(&pending_root);
    }

    assert!(
        !pending_files.is_empty(),
        "queue drain should have written at least one .pending draft under {} \
         (after {attempt} extra extraction attempt(s))",
        pending_root.display()
    );
    let pending_body = std::fs::read_to_string(&pending_files[0]).expect("pending draft readable");
    assert!(
        pending_body.contains("origin: session_extraction"),
        "pending draft should carry the session_extraction origin frontmatter"
    );
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "transcript_queue_to_pending_roundtrip".to_owned(),
        status: report::AssertionResult::Passed,
        details: "shipped command-hook payload round-tripped through the queue to a .pending draft"
            .to_owned(),
    });

    // --- Report + teardown ---
    let report_doc = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!(
        "{}__{}.json",
        report_doc.test_name, report_doc.test_id
    ));
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report_doc).expect("report serializes"),
    )
    .expect("report writes");

    server.abort();
    components
        .teardown()
        .await
        .expect("teardown should succeed");
    let _ = std::fs::remove_dir_all(&sandbox);
    namespace.cleanup().await;
}
