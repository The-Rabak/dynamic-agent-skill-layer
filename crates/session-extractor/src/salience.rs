//! Salience gate (#189) — deterministic skill-density scoring over #185 episodes.
//!
//! ## Purpose
//!
//! An extraction pipeline that runs an LLM call on every episode of every session scales cost
//! linearly with session length. Most episodes (read-only exploration, navigation, chit-chat,
//! abandoned dead-ends) contain no durable skill. This module is the cheap pre-filter that spends
//! LLM budget only where skill density is high, while never silently hiding what it skipped.
//!
//! ## Design
//!
//! Scoring is **pure and deterministic** — no LLM, no I/O. Five signals contribute to a score:
//!
//! 1. **Resolved error arc** — the episode contains both a failing `ToolResult` and a later
//!    succeeding `ToolResult` for the same tool name (failing→passing pattern).
//! 2. **Stated preference / imperative language** — a `UserMessage` contains an imperative keyword
//!    ("always", "never", "prefer", "don't", etc.) indicating a standing directive.
//! 3. **Named failure mode** — a `ToolResult` signals an error with a non-zero exit code or an
//!    error flag, surfacing a concrete failure string worth extracting.
//! 4. **Persisted file edit** — the episode contains at least one `FileEdit` event, meaning the
//!    assistant made a durable code change.
//! 5. **Config / command snippet** — a `ToolCall` to `"Bash"` is present, indicating a shell
//!    command was executed (often configuration or build work).
//!
//! Read-only / exploratory episodes (only `Read` calls, navigation, no edits/errors/preferences)
//! score at or near zero and are candidates for gating.
//!
//! ## Hard-keep invariant
//!
//! An episode that contains a **resolved error arc** OR a **stated preference** is NEVER gated,
//! regardless of the numeric score and regardless of how aggressive the `SalienceConfig` is.
//! Hard-keep rules override the gate unconditionally.
//!
//! ## No silent truncation
//!
//! Every gated episode is:
//! - Counted and included in the [`GateResult::gated`] return value.
//! - Logged via [`tracing::debug`] with its episode index, score, and gate reason.
//!
//! The calling layer can report the gated count to the user. Nothing is silently dropped.

use std::collections::{HashMap, HashSet};

use domain::SessionEvent;
use tracing::debug;

use crate::segmentation::Episode;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Aggressiveness mode for the salience gate.
///
/// The gate selects which episodes to pass to the LLM map step. Two modes are supported:
/// - [`GateMode::TopK`]: keep the K highest-scoring episodes (plus all hard-keep episodes).
/// - [`GateMode::Threshold`]: keep all episodes with `score >= min_score` (plus hard-keeps).
///
/// Both modes always respect the hard-keep invariant: episodes with a resolved error arc OR a
/// stated preference are NEVER gated, regardless of mode or score.
#[derive(Debug, Clone, PartialEq)]
pub enum GateMode {
    /// Keep only the top-K highest-scoring episodes, plus all hard-keep episodes.
    ///
    /// Hard-keep episodes are included unconditionally and do NOT count against K.
    TopK(usize),
    /// Keep all episodes whose score is at or above `min_score`, plus all hard-keep episodes.
    ///
    /// `min_score` is in `[0.0, 1.0]`. Setting it to `0.0` keeps all episodes (gate is a no-op).
    Threshold(f32),
}

/// Configuration for the salience gate.
///
/// The default is recall-biased: a low threshold that gates only clearly-empty (score = 0.0)
/// episodes. This ensures no skill-bearing episode is dropped unless it has zero signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SalienceConfig {
    /// The gate aggressiveness mode. See [`GateMode`] for semantics.
    pub mode: GateMode,
}

impl Default for SalienceConfig {
    /// Returns the recall-biased default: gate only episodes with a score of exactly 0.0.
    ///
    /// A score of 0.0 means: no resolved error arc, no stated preference, no named failure, no
    /// file edit, and no Bash command — a purely passive episode with no actionable signals.
    fn default() -> Self {
        Self {
            mode: GateMode::Threshold(0.01),
        }
    }
}

// ---------------------------------------------------------------------------
// Score signals and weights
// ---------------------------------------------------------------------------

/// Weights assigned to each skill-density signal.
///
/// Weights are chosen so that any single strong signal (resolved arc, preference) lifts the score
/// clearly above the recall-biased default threshold (0.01), while a combination of weaker signals
/// (edit + command) is also treated as skill-bearing.
///
/// Signal weights (sum does not need to equal 1.0 — scores are clamped to [0.0, 1.0]):
/// - Resolved error arc: 0.60 — strongest signal; a failing→passing repair almost always contains durable skill.
/// - Stated preference: 0.55 — strong; explicit user directive is always skill-bearing.
/// - Named failure mode (non-resolved error): 0.35 — worth LLM attention even if not yet resolved.
/// - Persisted file edit: 0.30 — durable code change is strong evidence of procedural skill.
/// - Bash command present: 0.15 — config/build work, weaker on its own.
const WEIGHT_RESOLVED_ARC: f32 = 0.60;
const WEIGHT_STATED_PREFERENCE: f32 = 0.55;
const WEIGHT_NAMED_FAILURE: f32 = 0.35;
const WEIGHT_FILE_EDIT: f32 = 0.30;
const WEIGHT_BASH_COMMAND: f32 = 0.15;

/// The keyword phrases that detect standing preferences / imperative directives in a user turn.
///
/// These are the same signal phrases used by the `preamble` module — kept in sync by convention.
/// They are reproduced here rather than re-exported from preamble to keep salience.rs self-contained
/// (the preamble module's detection function is private and the salience gate is a different concern).
const PREFERENCE_SIGNAL_PHRASES: &[&str] = &[
    "always ",
    "never ",
    "prefer ",
    "don't ",
    "do not ",
    "i want ",
    "avoid ",
    "make sure ",
    "please use ",
    "please don't ",
    "please never ",
    "please always ",
];

// ---------------------------------------------------------------------------
// Per-episode scoring
// ---------------------------------------------------------------------------

/// The computed skill-density score and hard-keep flag for a single episode.
///
/// Produced by [`score_episode`] for each episode before gating.
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeScore {
    /// Normalized skill-density score in `[0.0, 1.0]`. Higher means more skill-bearing.
    pub score: f32,
    /// `true` when this episode must never be gated (resolved error arc OR stated preference).
    pub hard_keep: bool,
    /// The primary signal that triggered `hard_keep`, if any.
    pub hard_keep_reason: Option<&'static str>,
}

/// Scores a single episode for skill density using the five deterministic signals.
///
/// `episode_events` is the slice of `SessionEvent` values covered by this episode (already
/// resolved from `episode.event_indices` by the caller). Passing an empty slice yields score 0.0.
///
/// ## Return value
///
/// Returns an [`EpisodeScore`] with the normalized score and hard-keep flag. The score
/// is the sum of all triggered signal weights, clamped to `[0.0, 1.0]`.
pub fn score_episode(episode_events: &[&SessionEvent]) -> EpisodeScore {
    if episode_events.is_empty() {
        return EpisodeScore {
            score: 0.0,
            hard_keep: false,
            hard_keep_reason: None,
        };
    }

    let has_resolved_arc = detect_resolved_error_arc(episode_events);
    let has_stated_preference = detect_stated_preference(episode_events);
    let has_named_failure = detect_named_failure(episode_events);
    let has_file_edit = detect_file_edit(episode_events);
    let has_bash_command = detect_bash_command(episode_events);

    let mut raw_score: f32 = 0.0;
    if has_resolved_arc {
        raw_score += WEIGHT_RESOLVED_ARC;
    }
    if has_stated_preference {
        raw_score += WEIGHT_STATED_PREFERENCE;
    }
    if has_named_failure {
        raw_score += WEIGHT_NAMED_FAILURE;
    }
    if has_file_edit {
        raw_score += WEIGHT_FILE_EDIT;
    }
    if has_bash_command {
        raw_score += WEIGHT_BASH_COMMAND;
    }

    let score = raw_score.min(1.0);

    // Hard-keep: resolved arc OR stated preference — order matters for the reason label.
    let (hard_keep, hard_keep_reason) = if has_resolved_arc {
        (true, Some("resolved_error_arc"))
    } else if has_stated_preference {
        (true, Some("stated_preference"))
    } else {
        (false, None)
    };

    EpisodeScore {
        score,
        hard_keep,
        hard_keep_reason,
    }
}

// ---------------------------------------------------------------------------
// Signal detectors
// ---------------------------------------------------------------------------

/// Returns `true` when the episode contains a resolved error arc: a failing `ToolResult`
/// followed by a succeeding `ToolResult` for the same tool name.
///
/// Algorithm:
/// 1. Build a map from `tool_use_id` → tool name using `ToolCall` events.
/// 2. Scan `ToolResult` events in order. When a failing result is seen, record the tool name
///    as having an open error. When a succeeding result is seen for the same tool name, the arc
///    is resolved.
///
/// "Same tool name" matches the `segmentation` module's arc-detection heuristic: the tool name
/// is used (not the individual `tool_use_id`) because retries use a new ID but the same tool.
fn detect_resolved_error_arc(events: &[&SessionEvent]) -> bool {
    // Step 1: build tool_use_id → name lookup from ToolCall events.
    let mut tool_name_by_id: HashMap<&str, &str> = HashMap::new();
    for event in events {
        if let SessionEvent::ToolCall {
            tool_use_id, name, ..
        } = event
        {
            tool_name_by_id.insert(tool_use_id.as_str(), name.as_str());
        }
    }

    // Step 2: scan ToolResult events in order for failing→passing pattern.
    let mut tools_with_open_error: HashSet<&str> = HashSet::new();
    for event in events {
        if let SessionEvent::ToolResult {
            tool_use_id,
            is_error,
            exit_code,
            ..
        } = event
        {
            let tool_name = tool_name_by_id
                .get(tool_use_id.as_str())
                .copied()
                .unwrap_or("unknown");

            let is_failing = *is_error || exit_code.map_or(false, |code| code != 0);
            let is_passing = !is_error && exit_code.map_or(true, |code| code == 0);

            if is_failing {
                tools_with_open_error.insert(tool_name);
            } else if is_passing && tools_with_open_error.contains(tool_name) {
                // A prior error for this tool name was resolved.
                return true;
            }
        }
    }

    false
}

/// Returns `true` when any `UserMessage` in the episode contains an imperative/preference signal.
///
/// The signal phrases are the same as those used by the `preamble` module's preference detector.
/// The match is case-insensitive, on the lowercased content.
fn detect_stated_preference(events: &[&SessionEvent]) -> bool {
    for event in events {
        if let SessionEvent::UserMessage { content, .. } = event {
            let lower = content.to_ascii_lowercase();
            if PREFERENCE_SIGNAL_PHRASES
                .iter()
                .any(|phrase| lower.contains(phrase))
            {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when any `ToolResult` in the episode signals a named failure:
/// `is_error = true` OR a non-zero `exit_code`.
///
/// This signal fires even for unresolved failures (distinct from the resolved-arc signal), because
/// an episode that ends with an unresolved error still captures a named failure mode worth LLM review.
fn detect_named_failure(events: &[&SessionEvent]) -> bool {
    for event in events {
        if let SessionEvent::ToolResult {
            is_error,
            exit_code,
            ..
        } = event
        {
            if *is_error || exit_code.map_or(false, |code| code != 0) {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when any `FileEdit` event is present in the episode.
///
/// A `FileEdit` event means the assistant made a durable file change (Write, Edit, or MultiEdit),
/// which strongly correlates with a procedural skill being exercised.
fn detect_file_edit(events: &[&SessionEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, SessionEvent::FileEdit { .. }))
}

/// Returns `true` when any `ToolCall` to `"Bash"` is present in the episode.
///
/// A Bash invocation indicates config, build, or test work — weaker signal than a file edit
/// but still distinguishes active work from pure navigation/read-only exploration.
fn detect_bash_command(events: &[&SessionEvent]) -> bool {
    for event in events {
        if let SessionEvent::ToolCall { name, .. } = event {
            if name == "Bash" {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Gate result
// ---------------------------------------------------------------------------

/// A gated-out episode: the episode itself plus its score and the reason it was gated.
#[derive(Debug, Clone, PartialEq)]
pub struct GatedEpisode {
    /// The zero-based index of the episode in the original input slice.
    pub episode_index: usize,
    /// The episode that was gated out.
    pub episode: Episode,
    /// The computed skill-density score (in `[0.0, 1.0]`).
    pub score: f32,
    /// Human-readable reason the episode was gated (e.g. `"score_below_threshold(0.01)"`).
    pub gate_reason: String,
}

/// The output of [`gate_episodes`]: the episodes to send to the LLM map step, plus a full
/// record of every gated-out episode.
///
/// ## Invariants
///
/// - `kept` contains all hard-keep episodes unconditionally (resolved arc OR stated preference).
/// - `gated` is non-empty only when the gate actually removed episodes.
/// - `kept.len() + gated.len() == total input episode count`.
/// - Every gated episode is logged via `tracing::debug` before being added to `gated`.
#[derive(Debug, Clone, PartialEq)]
pub struct GateResult {
    /// The episodes that passed the gate and should be forwarded to the LLM map step.
    pub kept: Vec<Episode>,
    /// Every episode that was gated out, with score and reason. Never silently discarded.
    pub gated: Vec<GatedEpisode>,
}

// ---------------------------------------------------------------------------
// Gate entry point
// ---------------------------------------------------------------------------

/// Applies the salience gate to a set of episodes, returning kept and gated episodes.
///
/// ## Algorithm
///
/// 1. For each episode, resolve the `event_indices` to `SessionEvent` references using `events`.
///    Events are looked up by `SessionEvent::index()` — `events` may be in any order.
/// 2. Score each episode with [`score_episode`].
/// 3. Apply hard-keep: episodes with `EpisodeScore::hard_keep = true` are unconditionally kept,
///    regardless of score and regardless of the configured gate mode.
/// 4. Apply the gate mode to the remaining (non-hard-keep) episodes:
///    - [`GateMode::TopK`]: keep the top-K by score. Ties are broken by episode position (earlier
///      episodes win) for determinism.
///    - [`GateMode::Threshold`]: keep episodes with `score >= min_score`.
/// 5. Log every gated episode via `tracing::debug` and include it in [`GateResult::gated`].
///
/// ## Event lookup
///
/// `events` may contain more events than those referenced by `episodes` (it is the full session).
/// Events not referenced by any episode are ignored. Events referenced by an episode but absent
/// from `events` are silently skipped (logged at `tracing::debug`) — the episode is still scored
/// on the events that ARE present.
///
/// ## Panics
///
/// Never panics. All lookups are bounds-checked.
pub fn gate_episodes(
    episodes: &[Episode],
    events: &[SessionEvent],
    config: &SalienceConfig,
) -> GateResult {
    if episodes.is_empty() {
        return GateResult {
            kept: Vec::new(),
            gated: Vec::new(),
        };
    }

    // Build a map from event index → &SessionEvent for O(1) lookup.
    let event_by_index: HashMap<usize, &SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    // Score every episode.
    let scored: Vec<(usize, &Episode, EpisodeScore)> = episodes
        .iter()
        .enumerate()
        .map(|(episode_index, episode)| {
            let episode_events: Vec<&SessionEvent> = episode
                .event_indices
                .iter()
                .filter_map(|idx| {
                    let found = event_by_index.get(idx).copied();
                    if found.is_none() {
                        debug!(
                            episode_index,
                            missing_event_index = idx,
                            "salience gate: event index referenced by episode not found in events slice — skipping event"
                        );
                    }
                    found
                })
                .collect();
            let score = score_episode(&episode_events);
            (episode_index, episode, score)
        })
        .collect();

    // Partition into hard-keep and gate-eligible episodes.
    let mut hard_keep_episodes: Vec<(usize, &Episode, EpisodeScore)> = Vec::new();
    let mut gate_eligible: Vec<(usize, &Episode, EpisodeScore)> = Vec::new();
    for scored_ep in scored {
        if scored_ep.2.hard_keep {
            hard_keep_episodes.push(scored_ep);
        } else {
            gate_eligible.push(scored_ep);
        }
    }

    // Apply the gate mode to the gate-eligible episodes.
    let (passed_gate, gated_out) = apply_gate_mode(gate_eligible, config);

    // Build the final kept list: hard-keeps + gate-passed, in original episode order.
    // We reconstruct order by collecting all kept indices, then iterating episodes in order.
    let kept_indices: HashSet<usize> = hard_keep_episodes
        .iter()
        .map(|(idx, _, _)| *idx)
        .chain(passed_gate.iter().map(|(idx, _, _)| *idx))
        .collect();

    let kept: Vec<Episode> = episodes
        .iter()
        .enumerate()
        .filter_map(|(episode_index, episode)| {
            if kept_indices.contains(&episode_index) {
                Some(episode.clone())
            } else {
                None
            }
        })
        .collect();

    // Build and log the gated list.
    let gated: Vec<GatedEpisode> = gated_out
        .into_iter()
        .map(|(episode_index, episode, episode_score)| {
            let first_index = episode.event_indices.first().copied();
            let last_index = episode.event_indices.last().copied();
            debug!(
                episode_index,
                score = episode_score.score,
                first_event_index = ?first_index,
                last_event_index = ?last_index,
                "salience gate: gating episode with low skill-density score"
            );
            GatedEpisode {
                episode_index,
                episode: episode.clone(),
                score: episode_score.score,
                gate_reason: format_gate_reason(&episode_score, config),
            }
        })
        .collect();

    // Sort gated by episode_index for deterministic ordering in the return value.
    let mut gated = gated;
    gated.sort_by_key(|g| g.episode_index);

    GateResult { kept, gated }
}

/// Applies the configured [`GateMode`] to the gate-eligible (non-hard-keep) episodes.
///
/// Returns `(passed, gated_out)` — both in the original episode index ordering.
fn apply_gate_mode<'a>(
    eligible: Vec<(usize, &'a Episode, EpisodeScore)>,
    config: &SalienceConfig,
) -> (
    Vec<(usize, &'a Episode, EpisodeScore)>,
    Vec<(usize, &'a Episode, EpisodeScore)>,
) {
    match &config.mode {
        GateMode::Threshold(min_score) => {
            let mut passed = Vec::new();
            let mut gated = Vec::new();
            for item in eligible {
                if item.2.score >= *min_score {
                    passed.push(item);
                } else {
                    gated.push(item);
                }
            }
            (passed, gated)
        }
        GateMode::TopK(k) => {
            if eligible.is_empty() {
                return (Vec::new(), Vec::new());
            }
            // Sort by score descending, tie-break by episode index ascending (earlier = higher priority).
            let mut sorted: Vec<usize> = (0..eligible.len()).collect();
            sorted.sort_by(|&a, &b| {
                eligible[b]
                    .2
                    .score
                    .partial_cmp(&eligible[a].2.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| eligible[a].0.cmp(&eligible[b].0))
            });

            let kept_positions: HashSet<usize> = sorted.into_iter().take(*k).collect();
            let mut passed = Vec::new();
            let mut gated = Vec::new();
            for (pos, item) in eligible.into_iter().enumerate() {
                if kept_positions.contains(&pos) {
                    passed.push(item);
                } else {
                    gated.push(item);
                }
            }
            (passed, gated)
        }
    }
}

/// Formats a human-readable gate reason for a gated-out episode.
fn format_gate_reason(score: &EpisodeScore, config: &SalienceConfig) -> String {
    match &config.mode {
        GateMode::Threshold(min_score) => {
            format!(
                "score_below_threshold({min_score:.3}): actual={:.3}",
                score.score
            )
        }
        GateMode::TopK(k) => {
            format!("not_in_top_{k}: score={:.3}", score.score)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use domain::SessionEvent;

    // ---- Event construction helpers ----

    fn user_msg(index: usize, content: &str) -> SessionEvent {
        SessionEvent::UserMessage {
            index,
            content: content.to_owned(),
        }
    }

    fn assistant_msg(index: usize, content: &str) -> SessionEvent {
        SessionEvent::AssistantMessage {
            index,
            content: content.to_owned(),
        }
    }

    fn tool_call(index: usize, id: &str, name: &str, input_json: &str) -> SessionEvent {
        SessionEvent::ToolCall {
            index,
            tool_use_id: id.to_owned(),
            name: name.to_owned(),
            input_json: input_json.to_owned(),
        }
    }

    fn tool_result_ok(index: usize, id: &str, output: &str) -> SessionEvent {
        SessionEvent::ToolResult {
            index,
            tool_use_id: id.to_owned(),
            is_error: false,
            exit_code: Some(0),
            output: output.to_owned(),
        }
    }

    fn tool_result_err(index: usize, id: &str, output: &str) -> SessionEvent {
        SessionEvent::ToolResult {
            index,
            tool_use_id: id.to_owned(),
            is_error: true,
            exit_code: Some(1),
            output: output.to_owned(),
        }
    }

    fn file_edit(index: usize, id: &str, path: &str) -> SessionEvent {
        SessionEvent::FileEdit {
            index,
            tool_use_id: id.to_owned(),
            path: path.to_owned(),
            operation: "Write".to_owned(),
        }
    }

    fn read_only_episode(start_index: usize) -> (Vec<SessionEvent>, Episode) {
        // A read-only, exploratory episode: user asks, assistant replies, Read tool used.
        let events = vec![
            user_msg(start_index, "What does this function do?"),
            assistant_msg(start_index + 1, "It reads from a file."),
            tool_call(
                start_index + 2,
                &format!("r{start_index}"),
                "Read",
                r#"{"file_path":"src/lib.rs"}"#,
            ),
            tool_result_ok(start_index + 3, &format!("r{start_index}"), "fn foo() {}"),
        ];
        let episode = Episode {
            event_indices: events.iter().map(|e| e.index()).collect(),
            arc_id: None,
        };
        (events, episode)
    }

    fn error_fix_episode(start_index: usize) -> (Vec<SessionEvent>, Episode) {
        // An episode with a resolved error arc: cargo test fails, then passes.
        let id_fail = format!("t{start_index}f");
        let id_ok = format!("t{start_index}o");
        let events = vec![
            user_msg(start_index, "Run the tests."),
            tool_call(
                start_index + 1,
                &id_fail,
                "Bash",
                r#"{"command":"cargo test"}"#,
            ),
            tool_result_err(start_index + 2, &id_fail, "Exit code 1\ntest failed"),
            tool_call(
                start_index + 3,
                &id_ok,
                "Bash",
                r#"{"command":"cargo test"}"#,
            ),
            tool_result_ok(start_index + 4, &id_ok, "test result: ok"),
        ];
        let episode = Episode {
            event_indices: events.iter().map(|e| e.index()).collect(),
            arc_id: None,
        };
        (events, episode)
    }

    fn preference_episode(start_index: usize) -> (Vec<SessionEvent>, Episode) {
        // An episode with a stated preference.
        let events = vec![
            user_msg(
                start_index,
                "Always use `tracing::debug` instead of `println!`.",
            ),
            assistant_msg(start_index + 1, "Understood, I will use tracing::debug."),
        ];
        let episode = Episode {
            event_indices: events.iter().map(|e| e.index()).collect(),
            arc_id: None,
        };
        (events, episode)
    }

    // ---- Acceptance test 1: exploration-heavy session — only fix arcs kept ----

    /// Acceptance criterion 1: a session of mostly read-only exploration + two real fix arcs
    /// gates so that only the fix arcs are kept; the skipped episodes are returned and the count
    /// is non-zero.
    #[test]
    fn gate_keeps_fix_arcs_and_skips_read_only_exploration() {
        // Build a session: 5 read-only episodes + 2 error-fix episodes.
        let mut all_events: Vec<SessionEvent> = Vec::new();
        let mut episodes: Vec<Episode> = Vec::new();

        for i in 0..5usize {
            let (evs, ep) = read_only_episode(i * 10);
            all_events.extend(evs);
            episodes.push(ep);
        }
        let (fix_evs_1, fix_ep_1) = error_fix_episode(50);
        all_events.extend(fix_evs_1);
        let fix_ep_1_idx = episodes.len();
        episodes.push(fix_ep_1);

        let (fix_evs_2, fix_ep_2) = error_fix_episode(60);
        all_events.extend(fix_evs_2);
        let fix_ep_2_idx = episodes.len();
        episodes.push(fix_ep_2);

        // Use aggressive threshold so that only clearly skill-bearing episodes pass.
        let config = SalienceConfig {
            mode: GateMode::Threshold(0.50),
        };
        let result = gate_episodes(&episodes, &all_events, &config);

        // The two fix-arc episodes must be kept (hard-keep due to resolved error arc).
        assert!(
            result.kept.len() >= 2,
            "at least the two fix-arc episodes must be kept; got {} kept",
            result.kept.len()
        );

        // The 5 read-only episodes must be gated.
        assert!(
            result.gated.len() >= 5,
            "at least the 5 read-only episodes must be gated; got {} gated",
            result.gated.len()
        );

        // The kept set must contain the fix episodes (by checking their event indices).
        let fix_ep_1_indices: HashSet<usize> = episodes[fix_ep_1_idx]
            .event_indices
            .iter()
            .copied()
            .collect();
        let fix_ep_2_indices: HashSet<usize> = episodes[fix_ep_2_idx]
            .event_indices
            .iter()
            .copied()
            .collect();
        let kept_indices: HashSet<usize> = result
            .kept
            .iter()
            .flat_map(|ep| ep.event_indices.iter().copied())
            .collect();
        assert!(
            fix_ep_1_indices
                .iter()
                .all(|idx| kept_indices.contains(idx)),
            "all events of fix episode 1 must be in the kept set"
        );
        assert!(
            fix_ep_2_indices
                .iter()
                .all(|idx| kept_indices.contains(idx)),
            "all events of fix episode 2 must be in the kept set"
        );

        // Every gated episode must have a non-empty gate_reason.
        for gated in &result.gated {
            assert!(
                !gated.gate_reason.is_empty(),
                "gated episode {} must have a non-empty gate_reason",
                gated.episode_index
            );
        }

        // Total: kept + gated == input count.
        assert_eq!(
            result.kept.len() + result.gated.len(),
            episodes.len(),
            "kept + gated must equal total episode count"
        );
    }

    // ---- Acceptance test 2: hard-keep override — resolved arc and preference never gated ----

    /// Acceptance criterion 2: an episode containing a stated preference OR a resolved error arc
    /// is NEVER gated out, even at the most aggressive setting (TopK(0) would gate everything
    /// else, but hard-keep episodes still survive).
    #[test]
    fn hard_keep_episodes_never_gated_at_most_aggressive_setting() {
        let (pref_evs, pref_ep) = preference_episode(0);
        let (fix_evs, fix_ep) = error_fix_episode(10);
        let (read_evs, read_ep) = read_only_episode(20);

        let all_events: Vec<SessionEvent> = pref_evs
            .into_iter()
            .chain(fix_evs)
            .chain(read_evs)
            .collect();
        let episodes = vec![pref_ep, fix_ep, read_ep];

        // Most aggressive setting: keep ZERO non-hard-keep episodes.
        let aggressive_config = SalienceConfig {
            mode: GateMode::TopK(0),
        };
        let result = gate_episodes(&episodes, &all_events, &aggressive_config);

        // The preference episode (index 0) must be kept.
        let pref_kept = result.kept.iter().any(|ep| ep.event_indices.contains(&0));
        assert!(
            pref_kept,
            "episode with stated preference must NEVER be gated, even at TopK(0)"
        );

        // The fix episode (index 1) must be kept.
        let fix_kept = result.kept.iter().any(|ep| ep.event_indices.contains(&10));
        assert!(
            fix_kept,
            "episode with resolved error arc must NEVER be gated, even at TopK(0)"
        );

        // The read-only episode must be gated (no hard-keep signal, K=0 so no non-hard-keeps pass).
        assert_eq!(
            result.gated.len(),
            1,
            "exactly the read-only episode must be gated; gated: {:?}",
            result.gated
        );
        assert_eq!(
            result.gated[0].episode_index, 2,
            "the gated episode must be the read-only one (index 2)"
        );
    }

    // ---- Acceptance test 3: cost proxy — sub-linear kept count with equal recall of skill episodes ----

    /// Acceptance criterion 3: on a long synthetic session, the count of KEPT episodes with the
    /// gate ON is sub-linear vs. gate OFF, with equal recall of the planted skill-bearing episodes.
    ///
    /// Session: 20 read-only episodes + 3 skill-bearing episodes (1 pref + 1 fix arc + 1 file-edit).
    /// Gate OFF (Threshold 0.0) must keep all 23.
    /// Gate ON (Threshold 0.50) must keep < 23 and still include all 3 skill episodes.
    #[test]
    fn gate_on_reduces_kept_count_without_losing_skill_episodes() {
        let mut all_events: Vec<SessionEvent> = Vec::new();
        let mut episodes: Vec<Episode> = Vec::new();

        // 20 read-only exploration episodes.
        for i in 0..20usize {
            let (evs, ep) = read_only_episode(i * 10);
            all_events.extend(evs);
            episodes.push(ep);
        }

        // Plant 1: preference episode (hard-keep).
        let (pref_evs, pref_ep) = preference_episode(200);
        let pref_ep_event_idx = pref_ep.event_indices[0];
        all_events.extend(pref_evs);
        episodes.push(pref_ep);

        // Plant 2: error-fix episode (hard-keep via resolved arc).
        let (fix_evs, fix_ep) = error_fix_episode(210);
        let fix_ep_event_idx = fix_ep.event_indices[0];
        all_events.extend(fix_evs);
        episodes.push(fix_ep);

        // Plant 3: file-edit episode (scores via WEIGHT_FILE_EDIT + WEIGHT_BASH_COMMAND = 0.45).
        let edit_start = 220usize;
        let edit_events = vec![
            user_msg(edit_start, "Fix the bug in config.rs."),
            tool_call(
                edit_start + 1,
                "e1",
                "Write",
                r#"{"file_path":"config.rs","content":"fn config() {}"}"#,
            ),
            file_edit(edit_start + 2, "e1", "config.rs"),
            tool_result_ok(edit_start + 3, "e1", "written"),
        ];
        let edit_ep = Episode {
            event_indices: edit_events.iter().map(|e| e.index()).collect(),
            arc_id: None,
        };
        let edit_ep_event_idx = edit_ep.event_indices[0];
        all_events.extend(edit_events);
        episodes.push(edit_ep);

        let total_episodes = episodes.len();
        assert_eq!(total_episodes, 23, "should have 23 total episodes");

        // Gate OFF: keep everything.
        let gate_off = SalienceConfig {
            mode: GateMode::Threshold(0.0),
        };
        let result_off = gate_episodes(&episodes, &all_events, &gate_off);
        assert_eq!(
            result_off.kept.len(),
            total_episodes,
            "gate OFF must keep all episodes"
        );
        assert_eq!(result_off.gated.len(), 0, "gate OFF must gate no episodes");

        // Gate ON: threshold above 0.0 but below the file-edit score (0.30) so read-only
        // episodes (score=0.0) are gated while the file-edit episode (score=0.30) and
        // hard-keep episodes survive.
        let gate_on = SalienceConfig {
            mode: GateMode::Threshold(0.25),
        };
        let result_on = gate_episodes(&episodes, &all_events, &gate_on);

        // Kept count must be sub-linear (less than total).
        assert!(
            result_on.kept.len() < total_episodes,
            "gate ON must keep fewer than all {} episodes; kept {}",
            total_episodes,
            result_on.kept.len()
        );

        // All planted skill episodes must survive.
        let kept_event_indices: HashSet<usize> = result_on
            .kept
            .iter()
            .flat_map(|ep| ep.event_indices.iter().copied())
            .collect();

        assert!(
            kept_event_indices.contains(&pref_ep_event_idx),
            "preference episode (event index {pref_ep_event_idx}) must be kept"
        );
        assert!(
            kept_event_indices.contains(&fix_ep_event_idx),
            "error-fix episode (event index {fix_ep_event_idx}) must be kept"
        );
        assert!(
            kept_event_indices.contains(&edit_ep_event_idx),
            "file-edit episode (event index {edit_ep_event_idx}) must be kept (score 0.30 >= threshold 0.25)"
        );

        // All gated episodes must have a gate_reason.
        for gated in &result_on.gated {
            assert!(
                !gated.gate_reason.is_empty(),
                "each gated episode must have a gate_reason; episode_index={}",
                gated.episode_index
            );
        }

        // Total: kept + gated == total.
        assert_eq!(
            result_on.kept.len() + result_on.gated.len(),
            total_episodes,
            "kept + gated must equal total episode count"
        );
    }

    // ---- Additional unit tests for individual signals ----

    /// `score_episode` on an empty slice returns score 0.0 and no hard-keep.
    #[test]
    fn score_empty_episode_returns_zero_score_no_hard_keep() {
        let score = score_episode(&[]);
        assert_eq!(score.score, 0.0);
        assert!(!score.hard_keep);
        assert!(score.hard_keep_reason.is_none());
    }

    /// A purely read-only episode (Read tool, no errors, no edits, no preferences) scores 0.0.
    #[test]
    fn read_only_episode_scores_zero() {
        let events = vec![
            user_msg(0, "What does this function do?"),
            tool_call(1, "r1", "Read", r#"{"file_path":"src/lib.rs"}"#),
            tool_result_ok(2, "r1", "fn foo() {}"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert_eq!(
            score.score, 0.0,
            "read-only episode must score 0.0; got {}",
            score.score
        );
        assert!(!score.hard_keep, "read-only episode must not be hard-keep");
    }

    /// A resolved error arc sets hard_keep and contributes WEIGHT_RESOLVED_ARC to the score.
    #[test]
    fn resolved_arc_sets_hard_keep_and_score() {
        let events = vec![
            tool_call(0, "t1", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_err(1, "t1", "Exit code 1\nfailed"),
            tool_call(2, "t2", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_ok(3, "t2", "ok"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert!(
            score.score >= WEIGHT_RESOLVED_ARC,
            "resolved arc must contribute at least {WEIGHT_RESOLVED_ARC}; got {}",
            score.score
        );
        assert!(score.hard_keep, "resolved arc must set hard_keep");
        assert_eq!(
            score.hard_keep_reason,
            Some("resolved_error_arc"),
            "hard_keep_reason must be 'resolved_error_arc'"
        );
    }

    /// A stated preference sets hard_keep and contributes WEIGHT_STATED_PREFERENCE to the score.
    #[test]
    fn stated_preference_sets_hard_keep_and_score() {
        let events = vec![
            user_msg(0, "Always use tracing::debug instead of println."),
            assistant_msg(1, "Understood."),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert!(
            score.score >= WEIGHT_STATED_PREFERENCE,
            "stated preference must contribute at least {WEIGHT_STATED_PREFERENCE}; got {}",
            score.score
        );
        assert!(score.hard_keep, "stated preference must set hard_keep");
        assert_eq!(
            score.hard_keep_reason,
            Some("stated_preference"),
            "hard_keep_reason must be 'stated_preference'"
        );
    }

    /// An unresolved error (no subsequent success) scores via WEIGHT_NAMED_FAILURE but NOT
    /// WEIGHT_RESOLVED_ARC, and does NOT set hard_keep.
    #[test]
    fn unresolved_error_scores_named_failure_not_hard_keep() {
        let events = vec![
            tool_call(0, "t1", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_err(1, "t1", "Exit code 1\nfailed"),
            // No subsequent success — arc remains open.
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert!(
            score.score >= WEIGHT_NAMED_FAILURE,
            "named failure must contribute at least {WEIGHT_NAMED_FAILURE}; got {}",
            score.score
        );
        // Bash command also triggers.
        assert!(
            score.score >= WEIGHT_NAMED_FAILURE + WEIGHT_BASH_COMMAND,
            "named failure + bash command must both contribute; got {}",
            score.score
        );
        assert!(!score.hard_keep, "unresolved error must NOT set hard_keep");
    }

    /// A file edit contributes WEIGHT_FILE_EDIT to the score.
    #[test]
    fn file_edit_contributes_to_score() {
        let events = vec![
            tool_call(0, "e1", "Write", r#"{"file_path":"a.rs"}"#),
            file_edit(1, "e1", "a.rs"),
            tool_result_ok(2, "e1", "written"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert!(
            score.score >= WEIGHT_FILE_EDIT,
            "file edit must contribute at least {WEIGHT_FILE_EDIT}; got {}",
            score.score
        );
    }

    /// Score is clamped to 1.0 even when all signals fire.
    #[test]
    fn score_clamped_to_one_when_all_signals_fire() {
        // Episode with: resolved arc, stated preference, named failure, file edit, bash command.
        let events = vec![
            user_msg(0, "Always use strict mode. Run tests now."),
            tool_call(1, "t1", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_err(2, "t1", "Exit code 1\nfailed"),
            tool_call(3, "t2", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_ok(4, "t2", "ok"),
            tool_call(5, "e1", "Write", r#"{"file_path":"a.rs"}"#),
            file_edit(6, "e1", "a.rs"),
            tool_result_ok(7, "e1", "written"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        let score = score_episode(&event_refs);
        assert_eq!(
            score.score, 1.0,
            "score must be clamped to 1.0 when all signals fire; got {}",
            score.score
        );
    }

    /// `gate_episodes` on an empty slice returns empty kept and gated.
    #[test]
    fn gate_empty_episodes_returns_empty_result() {
        let result = gate_episodes(&[], &[], &SalienceConfig::default());
        assert!(result.kept.is_empty());
        assert!(result.gated.is_empty());
    }

    /// `GateMode::TopK(0)` gates all non-hard-keep episodes.
    #[test]
    fn top_k_zero_gates_all_non_hard_keep_episodes() {
        let (read_evs, read_ep) = read_only_episode(0);
        let episodes = vec![read_ep];
        let config = SalienceConfig {
            mode: GateMode::TopK(0),
        };
        let result = gate_episodes(&episodes, &read_evs, &config);
        assert_eq!(
            result.kept.len(),
            0,
            "TopK(0) must gate all non-hard-keep episodes"
        );
        assert_eq!(result.gated.len(), 1, "the one episode must be gated");
    }

    /// `GateMode::Threshold(0.0)` keeps all episodes.
    #[test]
    fn threshold_zero_keeps_all_episodes() {
        let (read_evs, read_ep) = read_only_episode(0);
        let (fix_evs, fix_ep) = error_fix_episode(10);
        let all_events: Vec<SessionEvent> = read_evs.into_iter().chain(fix_evs).collect();
        let episodes = vec![read_ep, fix_ep];
        let config = SalienceConfig {
            mode: GateMode::Threshold(0.0),
        };
        let result = gate_episodes(&episodes, &all_events, &config);
        assert_eq!(
            result.kept.len(),
            2,
            "Threshold(0.0) must keep all 2 episodes"
        );
        assert_eq!(
            result.gated.len(),
            0,
            "Threshold(0.0) must gate no episodes"
        );
    }

    /// The kept + gated count always equals the input episode count.
    #[test]
    fn kept_plus_gated_equals_total_episode_count() {
        let mut all_events: Vec<SessionEvent> = Vec::new();
        let mut episodes: Vec<Episode> = Vec::new();
        for i in 0..4usize {
            let (evs, ep) = read_only_episode(i * 10);
            all_events.extend(evs);
            episodes.push(ep);
        }
        let (fix_evs, fix_ep) = error_fix_episode(40);
        all_events.extend(fix_evs);
        episodes.push(fix_ep);

        for config in [
            SalienceConfig::default(),
            SalienceConfig {
                mode: GateMode::TopK(2),
            },
            SalienceConfig {
                mode: GateMode::Threshold(0.5),
            },
        ] {
            let result = gate_episodes(&episodes, &all_events, &config);
            assert_eq!(
                result.kept.len() + result.gated.len(),
                episodes.len(),
                "kept + gated must equal total for config {:?}",
                config
            );
        }
    }

    /// `detect_resolved_error_arc` returns false when the arc is not resolved in the episode.
    #[test]
    fn detect_resolved_arc_false_when_error_not_resolved() {
        let events = vec![
            tool_call(0, "t1", "Bash", r#"{}"#),
            tool_result_err(1, "t1", "fail"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        assert!(
            !detect_resolved_error_arc(&event_refs),
            "open (unresolved) arc must return false"
        );
    }

    /// `detect_resolved_error_arc` returns true for a failing→passing pattern.
    #[test]
    fn detect_resolved_arc_true_when_failure_followed_by_success() {
        let events = vec![
            tool_call(0, "t1", "Bash", r#"{}"#),
            tool_result_err(1, "t1", "fail"),
            tool_call(2, "t2", "Bash", r#"{}"#),
            tool_result_ok(3, "t2", "ok"),
        ];
        let event_refs: Vec<&SessionEvent> = events.iter().collect();
        assert!(
            detect_resolved_error_arc(&event_refs),
            "fail→pass arc must return true"
        );
    }
}
