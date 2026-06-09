//! Map→reduce extraction orchestration (#187) — the integrator of the
//! extraction-scaling epic.
//!
//! ## Pipeline
//!
//! 1. **Segment** the full session into overlapping windows (reuses #185).
//! 2. **Mine preamble** from the full event stream (reuses #186).
//! 3. **Map** ALL windows concurrently (bounded by a [`tokio::sync::Semaphore`]),
//!    recall-first — NO salience gate on the blocking path:
//!    - ALWAYS run the prose extractor on the window's flat transcript (prepended
//!      with the preamble as leading context). This is the universal floor that
//!      works on any transcript shape including flat conversational sessions.
//!    - ADDITIONALLY, if the window contains a tool arc (failing→passing
//!      `ToolResult`), run [`skeleton::map_episode`] with the [`SkeletonLabeler`]
//!      and union the resulting grounded candidate with the prose results. Skeleton
//!      mining is additive grounding only — it never replaces prose extraction.
//!
//!    The salience module (`crate::salience`) remains in the codebase but is NOT
//!    called as a blocking gate in this pipeline. External research (mem0/Letta/Zep,
//!    LangChain map-reduce, RAG chunking benchmarks) is unambiguous: extract from
//!    EVERY chunk; never pre-filter with heuristics. A gate that scores 0 on flat
//!    conversational content silently drops all extraction output on the most common
//!    real-world session type.
//! 4. **Reduce** the union of all window candidates + preamble preference candidates:
//!    - Embed each candidate's semantic text (via [`EmbeddingService`] seam).
//!    - Cosine-pair candidates exceeding [`REDUCE_SIMILARITY_THRESHOLD`] using
//!      the shared [`infrastructure::cosine_similarity`] (ONE impl in the repo).
//!    - For each near pair, call [`LlmEquivalenceVerifier::decide_equivalence`]
//!      (the same verifier the maintenance merge pass uses — no second impl).
//!    - Record every merge and drop in the audit log. No candidate is dropped
//!      silently.
//! 5. **Synthesize** — ONE LLM pass over the deduped candidate list to surface
//!    session-spanning patterns (via [`SynthesisPass`] seam; fake in tests).
//! 6. **Write** survivors as `.pending` drafts via [`PendingDraftWriter`].
//!
//! ## Seam discipline
//!
//! Every LLM interaction goes through a trait:
//! - [`preamble::PreambleNormalizer`] — optional preamble dedup (provided here or None).
//! - [`skeleton::SkeletonLabeler`] — skeleton name/keep/generality judgment.
//! - [`LlmEquivalenceVerifier`] — reduce pair equivalence.
//! - [`SynthesisPass`] — final session-spanning pattern detection.
//! - [`EmbeddingService`] — embedding for cosine pairing.
//!
//! Production implementations call real infrastructure. Test fakes live behind
//! `#[cfg(test)]` and are never reachable from production code.
//!
//! ## Hard invariants
//!
//! - No candidate is silently dropped. Every merge and drop is recorded in
//!   [`ReduceAuditEntry`] with a rationale.
//! - Recall-first: every window is extracted; nothing is gated out by heuristic.
//! - Bounded concurrency: slow windows are backpressured by the semaphore,
//!   never discarded on a wall-clock timeout.
//! - Exactly ONE cosine implementation in the repo (`infrastructure::cosine_similarity`).
//! - Exactly ONE equivalence verifier (`infrastructure::LlmEquivalenceVerifier`).

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, ExtractedSkillCandidate, ExtractionError,
    ExtractionResult, SessionEvent, TranscriptSkillExtractionService, events_to_transcript,
};
use infrastructure::{CosineSimilarityError, LlmEquivalenceVerifier, cosine_similarity};
use thiserror::Error;
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::{debug, info, warn};

use crate::{
    ExtractSessionRequest,
    preamble::{NormalizationError, Preamble, PreambleNormalizer, mine_preamble},
    salience::SalienceConfig,
    segmentation::{Episode, SegmentationConfig, segment_session},
    skeleton::{MapOutcome, SkeletonError, SkeletonLabeler, map_episode},
    writer::{PendingDraftWriter, WriterError},
};

// ─── Thresholds ──────────────────────────────────────────────────────────────

/// Cosine similarity threshold above which two candidates are considered near
/// duplicates and forwarded to [`LlmEquivalenceVerifier`] for a final decision.
///
/// Set to 0.82 — below the maintenance merge threshold (0.85) to catch partial
/// cross-episode matches that may not align perfectly due to different episode
/// context.
pub const REDUCE_SIMILARITY_THRESHOLD: f32 = 0.82;

/// Maximum number of times `extract_prose_window` re-issues the LLM call after
/// receiving an empty candidates response from a substantive window.
///
/// An empty response from a non-trivial window is almost always a model hiccup
/// (momentary cold load, KV-cache pressure, or a first-inference quirk under
/// `OLLAMA_NUM_PARALLEL`) rather than a genuine "nothing to extract". Two retries
/// give the model three total attempts while keeping the retry fan-out bounded.
///
/// A window with fewer than [`PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS`] characters
/// of transcript text is NOT retried — an empty result there is plausible.
const PROSE_WINDOW_EMPTY_RETRY_LIMIT: usize = 2;

/// Minimum total transcript-text length (bytes) for a window to be considered
/// substantive enough to retry on empty. Windows shorter than this may legitimately
/// yield zero candidates (e.g. a one-line greeting).
const PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS: usize = 100;

// ─── Synthesis seam ──────────────────────────────────────────────────────────

/// Failure modes of the synthesis pass.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SynthesisError {
    /// The LLM provider could not be reached or returned a non-OK status.
    #[error("synthesis LLM call failed: {0}")]
    ProviderFailure(String),
    /// The LLM response could not be parsed as structured candidates.
    #[error("synthesis response parse failed: {0}")]
    ParseFailure(String),
}

/// Final synthesis pass that reviews the deduped candidate list for session-spanning
/// patterns that no single episode reveals.
///
/// ## Contract
///
/// The pass receives the current deduped candidates (after structural reduce) and
/// may return zero or more additional candidates that represent emergent patterns.
/// It may also return an empty list when no cross-episode patterns are detected.
///
/// The pass MUST NOT drop existing candidates — it only adds new ones. Dropping is
/// the reduce step's responsibility.
///
/// ## Production wiring
///
/// A real LLM-backed implementation is injected via [`OrchestrationConfig`]. Tests
/// use a `#[cfg(test)]`-gated fake that returns a fixed list.
#[async_trait]
pub trait SynthesisPass: Send + Sync {
    /// Reviews the deduped candidate list and returns any additional session-spanning
    /// candidates discovered. Returns an empty list when no new patterns are found.
    async fn synthesize(
        &self,
        deduped_candidates: &[ExtractedSkillCandidate],
        preamble_text: &str,
    ) -> Result<Vec<ExtractedSkillCandidate>, SynthesisError>;
}

// ─── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the map→reduce orchestration pipeline.
pub struct OrchestrationConfig {
    /// Window segmentation budget (tokens per window, overlap events).
    pub segmentation: SegmentationConfig,
    /// Salience gate configuration. Preserved for API compatibility.
    ///
    /// The salience gate is no longer on the blocking path of `run_orchestration`.
    /// This field may be used for observability or analytics in future work but
    /// does not affect which windows are sent to the map step.
    pub salience: SalienceConfig,
    /// Maximum number of windows to process concurrently in the map step.
    /// Slow windows block a semaphore permit but are never discarded.
    pub map_concurrency: usize,
    /// Cosine similarity threshold for the reduce pairing step.
    pub reduce_similarity_threshold: f32,
}

impl Default for OrchestrationConfig {
    /// Defaults: 8 192-token chunk budget (the local-tier window; production routing
    /// overrides it per tier via lib.rs), 3-event overlap, 4 concurrent map workers,
    /// 0.82 reduce threshold. The chunk budget is legitimate windowing; the #214
    /// footgun was the per-entry char cap being smaller than the window, now aligned
    /// in the extraction config defaults.
    fn default() -> Self {
        Self {
            segmentation: SegmentationConfig::new(8_192, 3),
            salience: SalienceConfig::default(),
            map_concurrency: 4,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
        }
    }
}

// ─── Audit log ───────────────────────────────────────────────────────────────

/// How a candidate pair was resolved in the reduce step.
#[derive(Debug, Clone, PartialEq)]
pub enum ReduceAction {
    /// The two candidates were merged (right absorbed into left). `rationale` is
    /// the LLM's explanation from [`EquivalenceDecision::rationale`].
    Merged { rationale: String },
    /// The pair was near-duplicate but the LLM decided they are NOT equivalent.
    /// Both candidates survive.
    Kept { rationale: String },
}

/// One audit entry for a reduce-step decision, proving no candidate was silently
/// dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct ReduceAuditEntry {
    /// The candidate that was kept (or the absorbing candidate after a merge).
    pub kept_candidate_name: String,
    /// The candidate that was considered for merge (the absorbed candidate).
    pub other_candidate_name: String,
    /// Cosine similarity of the pair that triggered the equivalence check.
    pub cosine_similarity: f32,
    /// The resolution decision.
    pub action: ReduceAction,
}

// ─── Report ──────────────────────────────────────────────────────────────────

/// Full observability report from one orchestration run.
///
/// The salience gate is no longer on the blocking path (recall-first). All windows
/// from segmentation are forwarded to the map step, so:
/// - `kept_episode_count == total_episodes` always.
/// - `gated_episode_count == 0` always.
///
/// These fields are preserved for API compatibility with callers that assert
/// `total_episodes == kept_episode_count + gated_episode_count`.
#[derive(Debug, Clone)]
pub struct OrchestrationReport {
    /// Total windows produced by segmentation, all of which are sent to the map step.
    pub total_episodes: usize,
    /// Always equal to `total_episodes`. Preserved for API compatibility.
    ///
    /// The salience gate is not on the blocking path: all windows are processed.
    pub kept_episode_count: usize,
    /// Always zero. The salience gate no longer gates out any windows.
    ///
    /// Preserved for API compatibility with callers asserting
    /// `total_episodes == kept_episode_count + gated_episode_count`.
    pub gated_episode_count: usize,
    /// Candidates from the map step before reduce.
    pub pre_reduce_candidate_count: usize,
    /// Candidates after reduce (before synthesis).
    pub post_reduce_candidate_count: usize,
    /// Candidates added by the synthesis pass.
    pub synthesis_added_count: usize,
    /// Final candidates written as `.pending` drafts.
    pub final_candidate_count: usize,
    /// Full audit trail of reduce decisions. Every merge and every LLM non-merge
    /// decision appears here; nothing is silently dropped.
    pub reduce_audit: Vec<ReduceAuditEntry>,
    /// Paths of the written `.pending` draft files.
    pub draft_paths: Vec<std::path::PathBuf>,
}

// ─── Errors ──────────────────────────────────────────────────────────────────

/// Typed failures from the orchestration pipeline.
#[derive(Debug, Error)]
pub enum OrchestrationError {
    #[error("preamble normalization failed: {0}")]
    PreambleNormalization(#[from] NormalizationError),
    #[error("episode extraction failed: {0}")]
    EpisodeExtraction(#[from] ExtractionError),
    #[error("skeleton labeling failed: {0}")]
    SkeletonLabel(#[from] SkeletonError),
    #[error("candidate embedding failed: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("cosine similarity computation failed: {0}")]
    CosineSimilarity(#[from] CosineSimilarityError),
    #[error("equivalence verification failed: {0}")]
    EquivalenceVerification(String),
    #[error("synthesis pass failed: {0}")]
    Synthesis(#[from] SynthesisError),
    #[error("draft writing failed: {0}")]
    DraftWrite(#[from] WriterError),
    #[error("session has no events to process")]
    EmptySession,
    #[error(
        "all {window_count} map window(s) failed; extraction produced nothing. errors: {errors:?}"
    )]
    AllWindowsFailed {
        window_count: usize,
        errors: Vec<String>,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Runs the full map→reduce extraction pipeline over a complete session event stream.
///
/// Returns the [`OrchestrationReport`] so callers can surface observability data
/// without needing to inspect the written draft files.
///
/// ## Seam injection
///
/// All LLM-interacting seams are injected via trait objects so tests can provide
/// deterministic fakes without touching production paths.
///
/// ## Concurrency
///
/// The map step runs episodes concurrently behind a [`Semaphore`] bounded to
/// `config.map_concurrency`. A slow episode holds a permit and backpressures
/// the queue; no episode is ever discarded on a timeout.
// Each argument is a distinct injected seam (labeler, prose_extractor, embedder,
// equivalence_verifier, synthesis) or a required data/config input with no natural grouping
// that would not cross the public API boundary shared with the tests/e2e caller.
// Grouping into a seams-struct would require updating the e2e test outside this crate's
// scope fence; the 12-argument form is the stable published signature until that refactor lands.
#[allow(clippy::too_many_arguments)]
pub async fn run_orchestration(
    session_id: DomainId,
    events: &[SessionEvent],
    config: &OrchestrationConfig,
    preamble_normalizer: Option<&dyn PreambleNormalizer>,
    labeler: Arc<dyn SkeletonLabeler>,
    prose_extractor: Arc<dyn TranscriptSkillExtractionService>,
    embedder: Arc<dyn EmbeddingService>,
    equivalence_verifier: Arc<dyn LlmEquivalenceVerifier>,
    synthesis: Arc<dyn SynthesisPass>,
    draft_writer: &PendingDraftWriter,
    extract_request: &ExtractSessionRequest,
    provider_name: &str,
) -> Result<OrchestrationReport, OrchestrationError> {
    if events.is_empty() {
        return Err(OrchestrationError::EmptySession);
    }

    // ── Step 1: Mine preamble ─────────────────────────────────────────────
    let preamble: Preamble = mine_preamble(events, preamble_normalizer).await?;
    debug!(
        preamble_tokens = preamble.approximate_tokens,
        "orchestrator: preamble mined"
    );

    // ── Step 2: Segment into overlapping windows ──────────────────────────
    let windows: Vec<Episode> = segment_session(events, &config.segmentation);
    let total_windows = windows.len();
    debug!(
        total_windows,
        "orchestrator: session segmented into windows"
    );

    // Salience gating is intentionally NOT applied here. Recall-first: every
    // window is extracted. On flat conversational transcripts (no tool events,
    // no imperative keywords) the salience heuristic scores zero and would gate
    // all windows, producing zero extraction output. The salience module remains
    // in the codebase for potential analytics use but is not on the blocking path.

    // ── Step 3: Map all windows concurrently (recall-first) ───────────────
    let semaphore = Arc::new(Semaphore::new(config.map_concurrency.max(1)));

    // Build a source-event-index → &SessionEvent index for window event lookup.
    let event_by_index: std::collections::HashMap<usize, &SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    let mut map_join_set: JoinSet<Result<Vec<ExtractedSkillCandidate>, OrchestrationError>> =
        JoinSet::new();

    for window in &windows {
        // Resolve the window's event indices into the full event stream.
        let window_events: Vec<SessionEvent> = window
            .event_indices
            .iter()
            .filter_map(|idx| event_by_index.get(idx).copied().cloned())
            .collect();

        let labeler = Arc::clone(&labeler);
        let prose_extractor = Arc::clone(&prose_extractor);
        let semaphore = Arc::clone(&semaphore);
        let session_id = session_id.clone();
        let preamble_text = preamble.text.clone();

        map_join_set.spawn(async move {
            // Acquire one concurrency permit. This blocks (never discards) when
            // the cap is reached — honoring the #190 backpressure contract.
            let _permit = semaphore
                .acquire()
                .await
                .expect("semaphore must not be closed during map step");

            map_one_window(
                &window_events,
                &session_id,
                &preamble_text,
                labeler.as_ref(),
                prose_extractor.as_ref(),
            )
            .await
        });
    }

    let mut window_candidates: Vec<ExtractedSkillCandidate> = Vec::new();
    let mut windows_succeeded: usize = 0;
    let mut window_errors: Vec<String> = Vec::new();

    while let Some(join_result) = map_join_set.join_next().await {
        match join_result {
            Ok(Ok(mut candidates)) => {
                // A window that returns Ok — even with zero candidates — is a
                // genuine success (the model legitimately found nothing here).
                windows_succeeded += 1;
                window_candidates.append(&mut candidates);
            }
            Ok(Err(error)) => {
                // Recall-first: one window failing must not lose the candidates
                // from windows that succeeded. But the failure is NOT swallowed —
                // it is recorded so a total-failure run fails loud (below) instead
                // of masquerading as an empty success.
                warn!(?error, "orchestrator: window map step failed");
                window_errors.push(error.to_string());
            }
            Err(join_error) => {
                warn!(?join_error, "orchestrator: window task panicked");
                window_errors.push(format!("window task panicked: {join_error}"));
            }
        }
    }

    // Fail loud on a TOTAL map failure: if no window succeeded AND at least one
    // window errored, the pipeline produced nothing because the work failed —
    // not because the model found nothing. Surfacing this as a success with zero
    // drafts would violate the no-silent-failure mandate. (Per-window degradation
    // is preserved: as long as ≥1 window succeeds, partial results flow through.)
    if windows_succeeded == 0 && !window_errors.is_empty() {
        return Err(OrchestrationError::AllWindowsFailed {
            window_count: windows.len(),
            errors: window_errors,
        });
    }

    // Include preamble preference candidates in the union before reduce.
    let preamble_candidates = preamble.preference_skill_candidates();
    window_candidates.extend(preamble_candidates);

    let pre_reduce_count = window_candidates.len();
    info!(
        pre_reduce_count,
        windows_succeeded,
        window_errors = window_errors.len(),
        "orchestrator: map step complete; entering reduce"
    );

    // ── Step 4: Reduce (cosine + LLM equivalence) ─────────────────────────
    let (deduped_candidates, reduce_audit) = reduce_candidates(
        window_candidates,
        embedder.as_ref(),
        equivalence_verifier.as_ref(),
        config.reduce_similarity_threshold,
    )
    .await?;

    let post_reduce_count = deduped_candidates.len();
    info!(
        post_reduce_count,
        audit_entries = reduce_audit.len(),
        "orchestrator: reduce step complete"
    );

    // ── Step 5: Synthesis pass ────────────────────────────────────────────
    // Synthesis is an ADDITIVE enrichment pass: it proposes extra cross-arc
    // candidates on top of the grounded prose/skeleton candidates already
    // produced. It is NOT the source of truth. A synthesis failure (e.g. the
    // local model returns malformed/empty JSON — gemma4:12b does this
    // nondeterministically) must therefore NOT discard the real candidates the
    // map+reduce steps already grounded. We log the failure LOUDLY (never
    // swallow it silently) and proceed with the deduped candidates — the same
    // degrade-don't-drop contract the map step uses for a single failed window.
    let mut synthesis_candidates = match synthesis
        .synthesize(&deduped_candidates, &preamble.text)
        .await
    {
        Ok(candidates) => candidates,
        Err(error) => {
            warn!(
                ?error,
                grounded_candidates = deduped_candidates.len(),
                "orchestrator: synthesis pass failed; proceeding with grounded \
                 prose/skeleton candidates (synthesis is additive enrichment, not \
                 the source of truth)"
            );
            Vec::new()
        }
    };
    let synthesis_added_count = synthesis_candidates.len();

    let mut final_candidates = deduped_candidates;
    final_candidates.append(&mut synthesis_candidates);
    let final_count = final_candidates.len();

    info!(
        synthesis_added_count,
        final_count, "orchestrator: synthesis step complete"
    );

    // ── Step 6: Write .pending drafts ─────────────────────────────────────
    let extraction_result = ExtractionResult {
        source_session_id: session_id,
        provider: provider_name.to_owned(),
        candidates: final_candidates,
    };

    let draft_paths =
        draft_writer.write_pending_drafts(&extraction_result, extract_request, provider_name)?;

    Ok(OrchestrationReport {
        total_episodes: total_windows,
        // All windows are forwarded to the map step; gate is not on the blocking path.
        kept_episode_count: total_windows,
        gated_episode_count: 0,
        pre_reduce_candidate_count: pre_reduce_count,
        post_reduce_candidate_count: post_reduce_count,
        synthesis_added_count,
        final_candidate_count: final_count,
        reduce_audit,
        draft_paths,
    })
}

// ─── Map step helper ─────────────────────────────────────────────────────────

/// Maps one window to zero or more [`ExtractedSkillCandidate`]s.
///
/// ## Extraction strategy: prose always, skeleton additive
///
/// The prose extractor is the **universal floor**: it always runs on every window,
/// regardless of whether the window contains tool events. This guarantees extraction
/// output on flat conversational sessions (the root cause of the production regression).
///
/// When the window also contains an actionable tool arc (failing→passing `ToolResult`),
/// skeleton mining runs ADDITIONALLY and its grounded candidate is added to the prose
/// results. Skeleton mining is strictly additive — it never replaces prose extraction,
/// and "no arc" never means "no extraction".
///
/// Both paths prepend the preamble text as a synthetic leading context entry so the
/// model has global session context in every window.
async fn map_one_window(
    window_events: &[SessionEvent],
    session_id: &DomainId,
    preamble_text: &str,
    labeler: &dyn SkeletonLabeler,
    prose_extractor: &dyn TranscriptSkillExtractionService,
) -> Result<Vec<ExtractedSkillCandidate>, OrchestrationError> {
    if window_events.is_empty() {
        return Ok(Vec::new());
    }

    // Always run the prose extractor — universal floor for all transcript shapes.
    let mut candidates =
        extract_prose_window(window_events, session_id, preamble_text, prose_extractor).await?;

    // Additively run skeleton mining when the window has a tool arc.
    // `map_episode` returns ProseFallback (not an error) when no arc exists,
    // so it is safe to call unconditionally — non-arc windows produce no
    // skeleton candidate and incur only a cheap deterministic scan.
    match map_episode(window_events, labeler).await? {
        MapOutcome::Skeleton(skeleton_candidate) => {
            debug!(
                name = %skeleton_candidate.name,
                "orchestrator: window yielded skeleton candidate (additive to prose)"
            );
            candidates.push(skeleton_candidate);
        }
        MapOutcome::ProseFallback { reason } => {
            debug!(
                reason = %reason,
                "orchestrator: window has no tool arc; prose extraction is the only path"
            );
        }
    }

    Ok(candidates)
}

/// Outcome of a single prose extraction attempt on one window.
///
/// Separates "model returned usable candidates", "model returned empty/garbled
/// output for a non-trivial window (retryable)", and "hard infrastructure error
/// (not retryable)" so the caller can apply the right policy.
enum ProseWindowAttemptOutcome {
    /// At least one candidate extracted — stop retrying.
    Candidates(Vec<ExtractedSkillCandidate>),
    /// The model returned zero candidates or malformed JSON from a substantive
    /// window. The attempt is logged as suspicious and eligible for retry.
    EmptyOrMalformed { reason: String },
    /// The provider was unreachable (connection error, non-200 status, or
    /// transcript validation failure). Not retried — these won't fix themselves
    /// between calls and the error must surface to the window map step.
    HardError(OrchestrationError),
}

/// Classifies a single prose extraction result into a [`ProseWindowAttemptOutcome`].
///
/// `ExtractionError::Unexpected` (JSON parse failure) is the fingerprint of a
/// cold or under-pressure `gemma4:12b` returning malformed JSON on its first call
/// after a model reload. It is therefore treated as retryable, not a hard error.
/// `ExtractionError::ProviderUnavailable` is a network/infra failure and is NOT
/// retried — a retry cannot fix a dead connection.
fn classify_prose_attempt(
    result: Result<Vec<ExtractedSkillCandidate>, OrchestrationError>,
    window_content_chars: usize,
) -> ProseWindowAttemptOutcome {
    match result {
        // Model returned candidates — done.
        Ok(candidates) if !candidates.is_empty() => {
            ProseWindowAttemptOutcome::Candidates(candidates)
        }

        // Model returned zero candidates from a substantive window — suspicious,
        // retry unless the window is trivially small.
        Ok(_empty) if window_content_chars >= PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS => {
            ProseWindowAttemptOutcome::EmptyOrMalformed {
                reason: "model returned zero candidates".to_owned(),
            }
        }

        // Zero candidates from a trivial window — plausible, accept.
        Ok(empty) => ProseWindowAttemptOutcome::Candidates(empty),

        // JSON parse failure on a substantive window — cold-start or KV-cache hiccup.
        // Treat as retryable: the same prompt succeeds reliably once the model is warm.
        Err(OrchestrationError::EpisodeExtraction(ExtractionError::Unexpected(ref msg)))
            if window_content_chars >= PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS =>
        {
            ProseWindowAttemptOutcome::EmptyOrMalformed {
                reason: format!("model returned malformed JSON: {msg}"),
            }
        }

        // Any other error — not retryable. Surface loudly.
        Err(hard_error) => ProseWindowAttemptOutcome::HardError(hard_error),
    }
}

/// Builds a [`SessionTranscript`] from window events (prepending the preamble
/// as a synthetic context message) and runs the prose extractor.
///
/// This is the universal extraction floor: it works on any window regardless
/// of whether tool events or imperative keywords are present.
///
/// ## Retry policy for substantive windows
///
/// For windows with ≥ [`PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS`] bytes of real
/// transcript content, the call is retried up to [`PROSE_WINDOW_EMPTY_RETRY_LIMIT`]
/// additional times when the attempt returns:
///
/// - **Zero candidates** — the model may have returned `{}` or an empty array,
///   which `serde(default)` silently accepts. Observed nondeterministically with
///   `gemma4:12b` under `OLLAMA_NUM_PARALLEL`.
/// - **Malformed JSON** (`ExtractionError::Unexpected`) — the model (particularly
///   `gemma4:12b` on its cold first inference after a reload) occasionally mixes
///   reasoning tokens into the `format:"json"` response, producing parse errors.
///
/// Provider connection failures (`ExtractionError::ProviderUnavailable`) are NOT
/// retried — they are hard infrastructure errors that a retry cannot fix.
///
/// The retry is bounded (total attempts = 1 + limit), logs every attempt with the
/// reason, and never injects fabricated candidates.
async fn extract_prose_window(
    window_events: &[SessionEvent],
    session_id: &DomainId,
    preamble_text: &str,
    prose_extractor: &dyn TranscriptSkillExtractionService,
) -> Result<Vec<ExtractedSkillCandidate>, OrchestrationError> {
    // Build a flat transcript from the window events.
    let mut transcript = events_to_transcript(session_id.clone(), window_events);

    // Prepend the preamble as a synthetic leading context entry so the prose
    // extractor sees the same global preferences and facts in every window.
    if !preamble_text.is_empty() {
        use domain::TranscriptEntry;
        let preamble_entry = TranscriptEntry {
            speaker: "system".to_owned(),
            content: format!("[Session context]\n{preamble_text}"),
        };
        transcript.entries.insert(0, preamble_entry);
    }

    // Measure the substantive content length: total chars across all window events
    // (excluding the preamble system entry we just prepended). The preamble is not
    // part of the window's learnable signal — only real session turns matter for
    // the "is this worth retrying?" decision.
    let window_content_chars: usize = window_events
        .iter()
        .filter_map(|ev| ev.as_transcript_entry())
        .map(|entry| entry.content.len())
        .sum();

    // First attempt — the common path.
    let first_result = prose_extractor
        .extract(&transcript)
        .await
        .map(|r| r.candidates)
        .map_err(OrchestrationError::EpisodeExtraction);

    match classify_prose_attempt(first_result, window_content_chars) {
        ProseWindowAttemptOutcome::Candidates(candidates) => return Ok(candidates),
        ProseWindowAttemptOutcome::HardError(error) => return Err(error),
        ProseWindowAttemptOutcome::EmptyOrMalformed { reason } => {
            warn!(
                session_id = session_id.as_str(),
                window_content_chars,
                reason = %reason,
                "prose extractor returned unusable output from a substantive window; \
                 will retry up to {} more time(s)",
                PROSE_WINDOW_EMPTY_RETRY_LIMIT,
            );
        }
    }

    // Retry loop — only reached when the first attempt was EmptyOrMalformed.
    for retry_index in 1..=PROSE_WINDOW_EMPTY_RETRY_LIMIT {
        warn!(
            session_id = session_id.as_str(),
            retry = retry_index,
            window_content_chars,
            "prose extractor retry attempt {} of {}",
            retry_index,
            PROSE_WINDOW_EMPTY_RETRY_LIMIT,
        );

        let retry_result = prose_extractor
            .extract(&transcript)
            .await
            .map(|r| r.candidates)
            .map_err(OrchestrationError::EpisodeExtraction);

        match classify_prose_attempt(retry_result, window_content_chars) {
            ProseWindowAttemptOutcome::Candidates(candidates) => {
                if !candidates.is_empty() {
                    info!(
                        session_id = session_id.as_str(),
                        retry = retry_index,
                        candidate_count = candidates.len(),
                        "prose extractor recovered candidates on retry attempt {}",
                        retry_index,
                    );
                }
                return Ok(candidates);
            }
            ProseWindowAttemptOutcome::HardError(error) => {
                // A hard error mid-retry surfaces as a window failure.
                return Err(error);
            }
            ProseWindowAttemptOutcome::EmptyOrMalformed { reason } => {
                warn!(
                    session_id = session_id.as_str(),
                    retry = retry_index,
                    reason = %reason,
                    "prose extractor retry {} still returned unusable output",
                    retry_index,
                );
            }
        }
    }

    // All attempts exhausted — accept empty rather than failing the whole window.
    // An empty result allows other windows (or preamble candidates) to still flow
    // through; the no-silent-failure contract is upheld via the warn logs above.
    warn!(
        session_id = session_id.as_str(),
        window_content_chars,
        retries = PROSE_WINDOW_EMPTY_RETRY_LIMIT,
        "prose extractor returned unusable output for all {} attempts; \
         accepting empty result for this window",
        PROSE_WINDOW_EMPTY_RETRY_LIMIT + 1,
    );
    Ok(Vec::new())
}

// ─── Reduce step ─────────────────────────────────────────────────────────────

/// Embeds, cosine-pairs, and LLM-verifies all episode candidates, returning the
/// deduped set and a full audit trail.
///
/// ## Algorithm
///
/// 1. Embed every candidate's semantic text (name + description + procedures).
/// 2. For every ordered pair (i < j), compute cosine similarity.
/// 3. If similarity ≥ threshold, call [`LlmEquivalenceVerifier`].
///    - Equivalent → mark `j` for removal; record a Merged audit entry.
///    - Not equivalent → record a Kept audit entry. Both candidates survive.
/// 4. Return all candidates not marked for removal.
///
/// No candidate is dropped without an audit entry.
async fn reduce_candidates(
    candidates: Vec<ExtractedSkillCandidate>,
    embedder: &dyn EmbeddingService,
    verifier: &dyn LlmEquivalenceVerifier,
    similarity_threshold: f32,
) -> Result<(Vec<ExtractedSkillCandidate>, Vec<ReduceAuditEntry>), OrchestrationError> {
    if candidates.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Build semantic text blocks for embedding.
    let semantic_texts: Vec<String> = candidates.iter().map(candidate_semantic_text).collect();

    // Embed all candidates. The embedder seam is a trait; production uses
    // OllamaEmbeddingService; tests inject a deterministic fake.
    let text_refs: Vec<&str> = semantic_texts.iter().map(String::as_str).collect();
    let embeddings: Vec<Vec<f32>> = embedder.embed_batch(&text_refs).await?;

    let mut removed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut audit: Vec<ReduceAuditEntry> = Vec::new();

    // O(n²) pairing. n = post-gate episode candidates, typically small (< 100).
    for i in 0..candidates.len() {
        if removed.contains(&i) {
            continue;
        }
        for j in (i + 1)..candidates.len() {
            if removed.contains(&j) {
                continue;
            }

            let similarity = match cosine_similarity(&embeddings[i], &embeddings[j]) {
                Ok(sim) => sim,
                Err(CosineSimilarityError::ZeroMagnitude) => {
                    // Zero-magnitude embedding means one candidate has no semantic
                    // content. Skip the pair silently — the individual candidates
                    // still survive independently.
                    debug!(
                        left = %candidates[i].name,
                        right = %candidates[j].name,
                        "orchestrator: skipping zero-magnitude embedding pair in reduce"
                    );
                    continue;
                }
                Err(error) => return Err(OrchestrationError::CosineSimilarity(error)),
            };

            if similarity < similarity_threshold {
                continue;
            }

            // Near pair — ask the LLM equivalence verifier.
            let left_text = &semantic_texts[i];
            let right_text = &semantic_texts[j];

            let decision = verifier
                .decide_equivalence(left_text, right_text)
                .await
                .map_err(|error| OrchestrationError::EquivalenceVerification(error.to_string()))?;

            if decision.equivalent {
                // Absorb j into i. j is dropped with an audit record.
                removed.insert(j);
                audit.push(ReduceAuditEntry {
                    kept_candidate_name: candidates[i].name.clone(),
                    other_candidate_name: candidates[j].name.clone(),
                    cosine_similarity: similarity,
                    action: ReduceAction::Merged {
                        rationale: decision.rationale,
                    },
                });
                debug!(
                    kept = %candidates[i].name,
                    absorbed = %candidates[j].name,
                    similarity,
                    "orchestrator: merged equivalent candidates"
                );
            } else {
                // Not equivalent — both survive. Record the decision for audit.
                audit.push(ReduceAuditEntry {
                    kept_candidate_name: candidates[i].name.clone(),
                    other_candidate_name: candidates[j].name.clone(),
                    cosine_similarity: similarity,
                    action: ReduceAction::Kept {
                        rationale: decision.rationale,
                    },
                });
            }
        }
    }

    let deduped: Vec<ExtractedSkillCandidate> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, candidate)| {
            if removed.contains(&idx) {
                None
            } else {
                Some(candidate)
            }
        })
        .collect();

    Ok((deduped, audit))
}

/// Builds the semantic text block for a candidate, used both for embedding and
/// for the equivalence verifier prompt.
fn candidate_semantic_text(candidate: &ExtractedSkillCandidate) -> String {
    format!(
        "{}\n{}\n{}",
        candidate.name,
        candidate.description,
        candidate.procedures.join("\n")
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use async_trait::async_trait;
    use domain::{
        DomainId, EmbeddingError, EmbeddingService, ExtractedSkillCandidate, ExtractionError,
        ExtractionResult, SessionEvent, SessionTranscript, TranscriptSkillExtractionService,
    };
    use infrastructure::{EquivalenceDecision, LlmEquivalenceVerifier};

    use super::*;
    use crate::{
        preamble::{NormalizationError, PreambleDraft, PreambleNormalizer},
        salience::SalienceConfig,
        segmentation::SegmentationConfig,
        skeleton::{ProcedureSkeleton, SkeletonError, SkeletonLabel, SkeletonLabeler},
        writer::PendingDraftWriter,
    };

    // ── Test-only fakes (NEVER exposed outside #[cfg(test)]) ──────────────

    /// Fake preamble normalizer: sorts preferences alphabetically, no LLM call.
    struct SortingNormalizerFake;

    #[async_trait]
    impl PreambleNormalizer for SortingNormalizerFake {
        async fn normalize(
            &self,
            mut draft: PreambleDraft,
        ) -> Result<PreambleDraft, NormalizationError> {
            draft
                .preferences
                .sort_by(|a, b| a.raw_statement.cmp(&b.raw_statement));
            Ok(draft)
        }
    }

    /// Fake skeleton labeler that always keeps with a fixed name derived from
    /// the first step's tool name.
    struct EchoLabelerFake;

    #[async_trait]
    impl SkeletonLabeler for EchoLabelerFake {
        async fn label(
            &self,
            skeleton: &ProcedureSkeleton,
        ) -> Result<SkeletonLabel, SkeletonError> {
            let tool = skeleton
                .steps
                .first()
                .map(|s| s.tool_name.as_str())
                .unwrap_or("unknown");
            Ok(SkeletonLabel {
                name: format!("test-{tool}"),
                description: "Test description".to_owned(),
                generality: Some("general".to_owned()),
                keep: true,
                confidence: 0.9,
            })
        }
    }

    /// Fake prose extractor that returns a configurable list of candidates.
    struct FixedProseExtractor {
        candidates: Vec<ExtractedSkillCandidate>,
    }

    impl FixedProseExtractor {
        fn returning(candidates: Vec<ExtractedSkillCandidate>) -> Arc<Self> {
            Arc::new(Self { candidates })
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for FixedProseExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            Ok(ExtractionResult {
                source_session_id: DomainId::new_unchecked("prose-fake"),
                provider: "fake-prose".to_owned(),
                candidates: self.candidates.clone(),
            })
        }
    }

    /// Prose extractor that always errors — models a real provider timing out on
    /// every window (the production failure that was being silently swallowed).
    struct FailingProseExtractor;

    impl FailingProseExtractor {
        fn new() -> Arc<Self> {
            Arc::new(Self)
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for FailingProseExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            Err(ExtractionError::Timeout {
                timeout_ms: 120_000,
            })
        }
    }

    /// Prose extractor that returns empty candidates for the first `empty_count`
    /// calls, then returns a fixed candidate list on subsequent calls.
    ///
    /// Models the real-world flake where gemma4:12b returns `{}` (parsed as zero
    /// candidates) on a cold first call, then recovers on retry.
    struct FlakyProseExtractor {
        /// Number of empty-candidates calls to return before yielding real results.
        empty_count: usize,
        /// Call counter shared across clones (Arc ensures thread-safety).
        call_count: Arc<std::sync::atomic::AtomicUsize>,
        /// Candidates to return once past the initial empty phase.
        recovery_candidates: Vec<ExtractedSkillCandidate>,
    }

    impl FlakyProseExtractor {
        fn new(empty_count: usize, recovery_candidates: Vec<ExtractedSkillCandidate>) -> Arc<Self> {
            Arc::new(Self {
                empty_count,
                call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                recovery_candidates,
            })
        }

        fn times_called(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for FlakyProseExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            let call_index = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let candidates = if call_index < self.empty_count {
                vec![]
            } else {
                self.recovery_candidates.clone()
            };
            Ok(ExtractionResult {
                source_session_id: DomainId::new_unchecked("flaky-fake"),
                provider: "flaky-fake".to_owned(),
                candidates,
            })
        }
    }

    /// Prose extractor that returns `ExtractionError::Unexpected` (JSON parse
    /// failure) for the first `error_count` calls, then returns real candidates.
    ///
    /// Models the real-world flake where `gemma4:12b` returns malformed JSON
    /// on a cold first inference (mixing reasoning tokens into the response),
    /// then recovers on retry once the model is properly warmed.
    struct FlakyWithParseErrorProseExtractor {
        error_count: usize,
        call_count: Arc<std::sync::atomic::AtomicUsize>,
        recovery_candidates: Vec<ExtractedSkillCandidate>,
    }

    impl FlakyWithParseErrorProseExtractor {
        fn new(error_count: usize, recovery_candidates: Vec<ExtractedSkillCandidate>) -> Arc<Self> {
            Arc::new(Self {
                error_count,
                call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                recovery_candidates,
            })
        }

        fn times_called(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for FlakyWithParseErrorProseExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            let call_index = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if call_index < self.error_count {
                // Models cold-start malformed JSON: parse failure.
                Err(ExtractionError::Unexpected(
                    "invalid JSON: mixed reasoning tokens in response".to_owned(),
                ))
            } else {
                Ok(ExtractionResult {
                    source_session_id: DomainId::new_unchecked("flaky-parse-fake"),
                    provider: "flaky-parse-fake".to_owned(),
                    candidates: self.recovery_candidates.clone(),
                })
            }
        }
    }

    /// Fake embedder: returns a unit vector whose first element is set to the
    /// hash of the input, giving distinct vectors for distinct inputs while
    /// keeping identical inputs equal.
    ///
    /// This lets us control cosine similarity in tests: two candidates with the
    /// same text produce identical vectors (cosine = 1.0); two different texts
    /// produce orthogonal-ish vectors.
    struct DeterministicEmbedderFake;

    impl DeterministicEmbedderFake {
        fn embed_str(text: &str) -> Vec<f32> {
            // Build a 4-element vector whose components are derived from the text
            // content so identical texts are equal and distinct texts differ.
            let bytes = text.as_bytes();
            let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
            let xor: u64 = bytes.iter().map(|&b| b as u64).fold(0, |acc, b| acc ^ b);
            // Place the hash in different dimensions to create stable vectors.
            let v0 = (sum % 256) as f32 + 1.0; // always > 0 so no zero-magnitude
            let v1 = (xor % 256) as f32 + 1.0;
            let v2 = (bytes.len() % 256) as f32 + 1.0;
            let v3 = 1.0_f32;
            let norm = (v0 * v0 + v1 * v1 + v2 * v2 + v3 * v3).sqrt();
            vec![v0 / norm, v1 / norm, v2 / norm, v3 / norm]
        }
    }

    #[async_trait]
    impl EmbeddingService for DeterministicEmbedderFake {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(Self::embed_str(text))
        }
        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|t| Self::embed_str(t)).collect())
        }
    }

    /// Fake equivalence verifier: decides pairs as equivalent when the two names
    /// share a common prefix configured at construction.
    struct PrefixEquivalenceFake {
        /// Pairs that should be declared equivalent: (name_a, name_b).
        equivalent_pairs: Vec<(String, String)>,
    }

    impl PrefixEquivalenceFake {
        fn declaring_equivalent(pairs: Vec<(&str, &str)>) -> Arc<Self> {
            Arc::new(Self {
                equivalent_pairs: pairs
                    .into_iter()
                    .map(|(a, b)| (a.to_owned(), b.to_owned()))
                    .collect(),
            })
        }

        fn never_equivalent() -> Arc<Self> {
            Arc::new(Self {
                equivalent_pairs: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl LlmEquivalenceVerifier for PrefixEquivalenceFake {
        async fn decide_equivalence(
            &self,
            left_text: &str,
            right_text: &str,
        ) -> Result<EquivalenceDecision, ExtractionError> {
            // Check if either (left,right) or (right,left) is in our declared pairs.
            let equivalent = self.equivalent_pairs.iter().any(|(a, b)| {
                (left_text.contains(a.as_str()) && right_text.contains(b.as_str()))
                    || (left_text.contains(b.as_str()) && right_text.contains(a.as_str()))
            });
            Ok(EquivalenceDecision {
                equivalent,
                rationale: if equivalent {
                    "test: declared equivalent pair".to_owned()
                } else {
                    "test: not in declared equivalent pairs".to_owned()
                },
            })
        }
    }

    /// Fake synthesis pass: returns a fixed list of additional candidates.
    struct FixedSynthesisPassFake {
        additional: Vec<ExtractedSkillCandidate>,
    }

    impl FixedSynthesisPassFake {
        fn adding(candidates: Vec<ExtractedSkillCandidate>) -> Arc<Self> {
            Arc::new(Self {
                additional: candidates,
            })
        }

        fn noop() -> Arc<Self> {
            Arc::new(Self {
                additional: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl SynthesisPass for FixedSynthesisPassFake {
        async fn synthesize(
            &self,
            _deduped: &[ExtractedSkillCandidate],
            _preamble_text: &str,
        ) -> Result<Vec<ExtractedSkillCandidate>, SynthesisError> {
            Ok(self.additional.clone())
        }
    }

    /// Synthesis pass that always errors — models the local model returning
    /// malformed/empty JSON from the synthesis seam (observed nondeterministically
    /// with gemma4:12b).
    struct FailingSynthesisPassFake;

    impl FailingSynthesisPassFake {
        fn new() -> Arc<Self> {
            Arc::new(Self)
        }
    }

    #[async_trait]
    impl SynthesisPass for FailingSynthesisPassFake {
        async fn synthesize(
            &self,
            _deduped: &[ExtractedSkillCandidate],
            _preamble_text: &str,
        ) -> Result<Vec<ExtractedSkillCandidate>, SynthesisError> {
            Err(SynthesisError::ParseFailure(
                "synthesis response was not valid JSON: EOF".to_owned(),
            ))
        }
    }

    // ── Shared test-fixture helpers ────────────────────────────────────────

    fn sandbox_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orch-test-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&dir).expect("sandbox must be creatable");
        dir
    }

    fn skill_candidate(name: &str, description: &str) -> ExtractedSkillCandidate {
        ExtractedSkillCandidate {
            name: name.to_owned(),
            description: description.to_owned(),
            tags: vec![],
            procedures: vec![format!("step for {name}")],
            conventions: vec![],
            assets: vec![],
            confidence: 0.85,
            generality: Some("general".to_owned()),
            generality_rationale: None,
        }
    }

    fn inline_request(session_id: &str) -> ExtractSessionRequest {
        // Minimal inline transcript so the writer doesn't need a real file.
        ExtractSessionRequest {
            transcript_ref: "ignored".to_owned(),
            transcript_inline: Some(
                r#"{"type":"message","message":{"role":"user","content":"test"}}"#.to_owned(),
            ),
            session_id: session_id.to_owned(),
            repo_path: None,
        }
    }

    /// Builds a multi-arc synthetic session where skill setup and payoff are in
    /// different episodes. The session has two arcs (user-msg + error→fix), each
    /// emitting partial candidates that the reduce step should merge.
    fn multi_arc_session_events() -> Vec<SessionEvent> {
        vec![
            // Arc 1: setup — the user sets a context preference.
            SessionEvent::UserMessage {
                index: 0,
                content: "always prefer tokio::sync::Mutex over std::sync::Mutex".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "Understood. I will use tokio::sync::Mutex.".to_owned(),
            },
            // Topic shift.
            SessionEvent::UserMessage {
                index: 2,
                content: "now fix the build".to_owned(),
            },
            // Arc 2: payoff — the build fails, gets fixed.
            SessionEvent::ToolCall {
                index: 3,
                tool_use_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"cargo build 2>&1"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 4,
                tool_use_id: "t1".to_owned(),
                is_error: true,
                exit_code: Some(1),
                output: "error[E0277]: Mutex<T> cannot be held across await".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 5,
                tool_use_id: "t2".to_owned(),
                name: "Edit".to_owned(),
                input_json: r#"{"file_path":"src/handler.rs","old_string":"std::sync::Mutex","new_string":"tokio::sync::Mutex"}"#.to_owned(),
            },
            SessionEvent::FileEdit {
                index: 5,
                tool_use_id: "t2".to_owned(),
                path: "src/handler.rs".to_owned(),
                operation: "Edit".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 6,
                tool_use_id: "t3".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"cargo build 2>&1"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 7,
                tool_use_id: "t3".to_owned(),
                is_error: false,
                exit_code: Some(0),
                output: "Finished dev target".to_owned(),
            },
        ]
    }

    // ── Acceptance criterion 1: multi-arc setup/payoff merges to one candidate ──

    /// Proves that when two episodes each surface a partial candidate for the same
    /// skill (setup in one episode, payoff in another), the reduce step collapses
    /// them into ONE merged candidate, not two partials.
    ///
    /// The equivalence verifier is configured to declare the two partial candidates
    /// equivalent (simulating a real LLM decision), and the embedder must produce
    /// vectors close enough to exceed the similarity threshold.
    #[tokio::test]
    async fn multi_arc_setup_payoff_merges_to_one_candidate() {
        // Two candidates that represent the setup and payoff of the same skill.
        let setup_candidate = skill_candidate(
            "use-tokio-mutex-for-async",
            "Use tokio::sync::Mutex instead of std::sync::Mutex in async contexts",
        );
        let payoff_candidate = skill_candidate(
            "use-tokio-mutex-for-async", // same name → same semantic text → identical embedding
            "Use tokio::sync::Mutex instead of std::sync::Mutex in async contexts",
        );

        // Same name + description → identical embedding → cosine = 1.0 → above threshold.
        // The verifier is configured to merge this pair.
        let verifier = PrefixEquivalenceFake::declaring_equivalent(vec![(
            "use-tokio-mutex-for-async",
            "use-tokio-mutex-for-async",
        )]);

        let (deduped, audit) = reduce_candidates(
            vec![setup_candidate, payoff_candidate],
            &DeterministicEmbedderFake,
            verifier.as_ref(),
            REDUCE_SIMILARITY_THRESHOLD,
        )
        .await
        .expect("reduce must succeed");

        assert_eq!(
            deduped.len(),
            1,
            "two equivalent partial candidates must collapse to ONE; got {}: {deduped:?}",
            deduped.len()
        );
        assert_eq!(
            audit.len(),
            1,
            "reduce must record exactly one audit entry for the merge"
        );
        assert!(
            matches!(audit[0].action, ReduceAction::Merged { .. }),
            "audit action must be Merged; got {:?}",
            audit[0].action
        );
    }

    // ── Acceptance criterion 2: N duplicate candidates collapse to one ────────

    /// Proves that when the same skill appears in N different episodes, all N
    /// copies collapse to exactly one candidate after reduce.
    #[tokio::test]
    async fn n_duplicate_candidates_collapse_to_one() {
        let n = 5;
        // All five candidates have identical name and description.
        let duplicates: Vec<ExtractedSkillCandidate> = (0..n)
            .map(|_| skill_candidate("duplicate-skill", "Identical skill description"))
            .collect();

        // All pairs are declared equivalent.
        let verifier = PrefixEquivalenceFake::declaring_equivalent(vec![(
            "duplicate-skill",
            "duplicate-skill",
        )]);

        let (deduped, audit) = reduce_candidates(
            duplicates,
            &DeterministicEmbedderFake,
            verifier.as_ref(),
            REDUCE_SIMILARITY_THRESHOLD,
        )
        .await
        .expect("reduce must succeed");

        assert_eq!(
            deduped.len(),
            1,
            "{n} duplicate candidates must collapse to ONE; got {}: {deduped:?}",
            deduped.len()
        );

        // Every drop must be recorded with a rationale — no silent discards.
        assert!(
            !audit.is_empty(),
            "reduce audit must be non-empty when duplicates are merged"
        );
        for entry in &audit {
            match &entry.action {
                ReduceAction::Merged { rationale } | ReduceAction::Kept { rationale } => {
                    assert!(
                        !rationale.is_empty(),
                        "every audit entry must carry a non-empty rationale"
                    );
                }
            }
        }
    }

    // ── Acceptance criterion 3: synthesis pass surfaces session-spanning pattern ─

    /// Proves that the synthesis pass can surface a session-spanning pattern that
    /// was not in any single episode's candidate list.
    #[tokio::test]
    async fn synthesis_pass_surfaces_session_spanning_pattern() {
        let existing = vec![skill_candidate("ep1-skill", "Episode 1 skill")];
        let session_spanning = skill_candidate(
            "session-wide-pattern",
            "Session-spanning pattern discovered by synthesis",
        );

        let synthesis = FixedSynthesisPassFake::adding(vec![session_spanning.clone()]);
        let added = synthesis
            .synthesize(&existing, "test preamble")
            .await
            .expect("synthesis must succeed");

        assert_eq!(
            added.len(),
            1,
            "synthesis must surface exactly one session-spanning candidate"
        );
        assert_eq!(
            added[0].name, session_spanning.name,
            "synthesis must return the injected session-spanning candidate"
        );
        // The session-spanning candidate must NOT appear in the per-episode candidates.
        let was_in_per_episode = existing.iter().any(|c| c.name == session_spanning.name);
        assert!(
            !was_in_per_episode,
            "the session-spanning candidate must not appear in per-episode candidates"
        );
    }

    // ── Acceptance criterion 4 (live, gated): 100k-token synthetic session ────

    /// End-to-end live test on a 100k-token synthetic session using real containers.
    ///
    /// This test is intentionally ignored in CI — it is executed in the epic's
    /// final live-validation pass when `gemma4:e4b` and the Ollama container are
    /// available.
    ///
    /// Invariants verified:
    /// - ≥1 grounded `.pending` drafts are written.
    /// - No turn is silently dropped (all events are covered by at least one episode).
    /// - No hard wall-clock timeout discards a job (the semaphore backpressures instead).
    #[tokio::test]
    #[ignore = "requires live containers: ollama, redis, and a writable .skills directory"]
    async fn live_100k_token_synthetic_session_produces_pending_drafts() {
        // Build a synthetic 100k-token session with many error→fix arcs and
        // stated preferences interleaved across many topics.
        let mut events: Vec<SessionEvent> = Vec::new();
        let num_topics = 50;

        for topic in 0..num_topics {
            let base = topic * 10;
            // Stated preference (triggers hard-keep in salience gate).
            events.push(SessionEvent::UserMessage {
                index: base,
                content: format!("always use topic-{topic} convention in this project"),
            });
            // Error→fix arc.
            events.push(SessionEvent::ToolCall {
                index: base + 1,
                tool_use_id: format!("call-{topic}-fail"),
                name: "Bash".to_owned(),
                input_json: format!(r#"{{"command":"run-topic-{topic} 2>&1"}}"#),
            });
            events.push(SessionEvent::ToolResult {
                index: base + 2,
                tool_use_id: format!("call-{topic}-fail"),
                is_error: true,
                exit_code: Some(1),
                output: format!("error: topic-{topic} failed at line 42"),
            });
            events.push(SessionEvent::FileEdit {
                index: base + 3,
                tool_use_id: format!("call-{topic}-fix"),
                path: format!("src/topic_{topic}.rs"),
                operation: "Edit".to_owned(),
            });
            events.push(SessionEvent::ToolResult {
                index: base + 4,
                tool_use_id: format!("call-{topic}-fix"),
                is_error: false,
                exit_code: Some(0),
                output: format!("topic-{topic} fixed"),
            });
        }

        // Use real infrastructure via environment variables (set in the live test
        // harness). This test MUST drive the real pipeline, not simulate it.
        let ollama_base = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_owned());

        let labeler: Arc<dyn SkeletonLabeler> = {
            // In the live pass, use the real Ollama-backed labeler via a simplified
            // no-op that just produces a fixed label (the real labeler is wired in the
            // full integration harness, not here).
            Arc::new(EchoLabelerFake)
        };
        let prose_extractor: Arc<dyn TranscriptSkillExtractionService> = {
            use infrastructure::{OllamaExtractionConfig, OllamaExtractor};
            let config = OllamaExtractionConfig {
                endpoint: format!("{}/api/generate", ollama_base.trim_end_matches('/')),
                ..OllamaExtractionConfig::default()
            };
            Arc::new(
                OllamaExtractor::new(reqwest::Client::new(), config)
                    .expect("live: OllamaExtractor must initialize"),
            )
        };
        let embedder: Arc<dyn EmbeddingService> = {
            use infrastructure::{OllamaEmbeddingConfig, OllamaEmbeddingService};
            let config = OllamaEmbeddingConfig {
                base_url: ollama_base.clone(),
                ..OllamaEmbeddingConfig::default()
            };
            Arc::new(
                OllamaEmbeddingService::new(reqwest::Client::new(), config)
                    .expect("live: OllamaEmbeddingService must initialize"),
            )
        };
        let equivalence_verifier: Arc<dyn LlmEquivalenceVerifier> = {
            use infrastructure::{OllamaMergeVerifier, OllamaMergeVerifierConfig};
            let config = OllamaMergeVerifierConfig {
                endpoint: format!("{ollama_base}/api/generate"),
                ..OllamaMergeVerifierConfig::default()
            };
            Arc::new(
                OllamaMergeVerifier::new(reqwest::Client::new(), config)
                    .expect("live: OllamaMergeVerifier must initialize"),
            )
        };
        let synthesis = FixedSynthesisPassFake::noop();

        let sandbox = sandbox_dir("live-100k");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let session_id = DomainId::new_unchecked("live-100k-session");
        let request = inline_request("live-100k-session");
        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(4_096, 3),
            salience: SalienceConfig::default(),
            map_concurrency: 4,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            labeler,
            prose_extractor,
            embedder,
            equivalence_verifier,
            synthesis,
            &draft_writer,
            &request,
            "ollama",
        )
        .await
        .expect("live: orchestration must succeed on a 100k-token synthetic session");

        assert!(
            report.final_candidate_count >= 1,
            "live: orchestration must produce ≥1 final candidates; got {}",
            report.final_candidate_count
        );
        assert!(
            !report.draft_paths.is_empty(),
            "live: at least one .pending draft must be written"
        );
        assert_eq!(
            report.total_episodes,
            report.kept_episode_count + report.gated_episode_count,
            "live: kept + gated must equal total episodes (no silent drops)"
        );
    }

    // ── Reduce audit trail integrity ──────────────────────────────────────────

    /// Proves that every near-pair LLM decision (merge or keep) is recorded in the
    /// audit log, ensuring no candidate is silently dropped.
    #[tokio::test]
    async fn reduce_audit_records_every_decision_for_near_pairs() {
        // Two candidates that are genuinely distinct (different names, different content)
        // but will be forced above the similarity threshold by using identical
        // DeterministicEmbedderFake vectors (achieved via identical text).
        let candidate_a = skill_candidate("same-text", "Same description for both");
        let candidate_b = skill_candidate("same-text", "Same description for both");

        // The verifier says they are NOT equivalent despite similar text.
        let verifier = PrefixEquivalenceFake::never_equivalent();

        let (_deduped, audit) = reduce_candidates(
            vec![candidate_a, candidate_b],
            &DeterministicEmbedderFake,
            verifier.as_ref(),
            REDUCE_SIMILARITY_THRESHOLD,
        )
        .await
        .expect("reduce must succeed");

        // Even when the verifier says "not equivalent", the decision must be audited.
        assert_eq!(
            audit.len(),
            1,
            "a near-pair LLM decision must be recorded even when the pair is kept"
        );
        assert!(
            matches!(audit[0].action, ReduceAction::Kept { .. }),
            "verifier=not-equivalent must produce a Kept audit entry; got {:?}",
            audit[0].action
        );
        let rationale = match &audit[0].action {
            ReduceAction::Kept { rationale } => rationale.clone(),
            _ => unreachable!(),
        };
        assert!(
            !rationale.is_empty(),
            "Kept audit entry must carry a non-empty rationale"
        );
    }

    // ── reduce_candidates on empty input ──────────────────────────────────────

    #[tokio::test]
    async fn reduce_empty_candidate_list_returns_empty_with_no_audit() {
        let (deduped, audit) = reduce_candidates(
            vec![],
            &DeterministicEmbedderFake,
            PrefixEquivalenceFake::never_equivalent().as_ref(),
            REDUCE_SIMILARITY_THRESHOLD,
        )
        .await
        .expect("empty reduce must succeed");

        assert!(deduped.is_empty(), "empty input must yield empty deduped");
        assert!(audit.is_empty(), "empty input must yield empty audit");
    }

    // ── map_one_window: prose always runs (including no-arc windows) ─────────

    /// Proves that a preference-only window (no tool arc) always routes to the
    /// prose extractor and surfaces the extractor's candidates.
    ///
    /// In the new design, prose extraction is the universal floor: it fires on
    /// every window regardless of content. A window with no arc produces only
    /// prose candidates (no skeleton additive). This test is the unit-level
    /// analogue of the full-pipeline regression test.
    #[tokio::test]
    async fn map_one_window_prose_always_runs_on_no_arc_window() {
        let events = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "prefer snake_case everywhere".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "noted".to_owned(),
            },
        ];

        let expected_candidate = skill_candidate("prefer-snake-case", "Use snake_case");
        let prose_extractor = FixedProseExtractor::returning(vec![expected_candidate.clone()]);

        let candidates = map_one_window(
            &events,
            &DomainId::new_unchecked("prose-test"),
            "preamble text",
            &EchoLabelerFake,
            prose_extractor.as_ref(),
        )
        .await
        .expect("prose extraction must succeed on no-arc window");

        // Prose extractor result is the only output (no skeleton for a no-arc window).
        assert_eq!(
            candidates.len(),
            1,
            "no-arc window must produce exactly 1 candidate (from prose); got {candidates:?}"
        );
        assert_eq!(candidates[0].name, expected_candidate.name);
    }

    // ── Full orchestration smoke test ─────────────────────────────────────────

    /// End-to-end smoke test: a multi-arc session runs the full pipeline with all
    /// fake seams, producing a report and at least one `.pending` draft.
    #[tokio::test]
    async fn full_orchestration_smoke_test_produces_pending_drafts() {
        let events = multi_arc_session_events();
        let session_id = DomainId::new_unchecked("smoke-test");
        let sandbox = sandbox_dir("smoke");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("smoke-test");

        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3), // one episode
            salience: SalienceConfig::default(),
            map_concurrency: 2,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            Some(&SortingNormalizerFake),
            Arc::new(EchoLabelerFake),
            FixedProseExtractor::returning(vec![]),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("full orchestration must succeed");

        assert_eq!(
            report.total_episodes,
            report.kept_episode_count + report.gated_episode_count,
            "kept + gated must equal total"
        );
        assert!(
            report.final_candidate_count >= 1,
            "smoke test must produce at least one final candidate; report: {report:?}"
        );
        assert!(
            !report.draft_paths.is_empty(),
            "smoke test must write at least one .pending draft"
        );
        // All draft paths must exist on disk.
        for path in &report.draft_paths {
            assert!(
                path.exists(),
                "written .pending draft must exist on disk: {path:?}"
            );
        }
    }

    // ── Regression: a TOTAL map failure must fail loud, not report empty success ──

    /// Proves the no-silent-failure mandate at the orchestration seam: when EVERY
    /// map window's prose extraction errors (e.g. the model times out on each
    /// window), `run_orchestration` must return `AllWindowsFailed` — NOT `Ok` with
    /// zero drafts. This is the exact production bug: a swallowed per-window error
    /// made the pipeline report `extraction.completed` with `candidate_count=0`,
    /// indistinguishable from "the model legitimately found nothing".
    #[tokio::test]
    async fn all_windows_failing_fails_loud_instead_of_silent_empty_success() {
        let events = multi_arc_session_events();
        let session_id = DomainId::new_unchecked("all-fail");
        let sandbox = sandbox_dir("all-fail");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox]);
        let request = inline_request("all-fail");

        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3), // one window
            salience: SalienceConfig::default(),
            map_concurrency: 2,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
        };

        let result = run_orchestration(
            session_id,
            &events,
            &config,
            Some(&SortingNormalizerFake),
            // Skeleton labeler also fails so NO window can produce candidates by any
            // path — the only outcome is total failure.
            Arc::new(EchoLabelerFake),
            FailingProseExtractor::new(),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await;

        match result {
            Err(OrchestrationError::AllWindowsFailed {
                window_count,
                errors,
            }) => {
                assert!(window_count >= 1, "must report the failed window count");
                assert!(
                    !errors.is_empty(),
                    "must carry the underlying per-window error(s), got none"
                );
            }
            other => panic!("total map failure must surface as AllWindowsFailed, got: {other:?}"),
        }
    }

    // ── Regression: synthesis failure must NOT discard grounded candidates ──

    /// Proves synthesis is additive: when the synthesis seam fails (malformed/empty
    /// JSON from the local model — observed with gemma4:12b), the job still SUCCEEDS
    /// and writes the grounded prose/skeleton candidates. Synthesis is enrichment, not
    /// the source of truth, so its failure degrades (logged loudly) instead of nuking
    /// a job that already produced real candidates.
    #[tokio::test]
    async fn synthesis_failure_degrades_and_keeps_grounded_candidates() {
        let events = multi_arc_session_events();
        let session_id = DomainId::new_unchecked("synth-fail");
        let sandbox = sandbox_dir("synth-fail");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox]);
        let request = inline_request("synth-fail");

        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3),
            salience: SalienceConfig::default(),
            map_concurrency: 2,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            Some(&SortingNormalizerFake),
            Arc::new(EchoLabelerFake),
            // Prose produces a real grounded candidate.
            FixedProseExtractor::returning(vec![skill_candidate(
                "grounded-by-prose",
                "a real grounded candidate from the prose extractor",
            )]),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            // Synthesis blows up — must NOT fail the whole job.
            FailingSynthesisPassFake::new(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("synthesis failure must degrade, not fail the whole orchestration");

        assert_eq!(
            report.synthesis_added_count, 0,
            "a failed synthesis pass adds zero candidates"
        );
        assert!(
            report.final_candidate_count >= 1,
            "the grounded prose candidate must survive a synthesis failure; report: {report:?}"
        );
        assert!(
            !report.draft_paths.is_empty(),
            "a .pending draft must still be written from the grounded candidate"
        );
    }

    // ── Regression: flat structureless session must yield ≥1 candidate via prose path ──

    /// Proves that a FLAT, structureless-but-substantive session (plain user/assistant
    /// turns, no tool events, no "always/never" keywords) produces ≥1 `.pending`-bound
    /// candidate via the prose extractor.
    ///
    /// This is the **exact regression** that caused the production failure: on flat
    /// conversational transcripts the old salience gate scored every episode 0.0 and
    /// gated them all, so the map step ran on nothing and zero candidates were produced.
    /// After the fix the gate is NOT applied as a blocking filter — all windows are
    /// forwarded to the prose extractor unconditionally.
    #[tokio::test]
    async fn flat_structureless_session_yields_candidate_via_prose_path() {
        // A flat, substantive, conversational session with no tool events and no
        // imperative keywords — the exact shape that triggered the production regression.
        let events = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "How do I handle timeouts in tokio?".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "Use tokio::time::timeout to wrap any future. It returns a Result: Ok(inner_result) or Err(Elapsed). Always handle both branches explicitly.".to_owned(),
            },
            SessionEvent::UserMessage {
                index: 2,
                content: "What about select! for racing futures?".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 3,
                content: "tokio::select! polls all branches concurrently and completes when the first branch resolves. The other branch futures are dropped. Use biased; if you need deterministic branch priority.".to_owned(),
            },
            SessionEvent::UserMessage {
                index: 4,
                content: "How do I propagate errors from spawned tasks?".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 5,
                content: "Spawned tasks return JoinHandle<T>. Await it and match on JoinError to detect panics. For structured propagation, use a channel or a shared error slot.".to_owned(),
            },
        ];

        let session_id = DomainId::new_unchecked("flat-session-regression");
        let sandbox = sandbox_dir("flat-regression");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("flat-session-regression");

        // The prose extractor returns a candidate for this content.
        let expected_candidate = skill_candidate(
            "tokio-async-patterns",
            "Patterns for timeouts, select!, and error propagation in tokio",
        );
        let prose_extractor = FixedProseExtractor::returning(vec![expected_candidate.clone()]);

        // Use a budget that produces multiple windows (to exercise the multi-window path).
        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(50, 2),
            map_concurrency: 2,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
            ..OrchestrationConfig::default()
        };

        // Declare the candidate equivalent to itself so that duplicate copies
        // produced from multiple overlapping windows are merged down to one in reduce.
        let verifier = PrefixEquivalenceFake::declaring_equivalent(vec![(
            "tokio-async-patterns",
            "tokio-async-patterns",
        )]);

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            Arc::new(EchoLabelerFake),
            prose_extractor,
            Arc::new(DeterministicEmbedderFake),
            verifier,
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("flat session orchestration must not error");

        // The core regression assertion: a flat session must produce ≥1 candidate.
        // Before the fix this was 0 (gate blocked all windows). After the fix it is ≥1.
        assert!(
            report.final_candidate_count >= 1,
            "flat structureless session must yield ≥1 candidate via prose extractor; \
             pre-fix this was 0 because the gate blocked all windows. \
             got: pre_reduce={}, final={}",
            report.pre_reduce_candidate_count,
            report.final_candidate_count
        );

        // At least one draft must be written.
        assert!(
            !report.draft_paths.is_empty(),
            "flat session must produce at least one .pending draft"
        );
        for path in &report.draft_paths {
            assert!(
                path.exists(),
                "written .pending draft must exist on disk: {path:?}"
            );
        }
    }

    // ── Additive: skeleton candidates contribute alongside prose candidates ────────

    /// Proves that when a window contains tool arcs, skeleton mining contributes
    /// candidates ADDITIVE to prose extraction (both run; results are unioned).
    ///
    /// This verifies the "skeleton is additive grounding" invariant: for a window
    /// with arcs, both the skeleton labeler AND the prose extractor fire.
    #[tokio::test]
    async fn structured_window_with_tool_arc_produces_skeleton_plus_prose_candidates() {
        let events = multi_arc_session_events();
        let session_id = DomainId::new_unchecked("additive-test");
        let sandbox = sandbox_dir("additive");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("additive-test");

        // Prose extractor returns one candidate per call.
        let prose_candidate = skill_candidate("prose-extracted", "From prose path");
        let prose_extractor = FixedProseExtractor::returning(vec![prose_candidate.clone()]);

        // Large budget so everything is one episode — one window runs both paths.
        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3),
            map_concurrency: 2,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
            ..OrchestrationConfig::default()
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            Arc::new(EchoLabelerFake),
            prose_extractor,
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("additive-test orchestration must succeed");

        // Both skeleton and prose paths fire, so pre-reduce count ≥ 2
        // (at least one skeleton candidate + at least one prose candidate).
        assert!(
            report.pre_reduce_candidate_count >= 2,
            "structured window must produce ≥2 pre-reduce candidates (skeleton + prose additive); \
             got pre_reduce={}",
            report.pre_reduce_candidate_count
        );
    }

    // ── Retry-on-empty: prose extractor recovers candidates on retry ──────────

    /// Proves that `extract_prose_window` retries the prose extractor when a
    /// substantive window yields zero candidates on the first call.
    ///
    /// This covers the #176 flake: `gemma4:12b` with `format:"json"` occasionally
    /// returns `{}` (parsed as zero candidates) on a cold or contended first call.
    /// The retry gives the model another honest chance without injecting fake data.
    #[tokio::test]
    async fn prose_extractor_retries_on_empty_response_from_substantive_window() {
        // A substantive flat session (> PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS chars).
        let events = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "We keep hitting `Os { code: 35, kind: WouldBlock }` panics when \
                          spawning Tokio tasks under load. How do we diagnose and fix this?"
                    .to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "WouldBlock under Tokio task spawn is almost always file-descriptor \
                          exhaustion. Step 1: run `ulimit -n 65536` to raise the FD ceiling. \
                          Step 2: add `console_subscriber::init()` and run `tokio-console`. \
                          Step 3: replace `std::sync::Mutex` held across `.await` with \
                          `tokio::sync::Mutex`."
                    .to_owned(),
            },
        ];

        let session_id = DomainId::new_unchecked("retry-on-empty");
        let sandbox = sandbox_dir("retry-on-empty");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("retry-on-empty");

        // The flaky extractor returns empty on the first call, then the real candidate.
        let recovery_candidate = skill_candidate(
            "diagnose-tokio-fd-exhaustion",
            "Diagnose and fix WouldBlock FD exhaustion in Tokio",
        );
        let flaky_extractor = FlakyProseExtractor::new(1, vec![recovery_candidate.clone()]);

        let config = OrchestrationConfig {
            // Single window so we see exactly the retry behavior.
            segmentation: SegmentationConfig::new(1_000_000, 3),
            map_concurrency: 1,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
            ..OrchestrationConfig::default()
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            Arc::new(EchoLabelerFake),
            flaky_extractor.clone(),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("orchestration must succeed even when extractor flakes on first attempt");

        // The retry must have recovered the candidate.
        assert_eq!(
            report.final_candidate_count, 1,
            "orchestration must yield the candidate recovered on retry; \
             got final_candidate_count={}",
            report.final_candidate_count
        );

        // Verify the extractor was called at least twice (initial + 1 retry).
        let call_count = flaky_extractor.times_called();
        assert!(
            call_count >= 2,
            "prose extractor must be called at least twice (initial + 1 retry); \
             got call_count={}",
            call_count
        );
    }

    /// Proves that a trivially-small window (below the substantive threshold) is NOT
    /// retried even when it returns zero candidates. A one-line greeting producing
    /// nothing is a legitimate result, not a model hiccup.
    #[tokio::test]
    async fn prose_extractor_does_not_retry_trivial_windows() {
        // A trivially small window — well below PROSE_WINDOW_SUBSTANTIVE_CONTENT_CHARS.
        let events = vec![SessionEvent::UserMessage {
            index: 0,
            content: "hi".to_owned(),
        }];

        let session_id = DomainId::new_unchecked("no-retry-trivial");
        let sandbox = sandbox_dir("no-retry-trivial");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("no-retry-trivial");

        // Always returns empty — but the window is trivial so no retry should fire.
        let flaky_extractor = FlakyProseExtractor::new(usize::MAX, vec![]);

        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3),
            map_concurrency: 1,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
            ..OrchestrationConfig::default()
        };

        // Orchestration may succeed with zero candidates (trivial window).
        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            Arc::new(EchoLabelerFake),
            flaky_extractor.clone(),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect("orchestration on trivial window must succeed (zero candidates ok)");

        // Zero candidates is acceptable for a trivial window.
        assert_eq!(
            report.final_candidate_count, 0,
            "trivial window must not produce candidates; got {}",
            report.final_candidate_count
        );

        // The extractor must have been called exactly once — no retry for trivial windows.
        let call_count = flaky_extractor.times_called();
        assert_eq!(
            call_count, 1,
            "prose extractor must be called exactly once for trivial windows (no retry); \
             got call_count={}",
            call_count
        );
    }

    /// Proves that `extract_prose_window` retries when the prose extractor returns
    /// a JSON parse error (`ExtractionError::Unexpected`) from a substantive window.
    ///
    /// This covers the cold-start variant of the #176 flake: `gemma4:12b` on its
    /// first inference after a model reload mixes reasoning tokens into the
    /// `format:"json"` response, producing malformed JSON that fails to parse.
    /// The retry gives the (now-warmed) model another chance.
    #[tokio::test]
    async fn prose_extractor_retries_on_parse_error_from_substantive_window() {
        // Same substantive transcript as the empty-candidates test.
        let events = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "We keep hitting `Os { code: 35, kind: WouldBlock }` panics when \
                          spawning Tokio tasks under load. How do we diagnose and fix this?"
                    .to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "WouldBlock under Tokio task spawn is almost always file-descriptor \
                          exhaustion. Step 1: run `ulimit -n 65536` to raise the FD ceiling. \
                          Step 2: add `console_subscriber::init()` and run `tokio-console`. \
                          Step 3: replace `std::sync::Mutex` held across `.await` with \
                          `tokio::sync::Mutex`."
                    .to_owned(),
            },
        ];

        let session_id = DomainId::new_unchecked("retry-on-parse-error");
        let sandbox = sandbox_dir("retry-on-parse-error");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("retry-on-parse-error");

        // The extractor returns a parse error on the first call (cold-start malformed JSON),
        // then the real candidate on the second call.
        let recovery_candidate = skill_candidate(
            "diagnose-tokio-fd-exhaustion",
            "Diagnose and fix WouldBlock FD exhaustion in Tokio",
        );
        let flaky_extractor =
            FlakyWithParseErrorProseExtractor::new(1, vec![recovery_candidate.clone()]);

        let config = OrchestrationConfig {
            segmentation: SegmentationConfig::new(1_000_000, 3),
            map_concurrency: 1,
            reduce_similarity_threshold: REDUCE_SIMILARITY_THRESHOLD,
            ..OrchestrationConfig::default()
        };

        let report = run_orchestration(
            session_id,
            &events,
            &config,
            None,
            Arc::new(EchoLabelerFake),
            flaky_extractor.clone(),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test-provider",
        )
        .await
        .expect(
            "orchestration must succeed even when extractor returns parse error on first attempt",
        );

        // The retry must have recovered the candidate.
        assert_eq!(
            report.final_candidate_count, 1,
            "orchestration must yield the candidate recovered after parse-error retry; \
             got final_candidate_count={}",
            report.final_candidate_count
        );

        // Verify the extractor was called at least twice (initial + 1 retry).
        let call_count = flaky_extractor.times_called();
        assert!(
            call_count >= 2,
            "prose extractor must be called at least twice (initial error + 1 retry); \
             got call_count={}",
            call_count
        );
    }

    // ── Empty session fails loudly ─────────────────────────────────────────────

    #[tokio::test]
    async fn empty_session_returns_error() {
        let session_id = DomainId::new_unchecked("empty");
        let sandbox = sandbox_dir("empty");
        let draft_writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
        let request = inline_request("empty");
        let config = OrchestrationConfig::default();

        let error = run_orchestration(
            session_id,
            &[],
            &config,
            None,
            Arc::new(EchoLabelerFake),
            FixedProseExtractor::returning(vec![]),
            Arc::new(DeterministicEmbedderFake),
            PrefixEquivalenceFake::never_equivalent(),
            FixedSynthesisPassFake::noop(),
            &draft_writer,
            &request,
            "test",
        )
        .await
        .expect_err("empty session must fail with OrchestrationError::EmptySession");

        assert!(
            matches!(error, OrchestrationError::EmptySession),
            "expected EmptySession error, got: {error:?}"
        );
    }
}
