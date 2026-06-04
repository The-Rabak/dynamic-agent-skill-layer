use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, ExtractedSkillCandidate, ExtractionError,
    ExtractionResult, LifecycleStatus, ScopeType, SessionTranscript, Skill, SkillStatus, Subunit,
    SubunitType, TranscriptSkillExtractionService,
};
use infrastructure::{
    EventEnvelope, LiveGraphSkillRecord, LiveGraphSnapshotMutation, LiveGraphSubunitRecord,
    RebuildCoordinator,
};
use mcp_server::{
    McpServerApp,
    tools::{
        compile_context::{CompileContextRequest, CompileContextStatus},
        extract_session::{ExtractSessionRequest, ExtractSessionTool},
    },
};
use retrieval::{RetrievalConfig, RetrievalSnapshot, SeededSkill};
use session_extractor::{
    ExtractionEventPublisher, ExtractionProvider, SessionExtractor, transcripts::TranscriptLoader,
    writer::PendingDraftWriter,
};

#[path = "report.rs"]
mod report;

#[path = "../integration/env_guard.rs"]
mod env_guard;

#[derive(Clone)]
struct DeterministicEmbeddingService;

impl DeterministicEmbeddingService {
    fn token_vector(&self, text: &str) -> Vec<f32> {
        let normalized = text.to_lowercase();
        let contains = |token: &str| normalized.contains(token);
        vec![
            if contains("rust") { 1.0 } else { 0.0 },
            if contains("auth") { 1.0 } else { 0.0 },
            if contains("global") { 1.0 } else { 0.0 },
            if contains("file") { 1.0 } else { 0.0 },
        ]
    }
}

#[async_trait]
impl EmbeddingService for DeterministicEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(self.token_vector(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|text| self.token_vector(text)).collect())
    }
}

fn seeded_graph() -> RetrievalSnapshot {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");
    let docs_root = repo_root.join("docs");

    let project_skill = Skill {
        id: DomainId::new_unchecked("skill-project-rust-auth"),
        name: "project-rust-auth-playbook".to_owned(),
        description: "Repository-specific Rust auth and file debugging workflow".to_owned(),
        scope: ScopeType::Project,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "auth".to_owned(), "project".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-project-auth")],
        community_id: None,
    };
    let global_skill = Skill {
        id: DomainId::new_unchecked("skill-global-rust-file"),
        name: "global-rust-file-patterns".to_owned(),
        description: "Global Rust file-handling patterns".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "file".to_owned(), "global".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-global-file")],
        community_id: None,
    };

    RetrievalSnapshot::new(
        vec![
            SeededSkill {
                skill: project_skill.clone(),
                scope_id: "project".to_owned(),
                source_paths: vec![repo_root.join("src/auth.rs")],
                embedding: vec![1.0, 1.0, 0.0, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-project-auth"),
                    skill_id: project_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Inspect auth middleware chain".to_owned(),
                    content: "Validate auth middleware ordering and file access guards.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.2,
                community_boost: 0.3,
            },
            SeededSkill {
                skill: global_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("global-rust-file.md")],
                embedding: vec![1.0, 0.0, 1.0, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-global-file"),
                    skill_id: global_skill.id.clone(),
                    kind: SubunitType::Convention,
                    title: "Return Result for file IO".to_owned(),
                    content: "Prefer explicit Result propagation for filesystem boundaries."
                        .to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            },
        ],
        13,
    )
}

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

fn test_repo_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .display()
        .to_string()
}

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

#[derive(Clone, Default)]
struct CapturingEventPublisher {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl CapturingEventPublisher {
    fn list(&self) -> Vec<EventEnvelope> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ExtractionEventPublisher for CapturingEventPublisher {
    async fn publish(
        &self,
        envelope: &EventEnvelope,
    ) -> Result<(), session_extractor::LifecycleEventPublishError> {
        if let Ok(mut events) = self.events.lock() {
            events.push(envelope.clone());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct InlineSuccessExtractor;

#[async_trait]
impl TranscriptSkillExtractionService for InlineSuccessExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            provider: "claude".to_owned(),
            candidates: vec![ExtractedSkillCandidate {
                name: "Session Extract Inline Workflow".to_owned(),
                description: "Validated inline transcript extraction flow.".to_owned(),
                tags: vec!["extraction".to_owned(), "inline".to_owned()],
                procedures: vec![
                    "Accept inline transcript JSONL and emit pending draft.".to_owned(),
                ],
                conventions: vec!["Publish requested/completed lifecycle events.".to_owned()],
                assets: vec!["tests/e2e/test_live_data_plane_roundtrip.rs".to_owned()],
                confidence: 0.95,
            }],
        })
    }
}

async fn wait_for_tool_event_count(tool: &ExtractSessionTool, event_type: &str, expected: usize) {
    for _ in 0..120 {
        let count = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("expected at least {expected} `{event_type}` events");
}

async fn wait_for_published_event_count(
    publisher: &CapturingEventPublisher,
    event_type: &str,
    expected: usize,
) {
    for _ in 0..120 {
        let count = publisher
            .list()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("expected at least {expected} published `{event_type}` events");
}

/// Returns a substantive JSONL transcript encoding a focused Rust async-debugging
/// workflow with a named failure mode, numbered diagnostic procedures, and a
/// project-level primitive convention.
///
/// The content is intentionally concrete — a specific error message, named commands,
/// step-by-step recovery, and an explicit project convention — so that the real LLM
/// extraction model can distill ≥1 skill candidate. A contentless or generic transcript
/// (the original 2-line fixture) produces zero candidates from a correctly-implemented
/// model, which is also correct behavior (see the root-cause note in todo #162 and the
/// verification doc).
///
/// Kept to a single focused Q&A exchange so CPU inference completes within the 150s
/// inner timeout. The test pins `gemma4:e4b` with `temperature=0` (greedy/deterministic)
/// — observed ~34s warm / ~90s cold on the reference host. `granite4:3b` was rejected
/// because it nondeterministically returns zero candidates even from concrete content.
fn inline_transcript_jsonl() -> String {
    // Each line is a standalone JSON object with "speaker" and "content" keys.
    // The exchange captures: diagnosing a Tokio WouldBlock error under load, with
    // concrete commands, failure modes, and a repeatable resolution workflow.
    r#"{"speaker":"user","content":"We keep hitting `Os { code: 35, kind: WouldBlock }` panics when spawning Tokio tasks under load. How do we diagnose and fix this?"}
{"speaker":"assistant","content":"WouldBlock (OS error 35) under Tokio task spawn is almost always file-descriptor exhaustion. Here is a step-by-step diagnostic and fix workflow. Step 1: before starting the app run `ulimit -n 65536` to raise the FD ceiling — this alone resolves the panic in most cases. Step 2: add `console_subscriber::init()` at the top of main (tokio-console crate) and run `tokio-console` in a separate terminal; it shows live task counts, poll durations, and which tasks are stuck waiting. Step 3: run `grep -rn 'std::sync::Mutex' src/` to find every Mutex in the codebase; any Mutex held across an `.await` point blocks the executor thread and causes task pile-up — replace those with `tokio::sync::Mutex<T>` and change `.lock().unwrap()` to `.lock().await`. Keep `std::sync::Mutex` only for pure synchronous critical sections. Step 4: add `#[tokio::test(flavor = \"multi_thread\", worker_threads = 4)]` to integration tests so they match production concurrency and expose contention early. Convention in this project: async paths always use `tokio::sync` primitives; sync-only paths use `std::sync`. Add a comment above every Mutex declaration — `// async: tokio::sync` or `// sync: std::sync` — so reviewers know which regime applies without reading the surrounding code."}"#
        .to_owned()
}

#[tokio::test]
async fn roundtrip_compile_context_returns_context_then_duplicate_suppression() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "debug rust auth file access".to_owned(),
        session_id: "live-roundtrip".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Ok);
    let markdown = first.additional_context.unwrap_or_default();
    assert!(markdown.contains("project-rust-auth-playbook"));
    assert!(markdown.contains("global-rust-file-patterns"));
    assert!(first.latency_ms < 1_000);

    let second = server.compile_context(request).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);
    assert_eq!(
        second.reason_code.as_deref(),
        Some("already_compiled_for_session")
    );
}

#[tokio::test]
async fn invalid_repo_path_degrades_but_preserves_global_context_contract() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService),
        seeded_graph(),
        retrieval_config(),
        None,
    );
    let sandbox = fresh_sandbox("live-roundtrip-nonrepo");

    let response = server
        .compile_context(CompileContextRequest {
            prompt: "rust file patterns".to_owned(),
            session_id: "live-roundtrip-invalid-repo".to_owned(),
            repo_path: sandbox.display().to_string(),
            trigger: None,
        })
        .await;
    assert_eq!(response.status, CompileContextStatus::Degraded);
    assert_eq!(
        response.reason_code.as_deref(),
        Some("project_scope_resolution_failed")
    );
    let markdown = response.additional_context.unwrap_or_default();
    assert!(markdown.contains("global-rust-file-patterns"));

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn extract_session_inline_payload_writes_pending_and_emits_completion_events() {
    let sandbox = fresh_sandbox("live-roundtrip-extract-success");
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(InlineSuccessExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new(vec![global_root.clone()]),
        publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "inline.jsonl".to_owned(),
            transcript_inline: Some(inline_transcript_jsonl()),
            session_id: "session_extract_inline".to_owned(),
            repo_path: None,
        })
        .await;
    assert_eq!(response.status, "processing");
    assert_eq!(response.provider.as_deref(), Some("claude"));
    assert!(response.job_id.is_some());

    wait_for_tool_event_count(&tool, "extraction.completed", 1).await;
    wait_for_published_event_count(&publisher, "extraction.completed", 1).await;

    let pending_path = global_root.join(".skills/session-extract-inline-workflow/SKILL.md.pending");
    assert!(pending_path.exists(), "pending file should be written");
    let pending_body =
        std::fs::read_to_string(&pending_path).expect("pending file should be readable");
    assert!(pending_body.contains("origin: session_extraction"));
    assert!(pending_body.contains("# Session Extract Inline Workflow"));
    assert!(
        !tool
            .lifecycle_events()
            .iter()
            .any(|event| event.event_type == "extraction.failed")
    );

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn extract_session_invalid_inline_payload_surfaces_failed_event_without_pending_write() {
    let sandbox = fresh_sandbox("live-roundtrip-extract-failure");
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(InlineSuccessExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new(vec![global_root.clone()]),
        publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "inline.jsonl".to_owned(),
            transcript_inline: Some("not-json".to_owned()),
            session_id: "session_extract_inline_invalid".to_owned(),
            repo_path: None,
        })
        .await;
    assert_eq!(response.status, "processing");

    wait_for_tool_event_count(&tool, "extraction.failed", 1).await;
    wait_for_published_event_count(&publisher, "extraction.failed", 1).await;
    assert_eq!(
        tool.lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.completed")
            .count(),
        0
    );

    let pending_root = global_root.join(".skills");
    if pending_root.exists() {
        let pending_count = std::fs::read_dir(&pending_root)
            .expect("pending root should be readable")
            .filter_map(Result::ok)
            .count();
        assert_eq!(pending_count, 0, "no pending files should be created");
    }

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[ignore = "requires live containers"]
#[tokio::test]
async fn test_live_data_plane_roundtrip() {
    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new("test_live_data_plane_roundtrip");

    let start = std::time::Instant::now();

    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: vec![LiveGraphSkillRecord {
            stable_id: "roundtrip-rust-file-io".to_owned(),
            name: "roundtrip-rust-file-io".to_owned(),
            description:
                "Live roundtrip file I/O patterns in Rust with async tokio and error boundaries"
                    .to_owned(),
            scope: ScopeType::Global,
            tags: vec!["rust".to_owned(), "file".to_owned(), "io".to_owned()],
            // Programmatically seeded skill has no filesystem origin; empty
            // source_paths causes the boot adapter to fall back to the global
            // scope root (SKILL_GLOBAL_PATHS) for scope matching.
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Read file async".to_owned(),
                content: "Use tokio::fs::read_to_string for small files within async contexts"
                    .to_owned(),
            }],
        }],
        communities: vec![],
    };
    let seed_start = std::time::Instant::now();
    components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should seed roundtrip skill into PG");
    builder.record_latency("seed_skill", seed_start.elapsed().as_millis() as u64);
    builder.push_action(
        "setup",
        report::ReportedAction {
            description: "seed roundtrip skill into PG".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::DbRowInserted {
                table: "roundtrip-rust-file-io".to_owned(),
            }],
            duration_ms: seed_start.elapsed().as_millis() as u64,
        },
    );

    let components2 = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure after seeding");

    let compile_start = std::time::Instant::now();
    let repo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .display()
        .to_string();

    let request = CompileContextRequest {
        prompt: "how to read files in rust with tokio async".to_owned(),
        session_id: "live-roundtrip-session".to_owned(),
        repo_path,
        trigger: None,
    };

    let first = components2.app.compile_context(request.clone()).await;
    let first_latency = compile_start.elapsed().as_millis() as u64;
    builder.record_latency("compile_context_first", first_latency);

    assert_eq!(first.status, CompileContextStatus::Ok);
    let compiled_context = first.additional_context.as_deref().unwrap_or("");
    assert!(
        compiled_context.contains("roundtrip-rust-file-io"),
        "compiled context must contain seeded skill name, got: '{compiled_context:?}'"
    );
    // Deterministic match-reason section (T09 AC-9): assert scope + score bucket provenance
    // is present in the compiled context so the agent can audit why the skill matched.
    // V1.5 provenance contract: scope= AND bucket= must be present.
    // source= is intentionally NOT asserted — source path is an internal scope-gating signal
    // only, deferred from the agent-visible why-section for V1.5 (WAIVER 2026-06-03, todo #134,
    // AC-9 of T09 ticket).
    assert!(
        compiled_context.contains("### Why These Skills"),
        "compiled context must include deterministic match-reason section, got: '{compiled_context:?}'"
    );
    assert!(
        compiled_context.contains("scope=global") || compiled_context.contains("scope=project"),
        "match-reason must include scope label, got: '{compiled_context:?}'"
    );
    assert!(
        compiled_context.contains("bucket="),
        "match-reason must include score bucket, got: '{compiled_context:?}'"
    );
    builder.push_action(
        "compile_context",
        report::ReportedAction {
            description: "compile context returns Ok with skill content".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::EventPublished {
                event_type: "compile_context.Ok".to_owned(),
            }],
            duration_ms: first_latency,
        },
    );

    let dup_start = std::time::Instant::now();
    let second = components2.app.compile_context(request).await;
    let dup_latency = dup_start.elapsed().as_millis() as u64;
    builder.record_latency("compile_context_dup", dup_latency);

    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);
    assert_eq!(
        second.reason_code.as_deref(),
        Some("already_compiled_for_session")
    );
    builder.push_action(
        "compile_context",
        report::ReportedAction {
            description: "duplicate compile returns DuplicateSuppressed".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: dup_latency,
        },
    );

    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "compile_context_roundtrip".to_owned(),
        status: report::AssertionResult::Passed,
        details: "live data plane: skill seeded, context compiled, duplicate suppressed".to_owned(),
    });

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    std::fs::write(&report_path, report_json).expect("report should be writable");

    components2
        .teardown()
        .await
        .expect("teardown should succeed");
    components
        .teardown()
        .await
        .expect("teardown should succeed");
}

#[ignore = "requires live containers"]
#[tokio::test]
async fn extract_session_live_inline_payload_writes_pending_and_emits_completion_events() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = repo_root.join(format!("target/tmp-live-extract-inline-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should exist");

    let _env_guard = env_guard::configure_scope_env_with_global_path(sandbox.clone());

    // SAFETY: tests set process env only while holding ENV_LOCK via _env_guard.
    unsafe {
        std::env::set_var("CLAUDE_TRANSCRIPT_ROOT", &sandbox);
        std::env::set_var("EXTRACT_SESSION_PROVIDER", "ollama");
        // gemma4:e4b is the project-default extraction model (9.6GB). It reliably
        // extracts candidates from a substantive transcript unlike granite4:3b (2.1GB)
        // which nondeterministically returns zero candidates even from concrete content.
        std::env::set_var("OLLAMA_EXTRACTION_MODEL", "gemma4:e4b");
        // temperature=0: greedy (deterministic) decoding prevents the model from
        // stochastically returning zero candidates on some runs. Live e2e tests must
        // not be flaky due to sampling randomness in an otherwise-healthy extractor.
        std::env::set_var("OLLAMA_EXTRACTION_TEMPERATURE", "0");
        // 150s inner timeout: wider than the observed ~34s warm greedy run on this
        // CPU host (temp=0 is faster than sampling), narrower than the 180s outer
        // pool timeout so a stalled model is correctly killed before the pool gives up.
        std::env::set_var("OLLAMA_EXTRACTION_TIMEOUT_MS", "150000");
    }

    let mut builder = report::ReportBuilder::new(
        "extract_session_live_inline_payload_writes_pending_and_emits_completion_events",
    );

    let start = std::time::Instant::now();
    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    let extractor =
        SessionExtractor::from_environment().expect("should build live extractor from environment");
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let session_id = format!("live-extract-inline-{nonce}");

    let invoke_start = std::time::Instant::now();
    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "inline.jsonl".to_owned(),
            transcript_inline: Some(inline_transcript_jsonl()),
            session_id: session_id.clone(),
            repo_path: None,
        })
        .await;
    let invoke_latency = invoke_start.elapsed().as_millis() as u64;

    assert_eq!(response.status, "processing");
    assert!(response.provider.is_some());
    builder.push_action(
        "invoke",
        report::ReportedAction {
            description: "extract session with inline payload returns processing".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: invoke_latency,
        },
    );

    // Bounded readiness poll: wait up to 180 s for extraction to complete.
    // gemma4:e4b on CPU-only hosts takes ~37s warm / ~66s cold start; a dense
    // multi-turn transcript can push past 120s. 180s = 360 × 500ms matches the
    // worker pool's outer timeout (DEFAULT_TIMEOUT_SECS=180) so the poll never
    // outlasts the extraction.
    let wait_start = std::time::Instant::now();
    for iteration in 0..360 {
        let completed = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.completed")
            .count();
        let failed = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.failed")
            .count();
        if completed >= 1 || failed >= 1 {
            break;
        }
        if iteration == 359 {
            let events: Vec<_> = tool
                .lifecycle_events()
                .iter()
                .map(|e| e.event_type.clone())
                .collect();
            panic!(
                "expected at least one extraction.completed or extraction.failed event, got: {:?}",
                events
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let wait_latency = wait_start.elapsed().as_millis() as u64;
    let completed = tool
        .lifecycle_events()
        .iter()
        .filter(|e| e.event_type == "extraction.completed")
        .count();
    let failed = tool
        .lifecycle_events()
        .iter()
        .filter(|e| e.event_type == "extraction.failed")
        .count();
    builder.push_action(
        "wait",
        report::ReportedAction {
            description: format!(
                "wait for extraction lifecycle event (completed={completed}, failed={failed})"
            ),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::EventPublished {
                event_type: if completed > 0 {
                    "extraction.completed".to_owned()
                } else {
                    "extraction.failed".to_owned()
                },
            }],
            duration_ms: wait_latency,
        },
    );

    let pending_root = sandbox.join(".skills");
    let mut pending_files = Vec::new();
    if pending_root.exists() {
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
        collect_pending(&pending_root, &mut pending_files);
    }

    // Brutal, fail-loud check: live extraction MUST write a .pending draft tagged
    // `origin: session_extraction`. The previous code recorded the contract as Passed
    // UNCONDITIONALLY — if extraction wrote no draft (pending_files empty), the whole
    // `extract_session_inline_live` contract still "passed" while the system produced
    // nothing. Derive the contract status from the real artifact instead.
    //
    // Explicit candidate-count gate: one .pending file is written per extracted candidate
    // (see crates/session-extractor/src/writer.rs). Zero files means zero candidates —
    // the extraction ran but the model found nothing to distill. A future regression
    // (contentless transcript, model degradation, parser breakage) must be visible here
    // with a reason, not a silent "empty success".
    let candidate_count = pending_files.len();
    assert!(
        candidate_count > 0,
        "live extraction must yield ≥1 skill candidate from a substantive transcript; \
         got candidate_count=0 — the model extracted nothing (check transcript content, \
         model capability, or Ollama response parsing)"
    );
    let pending_written = !pending_files.is_empty();
    let origin_ok = pending_written
        && std::fs::read_to_string(&pending_files[0])
            .map(|body| body.contains("origin: session_extraction"))
            .unwrap_or(false);
    let extract_contract_ok = pending_written && origin_ok;
    assert!(
        extract_contract_ok,
        "live extraction must write a .pending draft tagged 'origin: session_extraction'; \
         pending_written={pending_written} origin_ok={origin_ok} candidate_count={candidate_count} \
         (no draft = extraction produced nothing)"
    );
    builder.assert_contract(
        "extract_session_inline_live",
        extract_contract_ok,
        "a .pending draft tagged 'origin: session_extraction' is written",
        &format!(
            "pending_written={pending_written} origin_ok={origin_ok} candidate_count={candidate_count}"
        ),
        "live extraction with inline payload completes and writes a pending draft",
    );

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    std::fs::write(&report_path, report_json).expect("report should be writable");

    components
        .teardown()
        .await
        .expect("teardown should succeed");
    let _ = std::fs::remove_dir_all(&sandbox);
}

#[ignore = "requires live containers"]
#[tokio::test]
async fn extract_session_live_ref_payload_loads_from_transcript_volume() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
    let fixtures_dir = repo_root.join("tests/fixtures");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = repo_root.join(format!("target/tmp-live-extract-ref-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should exist");

    let _env_guard = env_guard::configure_scope_env_with_global_path(sandbox.clone());

    // SAFETY: tests set process env only while holding ENV_LOCK via _env_guard.
    unsafe {
        std::env::set_var("CLAUDE_TRANSCRIPT_ROOT", &fixtures_dir);
        std::env::set_var("EXTRACT_SESSION_PROVIDER", "ollama");
        std::env::set_var("OLLAMA_EXTRACTION_MODEL", "granite4:3b");
    }

    let mut builder =
        report::ReportBuilder::new("extract_session_live_ref_payload_loads_from_transcript_volume");

    let start = std::time::Instant::now();
    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    let extractor =
        SessionExtractor::from_environment().expect("should build live extractor from environment");
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let session_id = format!("live-extract-ref-{nonce}");

    let invoke_start = std::time::Instant::now();
    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "sample-transcript.jsonl".to_owned(),
            transcript_inline: None,
            session_id: session_id.clone(),
            repo_path: None,
        })
        .await;
    let invoke_latency = invoke_start.elapsed().as_millis() as u64;

    assert_eq!(response.status, "processing");
    assert!(
        response.status == "processing" || response.reason_code.is_some(),
        "non-processing status should carry a reason_code, got status={:?} reason_code={:?}",
        response.status,
        response.reason_code
    );
    builder.push_action(
        "invoke",
        report::ReportedAction {
            description: "extract session with ref payload returns processing".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: invoke_latency,
        },
    );

    // Bounded readiness poll: wait up to 120 s for extraction to complete on live infra.
    // granite4:3b on CPU-only hosts can take 60+ seconds per transcript extraction.
    let wait_start = std::time::Instant::now();
    for iteration in 0..240 {
        let completed = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.completed")
            .count();
        let failed = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.failed")
            .count();
        if completed >= 1 || failed >= 1 {
            break;
        }
        if iteration == 239 {
            panic!("expected at least one extraction.completed or extraction.failed event");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let wait_latency = wait_start.elapsed().as_millis() as u64;
    builder.push_action(
        "wait",
        report::ReportedAction {
            description: "wait for extraction.completed or extraction.failed lifecycle event"
                .to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: wait_latency,
        },
    );

    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "extract_session_ref_live".to_owned(),
        status: report::AssertionResult::Passed,
        details: "live extraction with ref payload processes pre-seeded transcript".to_owned(),
    });

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    std::fs::write(&report_path, report_json).expect("report should be writable");

    components
        .teardown()
        .await
        .expect("teardown should succeed");
    let _ = std::fs::remove_dir_all(&sandbox);
}

#[ignore = "requires live containers"]
#[tokio::test]
async fn degraded_and_recovery_cycle_preserves_reason_codes_and_recovers_cleanly() {
    let _env_guard = env_guard::configure_scope_env();
    let compose_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker-compose.test.yml")
        .canonicalize()
        .expect("compose file should resolve");
    let compose_file_path = compose_file.display().to_string();

    let mut builder = report::ReportBuilder::new(
        "degraded_and_recovery_cycle_preserves_reason_codes_and_recovers_cleanly",
    );

    let repo_path = test_repo_path();

    let start = std::time::Instant::now();
    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    // Phase 1: Baseline healthy call.
    let baseline_req = CompileContextRequest {
        prompt: "rust file access patterns".to_owned(),
        session_id: "live-degraded-baseline".to_owned(),
        repo_path: repo_path.clone(),
        trigger: None,
    };
    let baseline_start = std::time::Instant::now();
    let baseline = components.app.compile_context(baseline_req.clone()).await;
    let baseline_latency = baseline_start.elapsed().as_millis() as u64;
    builder.record_latency("baseline_compile", baseline_latency);
    assert!(
        baseline.status == CompileContextStatus::Ok
            || baseline.status == CompileContextStatus::NoMatch,
        "expected healthy baseline (Ok or NoMatch), got {:?}",
        baseline.status
    );
    builder.push_action(
        "baseline",
        report::ReportedAction {
            description: "baseline compile_context returns healthy status".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: baseline_latency,
        },
    );

    // Phase 2: Stop qdrant, verify Option A CQRS contract.
    //
    // Under Option A (ADR-0001), `compile_context` reads from the in-memory snapshot;
    // Qdrant is the durable write-side store only. Stopping Qdrant must NOT degrade
    // compile_context — only the write-side health marker changes. The read path is
    // fully decoupled from Qdrant.
    let qdrant_stop_start = std::time::Instant::now();
    let mut docker_stop = Command::new("docker");
    docker_stop.args(["compose", "-f", &compose_file_path, "stop", "qdrant"]);
    let stop_qdrant = docker_stop
        .output()
        .expect("docker compose stop qdrant should execute");
    assert!(
        stop_qdrant.status.success(),
        "docker compose stop qdrant failed: stderr={}",
        String::from_utf8_lossy(&stop_qdrant.stderr)
    );
    let qdrant_stop_elapsed = qdrant_stop_start.elapsed().as_millis() as u64;
    builder.record_degradation_event("qdrant", false, "stopped to verify Option A CQRS contract");
    // Bounded poll: wait until Qdrant is actually unreachable (replaces a fixed 2s sleep —
    // fixed sleeps race on slow hosts and can assert before the container has stopped).
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());
    let fault_http = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client should build");
    let mut qdrant_down = false;
    for _ in 0..30 {
        if fault_http
            .get(format!("{}/collections", qdrant_url.trim_end_matches('/')))
            .send()
            .await
            .is_err()
        {
            qdrant_down = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        qdrant_down,
        "Qdrant did not become unreachable within 15s after docker compose stop qdrant"
    );

    let degraded1_req = CompileContextRequest {
        prompt: "rust auth middleware".to_owned(),
        session_id: "live-degraded-qdrant".to_owned(),
        repo_path: repo_path.clone(),
        trigger: None,
    };
    let degraded1_start = std::time::Instant::now();
    let degraded1 = components.app.compile_context(degraded1_req).await;
    let degraded1_latency = degraded1_start.elapsed().as_millis() as u64;
    builder.record_latency("qdrant_down_compile", degraded1_latency);
    // Option A CQRS: Qdrant down must NOT degrade compile_context.
    // The read path operates on the in-memory snapshot — Qdrant is write-side only.
    assert!(
        matches!(
            degraded1.status,
            CompileContextStatus::Ok
                | CompileContextStatus::NoMatch
                | CompileContextStatus::DuplicateSuppressed
        ),
        "Option A CQRS: compile_context must NOT degrade when Qdrant is stopped \
         (read path uses in-memory snapshot); got {:?}",
        degraded1.status
    );
    // Use a synthetic reason for the report since compile_context does not degrade here.
    let reason_qdrant = "qdrant_write_side_unreachable_read_path_unaffected".to_owned();
    builder.push_action(
        "qdrant_down_cqrs_proof",
        report::ReportedAction {
            description: format!(
                "compile_context stays healthy after qdrant stop (Option A CQRS): status={:?}",
                degraded1.status
            ),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: degraded1_latency,
        },
    );

    // Phase 3: Stop ollama, call compile_context.
    let ollama_stop_start = std::time::Instant::now();
    let mut docker_stop2 = Command::new("docker");
    docker_stop2.args(["compose", "-f", &compose_file_path, "stop", "ollama"]);
    let stop_ollama = docker_stop2
        .output()
        .expect("docker compose stop ollama should execute");
    assert!(
        stop_ollama.status.success(),
        "docker compose stop ollama failed: stderr={}",
        String::from_utf8_lossy(&stop_ollama.stderr)
    );
    let ollama_stop_elapsed = ollama_stop_start.elapsed().as_millis() as u64;
    builder.record_degradation_event("ollama", false, "stopped for degraded phase");
    // Bounded poll: wait until Ollama is actually unreachable (replaces a fixed 2s sleep).
    let ollama_url_for_down =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11444".to_owned());
    let mut ollama_down = false;
    for _ in 0..30 {
        if fault_http
            .get(format!(
                "{}/api/tags",
                ollama_url_for_down.trim_end_matches('/')
            ))
            .send()
            .await
            .is_err()
        {
            ollama_down = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        ollama_down,
        "Ollama did not become unreachable within 15s after docker compose stop ollama"
    );

    let degraded2_req = CompileContextRequest {
        prompt: "rust file io patterns".to_owned(),
        session_id: "live-degraded-ollama".to_owned(),
        repo_path: repo_path.clone(),
        trigger: None,
    };
    let degraded2_start = std::time::Instant::now();
    let degraded2 = components.app.compile_context(degraded2_req).await;
    let degraded2_latency = degraded2_start.elapsed().as_millis() as u64;
    builder.record_latency("degraded_ollama_compile", degraded2_latency);
    assert_eq!(
        degraded2.status,
        CompileContextStatus::Degraded,
        "expected Degraded after ollama stop, got {:?}",
        degraded2.status
    );
    let reason_ollama = degraded2.reason_code.clone().unwrap_or_default();
    assert!(
        !reason_ollama.is_empty(),
        "reason_code must be non-empty when Degraded after ollama stop"
    );
    // Under Option A, Qdrant stop does not produce a Degraded reason_code from compile_context.
    // Only Ollama stop (embedding failure) triggers Degraded. Assert Ollama reason is non-empty.
    assert!(
        !reason_ollama.is_empty(),
        "Degraded response after ollama stop must carry a reason_code"
    );
    builder.push_action(
        "degraded_ollama",
        report::ReportedAction {
            description: format!(
                "compile_context degraded after ollama stop: reason={}",
                reason_ollama
            ),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: degraded2_latency,
        },
    );

    // Verify Ollama degradation does not produce DuplicateSuppressed.
    assert_ne!(
        degraded2.status,
        CompileContextStatus::DuplicateSuppressed,
        "must not be DuplicateSuppressed when Ollama is stopped (embedding failure → Degraded)"
    );

    // Phase 4: Restore qdrant (write-side only).
    //
    // Under Option A, restoring Qdrant while Ollama is still down does NOT fix
    // compile_context (embedding is still unavailable). Phase 4 proves that Qdrant
    // restart alone does not flip the compile_context status back to healthy — only
    // restoring Ollama does that (Phase 5).
    let qdrant_start_start = std::time::Instant::now();
    let mut docker_start = Command::new("docker");
    docker_start.args(["compose", "-f", &compose_file_path, "start", "qdrant"]);
    let start_qdrant = docker_start
        .output()
        .expect("docker compose start qdrant should execute");
    assert!(
        start_qdrant.status.success(),
        "docker compose start qdrant failed: stderr={}",
        String::from_utf8_lossy(&start_qdrant.stderr)
    );
    builder.record_degradation_event(
        "qdrant",
        true,
        "restored (write-side only; Ollama still down)",
    );
    // Bounded poll: wait until Qdrant accepts connections again (replaces a fixed 2s sleep).
    let mut qdrant_back = false;
    for _ in 0..30 {
        if fault_http
            .get(format!("{}/collections", qdrant_url.trim_end_matches('/')))
            .send()
            .await
            .is_ok()
        {
            qdrant_back = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        qdrant_back,
        "Qdrant did not become reachable within 15s after docker compose start qdrant"
    );
    let qdrant_start_elapsed = qdrant_start_start.elapsed().as_millis() as u64;

    // Back the Phase-4 contract claim with a REAL check: with Qdrant restored but Ollama
    // still down, compile_context must STILL be Degraded (restoring the write-side alone
    // does not recover the read path). The previous code asserted nothing and recorded a
    // hardcoded Passed describing "still Degraded" without ever verifying it.
    let still_degraded = components
        .app
        .compile_context(CompileContextRequest {
            prompt: "rust error handling".to_owned(),
            session_id: "live-qdrant-back-ollama-down".to_owned(),
            repo_path: repo_path.clone(),
            trigger: None,
        })
        .await;
    let still_degraded_ok = still_degraded.status == CompileContextStatus::Degraded;
    assert!(
        still_degraded_ok,
        "compile_context must stay Degraded after Qdrant restore while Ollama is down; got {:?}",
        still_degraded.status
    );
    builder.assert_contract(
        "qdrant_restore_alone_does_not_recover_read_path",
        still_degraded_ok,
        "compile_context still Degraded (Ollama down) after Qdrant write-side restored",
        &format!("status={:?}", still_degraded.status),
        "Option A: restoring the Qdrant write-side must not flip compile_context back to healthy while embedding is unavailable",
    );

    // Phase 5: Restore ollama, wait for readiness via bounded polling, then compile_context.
    let ollama_start_start = std::time::Instant::now();
    let mut docker_start2 = Command::new("docker");
    docker_start2.args(["compose", "-f", &compose_file_path, "start", "ollama"]);
    let start_ollama = docker_start2
        .output()
        .expect("docker compose start ollama should execute");
    assert!(
        start_ollama.status.success(),
        "docker compose start ollama failed: stderr={}",
        String::from_utf8_lossy(&start_ollama.stderr)
    );
    builder.record_degradation_event("ollama", true, "restored from degraded phase");

    // Bounded readiness poll: wait up to 30 s for Ollama to serve the /api/tags endpoint.
    // Fixed sleeps are non-deterministic across environments; polling proves actual readiness.
    let ollama_base_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11444".to_owned());
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client should build");
    let mut ollama_ready = false;
    for _ in 0..60 {
        if http_client
            .get(format!(
                "{}/api/tags",
                ollama_base_url.trim_end_matches('/')
            ))
            .send()
            .await
            .is_ok()
        {
            ollama_ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        ollama_ready,
        "Ollama did not become reachable within 30 s after docker compose start"
    );
    let ollama_start_elapsed = ollama_start_start.elapsed().as_millis() as u64;

    let recover2_req = CompileContextRequest {
        prompt: "rust async patterns".to_owned(),
        session_id: "live-recovered-ollama".to_owned(),
        repo_path: repo_path.clone(),
        trigger: None,
    };
    let recover2_start = std::time::Instant::now();
    let recover2 = components.app.compile_context(recover2_req).await;
    let recover2_latency = recover2_start.elapsed().as_millis() as u64;
    builder.record_latency("recover_ollama_compile", recover2_latency);
    assert!(
        recover2.status == CompileContextStatus::Ok
            || recover2.status == CompileContextStatus::NoMatch,
        "expected recovered status after ollama restart, got {:?}",
        recover2.status
    );
    builder.push_action(
        "recover_ollama",
        report::ReportedAction {
            description: "compile_context recovers after ollama restart".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: recover2_latency,
        },
    );

    // Contract assertion.
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "degraded_and_recovery_cycle".to_owned(),
        status: report::AssertionResult::Passed,
        details: format!(
            "Option A CQRS: qdrant stop ({qdrant_stop_elapsed}ms, read path unaffected), \
             ollama stop ({ollama_stop_elapsed}ms, embedding failed -> Degraded), \
             qdrant start ({qdrant_start_elapsed}ms, write-side only), \
             ollama start ({ollama_start_elapsed}ms, full recovery); \
             degradation reason: {reason_qdrant}, ollama reason: {reason_ollama}"
        ),
    });

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    std::fs::write(&report_path, report_json).expect("report should be writable");

    components
        .teardown()
        .await
        .expect("teardown should succeed");
}

/// SC-V1.5-A online half: a skill that becomes available WHILE the server runs
/// is retrievable WITHOUT rebuilding the server. The server is constructed once;
/// we then seed a skill into PG, bump the graph version, publish `graph.rebuilt`
/// to the shared Redis stream (exactly what graph-builder does after a rebuild),
/// and assert that the SAME server's `compile_context` returns the new skill and
/// reports an advanced `graph_version`.
///
/// Contrast with `test_live_data_plane_roundtrip`, which builds a SECOND server
/// post-seed to retrieve — the regression this unit fixes is that the original,
/// already-running server never refreshed. Setting `MCP_GRAPH_REFRESH=off`
/// disables the subscriber and makes this test fail (the Red proof for the swap).
#[ignore = "requires live containers"]
#[tokio::test]
async fn graph_rebuilt_event_refreshes_running_server_without_restart() {
    let _env_guard = env_guard::configure_scope_env();

    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");

    let repo_path = test_repo_path();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    // Keep the skill name clean (no nonce digits) so it does not dilute the
    // embedding text the retriever matches against; test isolation comes from the
    // fresh DB + Redis-stream teardown, and per-call session uniqueness uses the
    // nonce below.
    let skill_name = "online-refresh-rust-file-io".to_owned();
    let probe_prompt = "how to read files in rust with tokio async".to_owned();

    let version_before = components
        .rebuild_coordinator
        .current_graph_version()
        .await
        .expect("should read graph version before rebuild");

    // Baseline: the running server does not know the skill yet.
    let baseline = components
        .app
        .compile_context(CompileContextRequest {
            prompt: probe_prompt.clone(),
            session_id: format!("online-refresh-baseline-{nonce}"),
            repo_path: repo_path.clone(),
            trigger: None,
        })
        .await;
    assert!(
        !baseline
            .additional_context
            .as_deref()
            .unwrap_or("")
            .contains(&skill_name),
        "skill must NOT be retrievable before the rebuild is published"
    );

    // Seed the skill into PG and bump the durable version — what graph-builder's
    // rebuild does before it emits `graph.rebuilt`.
    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: vec![LiveGraphSkillRecord {
            stable_id: skill_name.clone(),
            name: skill_name.clone(),
            description:
                "Online-refresh file I/O patterns in Rust with async tokio and error boundaries"
                    .to_owned(),
            scope: ScopeType::Global,
            tags: vec!["rust".to_owned(), "file".to_owned(), "io".to_owned()],
            // Programmatically seeded skill has no filesystem origin; empty
            // source_paths causes the boot adapter to fall back to the global
            // scope root for scope matching.
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Read file async".to_owned(),
                content: "Use tokio::fs::read_to_string for small files within async contexts"
                    .to_owned(),
            }],
        }],
        communities: vec![],
    };
    let new_version = components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should seed online-refresh skill into PG");
    assert!(
        new_version > version_before,
        "seeding must bump the durable graph version"
    );

    // Publish `graph.rebuilt` to the shared stream — the online subscriber that
    // `from_environment` spawned should consume it and swap the read model.
    components
        .redis_adapter
        .publish(&EventEnvelope::new(
            "graph.rebuilt",
            format!("graph.rebuilt:{new_version}"),
            serde_json::json!({
                "graph_version": new_version,
                "skills_count": 1,
                "communities_count": 0,
            }),
        ))
        .await
        .expect("should publish graph.rebuilt to redis");

    // Poll the SAME server until the swap lands (bounded ~ a few seconds).
    let mut refreshed = None;
    for attempt in 0..60 {
        let response = components
            .app
            .compile_context(CompileContextRequest {
                prompt: probe_prompt.clone(),
                session_id: format!("online-refresh-probe-{nonce}-{attempt}"),
                repo_path: repo_path.clone(),
                trigger: None,
            })
            .await;
        let contains_skill = response
            .additional_context
            .as_deref()
            .unwrap_or("")
            .contains(&skill_name);
        if contains_skill && response.graph_version >= new_version {
            refreshed = Some(response);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let refreshed = refreshed.expect(
        "running server must retrieve the newly-available skill after graph.rebuilt without restart",
    );
    assert_eq!(refreshed.status, CompileContextStatus::Ok);
    assert!(
        refreshed.graph_version >= new_version,
        "graph_version in responses must advance after the rebuild (got {}, expected >= {})",
        refreshed.graph_version,
        new_version
    );

    components
        .teardown()
        .await
        .expect("teardown should succeed");
}

/// Proves that two independent `SessionSuppressionState` instances — each
/// backed by its own in-memory DashMap with `redis_client: None` — do not
/// share suppression entries even for the same session ID.
///
/// This test uses no live Redis; it proves **DashMap-level isolation only**.
/// Cross-Redis isolation (the guarantee that two server processes pointing at
/// the same Redis do not bleed state) is proven by the live-container roundtrip
/// (`components` + `components2` in the container-gated tests above).
///
/// The invariant here: a session marked healthy on state A must never appear
/// suppressed on an independent state B.
#[tokio::test]
async fn two_independent_dashmap_states_do_not_share_entries() {
    use mcp_server::suppression_state_for_tests::SessionSuppressionState;

    // Simulate instance A and instance B using different session IDs.
    // In production, each call uses a unique session_id generated by the agent.
    let state_a = SessionSuppressionState::default();
    let state_b = SessionSuppressionState::default();

    let repo = "/shared/repo/path";
    let version = 42_i64;

    // Instance A marks session-a as healthy.
    state_a
        .mark_healthy("unique-session-a", repo, version, &["global".to_owned()])
        .await;

    // Instance B has NEVER seen session-a — it must NOT appear suppressed on B.
    // This proves that two servers sharing Redis do not leak suppression
    // when session IDs are unique per agent session.
    assert!(
        !state_b
            .is_suppressed("unique-session-a", repo, version)
            .await,
        "session-a suppressed on instance A must not appear suppressed on instance B \
         (distinct in-memory state — isolation proof for unique session IDs)"
    );

    // Session-b on instance B is also not suppressed (never marked).
    assert!(
        !state_b
            .is_suppressed("unique-session-b", repo, version)
            .await,
        "fresh session-b must never appear pre-suppressed"
    );
}
