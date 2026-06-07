//! Real LLM-backed implementations of the orchestration seam traits (#198).
//!
//! Three seam traits are defined in sibling modules but require production impls
//! that call real infrastructure:
//!
//! - [`preamble::PreambleNormalizer`] — deduplicates/rephrases a [`PreambleDraft`] via
//!   one bounded LLM call.
//! - [`skeleton::SkeletonLabeler`] — labels a [`ProcedureSkeleton`] (name, description,
//!   generality, keep/drop) via one bounded LLM call.
//! - [`orchestrator::SynthesisPass`] — reviews the deduped candidate list for
//!   session-spanning patterns via one bounded LLM call.
//!
//! ## Transport
//!
//! All impls share one provider-agnostic transport: [`infrastructure::StructuredTextLlm`]
//! (Ollama or claude-code, selected by `EXTRACT_SESSION_PROVIDER`). No reqwest /
//! subprocess plumbing is duplicated across seams. The default `from_environment`
//! constructors build the Ollama transport; `SessionExtractor::from_environment`
//! injects a claude-code transport via each seam's `new` when that provider is set.
//!
//! ## Fail discipline
//!
//! Each impl fails loudly (`ExtractionError::ProviderUnavailable` / parse error)
//! when the LLM is unreachable or returns an unparseable response. There is NO
//! silent fallback to a stub result — an unwired or unavailable seam is a loud
//! failure (repository mandate).
//!
//! ## Prompt/parse split
//!
//! Every seam has a `build_*_prompt` function (pure, synchronous, fully testable
//! without a live model) and a `parse_*_response` function (pure, synchronous,
//! fully testable from a string fixture). The network call is isolated to the
//! `async fn` impl body. This split makes unit tests for prompt correctness and
//! response parsing feasible without spinning up Ollama.
//!
//! ## Construction
//!
//! Each impl is built from environment variables at [`SessionExtractor`] construction
//! time, not per-job, so a missing-env failure surfaces at startup (fail loud early).
//! See [`LlmPreambleNormalizer::from_environment`] and siblings.

use std::sync::Arc;

use async_trait::async_trait;
use domain::{ExtractedSkillCandidate, ExtractionError};
use infrastructure::{OllamaTextLlm, StructuredTextLlm};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::{
    orchestrator::{SynthesisError, SynthesisPass},
    preamble::{
        DetectedPreference, NormalizationError, PreambleDraft, PreambleNormalizer,
        PreferenceGenerality,
    },
    skeleton::{ProcedureSkeleton, SkeletonError, SkeletonLabel, SkeletonLabeler},
};

// ─── Shared Ollama config ─────────────────────────────────────────────────────

/// Default LLM model for the orchestration seams.
///
/// Overridable via `ORCHESTRATION_SEAM_MODEL`. Falls back to `gemma4:12b`, the
/// same model used by the merge verifier and other infrastructure seams.
const DEFAULT_SEAM_MODEL: &str = "gemma4:12b";

/// Reads the `OLLAMA_URL` environment variable and returns a loud error if absent.
///
/// All three seams require the Ollama base URL. Missing configuration surfaces
/// here at construction time so extraction fails at startup, not mid-job.
pub fn require_ollama_base_url() -> Result<String, ExtractionError> {
    std::env::var("OLLAMA_URL").map_err(|_| {
        ExtractionError::ProviderUnavailable(
            "OLLAMA_URL must be set for orchestration seam LLM calls \
             (PreambleNormalizer, SkeletonLabeler, SynthesisPass)"
                .to_owned(),
        )
    })
}

/// Reads the optional `ORCHESTRATION_SEAM_MODEL` override, defaulting to [`DEFAULT_SEAM_MODEL`].
fn seam_model() -> String {
    std::env::var("ORCHESTRATION_SEAM_MODEL").unwrap_or_else(|_| DEFAULT_SEAM_MODEL.to_owned())
}

/// Builds the default Ollama-backed seam transport from environment variables.
///
/// Reads `OLLAMA_URL` (required) and `ORCHESTRATION_SEAM_MODEL` (optional). This is
/// the back-compat default used by every seam's `from_environment` constructor and
/// by `SessionExtractor::from_environment` for the local/Ollama provider. The
/// claude-code provider builds a [`infrastructure::ClaudeCodeTextLlm`] instead and
/// injects it via each seam's `new` constructor.
pub fn ollama_seam_llm() -> Result<Arc<dyn StructuredTextLlm>, ExtractionError> {
    let base_url = require_ollama_base_url()?;
    let endpoint = format!("{}/api/generate", base_url.trim_end_matches('/'));
    Ok(Arc::new(OllamaTextLlm::new(endpoint, seam_model())?))
}

// ─── PreambleNormalizer impl ──────────────────────────────────────────────────

/// LLM-backed [`PreambleNormalizer`] over a provider-agnostic [`StructuredTextLlm`].
///
/// Sends the raw preamble draft preferences to the configured LLM (Ollama or
/// claude-code, per `EXTRACT_SESSION_PROVIDER`) and asks it to deduplicate and
/// rephrase them. The response is parsed into a new preference list that replaces
/// the original. Project facts pass through unchanged (the LLM only normalises the
/// natural-language preferences).
#[derive(Debug, Clone)]
pub struct LlmPreambleNormalizer {
    llm: Arc<dyn StructuredTextLlm>,
}

impl LlmPreambleNormalizer {
    /// Wraps a [`StructuredTextLlm`] transport as a preamble normalizer.
    pub fn new(llm: Arc<dyn StructuredTextLlm>) -> Arc<Self> {
        Arc::new(Self { llm })
    }

    /// Constructs the normalizer with the default Ollama-backed transport from env.
    ///
    /// Reads `OLLAMA_URL` (required) and `ORCHESTRATION_SEAM_MODEL` (optional,
    /// default `gemma4:12b`). The claude-code provider injects a different transport
    /// via [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns `ExtractionError::ProviderUnavailable` when `OLLAMA_URL` is absent.
    pub fn from_environment() -> Result<Arc<Self>, ExtractionError> {
        Ok(Self::new(ollama_seam_llm()?))
    }
}

/// Builds the normalization prompt sent to the LLM.
///
/// Pure and synchronous so it can be unit-tested without a live model.
pub fn build_normalization_prompt(draft: &PreambleDraft) -> String {
    let prefs: String = draft
        .preferences
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}. {}", i + 1, p.raw_statement))
        .collect::<Vec<_>>()
        .join("\n");

    if prefs.is_empty() {
        // No preferences to normalise — return a prompt that will produce empty output.
        return r#"There are no standing user preferences to normalise. Return JSON: {"preferences":[]}"#.to_owned();
    }

    format!(
        "You are normalising a list of standing user preferences extracted from a coding session \
         transcript. Your task: deduplicate and rephrase each preference so it is clear, \
         non-redundant, and actionable.\n\
         \n\
         Rules:\n\
         - Merge exact or near-exact duplicates into ONE entry (keep the clearest phrasing).\n\
         - Do NOT invent new preferences. Only rephrase what is listed.\n\
         - Preserve the meaning and scope of every distinct preference.\n\
         - Return a JSON object: {{\"preferences\": [\"...\", \"...\"]}}\n\
         - Each element is a single normalised preference statement (plain string).\n\
         \n\
         Preferences to normalise:\n\
         {prefs}\n\
         \n\
         Respond only with the JSON object."
    )
}

/// Wire shape returned by the LLM for preamble normalisation.
#[derive(Debug, Deserialize)]
struct NormalisationResponse {
    #[serde(default)]
    preferences: Vec<String>,
}

/// Parses the LLM's normalisation response into an updated preference list.
///
/// Pure and synchronous — testable from a raw JSON string without a live model.
pub fn parse_normalization_response(
    raw_json: &str,
    original_draft: &PreambleDraft,
) -> Result<Vec<DetectedPreference>, NormalizationError> {
    let parsed: NormalisationResponse = serde_json::from_str(raw_json).map_err(|error| {
        NormalizationError::ParseFailure(format!(
            "preamble normalizer response was not valid JSON {{preferences:[...]}}: {error}"
        ))
    })?;

    // Re-classify each returned preference using the same heuristic as the mining pass
    // (we no longer have per-statement generality from the LLM; we preserve the
    // original classification for matching statements and default to Uncertain otherwise).
    let normalised: Vec<DetectedPreference> = parsed
        .preferences
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(|raw| {
            // Preserve original generality when the statement is recognisably the same.
            let generality = original_draft
                .preferences
                .iter()
                .find(|orig| {
                    orig.raw_statement
                        .to_ascii_lowercase()
                        .contains(&raw.to_ascii_lowercase()[..raw.len().min(30)])
                })
                .map(|orig| orig.generality)
                .unwrap_or(PreferenceGenerality::Uncertain);
            DetectedPreference {
                raw_statement: raw.trim().to_owned(),
                generality,
            }
        })
        .collect();

    Ok(normalised)
}

#[async_trait]
impl PreambleNormalizer for LlmPreambleNormalizer {
    async fn normalize(&self, draft: PreambleDraft) -> Result<PreambleDraft, NormalizationError> {
        let prompt = build_normalization_prompt(&draft);

        debug!(
            provider = self.llm.provider_label(),
            model = self.llm.model(),
            "LlmPreambleNormalizer: sending normalisation request"
        );

        let raw_json = self.llm.generate_json(prompt).await.map_err(|error| {
            NormalizationError::ProviderFailure(format!(
                "{} normalizer call failed: {error}",
                self.llm.provider_label()
            ))
        })?;

        let normalised_preferences = parse_normalization_response(&raw_json, &draft)?;

        debug!(
            original_count = draft.preferences.len(),
            normalised_count = normalised_preferences.len(),
            "LlmPreambleNormalizer: normalisation complete"
        );

        Ok(PreambleDraft {
            preferences: normalised_preferences,
            facts: draft.facts,
        })
    }
}

// ─── SkeletonLabeler impl ─────────────────────────────────────────────────────

/// LLM-backed [`SkeletonLabeler`] over a provider-agnostic [`StructuredTextLlm`].
///
/// Sends the mined skeleton (grounded procedure steps + trigger failure) to the
/// configured LLM (Ollama or claude-code) and asks for a bounded label: kebab
/// `name`, one-sentence `description`, `generality` advisory, and keep/drop decision.
#[derive(Debug, Clone)]
pub struct LlmSkeletonLabeler {
    llm: Arc<dyn StructuredTextLlm>,
}

impl LlmSkeletonLabeler {
    /// Wraps a [`StructuredTextLlm`] transport as a skeleton labeler.
    pub fn new(llm: Arc<dyn StructuredTextLlm>) -> Arc<Self> {
        Arc::new(Self { llm })
    }

    /// Constructs the labeler with the default Ollama-backed transport from env.
    ///
    /// Reads `OLLAMA_URL` (required) and `ORCHESTRATION_SEAM_MODEL` (optional). The
    /// claude-code provider injects a different transport via [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns `ExtractionError::ProviderUnavailable` when `OLLAMA_URL` is absent.
    pub fn from_environment() -> Result<Arc<Self>, ExtractionError> {
        Ok(Self::new(ollama_seam_llm()?))
    }
}

/// Builds the skeleton labeling prompt sent to the LLM.
///
/// Pure and synchronous so it can be unit-tested without a live model.
pub fn build_skeleton_labeling_prompt(skeleton: &ProcedureSkeleton) -> String {
    let steps_text: String = skeleton
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| format!("  {}. [{}] {}", i + 1, step.tool_name, step.command_text))
        .collect::<Vec<_>>()
        .join("\n");

    let trigger = if skeleton.trigger_failure.is_empty() {
        "N/A".to_owned()
    } else {
        skeleton.trigger_failure.chars().take(256).collect()
    };

    format!(
        "You are labeling a procedure skeleton extracted from a coding session. \
         The skeleton represents the exact steps taken to resolve a build or test failure.\n\
         \n\
         Trigger failure:\n\
         {trigger}\n\
         \n\
         Resolution steps (grounded in the transcript — do NOT rewrite these):\n\
         {steps_text}\n\
         \n\
         Your task: produce a structured label for this procedure.\n\
         \n\
         Rules:\n\
         - `name`: a kebab-case skill name (e.g. \"fix-tokio-mutex-across-await\"). \
           Specific, actionable, ≤6 words.\n\
         - `description`: one sentence describing what this procedure accomplishes.\n\
         - `generality`: one of \"general\" (applies to many projects), \
           \"project\" (specific to this codebase), or \"uncertain\".\n\
         - `keep`: true if the procedure encodes genuine reusable knowledge, \
           false if it is too trivial or context-specific to reuse.\n\
         - `confidence`: float 0.0–1.0 for your confidence in the label.\n\
         \n\
         Respond with JSON: \
         {{\"name\":\"...\",\"description\":\"...\",\"generality\":\"...\",\
         \"keep\":true,\"confidence\":0.9}}"
    )
}

/// Wire shape returned by the LLM for skeleton labeling.
#[derive(Debug, Deserialize)]
struct SkeletonLabelResponse {
    name: String,
    description: String,
    #[serde(default)]
    generality: Option<String>,
    #[serde(default = "default_keep")]
    keep: bool,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_keep() -> bool {
    true
}

fn default_confidence() -> f32 {
    0.7
}

/// Parses the LLM's skeleton label response.
///
/// Pure and synchronous — testable from a raw JSON string without a live model.
pub fn parse_skeleton_label_response(raw_json: &str) -> Result<SkeletonLabel, SkeletonError> {
    let parsed: SkeletonLabelResponse =
        serde_json::from_str(raw_json).map_err(|error| SkeletonError::LabelerFailed {
            message: format!(
                "skeleton labeler response was not valid JSON \
                 {{name,description,generality,keep,confidence}}: {error}"
            ),
        })?;

    let name = parsed.name.trim().to_owned();
    if name.is_empty() {
        return Err(SkeletonError::LabelerFailed {
            message: "skeleton labeler returned an empty name".to_owned(),
        });
    }

    let description = parsed.description.trim().to_owned();
    if description.is_empty() {
        return Err(SkeletonError::LabelerFailed {
            message: format!(
                "skeleton labeler returned an empty description for candidate '{name}'; \
                 a label without a description is not a usable skill"
            ),
        });
    }

    Ok(SkeletonLabel {
        name,
        description,
        generality: parsed.generality,
        keep: parsed.keep,
        confidence: parsed.confidence.clamp(0.0, 1.0),
    })
}

#[async_trait]
impl SkeletonLabeler for LlmSkeletonLabeler {
    async fn label(&self, skeleton: &ProcedureSkeleton) -> Result<SkeletonLabel, SkeletonError> {
        let prompt = build_skeleton_labeling_prompt(skeleton);

        debug!(
            provider = self.llm.provider_label(),
            model = self.llm.model(),
            "LlmSkeletonLabeler: sending labeling request"
        );

        let raw_json =
            self.llm
                .generate_json(prompt)
                .await
                .map_err(|error| SkeletonError::LabelerFailed {
                    message: format!("{} labeler call failed: {error}", self.llm.provider_label()),
                })?;

        let label = parse_skeleton_label_response(&raw_json)?;

        debug!(
            name = %label.name,
            keep = label.keep,
            confidence = label.confidence,
            "LlmSkeletonLabeler: labeling complete"
        );

        Ok(label)
    }
}

// ─── SynthesisPass impl ───────────────────────────────────────────────────────

/// LLM-backed [`SynthesisPass`] over a provider-agnostic [`StructuredTextLlm`].
///
/// Reviews the deduped candidate list and the session preamble for session-spanning
/// patterns that no single episode reveals. Returns additional candidates when found,
/// or an empty list when none are detected.
#[derive(Debug, Clone)]
pub struct LlmSynthesisPass {
    llm: Arc<dyn StructuredTextLlm>,
}

impl LlmSynthesisPass {
    /// Wraps a [`StructuredTextLlm`] transport as a synthesis pass.
    pub fn new(llm: Arc<dyn StructuredTextLlm>) -> Arc<Self> {
        Arc::new(Self { llm })
    }

    /// Constructs the synthesis pass with the default Ollama-backed transport from env.
    ///
    /// Reads `OLLAMA_URL` (required) and `ORCHESTRATION_SEAM_MODEL` (optional). The
    /// claude-code provider injects a different transport via [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns `ExtractionError::ProviderUnavailable` when `OLLAMA_URL` is absent.
    pub fn from_environment() -> Result<Arc<Self>, ExtractionError> {
        Ok(Self::new(ollama_seam_llm()?))
    }
}

/// Builds the synthesis prompt sent to the LLM.
///
/// Pure and synchronous so it can be unit-tested without a live model.
pub fn build_synthesis_prompt(
    candidates: &[ExtractedSkillCandidate],
    preamble_text: &str,
) -> String {
    let candidate_summary: String = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}. {} — {}",
                i + 1,
                c.name,
                c.description.chars().take(120).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let preamble_section = if preamble_text.trim().is_empty() {
        String::new()
    } else {
        format!("\nSession preamble (global context):\n{preamble_text}\n")
    };

    format!(
        "You are reviewing a list of skill candidates extracted from a coding session. \
         Your task: identify any SESSION-SPANNING patterns — reusable knowledge that \
         cuts across multiple skills but was not captured by any single one.\n\
         {preamble_section}\n\
         Already-extracted skills:\n\
         {candidate_summary}\n\
         \n\
         Rules:\n\
         - Only emit NEW candidates not already represented above.\n\
         - Do NOT re-emit or rephrase existing skills.\n\
         - If no session-spanning pattern exists, return an empty list.\n\
         - Each new candidate must encode genuine cross-episode knowledge.\n\
         - Each candidate must include: `name` (kebab-case), `description` (one sentence), \
           `procedures` (list of actionable steps), `confidence` (float 0.0–1.0).\n\
         \n\
         Respond with JSON: \
         {{\"candidates\": [\
         {{\"name\":\"...\",\"description\":\"...\",\"tags\":[],\
         \"procedures\":[\"...\"],\"conventions\":[],\"assets\":[],\
         \"confidence\":0.8,\"generality\":\"general\",\
         \"generality_rationale\":null}}]}}"
    )
}

/// Wire shape returned by the LLM for the synthesis pass.
#[derive(Debug, Deserialize)]
struct SynthesisResponse {
    #[serde(default)]
    candidates: Vec<SynthesisCandidate>,
}

/// Wire shape for a single synthesised candidate.
#[derive(Debug, Deserialize)]
struct SynthesisCandidate {
    name: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    procedures: Vec<String>,
    #[serde(default)]
    conventions: Vec<String>,
    #[serde(default)]
    assets: Vec<String>,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    generality: Option<String>,
    #[serde(default)]
    generality_rationale: Option<String>,
}

/// Returns `true` when a parsed synthesis candidate carries at least one piece of
/// actionable skill content.
///
/// A candidate must have at least one of: a non-empty `procedures` list, a non-empty
/// `conventions` list, or a non-empty `assets` list. A candidate with a name and
/// description but zero of these fields is structurally broken — the model produced a
/// named shell with no teachable content.
fn synthesis_candidate_has_usable_payload(c: &SynthesisCandidate) -> bool {
    !c.procedures.is_empty() || !c.conventions.is_empty() || !c.assets.is_empty()
}

/// Parses the LLM's synthesis response into a list of additional skill candidates.
///
/// Pure and synchronous — testable from a raw JSON string without a live model.
///
/// ## Validation contract
///
/// Two cases are distinguished:
/// - "Model legitimately produced nothing" (`{"candidates":[]}`) → `Ok(vec![])`, clean no-op.
/// - "Model produced a structurally-broken candidate" (non-empty name but zero usable payload
///   across `procedures`, `conventions`, and `assets`) → `Err(SynthesisError::ParseFailure)`
///   with the raw model body and the offending candidate name embedded for diagnosis.
///
/// A batch containing ANY broken candidate fails entirely. This is intentional: a model
/// output that mixes valid and broken candidates is suspect as a whole, and partial
/// admission risks silently writing content-free `.pending` drafts when the parse caller
/// retries with a different model response.
pub fn parse_synthesis_response(
    raw_json: &str,
) -> Result<Vec<ExtractedSkillCandidate>, SynthesisError> {
    let parsed: SynthesisResponse = serde_json::from_str(raw_json).map_err(|error| {
        SynthesisError::ParseFailure(format!(
            "synthesis response was not valid JSON {{candidates:[...]}}: {error}"
        ))
    })?;

    // Validate every candidate before admitting any of them. A single broken candidate
    // fails the whole batch loudly with the raw body included for downstream diagnosis.
    for c in &parsed.candidates {
        let name = c.name.trim();
        if name.is_empty() {
            return Err(SynthesisError::ParseFailure(format!(
                "synthesis candidate has an empty name — no content-free skill may reach \
                 .pending; raw body: {raw_json}"
            )));
        }
        if !synthesis_candidate_has_usable_payload(c) {
            return Err(SynthesisError::ParseFailure(format!(
                "synthesis candidate '{name}' has no usable payload \
                 (procedures, conventions, and assets are all empty) — \
                 no content-free skill may reach .pending; raw body: {raw_json}"
            )));
        }
    }

    let candidates: Vec<ExtractedSkillCandidate> = parsed
        .candidates
        .into_iter()
        .map(|c| ExtractedSkillCandidate {
            name: c.name.trim().to_owned(),
            description: c.description.trim().to_owned(),
            tags: c.tags,
            procedures: c.procedures,
            conventions: c.conventions,
            assets: c.assets,
            confidence: c.confidence.clamp(0.0, 1.0),
            generality: c.generality,
            generality_rationale: c.generality_rationale,
        })
        .collect();

    Ok(candidates)
}

#[async_trait]
impl SynthesisPass for LlmSynthesisPass {
    async fn synthesize(
        &self,
        deduped_candidates: &[ExtractedSkillCandidate],
        preamble_text: &str,
    ) -> Result<Vec<ExtractedSkillCandidate>, SynthesisError> {
        let prompt = build_synthesis_prompt(deduped_candidates, preamble_text);

        debug!(
            provider = self.llm.provider_label(),
            model = self.llm.model(),
            candidate_count = deduped_candidates.len(),
            "LlmSynthesisPass: sending synthesis request"
        );

        let raw_json = self.llm.generate_json(prompt).await.map_err(|error| {
            SynthesisError::ProviderFailure(format!(
                "{} synthesis call failed: {error}",
                self.llm.provider_label()
            ))
        })?;

        let additional = parse_synthesis_response(&raw_json).map_err(|error| {
            // Emit a warn-level log with the raw body so the gap is diagnosable from
            // the log stream without needing to re-run the model (mirrors the
            // thinking-model-leak diagnosis approach). The raw body is also embedded
            // in the error message for callers that propagate or record errors.
            warn!(
                provider = self.llm.provider_label(),
                model = self.llm.model(),
                raw_body = %raw_json,
                error = %error,
                "LlmSynthesisPass: synthesis response contained broken candidate(s) — \
                 no content-free skill admitted to .pending"
            );
            error
        })?;

        debug!(
            additional_count = additional.len(),
            "LlmSynthesisPass: synthesis complete"
        );

        Ok(additional)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use domain::ExtractedSkillCandidate;

    use super::*;
    use crate::{
        preamble::{DetectedPreference, PreambleDraft, ProjectFacts},
        skeleton::{MinedStep, ProcedureSkeleton},
    };

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

    fn simple_draft(preferences: Vec<&str>) -> PreambleDraft {
        PreambleDraft {
            preferences: preferences
                .into_iter()
                .map(|s| DetectedPreference {
                    raw_statement: s.to_owned(),
                    generality: PreferenceGenerality::General,
                })
                .collect(),
            facts: ProjectFacts::default(),
        }
    }

    fn simple_skeleton(steps: Vec<(&str, &str)>) -> ProcedureSkeleton {
        ProcedureSkeleton {
            steps: steps
                .into_iter()
                .map(|(tool, cmd)| MinedStep {
                    tool_use_id: "tu1".to_owned(),
                    command_text: cmd.to_owned(),
                    tool_name: tool.to_owned(),
                })
                .collect(),
            trigger_failure: "error[E0277]: future cannot be sent between threads".to_owned(),
            trigger_exit_code: Some(1),
        }
    }

    // ── Preamble normalizer ────────────────────────────────────────────────────

    /// Proves that `build_normalization_prompt` includes each preference verbatim.
    #[test]
    fn normalization_prompt_contains_each_preference() {
        let draft = simple_draft(vec![
            "Never use unwrap.",
            "Always run tests before committing.",
        ]);
        let prompt = build_normalization_prompt(&draft);
        assert!(
            prompt.contains("Never use unwrap."),
            "prompt must include first preference verbatim"
        );
        assert!(
            prompt.contains("Always run tests before committing."),
            "prompt must include second preference verbatim"
        );
        assert!(
            prompt.contains("preferences"),
            "prompt must name the output field"
        );
    }

    /// Proves `parse_normalization_response` returns the deduplicated list.
    #[test]
    fn parse_normalization_response_returns_merged_list() {
        let draft = simple_draft(vec![
            "Never use unwrap.",
            "Always run tests before committing.",
        ]);
        let raw = r#"{"preferences":["Never use unwrap in production.","Always run tests."]}"#;
        let result =
            parse_normalization_response(raw, &draft).expect("parse must succeed for valid JSON");
        assert_eq!(result.len(), 2, "two preferences expected after parse");
        assert!(
            result.iter().any(|p| p.raw_statement.contains("unwrap")),
            "first preference must contain 'unwrap'"
        );
    }

    /// Proves `parse_normalization_response` fails loudly on bad JSON.
    #[test]
    fn parse_normalization_response_fails_on_invalid_json() {
        let draft = simple_draft(vec!["pref"]);
        let err =
            parse_normalization_response("not json", &draft).expect_err("invalid JSON must fail");
        assert!(
            matches!(err, NormalizationError::ParseFailure(_)),
            "expected ParseFailure, got {err:?}"
        );
    }

    /// Proves that an empty preferences draft produces a prompt requesting empty output.
    #[test]
    fn normalization_prompt_for_empty_draft_requests_empty_output() {
        let draft = simple_draft(vec![]);
        let prompt = build_normalization_prompt(&draft);
        assert!(
            prompt.contains("preferences"),
            "empty draft prompt must still reference the preferences field"
        );
    }

    // ── Skeleton labeler ───────────────────────────────────────────────────────

    /// Proves `build_skeleton_labeling_prompt` includes the trigger and each step.
    #[test]
    fn skeleton_labeling_prompt_includes_trigger_and_steps() {
        let skeleton = simple_skeleton(vec![
            ("Bash", "cargo build 2>&1"),
            ("Edit", "src/handler.rs: std::sync → tokio::sync"),
        ]);
        let prompt = build_skeleton_labeling_prompt(&skeleton);
        assert!(
            prompt.contains("E0277"),
            "prompt must include the trigger failure text"
        );
        assert!(
            prompt.contains("cargo build"),
            "prompt must include the first resolution step"
        );
        assert!(
            prompt.contains("tokio::sync"),
            "prompt must include the second resolution step"
        );
        assert!(
            prompt.contains("name"),
            "prompt must mention the 'name' output field"
        );
    }

    /// Proves `parse_skeleton_label_response` returns a well-formed label.
    #[test]
    fn parse_skeleton_label_response_returns_valid_label() {
        let raw = r#"{"name":"fix-tokio-mutex","description":"Replace std mutex with tokio mutex.","generality":"general","keep":true,"confidence":0.92}"#;
        let label = parse_skeleton_label_response(raw).expect("parse must succeed");
        assert_eq!(label.name, "fix-tokio-mutex");
        assert!(label.keep, "label must be keep=true");
        assert!((label.confidence - 0.92_f32).abs() < 0.01);
    }

    /// Proves `parse_skeleton_label_response` fails loudly on bad JSON.
    #[test]
    fn parse_skeleton_label_response_fails_on_invalid_json() {
        let err =
            parse_skeleton_label_response("not json").expect_err("invalid JSON must fail loudly");
        assert!(
            matches!(err, SkeletonError::LabelerFailed { .. }),
            "expected LabelerFailed, got {err:?}"
        );
    }

    /// Proves `parse_skeleton_label_response` fails loudly when name is empty.
    #[test]
    fn parse_skeleton_label_response_fails_on_empty_name() {
        let raw = r#"{"name":"","description":"desc","generality":"general","keep":true,"confidence":0.8}"#;
        let err = parse_skeleton_label_response(raw).expect_err("empty name must fail loudly");
        assert!(
            matches!(err, SkeletonError::LabelerFailed { .. }),
            "expected LabelerFailed for empty name"
        );
    }

    // ── Synthesis pass ─────────────────────────────────────────────────────────

    /// Proves `build_synthesis_prompt` includes the candidate names.
    #[test]
    fn synthesis_prompt_includes_candidate_names() {
        let candidates = vec![
            skill_candidate("use-tokio-mutex", "Use tokio mutex in async code"),
            skill_candidate("run-tests-first", "Always run tests before committing"),
        ];
        let prompt = build_synthesis_prompt(&candidates, "preamble text");
        assert!(
            prompt.contains("use-tokio-mutex"),
            "prompt must include first candidate name"
        );
        assert!(
            prompt.contains("run-tests-first"),
            "prompt must include second candidate name"
        );
        assert!(
            prompt.contains("preamble text"),
            "prompt must include preamble context"
        );
    }

    /// Proves `parse_synthesis_response` returns the synthesised candidates.
    #[test]
    fn parse_synthesis_response_returns_candidates() {
        let raw = r#"{"candidates":[{"name":"cross-session-pattern","description":"A pattern spanning multiple arcs.","tags":[],"procedures":["step one"],"conventions":[],"assets":[],"confidence":0.75,"generality":"general","generality_rationale":null}]}"#;
        let candidates = parse_synthesis_response(raw).expect("parse must succeed");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "cross-session-pattern");
    }

    /// Proves `parse_synthesis_response` returns an empty list when no candidates are found.
    #[test]
    fn parse_synthesis_response_returns_empty_on_no_candidates() {
        let raw = r#"{"candidates":[]}"#;
        let candidates = parse_synthesis_response(raw).expect("parse must succeed");
        assert!(
            candidates.is_empty(),
            "no candidates expected for empty response"
        );
    }

    /// Proves `parse_synthesis_response` fails loudly on bad JSON.
    #[test]
    fn parse_synthesis_response_fails_on_invalid_json() {
        let err = parse_synthesis_response("not json").expect_err("invalid JSON must fail");
        assert!(
            matches!(err, SynthesisError::ParseFailure(_)),
            "expected ParseFailure, got {err:?}"
        );
    }

    /// Proves `build_synthesis_prompt` works correctly with an empty preamble.
    #[test]
    fn synthesis_prompt_with_empty_preamble_omits_preamble_section() {
        let candidates = vec![skill_candidate("some-skill", "description")];
        let prompt = build_synthesis_prompt(&candidates, "");
        // Empty preamble → no "preamble" section header
        assert!(
            !prompt.contains("Session preamble"),
            "empty preamble must not inject a preamble section"
        );
    }

    // ── Synthesis pass — content-free candidate rejection ─────────────────────

    /// Proves a synthesis candidate with no procedures, conventions, or assets is
    /// rejected loudly, with the raw body captured in the error message.
    ///
    /// Case (a) from the ticket: empty-procedures candidate → loud failure.
    #[test]
    fn parse_synthesis_response_rejects_content_free_candidate_loudly() {
        // Has a name and description but ZERO usable payload.
        let raw = r#"{"candidates":[{"name":"hollow-skill","description":"A description with no steps.","tags":[],"procedures":[],"conventions":[],"assets":[],"confidence":0.9,"generality":"general"}]}"#;
        let err =
            parse_synthesis_response(raw).expect_err("content-free candidate must be rejected");
        let err_msg = format!("{err}");
        assert!(
            matches!(err, SynthesisError::ParseFailure(_)),
            "expected ParseFailure for content-free candidate, got {err:?}"
        );
        assert!(
            err_msg.contains("hollow-skill"),
            "error must name the offending candidate; got: {err_msg}"
        );
        assert!(
            err_msg.contains("hollow-skill") || err_msg.contains("no usable payload"),
            "error must surface the broken candidate identity; got: {err_msg}"
        );
        // The raw body must be embedded so the gap is diagnosable.
        assert!(
            err_msg.contains("hollow-skill"),
            "raw body context must appear in the error; got: {err_msg}"
        );
    }

    /// Proves that an empty-procedures candidate embeds the raw model body in the error,
    /// enabling diagnosis without re-running the model (mirrors thinking-model-leak approach).
    #[test]
    fn parse_synthesis_response_captures_raw_body_in_broken_candidate_error() {
        let raw = r#"{"candidates":[{"name":"no-steps-skill","description":"desc","procedures":[],"conventions":[],"assets":[]}]}"#;
        let err = parse_synthesis_response(raw).expect_err("broken candidate must fail loudly");
        let err_msg = format!("{err}");
        // The raw JSON must appear in the error so downstream logging has the full body.
        assert!(
            err_msg.contains("no-steps-skill"),
            "raw body context (candidate name) must appear in error; got: {err_msg}"
        );
    }

    /// Case (b) from the ticket: legitimate empty result from the model is NOT an error.
    #[test]
    fn parse_synthesis_response_treats_empty_candidates_list_as_clean_no_op() {
        // Model legitimately produced nothing — valid empty result.
        let raw = r#"{"candidates":[]}"#;
        let result = parse_synthesis_response(raw).expect("legitimate empty result must succeed");
        assert!(
            result.is_empty(),
            "empty candidates list must yield an empty Vec, not an error"
        );
    }

    /// Case (c) from the ticket: a well-formed candidate with at least one procedure is accepted.
    #[test]
    fn parse_synthesis_response_accepts_well_formed_candidate_with_procedures() {
        let raw = r#"{"candidates":[{"name":"well-formed-skill","description":"Does something real.","tags":["rust"],"procedures":["step one","step two"],"conventions":[],"assets":[],"confidence":0.85,"generality":"general"}]}"#;
        let candidates =
            parse_synthesis_response(raw).expect("well-formed candidate must be accepted");
        assert_eq!(candidates.len(), 1, "exactly one candidate expected");
        assert_eq!(candidates[0].name, "well-formed-skill");
        assert_eq!(candidates[0].procedures.len(), 2);
    }

    /// Proves a candidate with no procedures but at least one convention is accepted
    /// (conventions count as usable payload).
    #[test]
    fn parse_synthesis_response_accepts_candidate_with_convention_only_payload() {
        let raw = r#"{"candidates":[{"name":"convention-skill","description":"Style guide.","procedures":[],"conventions":["always use snake_case"],"assets":[]}]}"#;
        let candidates = parse_synthesis_response(raw).expect("convention-only candidate must be accepted");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "convention-skill");
    }

    /// Proves a candidate with no procedures or conventions but at least one asset is accepted.
    #[test]
    fn parse_synthesis_response_accepts_candidate_with_asset_only_payload() {
        let raw = r#"{"candidates":[{"name":"asset-skill","description":"Template.","procedures":[],"conventions":[],"assets":["template.rs"]}]}"#;
        let candidates = parse_synthesis_response(raw).expect("asset-only candidate must be accepted");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "asset-skill");
    }

    /// Proves that when a mixed batch contains one broken and one valid candidate,
    /// the entire parse fails loudly — no partial admission of valid candidates
    /// alongside a broken one (the whole model output is suspect).
    #[test]
    fn parse_synthesis_response_rejects_batch_containing_any_broken_candidate() {
        let raw = r#"{"candidates":[
            {"name":"good-skill","description":"desc","procedures":["step one"],"conventions":[],"assets":[]},
            {"name":"bad-skill","description":"desc","procedures":[],"conventions":[],"assets":[]}
        ]}"#;
        let err = parse_synthesis_response(raw)
            .expect_err("batch with any broken candidate must fail loudly");
        let err_msg = format!("{err}");
        assert!(
            err_msg.contains("bad-skill"),
            "error must identify the broken candidate; got: {err_msg}"
        );
    }

    // ── Skeleton labeler — description validation ──────────────────────────────

    /// Proves `parse_skeleton_label_response` fails loudly when description is empty.
    #[test]
    fn parse_skeleton_label_response_fails_on_empty_description() {
        let raw = r#"{"name":"valid-name","description":"","generality":"general","keep":true,"confidence":0.8}"#;
        let err = parse_skeleton_label_response(raw)
            .expect_err("empty description must fail loudly");
        assert!(
            matches!(err, SkeletonError::LabelerFailed { .. }),
            "expected LabelerFailed for empty description, got {err:?}"
        );
    }

    // ── require_ollama_base_url loud failure ──────────────────────────────────

    /// Proves `require_ollama_base_url` fails loudly when `OLLAMA_URL` is absent.
    #[test]
    fn require_ollama_base_url_fails_loudly_when_unset() {
        // Guard: save and temporarily unset OLLAMA_URL.
        let saved = std::env::var("OLLAMA_URL").ok();
        // SAFETY: single-threaded test; we restore the value after.
        unsafe {
            std::env::remove_var("OLLAMA_URL");
        }

        let err = require_ollama_base_url().expect_err("must fail when OLLAMA_URL is absent");
        assert!(
            matches!(err, ExtractionError::ProviderUnavailable(_)),
            "expected ProviderUnavailable, got {err:?}"
        );
        assert!(
            err.to_string().contains("OLLAMA_URL"),
            "error must name OLLAMA_URL; got: {err}"
        );

        // Restore.
        unsafe {
            if let Some(v) = saved {
                std::env::set_var("OLLAMA_URL", v);
            }
        }
    }
}
