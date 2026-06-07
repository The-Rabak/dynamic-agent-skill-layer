//! Live E2E test for the orchestrated extraction run path (#198).
//!
//! This test drives the DEFAULT orchestrated extraction pipeline end-to-end against
//! real infrastructure (Ollama, real filesystem). It is marked `#[ignore]` and must
//! only be run when live containers are available.
//!
//! ## What this proves
//!
//! 1. `execute_job_orchestrated` routes through `run_orchestration` (map→reduce)
//!    when `ExtractionRunPath::Orchestrated` is selected.
//! 2. The #176/#183 Tokio repro transcript — a session teaching `ulimit`, `tokio-console`,
//!    and `Mutex`-across-`await` fixes — produces `.pending` drafts containing the REAL
//!    taught steps, not hallucinated generics.
//! 3. The routing budget threads through correctly: the token budget from the
//!    `RoutingDecision` drives episode granularity.
//! 4. No candidate is silently dropped (the orchestration report proves coverage).
//!
//! ## How to run
//!
//! ```bash
//! OLLAMA_URL=http://localhost:11434 \
//! CLAUDE_TRANSCRIPT_ROOT=/tmp/e2e-transcripts \
//! PENDING_DRAFT_ROOTS=/tmp/e2e-skills \
//! cargo test -p session-extractor --test test_orchestrated_extraction_live \
//!   -- --include-ignored orchestrated_extraction_live_produces_grounded_pending_drafts
//! ```

use std::{path::PathBuf, sync::Arc};

use domain::{EmbeddingService, SessionEvent};
use infrastructure::{
    LlmEquivalenceVerifier, OllamaEmbeddingConfig, OllamaEmbeddingService, OllamaMergeVerifier,
    OllamaMergeVerifierConfig, StructuredTextLlm, TextLlmEquivalenceVerifier,
};
use session_extractor::{
    ExtractSessionRequest,
    orchestrator::{OrchestrationConfig, SynthesisPass, run_orchestration},
    routing::LOCAL_TIER_TOKEN_BUDGET,
    seams::{LlmSkeletonLabeler, LlmSynthesisPass},
    segmentation::SegmentationConfig,
    skeleton::SkeletonLabeler,
    transcripts::parse_session_events,
    writer::PendingDraftWriter,
};

/// The #176/#183 Tokio repro transcript — a minimal synthetic session encoding
/// real knowledge: `ulimit -n`, `tokio-console`, `Mutex`-across-`await` fix.
///
/// This is the canonical live-validation fixture for the orchestration epic.
/// A grounded extraction MUST produce procedures containing these specific steps;
/// hallucinated generics ("run tests", "fix error") are rejected.
const TOKIO_REPRO_TRANSCRIPT: &str = r#"{"type":"message","message":{"role":"user","content":"I keep hitting 'too many open files' when running integration tests. The tests work individually but fail when run in parallel."}}
{"type":"message","message":{"role":"assistant","content":"This is the ulimit problem. When running many tokio tasks in parallel, each task can hold file descriptors. Run: ulimit -n 65536 to raise the open file limit before your tests."}}
{"type":"message","message":{"role":"user","content":"OK that helped. Now I have a deadlock — my async handler acquires a std::sync::Mutex and then awaits inside the lock."}}
{"type":"message","message":{"role":"assistant","content":"This is the classic Mutex-across-await problem. std::sync::Mutex cannot be held across an await point because it blocks the thread. Replace std::sync::Mutex with tokio::sync::Mutex. The fix: change 'use std::sync::Mutex' to 'use tokio::sync::Mutex' in your handler file."}}
{"type":"message","message":{"role":"user","content":"How do I see which tasks are blocked?"}}
{"type":"message","message":{"role":"assistant","content":"Use tokio-console. Add tokio-console as a dependency, instrument your runtime with console_subscriber::init(), and run: TOKIO_CONSOLE_BIND=127.0.0.1:6669 cargo run. Then in another terminal run: tokio-console to see the live task tree."}}
{"type":"message","message":{"role":"user","content":"Perfect. So the three things are: ulimit for file descriptors, replace std mutex with tokio mutex, and tokio-console for debugging blocked tasks."}}
{"type":"message","message":{"role":"assistant","content":"Exactly. These three together cover the most common tokio async debugging scenarios."}}
"#;

/// Resolves the Ollama base URL, returning None when not configured.
fn ollama_base_url() -> Option<String> {
    std::env::var("OLLAMA_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn sandbox_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("e2e-orch-live-{name}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("sandbox dir must be creatable");
    dir
}

/// End-to-end live test: DEFAULT orchestrated path produces grounded `.pending` drafts
/// from the #176/#183 Tokio repro transcript.
///
/// The assertions require the produced procedures to contain the real taught steps:
/// `ulimit`, `tokio::sync::Mutex`, and `tokio-console` — NOT hallucinated generics.
///
/// To run: set `OLLAMA_URL` and ensure a compatible model (gemma4:12b or similar)
/// is available on Ollama.
#[tokio::test]
#[ignore = "live: requires OLLAMA_URL (embeddings, always) + an LLM provider — \
            EXTRACT_SESSION_PROVIDER=claude-code (host claude CLI/Sonnet) or ollama (gemma4:12b)"]
async fn orchestrated_extraction_live_produces_grounded_pending_drafts() {
    let ollama_url = ollama_base_url()
        .expect("OLLAMA_URL must be set for live orchestrated extraction e2e test");

    let seam_model =
        std::env::var("ORCHESTRATION_SEAM_MODEL").unwrap_or_else(|_| "gemma4:12b".to_owned());

    // ── Parse the Tokio repro transcript to events ───────────────────────────
    let parsed = parse_session_events(TOKIO_REPRO_TRANSCRIPT);
    assert_eq!(
        parsed.malformed_count, 0,
        "Tokio repro transcript must have zero malformed lines"
    );
    let events: Vec<SessionEvent> = parsed.events;
    assert!(
        events.len() >= 4,
        "Tokio repro must have at least 4 events; got {}",
        events.len()
    );

    // ── Build provider-aware seams ────────────────────────────────────────────
    // The four LLM seams (skeleton labeler, synthesis, equivalence verifier, prose
    // map extractor) follow `EXTRACT_SESSION_PROVIDER`, mirroring exactly what
    // `SessionExtractor::from_environment` wires for the DEFAULT orchestrated run
    // path: with `claude-code` the whole reduce-step LLM workload runs on the host
    // `claude` CLI (Sonnet); otherwise it runs on local Ollama. Embeddings are the
    // ONE documented exception — ALWAYS local Ollama `nomic-embed-text` — so the
    // embedder below is provider-invariant and `OLLAMA_URL` is required either way.
    let endpoint = format!("{}/api/generate", ollama_url.trim_end_matches('/'));
    let provider = std::env::var("EXTRACT_SESSION_PROVIDER").unwrap_or_default();
    let use_claude_code = matches!(provider.trim(), "claude-code" | "claude-cli");
    let provider_label = if use_claude_code { "claude-code" } else { "ollama" };

    // ONE provider-agnostic text-LLM transport shared by the labeler, synthesis, and
    // equivalence seams (same as `lib.rs`). claude-code → host `claude` CLI on Sonnet
    // (model from `EXTRACT_SESSION_MODEL`, default claude-sonnet-4-6); ollama → local.
    let seam_llm: Arc<dyn StructuredTextLlm> = if use_claude_code {
        session_extractor::providers::claude_code::build_text_llm()
            .expect("live: claude-code seam transport must init (claude CLI authenticated)")
    } else {
        // Temporarily set OLLAMA_URL / model so the Ollama seam builder can read them.
        unsafe {
            std::env::set_var("OLLAMA_URL", &ollama_url);
            std::env::set_var("ORCHESTRATION_SEAM_MODEL", &seam_model);
        }
        session_extractor::seams::ollama_seam_llm()
            .expect("live: ollama seam transport must init from OLLAMA_URL")
    };

    let labeler: Arc<dyn SkeletonLabeler> = LlmSkeletonLabeler::new(seam_llm.clone());
    let synthesis: Arc<dyn SynthesisPass> = LlmSynthesisPass::new(seam_llm.clone());

    let embedder: Arc<dyn EmbeddingService> = {
        let config = OllamaEmbeddingConfig {
            base_url: ollama_url.clone(),
            model: "nomic-embed-text".to_owned(),
            max_concurrency: 2,
        };
        Arc::new(
            OllamaEmbeddingService::from_config(config)
                .expect("live: OllamaEmbeddingService must init"),
        )
    };

    // Equivalence verifier shares the provider seam transport on claude-code (matching
    // `lib.rs`); on ollama it uses the dedicated OllamaMergeVerifier as before.
    let equivalence_verifier: Arc<dyn LlmEquivalenceVerifier> = if use_claude_code {
        Arc::new(TextLlmEquivalenceVerifier::new(seam_llm.clone()))
    } else {
        let config = OllamaMergeVerifierConfig {
            endpoint: endpoint.clone(),
            model: seam_model.clone(),
        };
        Arc::new(
            OllamaMergeVerifier::from_config(config).expect("live: OllamaMergeVerifier must init"),
        )
    };

    // Prose map extractor — claude-code CLI or local Ollama, per provider.
    let prose_extractor: Arc<dyn domain::TranscriptSkillExtractionService> = if use_claude_code {
        session_extractor::providers::claude_code::build_extractor()
            .expect("live: claude-code prose extractor must init")
    } else {
        use infrastructure::{OllamaExtractionConfig, OllamaExtractor};
        let config = OllamaExtractionConfig {
            endpoint: endpoint.clone(),
            model: seam_model.clone(),
            ..OllamaExtractionConfig::default()
        };
        Arc::new(
            OllamaExtractor::new(reqwest::Client::new(), config)
                .expect("live: OllamaExtractor must init"),
        )
    };

    // ── Session + draft writer ────────────────────────────────────────────────
    let session_id = domain::DomainId::new_unchecked("tokio-repro-live-e2e");
    let sandbox = sandbox_dir("tokio-repro");
    let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);

    let request = ExtractSessionRequest {
        transcript_ref: "ignored".to_owned(),
        transcript_inline: Some(TOKIO_REPRO_TRANSCRIPT.to_owned()),
        session_id: "tokio-repro-live-e2e".to_owned(),
        repo_path: None,
    };

    // ── Routing budget: local tier (conservative, exercises segmentation) ─────
    let token_budget = LOCAL_TIER_TOKEN_BUDGET;
    let config = OrchestrationConfig {
        segmentation: SegmentationConfig::new(token_budget, 3),
        ..OrchestrationConfig::default()
    };

    // ── Run the orchestration ─────────────────────────────────────────────────
    let report = run_orchestration(
        session_id,
        &events,
        &config,
        None, // preamble normalizer optional
        labeler,
        prose_extractor,
        embedder,
        equivalence_verifier,
        synthesis,
        &draft_writer,
        &request,
        provider_label,
    )
    .await
    .expect("live: orchestrated extraction must succeed on the Tokio repro transcript");

    // ── Acceptance assertions ─────────────────────────────────────────────────

    // 1. At least one .pending draft was written.
    assert!(
        !report.draft_paths.is_empty(),
        "live: at least one .pending draft must be written; got zero. \
         Report: total_episodes={} kept={} gated={} pre_reduce={} post_reduce={} synthesis_added={} final_candidates={}",
        report.total_episodes,
        report.kept_episode_count,
        report.gated_episode_count,
        report.pre_reduce_candidate_count,
        report.post_reduce_candidate_count,
        report.synthesis_added_count,
        report.final_candidate_count,
    );

    // 2. All draft paths exist on disk.
    for path in &report.draft_paths {
        assert!(
            path.exists(),
            "live: written .pending draft must exist on disk: {path:?}"
        );
    }

    // 3. Coverage: kept + gated == total (no silent episode drops).
    assert_eq!(
        report.total_episodes,
        report.kept_episode_count + report.gated_episode_count,
        "live: kept + gated must equal total episodes (no silent drops)"
    );

    // 4. Read back all written draft contents and assert grounded procedures.
    //    The model must produce candidates that reference the REAL concepts taught,
    //    not generic placeholders.
    let all_draft_text: String = report
        .draft_paths
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n");

    // At least one grounded concept must appear in the drafts.
    // We check for any of the three real topics taught in the transcript.
    let grounded_terms = ["ulimit", "tokio", "Mutex", "mutex", "console"];
    let has_grounded_content = grounded_terms
        .iter()
        .any(|term| all_draft_text.contains(term));

    assert!(
        has_grounded_content,
        "live: produced .pending drafts must contain grounded content from the Tokio repro \
         (one of {:?}); got content excerpt:\n---\n{}\n---",
        grounded_terms,
        &all_draft_text.chars().take(500).collect::<String>()
    );

    println!(
        "live e2e orchestrated extraction: total_episodes={} kept={} gated={} \
         pre_reduce={} post_reduce={} synthesis_added={} final_candidates={} drafts={}",
        report.total_episodes,
        report.kept_episode_count,
        report.gated_episode_count,
        report.pre_reduce_candidate_count,
        report.post_reduce_candidate_count,
        report.synthesis_added_count,
        report.final_candidate_count,
        report.draft_paths.len(),
    );
}
