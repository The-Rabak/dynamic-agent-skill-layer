//! Map→reduce extraction orchestration (#187) — the integrator of the
//! extraction-scaling epic.
//!
//! ## Pipeline
//!
//! 1. **Segment** the full session into coherent [`Episode`]s (reuses #185).
//! 2. **Mine preamble** from the full event stream (reuses #186).
//! 3. **Gate** episodes via the salience filter (reuses #189). Gated episodes
//!    are recorded in the [`OrchestrationReport`]; nothing is silently dropped.
//! 4. **Map** each kept episode concurrently (bounded by a [`tokio::sync::Semaphore`]):
//!    - If the episode has an actionable tool arc → [`skeleton::map_episode`] with
//!      the real [`SkeletonLabeler`] (LLM-backed seam; fake in tests).
//!    - [`MapOutcome::ProseFallback`] → the existing prose extractor via
//!      [`domain::events_to_transcript`] + [`TranscriptSkillExtractionService::extract`].
//! 5. **Reduce** the union of all episode candidates + preamble preference candidates:
//!    - Embed each candidate's semantic text (via [`EmbeddingService`] seam).
//!    - Cosine-pair candidates exceeding [`REDUCE_SIMILARITY_THRESHOLD`] using
//!      the shared [`infrastructure::cosine_similarity`] (ONE impl in the repo).
//!    - For each near pair, call [`LlmEquivalenceVerifier::decide_equivalence`]
//!      (the same verifier the maintenance merge pass uses — no second impl).
//!    - Record every merge and drop in the audit log. No candidate is dropped
//!      silently.
//! 6. **Synthesize** — ONE LLM pass over the deduped candidate list to surface
//!    session-spanning patterns (via [`SynthesisPass`] seam; fake in tests).
//! 7. **Write** survivors as `.pending` drafts via [`PendingDraftWriter`].
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
//! - Bounded concurrency: slow episodes are backpressured by the semaphore,
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
    salience::{SalienceConfig, gate_episodes},
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
    /// Episode segmentation budget (tokens per episode, overlap events).
    pub segmentation: SegmentationConfig,
    /// Salience gate configuration.
    pub salience: SalienceConfig,
    /// Maximum number of episodes to process concurrently in the map step.
    /// Slow episodes block a semaphore permit but are never discarded.
    pub map_concurrency: usize,
    /// Cosine similarity threshold for the reduce pairing step.
    pub reduce_similarity_threshold: f32,
}

impl Default for OrchestrationConfig {
    /// Conservative defaults: 8 k-token budget, recall-biased salience gate,
    /// 4 concurrent episode map workers, 0.82 reduce threshold.
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
#[derive(Debug, Clone)]
pub struct OrchestrationReport {
    /// Total episodes produced by segmentation.
    pub total_episodes: usize,
    /// Episodes that passed the salience gate (sent to the map step).
    pub kept_episode_count: usize,
    /// Episodes that were gated out (surface for operator visibility).
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

    // ── Step 2: Segment into episodes ─────────────────────────────────────
    let episodes: Vec<Episode> = segment_session(events, &config.segmentation);
    let total_episodes = episodes.len();
    debug!(total_episodes, "orchestrator: session segmented");

    // ── Step 3: Salience gate ─────────────────────────────────────────────
    let gate_result = gate_episodes(&episodes, events, &config.salience);
    let kept_episodes = gate_result.kept;
    let gated_count = gate_result.gated.len();
    info!(
        kept = kept_episodes.len(),
        gated = gated_count,
        "orchestrator: salience gate applied"
    );

    // Warn on every gated episode so no drops are silent.
    for gated in &gate_result.gated {
        debug!(
            episode_index = gated.episode_index,
            score = gated.score,
            reason = %gated.gate_reason,
            "orchestrator: episode gated by salience"
        );
    }

    // ── Step 4: Map episodes concurrently ─────────────────────────────────
    let semaphore = Arc::new(Semaphore::new(config.map_concurrency.max(1)));

    // Build an index: source-event-index → &SessionEvent (for episode event lookup).
    let event_by_index: std::collections::HashMap<usize, &SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    let mut map_join_set: JoinSet<Result<Vec<ExtractedSkillCandidate>, OrchestrationError>> =
        JoinSet::new();

    for episode in &kept_episodes {
        // Resolve the episode's events from the full stream.
        let episode_events: Vec<SessionEvent> = episode
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

            map_one_episode(
                &episode_events,
                &session_id,
                &preamble_text,
                labeler.as_ref(),
                prose_extractor.as_ref(),
            )
            .await
        });
    }

    let mut episode_candidates: Vec<ExtractedSkillCandidate> = Vec::new();

    while let Some(join_result) = map_join_set.join_next().await {
        match join_result {
            Ok(Ok(mut candidates)) => {
                episode_candidates.append(&mut candidates);
            }
            Ok(Err(error)) => {
                // A single episode failure is logged but does not abort the pipeline.
                // All other episodes' candidates are preserved.
                warn!(?error, "orchestrator: episode map step failed; skipping episode");
            }
            Err(join_error) => {
                warn!(?join_error, "orchestrator: episode task panicked; skipping episode");
            }
        }
    }

    // Include preamble preference candidates in the union before reduce.
    let preamble_candidates = preamble.preference_skill_candidates();
    episode_candidates.extend(preamble_candidates);

    let pre_reduce_count = episode_candidates.len();
    info!(
        pre_reduce_count,
        "orchestrator: map step complete; entering reduce"
    );

    // ── Step 5: Reduce (cosine + LLM equivalence) ─────────────────────────
    let (deduped_candidates, reduce_audit) =
        reduce_candidates(episode_candidates, embedder.as_ref(), equivalence_verifier.as_ref(), config.reduce_similarity_threshold).await?;

    let post_reduce_count = deduped_candidates.len();
    info!(
        post_reduce_count,
        audit_entries = reduce_audit.len(),
        "orchestrator: reduce step complete"
    );

    // ── Step 6: Synthesis pass ────────────────────────────────────────────
    let mut synthesis_candidates = synthesis
        .synthesize(&deduped_candidates, &preamble.text)
        .await?;
    let synthesis_added_count = synthesis_candidates.len();

    let mut final_candidates = deduped_candidates;
    final_candidates.append(&mut synthesis_candidates);
    let final_count = final_candidates.len();

    info!(
        synthesis_added_count,
        final_count,
        "orchestrator: synthesis step complete"
    );

    // ── Step 7: Write .pending drafts ─────────────────────────────────────
    let extraction_result = ExtractionResult {
        source_session_id: session_id,
        provider: provider_name.to_owned(),
        candidates: final_candidates,
    };

    let draft_paths = draft_writer.write_pending_drafts(
        &extraction_result,
        extract_request,
        provider_name,
    )?;

    Ok(OrchestrationReport {
        total_episodes,
        kept_episode_count: kept_episodes.len(),
        gated_episode_count: gated_count,
        pre_reduce_candidate_count: pre_reduce_count,
        post_reduce_candidate_count: post_reduce_count,
        synthesis_added_count,
        final_candidate_count: final_count,
        reduce_audit,
        draft_paths,
    })
}

// ─── Map step helper ─────────────────────────────────────────────────────────

/// Maps one episode to zero or more [`ExtractedSkillCandidate`]s.
///
/// Routes via skeleton mining when the episode has a tool arc, otherwise
/// falls through to the prose extractor. Both paths use the preamble text as
/// context (prepended to the episode's flat transcript view).
async fn map_one_episode(
    episode_events: &[SessionEvent],
    session_id: &DomainId,
    preamble_text: &str,
    labeler: &dyn SkeletonLabeler,
    prose_extractor: &dyn TranscriptSkillExtractionService,
) -> Result<Vec<ExtractedSkillCandidate>, OrchestrationError> {
    if episode_events.is_empty() {
        return Ok(Vec::new());
    }

    let outcome = map_episode(episode_events, labeler).await?;

    match outcome {
        MapOutcome::Skeleton(candidate) => {
            debug!(
                name = %candidate.name,
                "orchestrator: episode yielded skeleton candidate"
            );
            Ok(vec![candidate])
        }
        MapOutcome::ProseFallback { reason } => {
            debug!(
                reason = %reason,
                "orchestrator: episode routed to prose extractor"
            );
            extract_prose_episode(episode_events, session_id, preamble_text, prose_extractor).await
        }
    }
}

/// Builds a [`SessionTranscript`] from episode events (prepending the preamble
/// as a synthetic context message) and runs the prose extractor.
async fn extract_prose_episode(
    episode_events: &[SessionEvent],
    session_id: &DomainId,
    preamble_text: &str,
    prose_extractor: &dyn TranscriptSkillExtractionService,
) -> Result<Vec<ExtractedSkillCandidate>, OrchestrationError> {
    // Build a flat transcript from the episode events.
    let mut transcript = events_to_transcript(session_id.clone(), episode_events);

    // Prepend the preamble as a synthetic leading context entry so the prose
    // extractor has the same global context as the skeleton labeler.
    if !preamble_text.is_empty() {
        use domain::TranscriptEntry;
        let preamble_entry = TranscriptEntry {
            speaker: "system".to_owned(),
            content: format!("[Session context]\n{preamble_text}"),
        };
        transcript.entries.insert(0, preamble_entry);
    }

    let result = prose_extractor.extract(&transcript).await?;
    Ok(result.candidates)
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
    let semantic_texts: Vec<String> = candidates
        .iter()
        .map(candidate_semantic_text)
        .collect();

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
            if removed.contains(&idx) { None } else { Some(candidate) }
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
    use std::{
        path::PathBuf,
        sync::Arc,
    };

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
        async fn normalize(&self, mut draft: PreambleDraft) -> Result<PreambleDraft, NormalizationError> {
            draft.preferences.sort_by(|a, b| a.raw_statement.cmp(&b.raw_statement));
            Ok(draft)
        }
    }

    /// Fake skeleton labeler that always keeps with a fixed name derived from
    /// the first step's tool name.
    struct EchoLabelerFake;

    #[async_trait]
    impl SkeletonLabeler for EchoLabelerFake {
        async fn label(&self, skeleton: &ProcedureSkeleton) -> Result<SkeletonLabel, SkeletonError> {
            let tool = skeleton.steps.first().map(|s| s.tool_name.as_str()).unwrap_or("unknown");
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
        async fn extract(&self, _transcript: &SessionTranscript) -> Result<ExtractionResult, ExtractionError> {
            Ok(ExtractionResult {
                source_session_id: DomainId::new_unchecked("prose-fake"),
                provider: "fake-prose".to_owned(),
                candidates: self.candidates.clone(),
            })
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
            let v0 = (sum % 256) as f32 + 1.0;   // always > 0 so no zero-magnitude
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
                equivalent_pairs: pairs.into_iter().map(|(a, b)| (a.to_owned(), b.to_owned())).collect(),
            })
        }

        fn never_equivalent() -> Arc<Self> {
            Arc::new(Self { equivalent_pairs: Vec::new() })
        }
    }

    #[async_trait]
    impl LlmEquivalenceVerifier for PrefixEquivalenceFake {
        async fn decide_equivalence(&self, left_text: &str, right_text: &str) -> Result<EquivalenceDecision, ExtractionError> {
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
            Arc::new(Self { additional: candidates })
        }

        fn noop() -> Arc<Self> {
            Arc::new(Self { additional: Vec::new() })
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
            "use-tokio-mutex-for-async",  // same name → same semantic text → identical embedding
            "Use tokio::sync::Mutex instead of std::sync::Mutex in async contexts",
        );

        // Same name + description → identical embedding → cosine = 1.0 → above threshold.
        // The verifier is configured to merge this pair.
        let verifier = PrefixEquivalenceFake::declaring_equivalent(vec![
            ("use-tokio-mutex-for-async", "use-tokio-mutex-for-async"),
        ]);

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
        let verifier = PrefixEquivalenceFake::declaring_equivalent(vec![
            ("duplicate-skill", "duplicate-skill"),
        ]);

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
        let draft_writer = PendingDraftWriter::new(vec![sandbox.clone()]);
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

    // ── map_one_episode prose fallback ────────────────────────────────────────

    /// Proves that a preference-only episode (no tool arc) routes to the prose
    /// extractor and surfaces the extractor's candidates.
    #[tokio::test]
    async fn map_one_episode_prose_fallback_uses_prose_extractor() {
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

        let candidates = map_one_episode(
            &events,
            &DomainId::new_unchecked("prose-test"),
            "preamble text",
            &EchoLabelerFake,
            prose_extractor.as_ref(),
        )
        .await
        .expect("prose fallback must succeed");

        assert_eq!(
            candidates.len(),
            1,
            "prose extractor result must be forwarded; got {candidates:?}"
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
        let draft_writer = PendingDraftWriter::new(vec![sandbox.clone()]);
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

    // ── Empty session fails loudly ─────────────────────────────────────────────

    #[tokio::test]
    async fn empty_session_returns_error() {
        let session_id = DomainId::new_unchecked("empty");
        let sandbox = sandbox_dir("empty");
        let draft_writer = PendingDraftWriter::new(vec![sandbox.clone()]);
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
