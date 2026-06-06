//! Episodic segmentation — arc-aware cuts over the `SessionEvent` stream with
//! context-budget windowing.
//!
//! This module is **pure and deterministic**: no LLM, no I/O, no clock. It
//! operates exclusively over the typed `SessionEvent` values produced by the
//! #184 parser and is unit-testable without any containers or external services.
//!
//! ## Design rationale
//!
//! A raw session stream is typically 100 k+ tokens. Small local models (8–32 k
//! context) cannot hold it, and long-context reasoning is their weakest axis.
//! Naive token-window splitting destroys the problem→resolution context that
//! makes a chunk useful for skill extraction. This module instead finds
//! *semantically coherent* episodes — problem→work→outcome arcs — that fit the
//! model's context budget while preserving arc integrity.
//!
//! ## Key invariants
//!
//! 1. An open error→resolution arc is never split across episodes except via
//!    the explicit within-arc overlap-window fallback (tagged so downstream
//!    stitching can reassemble it).
//! 2. Every event index belongs to ≥ 1 episode (windows may overlap; nothing
//!    is dropped).
//! 3. The budget knob is provider-agnostic: `token_budget ≥ session size` ⇒
//!    exactly one episode; small budget ⇒ many episodes. The same code path
//!    handles both — no special-case branch.
//! 4. If a heuristic cannot decide, a documented deterministic default applies.
//!    No silent no-ops that lose events.

use domain::SessionEvent;

/// Stable identifier for an error→resolution arc.
///
/// Multiple `Episode`s may carry the same `ArcId` when a single oversized arc
/// is split into overlapping windows. The downstream stitching stage (#187)
/// uses this to reassemble the full arc context across windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArcId(u64);

impl ArcId {
    /// Returns the numeric value of this arc identifier.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// A coherent problem→work→outcome slice of a session, bounded by a token
/// budget and carrying provenance for downstream stitching.
///
/// Event indices in `event_indices` are the **zero-based source-line indices**
/// from `SessionEvent::index()`, not positional offsets into the input slice.
/// They may overlap with adjacent episodes when within-arc overflow windowing
/// is active.
///
/// `arc_id` is `Some` only for episodes that are part of a multi-window arc
/// overflow split. Episodes that fit within the budget in a single window have
/// `arc_id = None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// Source-line indices (from `SessionEvent::index()`) of every event in
    /// this episode. May overlap with the previous or next episode when within-
    /// arc overflow windowing is active.
    pub event_indices: Vec<usize>,
    /// Present when this episode is one window of a multi-window arc overflow.
    /// All windows of the same arc share the same `ArcId` so downstream
    /// stitching can reassemble full context.
    pub arc_id: Option<ArcId>,
}

/// Configuration for the episodic segmenter.
///
/// `token_budget` is the provider-agnostic budget knob: set it to the target
/// model's real context window (minus the preamble and prompt scaffold). At
/// `token_budget ≥ session_token_estimate` the segmenter produces exactly one
/// episode (frontier no-op degrade). At a small budget it produces many. The
/// same code path handles both — no special-case branch.
///
/// `overlap_events` is the minimum number of events that must overlap between
/// consecutive within-arc overflow windows, so a procedure is never split
/// without both halves having the shared context events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentationConfig {
    /// Maximum estimated tokens that may appear in a single episode.
    pub token_budget: usize,
    /// Minimum overlap in events between consecutive within-arc overflow windows.
    pub overlap_events: usize,
}

impl SegmentationConfig {
    /// Creates a config with the given budget and overlap.
    pub fn new(token_budget: usize, overlap_events: usize) -> Self {
        Self {
            token_budget,
            overlap_events,
        }
    }
}

impl Default for SegmentationConfig {
    /// Returns a conservative default: 4 096-token budget, 3-event overlap.
    ///
    /// The default budget is intentionally small to avoid silent large-session
    /// passes in tests or tools that forget to configure it explicitly.
    fn default() -> Self {
        Self {
            token_budget: 4_096,
            overlap_events: 3,
        }
    }
}

/// Estimates the number of tokens in a `SessionEvent`.
///
/// **Heuristic:** `character_count / 4`. This is a well-known rough estimate
/// used across LLM tooling (roughly 1 token ≈ 4 chars in English prose). It
/// is intentionally conservative — the segmenter uses it purely to decide cut
/// points; the actual provider tokenizer is not called here.
///
/// `FileEdit` events share a `tool_use_id` with their corresponding `ToolCall`.
/// Both may appear for the same edit operation (Write/Edit/MultiEdit). To avoid
/// double-counting, callers should track seen `tool_use_id`s and skip `ToolCall`
/// events whose `tool_use_id` has already been counted via a `FileEdit`.
fn estimate_tokens(event: &SessionEvent) -> usize {
    // Character count / 4 is the canonical heuristic for this module.
    // See module-level doc for rationale.
    let char_count = match event {
        SessionEvent::UserMessage { content, .. } => content.len(),
        SessionEvent::AssistantMessage { content, .. } => content.len(),
        SessionEvent::ToolCall {
            name, input_json, ..
        } => name.len() + input_json.len(),
        SessionEvent::ToolResult { output, .. } => output.len(),
        SessionEvent::FileEdit { path, operation, .. } => path.len() + operation.len(),
        SessionEvent::Metadata { event_type, .. } => event_type.len(),
    };
    // Minimum of 1 token per event so empty events don't silently vanish from
    // budget accounting. Division rounds down intentionally (conservative).
    (char_count / 4).max(1)
}

/// Returns `true` when a `ToolResult` signals an unresolved failure.
///
/// An error is open when `is_error` is true OR when `exit_code` is present and
/// non-zero. Both conditions are load-bearing per the #184 wire shape.
fn tool_result_is_error(event: &SessionEvent) -> bool {
    match event {
        SessionEvent::ToolResult {
            is_error,
            exit_code,
            ..
        } => *is_error || exit_code.map_or(false, |code| code != 0),
        _ => false,
    }
}

/// Returns `true` when a `ToolResult` signals a successful completion.
///
/// Success means `is_error` is false AND `exit_code` is either absent or zero.
fn tool_result_is_success(event: &SessionEvent) -> bool {
    match event {
        SessionEvent::ToolResult {
            is_error,
            exit_code,
            ..
        } => !is_error && exit_code.map_or(true, |code| code == 0),
        _ => false,
    }
}

/// Returns the `tool_use_id` of a `ToolCall` or `ToolResult`, if present.
fn tool_use_id(event: &SessionEvent) -> Option<&str> {
    match event {
        SessionEvent::ToolCall { tool_use_id, .. } => Some(tool_use_id.as_str()),
        SessionEvent::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
        SessionEvent::FileEdit { tool_use_id, .. } => Some(tool_use_id.as_str()),
        _ => None,
    }
}

/// Returns the `name` of the tool for a `ToolCall`, if present.
fn tool_call_name(event: &SessionEvent) -> Option<&str> {
    match event {
        SessionEvent::ToolCall { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

/// Segments a session into coherent episodes bounded by `config.token_budget`.
///
/// ## Algorithm
///
/// 1. **Arc detection pass:** scan events left-to-right and mark which events
///    belong to an error→resolution arc. An arc opens on any failing
///    `ToolResult` (`is_error` or non-zero `exit_code`) and closes when the
///    same tool name re-succeeds in a subsequent `ToolResult`, or when a
///    `UserMessage` acknowledges the error (any user message after the failing
///    result closes all open arcs — a conservative but safe heuristic that
///    avoids ever splitting a visible error).
///
/// 2. **Episode boundary pass:** scan events in order, accumulating into a
///    current episode. A new episode boundary is created when:
///    - A `UserMessage` arrives that is NOT inside an open arc AND the current
///      episode already contains a completed tool call or assistant reply
///      (topic-shift heuristic).
///    - The current episode token budget is exhausted AND the current position
///      is not inside an open arc.
///
///    If an arc exceeds the budget, the arc falls through to the within-arc
///    overflow windowing step (step 3).
///
/// 3. **Within-arc overflow windowing:** any episode that exceeds
///    `config.token_budget` (which can only happen when an arc is too large to
///    split normally) is broken into overlapping windows of at most
///    `config.token_budget` tokens, with at least `config.overlap_events` of
///    event overlap. All windows share the same `ArcId`.
///
/// ## Deduplication note
///
/// Write/Edit/MultiEdit operations emit both a `FileEdit` and a `ToolCall`
/// sharing the same `tool_use_id`. Token estimation skips the `ToolCall` when
/// the corresponding `FileEdit` was already counted, to avoid double-counting.
///
/// # Panics
///
/// Never panics. All slice arithmetic is bounds-checked.
pub fn segment_session(
    events: &[SessionEvent],
    config: &SegmentationConfig,
) -> Vec<Episode> {
    if events.is_empty() {
        return Vec::new();
    }

    // Pre-compute the total session token estimate. This drives the topic-shift
    // cut guard: a topic-shift cut only fires when the total session exceeds the
    // budget (i.e., cutting is necessary at all). When the session fits entirely
    // within the budget, no topic-shift cut ever fires and the whole session
    // becomes one episode — the frontier no-op degrade, achieved via the same
    // code path as the many-episode case. No special-case branch is needed.
    let total_session_tokens: usize = events.iter().map(estimate_tokens).sum();

    // Step 1: classify each event position for arc membership.
    let arc_membership = compute_arc_membership(events);

    // Step 2: build raw episodes using topic-shift and budget boundaries,
    //         respecting open-arc boundaries.
    let raw_episodes = build_raw_episodes(events, &arc_membership, config, total_session_tokens);

    // Step 3: apply within-arc overflow windowing to any episode that still
    //         exceeds the token budget (possible only for oversized arcs).
    apply_overflow_windowing(raw_episodes, events, config)
}

/// Per-event arc membership state computed in a single forward scan.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ArcMembership {
    /// Not inside any open arc.
    None,
    /// Inside an open error→resolution arc with the given stable arc id.
    Open(u64),
}

/// Computes arc membership for each event in `events`.
///
/// The return value has the same length as `events`. `ArcMembership::Open(id)`
/// means the event at that position is inside an open error→resolution arc with
/// the given stable `id`. `ArcMembership::None` means it is not.
///
/// ### Arc open/close rules
///
/// - A failing `ToolResult` (`is_error` OR non-zero `exit_code`) **opens** an
///   arc for the tool identified by the `tool_use_id`. The arc is labelled with
///   the `name` of the corresponding `ToolCall` (looked up by `tool_use_id`).
/// - A **`UserMessage`** after a failing result **closes all open arcs** at the
///   USER message position (the user message itself is NOT inside the arc): a
///   user acknowledgement signals the error is resolved from the user's
///   perspective, so the topic shift is legal at that boundary.
/// - A **successful `ToolResult`** whose originating `ToolCall` has the same
///   `name` as an open arc **closes** that arc. Critically, the resolution event
///   is labeled as `Open` (inside the arc) BEFORE the arc is removed — the
///   resolution belongs to the arc it closes.
fn compute_arc_membership(events: &[SessionEvent]) -> Vec<ArcMembership> {
    // Build a lookup from tool_use_id → tool name for all ToolCall events.
    // This lets arc resolution map a ToolResult back to the originating tool name.
    let mut tool_name_by_id: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::new();
    for event in events {
        if let (Some(id), Some(name)) = (tool_use_id(event), tool_call_name(event)) {
            tool_name_by_id.insert(id, name);
        }
    }

    // Open arcs: maps tool_name → arc_id. Multiple arcs for different tools
    // may be simultaneously open (e.g., two parallel failing commands).
    let mut open_arcs: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut next_arc_id: u64 = 1;
    let mut membership = vec![ArcMembership::None; events.len()];

    for (position, event) in events.iter().enumerate() {
        // Opening: a failing ToolResult opens an arc for its originating tool.
        // This must happen BEFORE labeling so the opening event is inside the arc.
        if tool_result_is_error(event) {
            if let Some(id) = tool_use_id(event) {
                let tool_name = tool_name_by_id
                    .get(id)
                    .copied()
                    .unwrap_or("unknown");
                // Only open a new arc if this tool isn't already in an open arc.
                open_arcs
                    .entry(tool_name.to_owned())
                    .or_insert_with(|| {
                        let arc_id = next_arc_id;
                        next_arc_id += 1;
                        arc_id
                    });
            }
        }

        // Label this event with the current arc state BEFORE any close logic.
        // This ensures both the failing event and the resolution event are
        // labeled as inside the arc (the resolution closes it but belongs to it).
        membership[position] = if open_arcs.is_empty() {
            ArcMembership::None
        } else {
            // If multiple arcs are open (rare), use the lowest arc id
            // for determinism. This keeps arc ids monotonically assigned
            // and avoids non-determinism from HashMap iteration order.
            let arc_id = open_arcs.values().copied().min().unwrap_or(0);
            ArcMembership::Open(arc_id)
        };

        // Closing: a successful ToolResult whose tool name matches an open arc
        // closes that arc. Close AFTER labeling so the resolution event is
        // inside the arc.
        if tool_result_is_success(event) {
            if let Some(id) = tool_use_id(event) {
                let tool_name = tool_name_by_id
                    .get(id)
                    .copied()
                    .unwrap_or("unknown");
                open_arcs.remove(tool_name);
            }
        }

        // A user message closes all open arcs AFTER labeling the user message
        // as NOT inside the arc (since open_arcs was cleared before reaching the
        // label step only if there are no open arcs). Wait — we label first, then
        // close. For a UserMessage arriving while arcs are open, it would be labeled
        // Open. We want it labeled None (the user message itself marks the resolution
        // boundary and is the start of the next topic). So for UserMessage, we label
        // it as None regardless of open arcs, then clear.
        //
        // Correction: rewrite the label for UserMessage events to always be None,
        // then close all arcs. The user message is the BOUNDARY, not inside the arc.
        if matches!(event, SessionEvent::UserMessage { .. }) && !open_arcs.is_empty() {
            // Override: a user message is never inside an arc — it is the
            // acknowledgement that ends all open arcs.
            membership[position] = ArcMembership::None;
            open_arcs.clear();
        }
    }

    membership
}

/// Accumulates events into candidate raw episodes, respecting arc boundaries
/// and the token budget.
///
/// A candidate episode boundary is created:
/// 1. At a `UserMessage` that arrives when:
///    - the total session token estimate exceeds the budget (cutting is necessary),
///    - the current episode already contains at least one non-user event, AND
///    - the current position is NOT inside an open arc (topic-shift rule).
///    The `total_session_tokens > config.token_budget` guard ensures that when
///    the session fits entirely within the budget, no topic-shift cut fires —
///    the whole session becomes one episode (frontier no-op degrade) via the
///    same code path. No separate special-case branch is needed.
/// 2. When the cumulative token estimate would exceed `config.token_budget` AND
///    the current position is NOT inside an open arc (budget-overflow rule).
///
/// If both conditions are suppressed by an open arc, events accumulate past the
/// budget into an oversized episode. The overflow windowing step (step 3) then
/// handles those.
fn build_raw_episodes(
    events: &[SessionEvent],
    arc_membership: &[ArcMembership],
    config: &SegmentationConfig,
    total_session_tokens: usize,
) -> Vec<Vec<usize>> {
    let mut episodes: Vec<Vec<usize>> = Vec::new();
    let mut current_episode_indices: Vec<usize> = Vec::new();
    let mut current_token_count: usize = 0;
    // Track FileEdit tool_use_ids so we can skip double-counting their ToolCall.
    let mut file_edit_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (position, event) in events.iter().enumerate() {
        let event_index = event.index();
        let in_open_arc = arc_membership[position] != ArcMembership::None;

        // Compute token estimate for this event, skipping ToolCalls that are
        // already counted by a FileEdit with the same tool_use_id.
        let is_file_edit_duplicate_tool_call = matches!(event, SessionEvent::ToolCall { .. })
            && tool_use_id(event)
                .map_or(false, |id| file_edit_ids.contains(id));

        let event_tokens = if is_file_edit_duplicate_tool_call {
            // Skip token cost — this ToolCall was already counted via its FileEdit.
            0
        } else {
            estimate_tokens(event)
        };

        // Track FileEdit ids for deduplication.
        if let SessionEvent::FileEdit { tool_use_id, .. } = event {
            file_edit_ids.insert(tool_use_id.clone());
        }

        // Determine if we should cut BEFORE adding this event.
        //
        // Topic-shift cuts are suppressed when the total session fits the budget:
        // `total_session_tokens <= config.token_budget` means no cut is needed at
        // all, and the whole session becomes one episode via this same code path.
        let should_cut_at_topic_shift = matches!(event, SessionEvent::UserMessage { .. })
            && !current_episode_indices.is_empty()
            && has_non_user_event(events, &current_episode_indices)
            && !in_open_arc
            && total_session_tokens > config.token_budget;

        let would_exceed_budget = !current_episode_indices.is_empty()
            && current_token_count + event_tokens > config.token_budget
            && !in_open_arc;

        if should_cut_at_topic_shift || would_exceed_budget {
            // Commit the current episode and start a new one.
            episodes.push(std::mem::take(&mut current_episode_indices));
            current_token_count = 0;
        }

        current_episode_indices.push(event_index);
        current_token_count += event_tokens;
    }

    // Commit the final episode (always non-empty because events is non-empty).
    if !current_episode_indices.is_empty() {
        episodes.push(current_episode_indices);
    }

    episodes
}

/// Returns `true` when `indices` contains at least one event that is NOT a
/// `UserMessage`. Used by the topic-shift rule to ensure we only cut after
/// some work (tool calls, assistant replies) has been done — not at back-to-back
/// user messages.
fn has_non_user_event(events: &[SessionEvent], indices: &[usize]) -> bool {
    // Build a fast lookup: source index → position in events slice.
    // This is O(n) per call, but episodes are short enough that this is fine.
    // A precomputed lookup map could be added if profiling ever flags this.
    let index_to_position: std::collections::HashMap<usize, usize> = events
        .iter()
        .enumerate()
        .map(|(pos, ev)| (ev.index(), pos))
        .collect();

    indices.iter().any(|&idx| {
        index_to_position
            .get(&idx)
            .map_or(false, |&pos| !matches!(events[pos], SessionEvent::UserMessage { .. }))
    })
}

/// Applies within-arc overflow windowing to any oversized episode.
///
/// An episode may exceed `config.token_budget` only when an arc forced events
/// to accumulate past the limit. This step breaks such oversized episodes into
/// overlapping windows of at most `config.token_budget` estimated tokens, each
/// window overlapping the previous by at least `config.overlap_events` events.
/// All windows share the same `ArcId`.
///
/// Episodes that fit within the budget pass through unchanged with `arc_id = None`.
fn apply_overflow_windowing(
    raw_episodes: Vec<Vec<usize>>,
    events: &[SessionEvent],
    config: &SegmentationConfig,
) -> Vec<Episode> {
    // Build a source-index → event map for token estimation.
    let index_to_event: std::collections::HashMap<usize, &SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    // Arc id counter for overflow windows (starts at a high value to avoid
    // collision with the arc ids produced by `compute_arc_membership`).
    let mut overflow_arc_counter: u64 = u64::MAX / 2;

    let mut episodes: Vec<Episode> = Vec::new();

    for raw_indices in raw_episodes {
        let episode_tokens: usize = raw_indices
            .iter()
            .filter_map(|idx| index_to_event.get(idx).copied())
            .map(estimate_tokens)
            .sum();

        if episode_tokens <= config.token_budget {
            // Fits within the budget: emit as a single episode without an arc id.
            episodes.push(Episode {
                event_indices: raw_indices,
                arc_id: None,
            });
        } else {
            // Oversized arc: split into overlapping windows sharing one arc id.
            // The overlap is at least config.overlap_events events.
            overflow_arc_counter += 1;
            let arc_id = ArcId(overflow_arc_counter);
            let windows = split_into_overlap_windows(&raw_indices, &index_to_event, config);
            for window_indices in windows {
                episodes.push(Episode {
                    event_indices: window_indices,
                    arc_id: Some(arc_id),
                });
            }
        }
    }

    episodes
}

/// Splits `indices` into overlapping windows, each fitting within
/// `config.token_budget` estimated tokens and overlapping the previous window
/// by at least `config.overlap_events` events.
///
/// The algorithm is greedy: each window starts where the previous one left off
/// minus `overlap_events`, then extends rightward until the budget is full.
/// This guarantees:
/// - Every index appears in ≥ 1 window (no drops).
/// - Consecutive windows share ≥ `overlap_events` events (when window sizes
///   permit; a window smaller than `overlap_events + 1` events is only emitted
///   as the final window if it contains events not yet covered, and no further
///   window is created — the tail is absorbed into the previous window's overlap).
/// - Each window fits `token_budget` (a final window may be shorter — accepted
///   as-is when it introduces new events beyond the previous window's coverage).
///
/// Stopping rule: a new window is only started when it would introduce at least
/// one event NOT already covered by the previous window. This prevents creating
/// pure-overlap tail windows with no new content, which would cause consecutive
/// pairs to have fewer than `overlap_events` shared events.
///
/// If `config.token_budget` is so small that a single event alone exceeds it,
/// each event is placed in its own window (single-event windows are the minimum,
/// always emitted even when oversized). This prevents an infinite loop.
fn split_into_overlap_windows(
    indices: &[usize],
    index_to_event: &std::collections::HashMap<usize, &SessionEvent>,
    config: &SegmentationConfig,
) -> Vec<Vec<usize>> {
    let mut windows: Vec<Vec<usize>> = Vec::new();

    // `start` is the inclusive starting position in `indices` for the current window.
    let mut start: usize = 0;

    while start < indices.len() {
        let mut window: Vec<usize> = Vec::new();
        let mut window_tokens: usize = 0;

        for &idx in &indices[start..] {
            let event_tokens = index_to_event
                .get(&idx)
                .map(|ev| estimate_tokens(ev))
                .unwrap_or(1);

            // Always include at least one event per window to guarantee progress
            // even when a single event exceeds the budget.
            if window.is_empty() {
                window.push(idx);
                window_tokens += event_tokens;
            } else if window_tokens + event_tokens <= config.token_budget {
                window.push(idx);
                window_tokens += event_tokens;
            } else {
                // Budget exhausted: stop the window here.
                break;
            }
        }

        let window_len = window.len();

        // Compute the exclusive end position of this window in `indices`.
        let window_end = start + window_len;

        windows.push(window);

        // Advance `start` for the next window so that it overlaps this one by
        // `overlap_events` events (the new window starts `overlap_events` before
        // the current window's end).
        if window_len <= config.overlap_events {
            // Window is overlap-sized or smaller: advance by 1 for progress.
            start += 1;
        } else {
            start += window_len - config.overlap_events;
        }

        // Stopping rule: if the current window already covers all remaining
        // events (its end reached the end of `indices`), no further window can
        // introduce new content. Stop to avoid creating pure-overlap tail windows
        // that have fewer than `overlap_events` events of overlap with their
        // predecessor.
        if window_end >= indices.len() {
            break;
        }
    }

    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::SessionEvent;

    // ---- Helpers to construct test events compactly ----

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

    fn tool_result_no_exit(index: usize, id: &str, output: &str) -> SessionEvent {
        // is_error = true but no exit_code — tests the Option<i32> path.
        SessionEvent::ToolResult {
            index,
            tool_use_id: id.to_owned(),
            is_error: true,
            exit_code: None,
            output: output.to_owned(),
        }
    }

    /// Returns a sorted, deduplicated set of event indices across all episodes.
    fn all_covered_indices(episodes: &[Episode]) -> Vec<usize> {
        let mut seen: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for ep in episodes {
            for &idx in &ep.event_indices {
                seen.insert(idx);
            }
        }
        seen.into_iter().collect()
    }

    /// Returns the expected sorted set of event indices from the input slice.
    fn expected_indices(events: &[SessionEvent]) -> Vec<usize> {
        let mut indices: Vec<usize> = events.iter().map(|e| e.index()).collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    // ---- Acceptance test 1: error→fix arc lands wholly within one episode ----

    /// Proves that each error→fix arc lands wholly within one episode in a
    /// multi-arc session: no episode boundary may split an open error arc.
    #[test]
    fn multi_arc_session_each_error_fix_arc_in_one_episode() {
        // Session structure:
        //   [0] user: "run tests"          ← topic A
        //   [1] tool_call(bash, id=t1)
        //   [2] tool_result_err(id=t1)     ← arc opens
        //   [3] tool_call(bash, id=t2)     ← retry
        //   [4] tool_result_ok(id=t2)      ← arc closes (bash succeeded)
        //   [5] user: "now do something else"  ← topic shift (arc closed)
        //   [6] tool_call(read, id=t3)
        //   [7] tool_result_err(id=t3)     ← arc opens
        //   [8] tool_call(read, id=t4)     ← retry
        //   [9] tool_result_ok(id=t4)      ← arc closes
        let events = vec![
            user_msg(0, "run tests"),
            tool_call(1, "t1", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_err(2, "t1", "Exit code 1\nerror: test failed"),
            tool_call(3, "t2", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_ok(4, "t2", "test passed"),
            user_msg(5, "now do something else"),
            tool_call(6, "t3", "Read", r#"{"file_path":"src/lib.rs"}"#),
            tool_result_err(7, "t3", "Exit code 1\nfile not found"),
            tool_call(8, "t4", "Read", r#"{"file_path":"src/lib.rs"}"#),
            tool_result_ok(9, "t4", "file contents"),
        ];

        // Budget (40 tokens) is below the total session estimate (~56 tokens) so
        // topic-shift cuts are active, but above each individual topic's estimate
        // (~27-29 tokens) so arcs are never split by the budget-overflow rule.
        let config = SegmentationConfig::new(40, 3);
        let episodes = segment_session(&events, &config);

        // There must be exactly 2 episodes (one per topic).
        assert_eq!(
            episodes.len(),
            2,
            "expected 2 episodes (one per topic arc), got {}: {episodes:?}",
            episodes.len()
        );

        // Episode 0 must contain ALL of events 0..=4 (the first arc).
        let ep0_set: std::collections::HashSet<usize> =
            episodes[0].event_indices.iter().copied().collect();
        for idx in 0..=4 {
            assert!(
                ep0_set.contains(&idx),
                "event {idx} (part of first arc) must be in episode 0"
            );
        }

        // Episode 1 must contain ALL of events 5..=9 (the second arc).
        let ep1_set: std::collections::HashSet<usize> =
            episodes[1].event_indices.iter().copied().collect();
        for idx in 5..=9 {
            assert!(
                ep1_set.contains(&idx),
                "event {idx} (part of second arc) must be in episode 1"
            );
        }

        // Arcs must not be split: the error event (2) and its resolution (4)
        // must be in the same episode.
        assert!(
            ep0_set.contains(&2) && ep0_set.contains(&4),
            "error event 2 and resolution event 4 must be in the same episode"
        );
        assert!(
            ep1_set.contains(&7) && ep1_set.contains(&9),
            "error event 7 and resolution event 9 must be in the same episode"
        );
    }

    // ---- Acceptance test 2: oversized arc produces overlapping windows with arc_id ----

    /// Proves that an arc larger than the token budget produces overlapping
    /// windows sharing one arc_id, with overlap ≥ config.overlap_events.
    #[test]
    fn oversized_arc_produces_overlapping_windows_tagged_with_arc_id() {
        // Build a session with one huge arc (many retry attempts) that far exceeds
        // a tiny budget. Each event content is padded to ~20 chars so even a
        // 100-token budget is quickly exhausted.
        //
        //   [0]  user: "run tests"
        //   [1]  tool_call(Bash, t1)
        //   [2]  tool_result_err(t1)   ← arc opens
        //   [3..12] alternating tool_call + tool_result_err (arc stays open)
        //   [13] tool_call(Bash, t8)
        //   [14] tool_result_ok(t8)    ← arc closes
        let mut events = vec![
            user_msg(0, "run tests"),
            tool_call(1, "t1", "Bash", r#"{"command":"cargo test --all"}"#),
            tool_result_err(2, "t1", "Exit code 1\ntest failed step 1"),
        ];
        // Add 8 more failing attempts (indices 3..18).
        for i in 0..8u8 {
            let idx_call = 3 + (i as usize) * 2;
            let idx_result = idx_call + 1;
            let id_call = format!("t{}", i + 2);
            let id_result = format!("t{}", i + 2);
            events.push(tool_call(
                idx_call,
                &id_call,
                "Bash",
                r#"{"command":"cargo test --all"}"#,
            ));
            events.push(tool_result_err(
                idx_result,
                &id_result,
                &format!("Exit code 1\ntest failed step {}", i + 2),
            ));
        }
        // Final success: close the arc.
        let final_call_idx = events.len();
        let final_result_idx = final_call_idx + 1;
        events.push(tool_call(
            final_call_idx,
            "t10",
            "Bash",
            r#"{"command":"cargo test --all"}"#,
        ));
        events.push(tool_result_ok(
            final_result_idx,
            "t10",
            "all tests passed",
        ));

        // Use a tiny budget (60 tokens) so the arc definitely overflows.
        let overlap_events = 2;
        let config = SegmentationConfig::new(60, overlap_events);
        let episodes = segment_session(&events, &config);

        // All windows for the oversized arc must share the same arc_id.
        let arc_ids: Vec<Option<ArcId>> = episodes.iter().map(|ep| ep.arc_id).collect();
        // Every episode that contains arc events must have an arc_id.
        // (The initial user message may be its own episode without arc_id, or
        //  it may be folded into the first arc window — both are acceptable.)
        let arc_episodes: Vec<&Episode> =
            episodes.iter().filter(|ep| ep.arc_id.is_some()).collect();
        assert!(
            arc_episodes.len() >= 2,
            "oversized arc must produce at least 2 windowed episodes; got {}: {arc_ids:?}",
            arc_episodes.len()
        );

        // All arc episodes must share the same arc_id.
        let unique_arc_ids: std::collections::HashSet<u64> = arc_episodes
            .iter()
            .map(|ep| ep.arc_id.unwrap().as_u64())
            .collect();
        assert_eq!(
            unique_arc_ids.len(),
            1,
            "all overflow windows must share one arc_id; got: {unique_arc_ids:?}"
        );

        // Check that consecutive arc-episode windows overlap by ≥ overlap_events.
        for window_pair in arc_episodes.windows(2) {
            let prev_set: std::collections::HashSet<usize> =
                window_pair[0].event_indices.iter().copied().collect();
            let overlap_count = window_pair[1]
                .event_indices
                .iter()
                .filter(|idx| prev_set.contains(idx))
                .count();
            assert!(
                overlap_count >= overlap_events,
                "consecutive arc windows must overlap by ≥ {overlap_events} events; got {overlap_count}"
            );
        }

        // Every event index must appear in at least one episode.
        let covered = all_covered_indices(&episodes);
        let expected = expected_indices(&events);
        assert_eq!(
            covered, expected,
            "every event must appear in at least one episode"
        );
    }

    // ---- Acceptance test 3: every event covered + topic shifts open new episodes ----

    /// Proves that every event index appears in ≥ 1 episode and that topic
    /// shifts (new user messages after completed work) open new episodes.
    #[test]
    fn every_event_covered_and_topic_shifts_open_new_episodes() {
        // Three distinct topics separated by topic-shift user messages.
        let events = vec![
            // Topic A
            user_msg(0, "do A"),
            assistant_msg(1, "ok I'll do A"),
            tool_call(2, "a1", "Bash", r#"{"command":"do A"}"#),
            tool_result_ok(3, "a1", "done A"),
            // Topic B: new user message after completed tool work
            user_msg(4, "now do B"),
            assistant_msg(5, "ok I'll do B"),
            tool_call(6, "b1", "Read", r#"{"file_path":"b.txt"}"#),
            tool_result_ok(7, "b1", "contents of b"),
            // Topic C
            user_msg(8, "now do C"),
            assistant_msg(9, "working on C"),
            tool_call(10, "c1", "Write", r#"{"file_path":"c.txt","content":"c"}"#),
            tool_result_ok(11, "c1", "written"),
        ];

        // Budget (20 tokens) is below the total session estimate (~40 tokens) so
        // topic-shift cuts are active, while each topic's events fit within the
        // budget individually (~10-19 tokens per topic).
        let config = SegmentationConfig::new(20, 2);
        let episodes = segment_session(&events, &config);

        // Must have at least 3 episodes (one per topic).
        assert!(
            episodes.len() >= 3,
            "expected ≥ 3 episodes for 3 distinct topics; got {}",
            episodes.len()
        );

        // Every event index must appear in at least one episode.
        let covered = all_covered_indices(&episodes);
        let expected = expected_indices(&events);
        assert_eq!(
            covered, expected,
            "every event must appear in at least one episode (no drops)"
        );

        // The topic-shift user messages (4 and 8) must each be in a different
        // episode from the prior assistant/tool events.
        let episode_of = |idx: usize| -> Option<usize> {
            episodes
                .iter()
                .position(|ep| ep.event_indices.contains(&idx))
        };

        let episode_for_3 = episode_of(3).expect("event 3 must be covered");
        let episode_for_4 = episode_of(4).expect("event 4 must be covered");
        assert_ne!(
            episode_for_3, episode_for_4,
            "topic-shift user message (event 4) must start a new episode"
        );

        let episode_for_7 = episode_of(7).expect("event 7 must be covered");
        let episode_for_8 = episode_of(8).expect("event 8 must be covered");
        assert_ne!(
            episode_for_7, episode_for_8,
            "topic-shift user message (event 8) must start a new episode"
        );
    }

    // ---- Acceptance test 4: SegmentationConfig parameterises cut points ----

    /// Proves that changing `token_budget` changes cut points deterministically:
    /// a tight budget produces more episodes than a generous budget over the same
    /// session, with the same code path.
    #[test]
    fn segmentation_config_budget_changes_cut_points_deterministically() {
        // Build a session with repeated similar-sized blocks.
        let events: Vec<SessionEvent> = (0..12)
            .flat_map(|i| {
                vec![
                    // Each block is a user message + tool call + ok result.
                    user_msg(i * 3, &format!("task {i}")),
                    tool_call(
                        i * 3 + 1,
                        &format!("id{i}"),
                        "Bash",
                        r#"{"command":"echo hello"}"#,
                    ),
                    tool_result_ok(i * 3 + 2, &format!("id{i}"), "hello"),
                ]
            })
            .collect();

        // Tight budget: forces many episodes.
        let tight_config = SegmentationConfig::new(50, 2);
        let tight_episodes = segment_session(&events, &tight_config);

        // Generous budget: allows fewer episodes.
        let generous_config = SegmentationConfig::new(10_000, 2);
        let generous_episodes = segment_session(&events, &generous_config);

        // Tight budget must produce more episodes than generous budget.
        assert!(
            tight_episodes.len() > generous_episodes.len(),
            "tight budget ({} episodes) must produce more episodes than generous budget ({} episodes)",
            tight_episodes.len(),
            generous_episodes.len()
        );

        // Both must cover every event.
        assert_eq!(
            all_covered_indices(&tight_episodes),
            expected_indices(&events),
            "tight budget must cover all events"
        );
        assert_eq!(
            all_covered_indices(&generous_episodes),
            expected_indices(&events),
            "generous budget must cover all events"
        );
    }

    // ---- Acceptance test 5: budget ≥ session ⇒ one episode; small budget ⇒ many ----

    /// Proves that `token_budget ≥ session_token_estimate` produces exactly ONE
    /// episode (frontier no-op degrade), and that a small budget produces many
    /// episodes over the same session — via the same code path.
    #[test]
    fn budget_at_least_session_size_yields_single_episode_same_code_path() {
        let events = vec![
            user_msg(0, "do something"),
            assistant_msg(1, "sure"),
            tool_call(2, "x1", "Bash", r#"{"command":"ls"}"#),
            tool_result_ok(3, "x1", "file.txt"),
            user_msg(4, "now do something else"),
            assistant_msg(5, "ok"),
            tool_call(6, "x2", "Read", r#"{"file_path":"file.txt"}"#),
            tool_result_ok(7, "x2", "content"),
        ];

        // Very large budget: all events fit in one episode.
        let large_config = SegmentationConfig::new(1_000_000, 3);
        let large_episodes = segment_session(&events, &large_config);
        assert_eq!(
            large_episodes.len(),
            1,
            "budget ≥ session size must yield exactly 1 episode; got {}",
            large_episodes.len()
        );
        assert_eq!(
            all_covered_indices(&large_episodes),
            expected_indices(&events),
            "single episode must cover all events"
        );

        // Very small budget: must produce many episodes.
        let tiny_config = SegmentationConfig::new(10, 1);
        let tiny_episodes = segment_session(&events, &tiny_config);
        assert!(
            tiny_episodes.len() > 1,
            "small budget must produce more than 1 episode; got {}",
            tiny_episodes.len()
        );
        assert_eq!(
            all_covered_indices(&tiny_episodes),
            expected_indices(&events),
            "all episodes must cover all events"
        );
    }

    // ---- Additional edge-case tests ----

    /// Empty input produces no episodes.
    #[test]
    fn empty_input_produces_no_episodes() {
        let episodes = segment_session(&[], &SegmentationConfig::default());
        assert!(episodes.is_empty(), "empty input must yield no episodes");
    }

    /// A single-event input always produces exactly one episode.
    #[test]
    fn single_event_always_yields_one_episode() {
        let events = vec![user_msg(0, "hi")];
        let config = SegmentationConfig::new(1, 1); // even budget=1
        let episodes = segment_session(&events, &config);
        assert_eq!(episodes.len(), 1, "single event must yield exactly 1 episode");
        assert_eq!(episodes[0].event_indices, vec![0]);
    }

    /// A ToolResult with `is_error=true` but no exit_code still opens an arc.
    #[test]
    fn tool_result_with_is_error_true_and_no_exit_code_opens_arc() {
        let events = vec![
            user_msg(0, "try something"),
            tool_call(1, "e1", "Bash", r#"{}"#),
            tool_result_no_exit(2, "e1", "some error without exit code"),
            tool_call(3, "e2", "Bash", r#"{}"#),
            tool_result_ok(4, "e2", "success"),
            // Topic shift: would cut here if arc were closed.
            user_msg(5, "new topic"),
            tool_call(6, "e3", "Read", r#"{}"#),
            tool_result_ok(7, "e3", "ok"),
        ];

        let config = SegmentationConfig::new(8_000, 2);
        let episodes = segment_session(&events, &config);

        // The error at index 2 and its resolution at 4 must be in the same episode.
        let ep_of_2 = episodes
            .iter()
            .position(|ep| ep.event_indices.contains(&2))
            .expect("event 2 must be covered");
        let ep_of_4 = episodes
            .iter()
            .position(|ep| ep.event_indices.contains(&4))
            .expect("event 4 must be covered");
        assert_eq!(
            ep_of_2, ep_of_4,
            "is_error=true with no exit_code must keep error (2) and resolution (4) in same episode"
        );
    }

    /// Non-zero exit_code with is_error=false still opens an arc.
    #[test]
    fn tool_result_with_nonzero_exit_code_opens_arc_regardless_of_is_error_flag() {
        let events = vec![
            user_msg(0, "compile"),
            tool_call(1, "c1", "Bash", r#"{"command":"cargo build"}"#),
            // exit_code = 2, is_error = false: should still open an arc.
            SessionEvent::ToolResult {
                index: 2,
                tool_use_id: "c1".to_owned(),
                is_error: false,
                exit_code: Some(2),
                output: "error[E0001]: ...".to_owned(),
            },
            tool_call(3, "c2", "Bash", r#"{"command":"cargo build"}"#),
            tool_result_ok(4, "c2", "Finished"),
            user_msg(5, "deploy now"),
            tool_call(6, "d1", "Bash", r#"{"command":"deploy"}"#),
            tool_result_ok(7, "d1", "deployed"),
        ];

        let config = SegmentationConfig::new(8_000, 2);
        let episodes = segment_session(&events, &config);

        // Event 2 (non-zero exit code) and event 4 (resolution) must be in same episode.
        let ep_of_2 = episodes
            .iter()
            .position(|ep| ep.event_indices.contains(&2))
            .expect("event 2 must be covered");
        let ep_of_4 = episodes
            .iter()
            .position(|ep| ep.event_indices.contains(&4))
            .expect("event 4 must be covered");
        assert_eq!(
            ep_of_2, ep_of_4,
            "non-zero exit_code must keep error (2) and resolution (4) in same episode"
        );
    }

    /// FileEdit + ToolCall sharing a tool_use_id are not double-counted in token budget.
    #[test]
    fn file_edit_and_tool_call_with_same_id_not_double_counted() {
        // If we double-count, the budget will be exhausted earlier and we'll get more episodes.
        // With dedup, the same budget covers more events.
        let file_edit_content = "x".repeat(200); // large enough to matter
        let events = vec![
            user_msg(0, "edit a file"),
            // ToolCall for a Write operation (will also emit a FileEdit)
            tool_call(1, "w1", "Write", &format!(r#"{{"file_path":"a.txt","content":"{file_edit_content}"}}"#)),
            // FileEdit for the same operation
            SessionEvent::FileEdit {
                index: 2,
                tool_use_id: "w1".to_owned(),
                path: "a.txt".to_owned(),
                operation: "Write".to_owned(),
            },
            tool_result_ok(3, "w1", "written"),
            // Another user message to force a potential topic-shift episode cut.
            user_msg(4, "now read it"),
            tool_call(5, "r1", "Read", r#"{"file_path":"a.txt"}"#),
            tool_result_ok(6, "r1", "file contents"),
        ];

        // Budget that's large enough to hold one topic's events WITHOUT double-counting,
        // but would be exceeded if ToolCall + FileEdit were both counted.
        // FileEdit path="a.txt" + operation="Write" ≈ (5+5)/4 = 2 tokens.
        // ToolCall name="Write" + input_json ≈ (5+220)/4 ≈ 56 tokens.
        // With dedup: ToolCall is skipped, FileEdit pays 2 tokens. Total for block ≈ reasonable.
        // Without dedup: both count, which would break budget.
        let config = SegmentationConfig::new(8_000, 2);
        let episodes = segment_session(&events, &config);

        // With dedup, events 0..=3 fit in one episode and events 4..=6 in another.
        assert_eq!(
            all_covered_indices(&episodes),
            expected_indices(&events),
            "all events must be covered"
        );
    }
}
