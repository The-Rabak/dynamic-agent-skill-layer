//! Deterministic procedure-skeleton mining + LLM-label split (#188).
//!
//! # Purpose
//! Small local models hallucinate when asked to "read this episode and invent a structured skill."
//! The real procedure steps are already sitting in the structured event log — the exact commands
//! that ran, their exit codes, and the edits that turned a failing result into a passing one.
//! This module mines those steps **deterministically** and reduces the LLM's job to a bounded
//! label transform: produce a kebab-case `name`, one-sentence `description`, a `generality` tag,
//! and a keep/drop confidence. The LLM never writes steps; steps come from the transcript.
//!
//! # Module layout
//! - [`ProcedureSkeleton`] — a mined skeleton: ordered, grounded steps + failure context.
//! - [`MinedStep`] — one grounded step, tracing to a concrete event by `tool_use_id`.
//! - [`SkeletonLabel`] — output of the bounded LLM label transform.
//! - [`SkeletonLabeler`] — async trait; production wiring is #187's job. Test fake is
//!   `#[cfg(test)]`-only.
//! - [`MapOutcome`] — the outcome of one episode's skeleton-mining pass: either a candidate
//!   ready for the reduce step or a prose-fallback signal for non-tool episodes.
//! - [`mine_skeleton`] — the pure deterministic miner; no I/O, no LLM.
//! - [`map_episode`] — async orchestrator: mine → label → assemble `ExtractedSkillCandidate`.
//!
//! # Hard invariants (enforced by design)
//! - Every step in a `ProcedureSkeleton` traces to a concrete `ToolCall`/`FileEdit` event via
//!   `tool_use_id`. The labeler never writes steps.
//! - `ToolCall` and `FileEdit` events sharing the same `tool_use_id` are deduped: only the
//!   `FileEdit` representation is kept (it carries the path and operation explicitly).
//! - Non-tool episodes (pure discussion / stated preferences) return `MapOutcome::ProseFallback`
//!   so #187 can route them to the existing prose extractor.

use async_trait::async_trait;
use domain::{ExtractedSkillCandidate, SessionEvent};

// ──────────────────────────────────────────────────────────────────────────────
// Data types
// ──────────────────────────────────────────────────────────────────────────────

/// One step in a mined procedure skeleton, grounded in a concrete session event.
///
/// Every `MinedStep` traces back to either a `ToolCall` or a `FileEdit` event via
/// `tool_use_id`. This is the grounding invariant: no step is ever authored by the LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinedStep {
    /// The `tool_use_id` from the originating event; used to prove grounding.
    pub tool_use_id: String,
    /// Human-readable command representation extracted from the event.
    ///
    /// For `Bash` tool calls this is the shell command string; for `FileEdit` events it
    /// includes the operation and path; for other tools it is `<tool_name>(<input_summary>)`.
    pub command_text: String,
    /// The tool name that produced this step (e.g. `"Bash"`, `"Edit"`, `"Write"`).
    pub tool_name: String,
}

/// A procedure skeleton mined deterministically from a tool-event arc.
///
/// The `steps` field contains the verbatim commands that constitute the resolution — exactly as
/// they appeared in the session log. The LLM only names this skeleton; it never writes steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureSkeleton {
    /// Ordered steps from the failing event to the passing event (inclusive).
    ///
    /// Every entry traces to a concrete session event. The vector is never empty when
    /// `mine_skeleton` returns a `Some`; an empty step list is always `None`.
    pub steps: Vec<MinedStep>,
    /// The first failure encountered that triggered this resolution arc.
    ///
    /// Carried verbatim from the failing `ToolResult`'s output (first 512 bytes) so the
    /// labeler can include the error context in the skill name/description without inventing it.
    pub trigger_failure: String,
    /// Exit code of the triggering failure, if present.
    pub trigger_exit_code: Option<i32>,
}

/// Label produced by the bounded LLM transform over one [`ProcedureSkeleton`].
///
/// The labeler's contract is narrow and explicit: name, describe, rate generality, decide keep/drop.
/// It never writes commands or procedure steps.
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonLabel {
    /// Kebab-case skill name (e.g. `"fix-tokio-mutex-across-await"`).
    pub name: String,
    /// One-sentence description of what this procedure accomplishes.
    pub description: String,
    /// Generality advisory: `"project"`, `"general"`, or `"uncertain"`.
    pub generality: Option<String>,
    /// Whether to keep this candidate (true) or drop it (false).
    pub keep: bool,
    /// Confidence in [0, 1] for the label decisions.
    pub confidence: f32,
}

/// Outcome of one episode's skeleton-mining pass.
///
/// The #187 map step inspects this variant to decide the extraction route:
/// - `Skeleton` → the candidate is ready for the reduce step.
/// - `ProseFallback` → the episode had no tool arc; route to the prose extractor.
#[derive(Debug, Clone, PartialEq)]
pub enum MapOutcome {
    /// A grounded candidate assembled from a mined skeleton + LLM label.
    Skeleton(ExtractedSkillCandidate),
    /// The episode contained no actionable tool arc; use the prose extractor instead.
    ///
    /// Carries a brief reason for observability/debugging.
    ProseFallback { reason: String },
}

/// Error type for the skeleton mining and labeling pipeline.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkeletonError {
    /// The LLM labeler rejected the skeleton or returned an unparseable response.
    #[error("labeler failed: {message}")]
    LabelerFailed { message: String },
}

// ──────────────────────────────────────────────────────────────────────────────
// Labeler trait (production seam — no fake implementation in this file)
// ──────────────────────────────────────────────────────────────────────────────

/// Bounded LLM label transform over a mined [`ProcedureSkeleton`].
///
/// Implementors receive the skeleton (ordered grounded steps + failure context) and produce
/// a [`SkeletonLabel`]: kebab `name`, one-sentence `description`, `generality` advisory, and
/// keep/drop + confidence.
///
/// # Contract
/// - The labeler MUST NOT write procedure steps or commands into the returned label.
///   Steps come from the transcript; the label only names and judges the skeleton.
/// - The labeler input is intentionally small (the skeleton summary), so it is fast and reliable
///   on small local models.
/// - Production wiring is #187's responsibility. Unit tests use a test-only fake behind
///   `#[cfg(test)]`.
#[async_trait]
pub trait SkeletonLabeler: Send + Sync {
    /// Labels a mined skeleton with a name, description, generality, and keep/drop judgment.
    async fn label(&self, skeleton: &ProcedureSkeleton) -> Result<SkeletonLabel, SkeletonError>;
}

// ──────────────────────────────────────────────────────────────────────────────
// Deterministic miner (pure — no I/O, no LLM)
// ──────────────────────────────────────────────────────────────────────────────

/// Mines a [`ProcedureSkeleton`] deterministically from a slice of session events.
///
/// # Algorithm
/// 1. Find the first `ToolResult` that is an error (is_error=true or exit_code≠0). This is the
///    "trigger failure" that opens a resolution arc.
/// 2. Collect all `ToolCall`/`FileEdit` events after the trigger failure, up to and including the
///    first `ToolResult` that succeeds (is_error=false, exit_code=0 or None). These are the
///    resolution steps.
/// 3. Deduplicate by `tool_use_id`: `FileEdit` wins over `ToolCall` for the same id (the file
///    edit carries richer information). Never double-count a step.
/// 4. Return `None` if no failing arc exists (the caller returns `ProseFallback` or skips).
///
/// # Grounding invariant
/// Every step in the returned skeleton traces to a concrete event by `tool_use_id`. The miner
/// never invents or synthesizes commands.
pub fn mine_skeleton(events: &[SessionEvent]) -> Option<ProcedureSkeleton> {
    // Step 1: find the first failing ToolResult.
    let trigger_index = events.iter().position(|event| match event {
        SessionEvent::ToolResult {
            is_error,
            exit_code,
            ..
        } => *is_error || exit_code.map_or(false, |code| code != 0),
        _ => false,
    })?;

    let trigger = match &events[trigger_index] {
        SessionEvent::ToolResult {
            output,
            exit_code,
            ..
        } => (output.clone(), *exit_code),
        _ => unreachable!("position filter guarantees ToolResult"),
    };
    let trigger_failure = truncate_failure_text(&trigger.0, 512);
    let trigger_exit_code = trigger.1;

    // Step 2: collect events after the trigger that constitute the resolution arc.
    //
    // A resolution arc runs from the trigger failure to the final successful `ToolResult`
    // that ends the run without a subsequent failure. We scan forward: each new failure
    // advances the arc's failure boundary; the last success in the episode becomes the
    // arc's closing boundary. This correctly handles sequences where intermediate
    // diagnostic tool calls succeed (e.g. grep finding a match) while the overall
    // resolution hasn't been confirmed yet.
    let arc_events = &events[trigger_index + 1..];

    // Find the position of the last successful ToolResult in the arc that is not followed
    // by another failure. Walk backwards from the end to find that position.
    let arc_end = find_resolution_arc_end(arc_events);

    let resolution_events = &arc_events[..arc_end];

    // Step 3: build steps, deduplicating by tool_use_id.
    // FileEdit wins over ToolCall for the same id.
    let steps = build_steps_deduplicated(resolution_events);

    if steps.is_empty() {
        return None;
    }

    Some(ProcedureSkeleton {
        steps,
        trigger_failure,
        trigger_exit_code,
    })
}

/// Finds the end boundary of the resolution arc within the post-trigger event slice.
///
/// A resolution arc ends at the **last** successful `ToolResult` that is not followed by a
/// subsequent failure. This correctly handles sequences where diagnostic calls (e.g. grep) succeed
/// mid-arc while the overall resolution has not yet been achieved: we do not prematurely cut at the
/// first intermediate success.
///
/// Returns the exclusive end index into `arc_events`. If no successful `ToolResult` exists,
/// returns `arc_events.len()` (include all remaining events).
fn find_resolution_arc_end(arc_events: &[SessionEvent]) -> usize {
    // Walk from the end: find the last ToolResult that succeeds and is not followed by a failure.
    // More precisely: the last success that comes after all failures have been resolved.
    //
    // Algorithm: scan forward tracking the most recent success position. Any time we see a
    // failure after a success, the success is no longer the closing boundary — the new failure
    // re-opens the arc. The final recorded success position is the arc end.
    let mut last_success_end: Option<usize> = None;

    for (pos, event) in arc_events.iter().enumerate() {
        match event {
            SessionEvent::ToolResult { is_error, exit_code, .. } => {
                let is_success = !is_error && exit_code.map_or(true, |code| code == 0);
                let is_failure = *is_error || exit_code.map_or(false, |code| code != 0);
                if is_success {
                    // Record as a candidate arc-end (may be superseded by a later failure).
                    last_success_end = Some(pos + 1);
                } else if is_failure {
                    // A new failure after a recorded success: the success was not the terminal one.
                    // Reset so we continue looking for the true arc end.
                    last_success_end = None;
                }
            }
            _ => {}
        }
    }

    last_success_end.unwrap_or(arc_events.len())
}

/// Builds an ordered list of [`MinedStep`]s from the resolution arc events, deduplicating by
/// `tool_use_id`. `FileEdit` wins over `ToolCall` for the same `tool_use_id` because the
/// file edit carries the path and operation explicitly, making it the richer representation.
///
/// Events are visited in source-line order (by their `index` field). Steps are emitted in that
/// same order so the skeleton is a faithful ordered trace of what happened.
fn build_steps_deduplicated(events: &[SessionEvent]) -> Vec<MinedStep> {
    // First pass: collect all ToolCall and FileEdit events, sorted by index.
    // We need to track which tool_use_ids have a FileEdit so we can prefer them.
    let mut file_edit_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut ordered_events: Vec<(usize, &SessionEvent)> = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::ToolCall { index, .. } | SessionEvent::FileEdit { index, .. } => {
                Some((*index, event))
            }
            _ => None,
        })
        .collect();
    // Sort by source-line index to preserve transcript order.
    ordered_events.sort_by_key(|(idx, _)| *idx);

    // Identify which tool_use_ids have a FileEdit representation.
    for (_, event) in &ordered_events {
        if let SessionEvent::FileEdit { tool_use_id, .. } = event {
            file_edit_ids.insert(tool_use_id.clone());
        }
    }

    // Second pass: emit steps in order, skipping ToolCall when a FileEdit exists for the same id.
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut steps = Vec::new();

    for (_, event) in ordered_events {
        match event {
            SessionEvent::FileEdit {
                tool_use_id,
                path,
                operation,
                ..
            } => {
                if seen_ids.insert(tool_use_id.clone()) {
                    steps.push(MinedStep {
                        tool_use_id: tool_use_id.clone(),
                        command_text: format!("{operation} {path}"),
                        tool_name: operation.clone(),
                    });
                }
            }
            SessionEvent::ToolCall {
                tool_use_id,
                name,
                input_json,
                ..
            } => {
                // Skip ToolCall if there is a FileEdit for the same id.
                if file_edit_ids.contains(tool_use_id) {
                    continue;
                }
                if seen_ids.insert(tool_use_id.clone()) {
                    let command_text = extract_command_text(name, input_json);
                    steps.push(MinedStep {
                        tool_use_id: tool_use_id.clone(),
                        command_text,
                        tool_name: name.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    steps
}

/// Extracts a human-readable command text from a tool call's name and raw JSON input.
///
/// For `Bash` calls, returns the `command` field. For other tools, returns `<name>(<summary>)`.
/// Never panics; falls back to the tool name if the input cannot be parsed.
fn extract_command_text(tool_name: &str, input_json: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(input_json);
    match tool_name {
        "Bash" => {
            parsed
                .ok()
                .and_then(|value| value.get("command").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| format!("Bash({input_json})"))
        }
        _ => {
            // For non-Bash tools, produce `<name>(<key>=<value>, ...)` from top-level input keys.
            let summary = parsed
                .ok()
                .and_then(|value| {
                    value.as_object().map(|obj| {
                        obj.iter()
                            .take(2)
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                })
                .unwrap_or_else(|| input_json.chars().take(60).collect());
            format!("{tool_name}({summary})")
        }
    }
}

/// Truncates a failure message to at most `max_bytes` bytes, preserving UTF-8 boundaries.
fn truncate_failure_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    // Find the last valid UTF-8 boundary at or before max_bytes.
    let boundary = text
        .char_indices()
        .take_while(|(byte_pos, _)| *byte_pos < max_bytes)
        .last()
        .map(|(pos, ch)| pos + ch.len_utf8())
        .unwrap_or(0);
    format!("{}…", &text[..boundary])
}

// ──────────────────────────────────────────────────────────────────────────────
// Async orchestrator
// ──────────────────────────────────────────────────────────────────────────────

/// Orchestrates one episode's skeleton mining and LLM labeling into a [`MapOutcome`].
///
/// # Decision logic
/// - If no actionable tool arc exists (no failing→passing `ToolResult` sequence with steps), returns
///   `MapOutcome::ProseFallback` so #187 can route the episode to the prose extractor.
/// - Otherwise: mines the skeleton deterministically, calls the labeler (bounded transform),
///   and assembles an [`ExtractedSkillCandidate`] whose `procedures` are the verbatim mined steps.
///
/// # Grounding invariant
/// The labeler is called with the skeleton so it can name and judge it. The labeler's output is
/// placed into `name`, `description`, `generality`, and `confidence` — never into `procedures`.
/// `procedures` always comes from `skeleton.steps`.
///
/// # Errors
/// Returns `Err(SkeletonError)` only when the labeler itself fails. Prose-fallback situations are
/// returned as `Ok(MapOutcome::ProseFallback{..})`, never as errors.
pub async fn map_episode(
    events: &[SessionEvent],
    labeler: &dyn SkeletonLabeler,
) -> Result<MapOutcome, SkeletonError> {
    let Some(skeleton) = mine_skeleton(events) else {
        return Ok(MapOutcome::ProseFallback {
            reason: "no actionable tool arc found in episode events".to_owned(),
        });
    };

    let label = labeler.label(&skeleton).await?;

    if !label.keep {
        return Ok(MapOutcome::ProseFallback {
            reason: format!(
                "labeler dropped skeleton with confidence {:.2}: {}",
                label.confidence, label.name
            ),
        });
    }

    // Assemble the candidate: procedures come from the grounded skeleton steps;
    // name/description/generality come from the labeler. Never swap these.
    let procedures: Vec<String> = skeleton
        .steps
        .iter()
        .map(|step| step.command_text.clone())
        .collect();

    let candidate = ExtractedSkillCandidate {
        name: label.name,
        description: label.description,
        tags: vec!["skeleton-mined".to_owned()],
        procedures,
        conventions: vec![],
        assets: vec![],
        confidence: label.confidence,
        generality: label.generality,
        generality_rationale: None,
    };

    Ok(MapOutcome::Skeleton(candidate))
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests (test-only fake labeler lives here — never exposed in production paths)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only labeler that echoes the skeleton's first step as the name and always keeps.
    ///
    /// This fake exists ONLY in `#[cfg(test)]`. It is never the production default.
    /// Its behavior: name = "test-<first-step-tool-name>", description = fixed string,
    /// generality = "general", keep = true, confidence = 0.9.
    struct EchoLabeler;

    #[async_trait]
    impl SkeletonLabeler for EchoLabeler {
        async fn label(
            &self,
            skeleton: &ProcedureSkeleton,
        ) -> Result<SkeletonLabel, SkeletonError> {
            let first_tool = skeleton
                .steps
                .first()
                .map(|step| step.tool_name.as_str())
                .unwrap_or("unknown");
            Ok(SkeletonLabel {
                name: format!("test-{first_tool}"),
                description: "Test-labeler description (not LLM-authored)".to_owned(),
                generality: Some("general".to_owned()),
                keep: true,
                confidence: 0.9,
            })
        }
    }

    /// Labeler that always drops the skeleton (keep=false).
    struct DroppingLabeler;

    #[async_trait]
    impl SkeletonLabeler for DroppingLabeler {
        async fn label(&self, _: &ProcedureSkeleton) -> Result<SkeletonLabel, SkeletonError> {
            Ok(SkeletonLabel {
                name: "dropped".to_owned(),
                description: "Dropped by labeler".to_owned(),
                generality: None,
                keep: false,
                confidence: 0.1,
            })
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Helpers that synthesize episode events
    // ──────────────────────────────────────────────────────────────────────────

    /// Builds a Tokio-repro episode: the user hits a build failure due to Mutex-across-await,
    /// the assistant edits the file and runs ulimit + tokio-console, then the build passes.
    ///
    /// This models the #176/#183 Tokio repro scenario that motivated the skeleton miner.
    fn tokio_repro_episode() -> Vec<SessionEvent> {
        vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "The build is failing with Mutex held across await point".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "I'll fix the Mutex-across-await issue in your async handler.".to_owned(),
            },
            // The assistant runs the failing build first.
            SessionEvent::ToolCall {
                index: 2,
                tool_use_id: "call-001".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"cargo build 2>&1"}"#.to_owned(),
            },
            // The build fails.
            SessionEvent::ToolResult {
                index: 3,
                tool_use_id: "call-001".to_owned(),
                is_error: true,
                exit_code: Some(1),
                output: "error[E0277]: Mutex<T> cannot be held across an await point\n  --> src/handler.rs:42:5".to_owned(),
            },
            // The assistant edits the file (FileEdit + ToolCall share tool_use_id "call-002").
            SessionEvent::ToolCall {
                index: 4,
                tool_use_id: "call-002".to_owned(),
                name: "Edit".to_owned(),
                input_json: r#"{"file_path":"src/handler.rs","old_string":"let guard = mutex.lock();","new_string":"let data = { let guard = mutex.lock().unwrap(); guard.clone() };"}"#.to_owned(),
            },
            SessionEvent::FileEdit {
                index: 4,
                tool_use_id: "call-002".to_owned(),
                path: "src/handler.rs".to_owned(),
                operation: "Edit".to_owned(),
            },
            // The assistant adjusts ulimit for tokio-console.
            SessionEvent::ToolCall {
                index: 5,
                tool_use_id: "call-003".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"ulimit -n 65536 && RUST_LOG=tokio=trace cargo build 2>&1"}"#.to_owned(),
            },
            // Build succeeds.
            SessionEvent::ToolResult {
                index: 6,
                tool_use_id: "call-003".to_owned(),
                is_error: false,
                exit_code: Some(0),
                output: "Compiling handler v0.1.0\n   Finished dev [unoptimized + debuginfo]".to_owned(),
            },
        ]
    }

    /// Builds a simple bash-only failing→passing episode.
    ///
    /// The user runs a test that fails, the assistant finds the root cause via a grep,
    /// applies a fix, and reruns — the test passes.
    fn bash_arc_episode() -> Vec<SessionEvent> {
        vec![
            SessionEvent::ToolCall {
                index: 0,
                tool_use_id: "b-001".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"cargo test my_feature 2>&1"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 1,
                tool_use_id: "b-001".to_owned(),
                is_error: true,
                exit_code: Some(101),
                output: "thread 'my_feature' panicked at 'assertion failed: left == right'".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 2,
                tool_use_id: "b-002".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"grep -n 'assert_eq' tests/my_feature.rs"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 3,
                tool_use_id: "b-002".to_owned(),
                is_error: false,
                exit_code: None,
                output: "12:    assert_eq!(result, 42);".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 4,
                tool_use_id: "b-003".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command":"cargo test my_feature 2>&1"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 5,
                tool_use_id: "b-003".to_owned(),
                is_error: false,
                exit_code: Some(0),
                output: "test my_feature ... ok".to_owned(),
            },
        ]
    }

    /// A preference-only episode with no tool calls.
    fn preference_only_episode() -> Vec<SessionEvent> {
        vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "I prefer snake_case for all Rust identifiers".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "Noted. I'll use snake_case for all identifiers going forward.".to_owned(),
            },
            SessionEvent::UserMessage {
                index: 2,
                content: "Also, always add doc comments to public functions".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 3,
                content: "Understood. I'll document all public APIs.".to_owned(),
            },
        ]
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Acceptance criterion 1: Tokio-repro episode mines real tokens, labeler adds no steps
    // ──────────────────────────────────────────────────────────────────────────

    /// Acceptance criterion 1: on a Tokio-repro episode (Mutex-across-await build failure),
    /// skeleton mining produces a procedure containing the actual used steps, and the labeler
    /// only names it — no command invented by the labeler appears in the candidate's procedures.
    #[tokio::test]
    async fn tokio_repro_skeleton_contains_real_tokens_and_labeler_adds_no_steps() {
        let events = tokio_repro_episode();

        let outcome = map_episode(&events, &EchoLabeler)
            .await
            .expect("map_episode must not error on a valid tool arc");

        let MapOutcome::Skeleton(candidate) = outcome else {
            panic!("expected Skeleton outcome for tokio-repro episode, got ProseFallback");
        };

        // The procedures must not be empty.
        assert!(
            !candidate.procedures.is_empty(),
            "procedures must be non-empty for a tool-arc episode"
        );

        // The file edit step must appear verbatim: "Edit src/handler.rs".
        let has_mutex_edit = candidate
            .procedures
            .iter()
            .any(|step| step.contains("src/handler.rs") && step.contains("Edit"));
        assert!(
            has_mutex_edit,
            "procedures must include the Edit src/handler.rs step from the transcript; got: {:?}",
            candidate.procedures
        );

        // The ulimit+tokio command must appear verbatim.
        let has_ulimit = candidate
            .procedures
            .iter()
            .any(|step| step.contains("ulimit") && step.contains("tokio"));
        assert!(
            has_ulimit,
            "procedures must include the ulimit+tokio-console command from the transcript; got: {:?}",
            candidate.procedures
        );

        // The labeler must not have invented any commands. The EchoLabeler sets
        // name = "test-Edit" (or "test-Bash"), description = fixed string. Neither
        // should appear in procedures (procedures come only from mined steps).
        let name_appears_in_procedures = candidate
            .procedures
            .iter()
            .any(|step| step == &candidate.name);
        assert!(
            !name_appears_in_procedures,
            "the labeler's name must not appear as a procedure step"
        );

        // Grounding invariant: every step must trace to an event. We verify this by checking
        // that the skeleton's steps all come from known tool_use_ids in the episode.
        let known_ids: std::collections::HashSet<_> = events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ToolCall { tool_use_id, .. }
                | SessionEvent::FileEdit { tool_use_id, .. } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect();
        let skeleton = mine_skeleton(&events).expect("skeleton must be present");
        for step in &skeleton.steps {
            assert!(
                known_ids.contains(step.tool_use_id.as_str()),
                "step tool_use_id '{}' must trace to a known event in the episode",
                step.tool_use_id
            );
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Acceptance criterion 2: bash arc yields exact commands that flipped result
    // ──────────────────────────────────────────────────────────────────────────

    /// Acceptance criterion 2: an episode with a failing→passing Bash arc yields a skeleton
    /// of the exact commands that flipped it, in transcript order.
    #[test]
    fn bash_arc_skeleton_contains_exact_commands_that_flipped_the_result() {
        let events = bash_arc_episode();
        let skeleton = mine_skeleton(&events).expect("bash arc must produce a skeleton");

        // Should have at least the grep and the re-run (not the initial failing call, which
        // is the trigger, not a resolution step).
        assert!(
            !skeleton.steps.is_empty(),
            "resolution steps must be non-empty"
        );

        // The trigger failure text must mention the assertion failure.
        assert!(
            skeleton.trigger_failure.contains("panicked"),
            "trigger_failure must carry the error text from the failing ToolResult; got: {}",
            skeleton.trigger_failure
        );

        // The exit code of the trigger must be 101.
        assert_eq!(
            skeleton.trigger_exit_code,
            Some(101),
            "trigger_exit_code must be 101"
        );

        // The grep command must appear in the steps.
        let has_grep = skeleton
            .steps
            .iter()
            .any(|step| step.command_text.contains("grep"));
        assert!(
            has_grep,
            "skeleton steps must include the grep command; got: {:?}",
            skeleton.steps
        );

        // The re-run cargo test command must appear.
        let has_rerun = skeleton
            .steps
            .iter()
            .any(|step| step.command_text.contains("cargo test my_feature"));
        assert!(
            has_rerun,
            "skeleton steps must include the cargo-test re-run; got: {:?}",
            skeleton.steps
        );

        // Steps are ordered (grep before cargo-test-rerun).
        let grep_pos = skeleton
            .steps
            .iter()
            .position(|step| step.command_text.contains("grep"))
            .expect("grep step must exist");
        let rerun_pos = skeleton
            .steps
            .iter()
            .position(|step| step.command_text.contains("cargo test my_feature"))
            .expect("rerun step must exist");
        assert!(
            grep_pos < rerun_pos,
            "grep must appear before cargo-test-rerun in the skeleton"
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Acceptance criterion 3: preference-only episode returns ProseFallback
    // ──────────────────────────────────────────────────────────────────────────

    /// Acceptance criterion 3: a non-tool, preference-only episode returns the prose-fallback
    /// signal (still extractable downstream), not an empty/dropped result.
    #[tokio::test]
    async fn preference_only_episode_returns_prose_fallback_not_empty_drop() {
        let events = preference_only_episode();

        let outcome = map_episode(&events, &EchoLabeler)
            .await
            .expect("map_episode must not error on a preference episode");

        match outcome {
            MapOutcome::ProseFallback { reason } => {
                assert!(
                    !reason.is_empty(),
                    "ProseFallback must carry a non-empty reason string"
                );
            }
            MapOutcome::Skeleton(_) => {
                panic!("preference-only episode must not produce a Skeleton outcome");
            }
        }

        // mine_skeleton must also return None directly.
        assert!(
            mine_skeleton(&events).is_none(),
            "mine_skeleton must return None for an episode with no tool arcs"
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Acceptance criterion 4: grounding assertion — every step maps to an event
    // ──────────────────────────────────────────────────────────────────────────

    /// Acceptance criterion 4 (grounding): every step in the skeleton maps to a concrete event
    /// in the episode, and the procedures in the assembled candidate contain transcript-real
    /// tokens (not prose invented by a labeler).
    #[tokio::test]
    async fn all_skeleton_steps_are_grounded_in_real_events() {
        let events = tokio_repro_episode();
        let skeleton = mine_skeleton(&events).expect("tokio repro must produce a skeleton");

        // Build the set of known tool_use_ids from the episode.
        let known_ids: std::collections::HashSet<String> = events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ToolCall { tool_use_id, .. }
                | SessionEvent::FileEdit { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect();

        for step in &skeleton.steps {
            assert!(
                known_ids.contains(&step.tool_use_id),
                "step tool_use_id '{}' (command: '{}') must map to a known event; known ids: {:?}",
                step.tool_use_id,
                step.command_text,
                known_ids
            );
        }

        // Assemble the candidate and verify procedures contain transcript-real tokens.
        let outcome = map_episode(&events, &EchoLabeler)
            .await
            .expect("map_episode must succeed");
        let MapOutcome::Skeleton(candidate) = outcome else {
            panic!("expected Skeleton outcome");
        };

        // Each procedure must correspond to a real command text from the episode.
        // We verify by checking at least one procedure contains a real file path or command.
        let has_real_path = candidate
            .procedures
            .iter()
            .any(|p| p.contains("src/handler.rs") || p.contains("ulimit") || p.contains("grep"));
        assert!(
            has_real_path,
            "at least one procedure must contain a transcript-real token (path or command); got: {:?}",
            candidate.procedures
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Deduplication invariant: FileEdit wins over ToolCall for the same tool_use_id
    // ──────────────────────────────────────────────────────────────────────────

    /// Verifies that when a `ToolCall` and a `FileEdit` share the same `tool_use_id`,
    /// only the `FileEdit` representation appears in the skeleton (no double-counting).
    #[test]
    fn dual_emission_tool_use_id_deduped_file_edit_wins() {
        // The tokio_repro_episode has call-002 emitted as both ToolCall(Edit) and FileEdit(Edit).
        let events = tokio_repro_episode();
        let skeleton = mine_skeleton(&events).expect("must produce a skeleton");

        // Count occurrences of "call-002" in the steps.
        let call_002_count = skeleton
            .steps
            .iter()
            .filter(|step| step.tool_use_id == "call-002")
            .count();
        assert_eq!(
            call_002_count, 1,
            "call-002 (dual-emitted as ToolCall+FileEdit) must appear exactly once in the skeleton"
        );

        // The winning representation must be the FileEdit one: "Edit src/handler.rs".
        let step = skeleton
            .steps
            .iter()
            .find(|step| step.tool_use_id == "call-002")
            .expect("call-002 step must exist");
        assert!(
            step.command_text.contains("src/handler.rs"),
            "FileEdit must win: command_text should contain the file path; got: {}",
            step.command_text
        );
        assert_eq!(
            step.tool_name, "Edit",
            "FileEdit must win: tool_name should be 'Edit'; got: {}",
            step.tool_name
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Labeler drop produces ProseFallback (not an error)
    // ──────────────────────────────────────────────────────────────────────────

    /// Verifies that when the labeler drops a skeleton (keep=false), map_episode returns
    /// ProseFallback — not an error and not an empty Skeleton.
    #[tokio::test]
    async fn labeler_drop_produces_prose_fallback() {
        let events = bash_arc_episode();

        let outcome = map_episode(&events, &DroppingLabeler)
            .await
            .expect("map_episode must not error when labeler drops");

        assert!(
            matches!(outcome, MapOutcome::ProseFallback { .. }),
            "labeler drop must produce ProseFallback, got: {outcome:?}"
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Trigger failure text is carried verbatim (truncated at 512 bytes)
    // ──────────────────────────────────────────────────────────────────────────

    /// Verifies that the trigger failure text is taken verbatim from the ToolResult output
    /// and is not LLM-authored or synthesized.
    #[test]
    fn trigger_failure_text_is_verbatim_from_tool_result() {
        let events = bash_arc_episode();
        let skeleton = mine_skeleton(&events).expect("bash arc must produce a skeleton");

        // The trigger failure text must contain the actual error text from the failing ToolResult.
        assert!(
            skeleton.trigger_failure.contains("panicked"),
            "trigger failure must contain 'panicked' from the ToolResult output; got: {}",
            skeleton.trigger_failure
        );
        assert!(
            skeleton
                .trigger_failure
                .contains("assertion failed: left == right"),
            "trigger failure must carry the assertion failure text; got: {}",
            skeleton.trigger_failure
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Only-metadata episode produces ProseFallback (no actionable arc)
    // ──────────────────────────────────────────────────────────────────────────

    /// Verifies that an episode consisting only of Metadata events returns ProseFallback.
    #[tokio::test]
    async fn metadata_only_episode_returns_prose_fallback() {
        let events = vec![
            SessionEvent::Metadata {
                index: 0,
                event_type: "mode".to_owned(),
            },
            SessionEvent::Metadata {
                index: 1,
                event_type: "ai-title".to_owned(),
            },
        ];

        let outcome = map_episode(&events, &EchoLabeler)
            .await
            .expect("map_episode must not error on metadata-only episode");

        assert!(
            matches!(outcome, MapOutcome::ProseFallback { .. }),
            "metadata-only episode must return ProseFallback"
        );
    }
}
