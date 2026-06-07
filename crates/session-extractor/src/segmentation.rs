//! Episodic segmentation — turn-aware overlapping windows over the `SessionEvent` stream.
//!
//! This module is **pure and deterministic**: no LLM, no I/O, no clock. It
//! operates exclusively over the typed `SessionEvent` values produced by the
//! #184 parser and is unit-testable without any containers or external services.
//!
//! ## Design rationale
//!
//! A raw session stream is typically 100k+ tokens. Small local models (8–32k
//! context) cannot hold it. This module splits sessions into overlapping windows
//! bounded by a token budget so every window fits in the model's context.
//!
//! **Why overlapping windows instead of arc-aware cuts?**
//!
//! Arc-aware cuts require tool events and keywords to fire. On flat conversational
//! transcripts (pure UserMessage/AssistantMessage, no Claude-Code tool events, no
//! "always/never" keywords) arc detection finds nothing and segment_session returns
//! zero usable episodes — causing total extraction failure. Overlapping windows work
//! identically on any transcript shape, recovering extraction on all session types.
//! External research (mem0/Letta/Zep, LangChain map-reduce, RAG chunking benchmarks)
//! is unambiguous: extract from EVERY chunk (recall-first), never pre-filter with
//! heuristics.
//!
//! ## Key invariants
//!
//! 1. Every event index appears in ≥ 1 window. No event is dropped.
//! 2. Consecutive windows overlap by exactly `overlap_events` events (or fewer if
//!    the tail window is smaller).
//! 3. The budget knob is provider-agnostic: `token_budget ≥ session_token_estimate`
//!    ⇒ exactly ONE window (frontier no-op degrade). Small budget ⇒ many windows.
//!    The same code path handles both — no special-case branch.
//! 4. A single event exceeding the budget is placed in its own window to guarantee
//!    progress (no infinite loop).

use domain::SessionEvent;

/// Stable identifier for an arc-tagged overflow window group.
///
/// Preserved for API compatibility. In the current turn-aware windowing algorithm,
/// `arc_id` is always `None` — windows are produced by budget-packing, not arc
/// detection. The field remains in [`Episode`] so no downstream code needs to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArcId(u64);

impl ArcId {
    /// Returns the numeric value of this arc identifier.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// One overlapping window of a session, bounded by a token budget.
///
/// Event indices in `event_indices` are the **zero-based source-line indices**
/// from `SessionEvent::index()`, not positional offsets into the input slice.
/// Adjacent windows overlap by `config.overlap_events` events (the last K events
/// of window N are the first K events of window N+1).
///
/// `arc_id` is always `None` in the current windowing algorithm. It is preserved
/// for API compatibility with downstream code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// Source-line indices (from `SessionEvent::index()`) of every event in
    /// this window. May overlap with the previous or next window.
    pub event_indices: Vec<usize>,
    /// Always `None` in the current windowing algorithm. Preserved for API
    /// compatibility.
    pub arc_id: Option<ArcId>,
}

/// Configuration for the episodic segmenter.
///
/// `token_budget` is the provider-agnostic budget knob: set it to the target
/// model's real context window (minus the preamble and prompt scaffold). At
/// `token_budget ≥ session_token_estimate` the segmenter produces exactly one
/// window (frontier no-op degrade). At a small budget it produces many. The
/// same code path handles both — no special-case branch.
///
/// `overlap_events` is the number of events that must overlap between consecutive
/// windows, so a multi-turn thought that spans a window boundary still appears in
/// full context in at least one window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentationConfig {
    /// Maximum estimated tokens that may appear in a single window.
    pub token_budget: usize,
    /// Number of events to carry over from the end of one window into the start
    /// of the next. Higher values increase recall at the cost of more LLM calls.
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
fn estimate_tokens(event: &SessionEvent) -> usize {
    let char_count = match event {
        SessionEvent::UserMessage { content, .. } => content.len(),
        SessionEvent::AssistantMessage { content, .. } => content.len(),
        SessionEvent::ToolCall {
            name, input_json, ..
        } => name.len() + input_json.len(),
        SessionEvent::ToolResult { output, .. } => output.len(),
        SessionEvent::FileEdit {
            path, operation, ..
        } => path.len() + operation.len(),
        SessionEvent::Metadata { event_type, .. } => event_type.len(),
    };
    // Minimum of 1 token per event so empty events don't silently vanish from
    // budget accounting. Division rounds down intentionally (conservative).
    (char_count / 4).max(1)
}

/// Segments a session into overlapping windows bounded by `config.token_budget`.
///
/// ## Algorithm
///
/// Events are packed greedily into windows:
/// 1. Starting from position 0, add events until adding the next event would
///    exceed `config.token_budget`. At least one event is always included (even
///    if it alone exceeds the budget) to guarantee progress.
/// 2. The next window starts at `window_start + (window_len - overlap_events)`,
///    so the last `overlap_events` events of the current window become the first
///    events of the next window.
/// 3. A new window is only started when it would introduce at least one event
///    beyond the previous window's coverage, preventing infinite tail loops.
///
/// ## Budget knob
///
/// `token_budget ≥ total_session_tokens` ⇒ exactly ONE window (the whole session
/// fits; no second window starts because there are no uncovered events). Small
/// budget ⇒ many windows. Same code path — no special-case branch.
///
/// ## Works on any transcript shape
///
/// Flat conversational transcripts (only UserMessage/AssistantMessage, no tool
/// events) produce non-empty windows covering every turn. There is no dependency
/// on tool-event structure, arc detection, or keyword matching.
///
/// # Panics
///
/// Never panics. All slice arithmetic is bounds-checked.
pub fn segment_session(events: &[SessionEvent], config: &SegmentationConfig) -> Vec<Episode> {
    if events.is_empty() {
        return Vec::new();
    }

    // Build the ordered source-index list. Events are in transcript order already,
    // but we track their `index()` values for the Episode event_indices field.
    let indexed: Vec<(usize, usize)> = events
        .iter()
        .enumerate()
        .map(|(pos, ev)| (pos, ev.index()))
        .collect();

    split_into_overlap_windows(&indexed, events, config)
}

/// Packs `indexed` (positional-offset, source-index pairs) into overlapping windows.
///
/// Each window contains events whose total estimated token count fits within
/// `config.token_budget`. Consecutive windows overlap by `config.overlap_events`
/// events. Every positional offset appears in ≥ 1 window.
fn split_into_overlap_windows(
    indexed: &[(usize, usize)],
    events: &[SessionEvent],
    config: &SegmentationConfig,
) -> Vec<Episode> {
    let mut windows: Vec<Episode> = Vec::new();

    // `window_start` is the inclusive starting position in `indexed`.
    let mut window_start: usize = 0;

    while window_start < indexed.len() {
        let mut window_source_indices: Vec<usize> = Vec::new();
        let mut window_tokens: usize = 0;

        for &(pos, source_idx) in &indexed[window_start..] {
            let event_tokens = estimate_tokens(&events[pos]);

            // Always include at least one event per window to guarantee progress
            // even when a single event exceeds the budget.
            if window_source_indices.is_empty() {
                window_source_indices.push(source_idx);
                window_tokens += event_tokens;
            } else if window_tokens + event_tokens <= config.token_budget {
                window_source_indices.push(source_idx);
                window_tokens += event_tokens;
            } else {
                // Budget exhausted: close the window here.
                break;
            }
        }

        let window_len = window_source_indices.len();
        let window_end_pos = window_start + window_len;

        windows.push(Episode {
            event_indices: window_source_indices,
            arc_id: None,
        });

        // Advance start so the next window overlaps this one by `overlap_events`.
        // If the window is overlap-sized or smaller, advance by 1 to guarantee
        // progress and avoid an infinite loop.
        if window_len <= config.overlap_events {
            window_start += 1;
        } else {
            window_start += window_len - config.overlap_events;
        }

        // Stop when the current window already covers all remaining events.
        // No further window can introduce new content.
        if window_end_pos >= indexed.len() {
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

    // ---- Acceptance test 1: flat transcript — non-empty windows, full coverage ----

    /// Proves that a FLAT transcript (only UserMessage/AssistantMessage, no tool
    /// events, no keywords) produces non-empty overlapping windows covering every
    /// event. This is the primary regression case: arc-detection found nothing on
    /// flat transcripts, yielding zero episodes and zero extraction output.
    #[test]
    fn flat_transcript_produces_non_empty_windows_covering_all_events() {
        let events = vec![
            user_msg(0, "How do I use tokio::spawn?"),
            assistant_msg(
                1,
                "You call tokio::spawn with an async block. The task runs concurrently.",
            ),
            user_msg(2, "What about cancellation?"),
            assistant_msg(3, "Use a JoinHandle and call abort() on it to cancel."),
            user_msg(4, "And error propagation?"),
            assistant_msg(
                5,
                "The task returns a JoinError if it panics; unwrap the Result.",
            ),
        ];

        // Use a budget that forces multiple windows on this 6-event session.
        let config = SegmentationConfig::new(20, 2);
        let episodes = segment_session(&events, &config);

        // Non-empty: at least one window must be produced.
        assert!(
            !episodes.is_empty(),
            "flat transcript must produce ≥1 window; got 0"
        );

        // Full coverage: every event appears in at least one window.
        let covered = all_covered_indices(&episodes);
        let expected = expected_indices(&events);
        assert_eq!(
            covered, expected,
            "flat transcript: every event must appear in at least one window (no drops)"
        );
    }

    // ---- Acceptance test 2: budget ≥ session ⇒ one window; small budget ⇒ many ----

    /// Proves that `token_budget ≥ session_token_estimate` produces exactly ONE
    /// window (frontier no-op degrade), and that a small budget produces many
    /// windows over the same session — via the same code path.
    #[test]
    fn budget_at_least_session_size_yields_single_window_same_code_path() {
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

        // Very large budget: all events fit in one window.
        let large_config = SegmentationConfig::new(1_000_000, 3);
        let large_episodes = segment_session(&events, &large_config);
        assert_eq!(
            large_episodes.len(),
            1,
            "budget ≥ session size must yield exactly 1 window; got {}",
            large_episodes.len()
        );
        assert_eq!(
            all_covered_indices(&large_episodes),
            expected_indices(&events),
            "single window must cover all events"
        );

        // Very small budget: must produce many windows.
        let tiny_config = SegmentationConfig::new(5, 1);
        let tiny_episodes = segment_session(&events, &tiny_config);
        assert!(
            tiny_episodes.len() > 1,
            "small budget must produce more than 1 window; got {}",
            tiny_episodes.len()
        );
        assert_eq!(
            all_covered_indices(&tiny_episodes),
            expected_indices(&events),
            "all windows must cover all events"
        );
    }

    // ---- Acceptance test 3: overlap of K events between consecutive windows ----

    /// Proves that consecutive windows overlap by ≥ `overlap_events` events when
    /// windows are large enough to carry the configured overlap (size > overlap_events).
    ///
    /// The budget is set so each window holds well more than overlap_events events,
    /// making the overlap invariant trivially satisfiable.
    #[test]
    fn consecutive_windows_overlap_by_configured_overlap_events() {
        // A session large enough to force multiple windows.
        // Each event: user ≈ 9 tokens, assistant ≈ 9 tokens.
        let events: Vec<SessionEvent> = (0..12)
            .flat_map(|i| {
                vec![
                    user_msg(i * 2, &format!("question {i} about something interesting")),
                    assistant_msg(
                        i * 2 + 1,
                        &format!("answer {i} with substantial content here"),
                    ),
                ]
            })
            .collect();

        // Budget = 55 tokens. Each pair ≈ 18 tokens, so each window holds ~3 pairs
        // (6 events, well above overlap_events=2). This guarantees windows are large
        // enough to carry the full overlap.
        let overlap_events = 2_usize;
        let config = SegmentationConfig::new(55, overlap_events);
        let episodes = segment_session(&events, &config);

        // Must produce multiple windows.
        assert!(
            episodes.len() > 1,
            "session must produce >1 windows with tight budget; got {}",
            episodes.len()
        );

        // Consecutive windows whose sizes both exceed overlap_events must overlap by
        // ≥ overlap_events. (Tail windows smaller than overlap_events are accepted as-is.)
        for pair in episodes.windows(2) {
            let prev_len = pair[0].event_indices.len();
            let curr_len = pair[1].event_indices.len();
            if prev_len > overlap_events && curr_len > overlap_events {
                let prev_set: std::collections::HashSet<usize> =
                    pair[0].event_indices.iter().copied().collect();
                let overlap_count = pair[1]
                    .event_indices
                    .iter()
                    .filter(|idx| prev_set.contains(idx))
                    .count();
                assert!(
                    overlap_count >= overlap_events,
                    "consecutive large windows must overlap by ≥ {overlap_events} events; got {overlap_count}"
                );
            }
        }
    }

    // ---- Acceptance test 4: structured transcript still produces windows covering all events ----

    /// Proves that a transcript with tool arcs (the original session type) still
    /// produces windows covering every event. The windowing algorithm is indifferent
    /// to event type — it works on flat and structured sessions identically.
    #[test]
    fn structured_transcript_with_tool_arcs_produces_full_coverage_windows() {
        let events = vec![
            user_msg(0, "run tests"),
            tool_call(1, "t1", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_err(2, "t1", "Exit code 1\nerror: test failed"),
            tool_call(3, "t2", "Bash", r#"{"command":"cargo test"}"#),
            tool_result_ok(4, "t2", "test passed"),
            user_msg(5, "now do something else"),
            tool_call(6, "t3", "Read", r#"{"file_path":"src/lib.rs"}"#),
            tool_result_ok(7, "t3", "file contents"),
        ];

        let config = SegmentationConfig::new(1_000_000, 3);
        let episodes = segment_session(&events, &config);

        assert_eq!(
            episodes.len(),
            1,
            "large budget must produce 1 window on structured transcript"
        );
        assert_eq!(
            all_covered_indices(&episodes),
            expected_indices(&events),
            "all events must be covered"
        );

        // With tight budget, multiple windows are produced, all covering every event.
        let tight_config = SegmentationConfig::new(8, 2);
        let tight_episodes = segment_session(&events, &tight_config);
        assert!(
            tight_episodes.len() > 1,
            "tight budget must produce >1 windows; got {}",
            tight_episodes.len()
        );
        assert_eq!(
            all_covered_indices(&tight_episodes),
            expected_indices(&events),
            "all events must be covered even with tight budget"
        );
    }

    // ---- Acceptance test 5: tight budget produces more windows than generous budget ----

    /// Proves that changing `token_budget` changes window count deterministically:
    /// a tight budget produces more windows than a generous budget over the same
    /// session, with the same code path.
    #[test]
    fn tight_budget_produces_more_windows_than_generous_budget() {
        let events: Vec<SessionEvent> = (0..12)
            .flat_map(|i| {
                vec![
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

        let tight_config = SegmentationConfig::new(15, 2);
        let tight_episodes = segment_session(&events, &tight_config);

        let generous_config = SegmentationConfig::new(10_000, 2);
        let generous_episodes = segment_session(&events, &generous_config);

        assert!(
            tight_episodes.len() > generous_episodes.len(),
            "tight budget ({} windows) must produce more windows than generous budget ({} windows)",
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

    // ---- Edge cases ----

    /// Empty input produces no windows.
    #[test]
    fn empty_input_produces_no_windows() {
        let episodes = segment_session(&[], &SegmentationConfig::default());
        assert!(episodes.is_empty(), "empty input must yield no windows");
    }

    /// A single-event input always produces exactly one window.
    #[test]
    fn single_event_always_yields_one_window() {
        let events = vec![user_msg(0, "hi")];
        let config = SegmentationConfig::new(1, 1); // even budget=1
        let episodes = segment_session(&events, &config);
        assert_eq!(
            episodes.len(),
            1,
            "single event must yield exactly 1 window"
        );
        assert_eq!(episodes[0].event_indices, vec![0]);
    }

    /// Every window has `arc_id = None` (arc-id is not used in windowing algorithm).
    #[test]
    fn all_windows_have_arc_id_none() {
        let events = vec![
            user_msg(0, "first message"),
            assistant_msg(1, "first reply"),
            user_msg(2, "second message"),
            assistant_msg(3, "second reply"),
        ];
        let config = SegmentationConfig::new(5, 1);
        let episodes = segment_session(&events, &config);
        for ep in &episodes {
            assert!(
                ep.arc_id.is_none(),
                "all windows must have arc_id = None in the windowing algorithm"
            );
        }
    }

    /// A flat transcript with a small budget produces many windows, each non-empty,
    /// with consecutive overlap. This is the core regression invariant.
    ///
    /// Overlap guarantee: consecutive windows whose sizes both exceed `overlap_events`
    /// overlap by ≥ `overlap_events`. Windows that are too short to carry the full
    /// overlap (size ≤ overlap_events) are only produced as tail windows and provide
    /// as much overlap as their size allows.
    #[test]
    fn flat_transcript_small_budget_many_non_empty_windows_with_overlap() {
        let events: Vec<SessionEvent> = (0..10)
            .flat_map(|i| {
                vec![
                    user_msg(
                        i * 2,
                        &format!("user turn {i} with some content to fill budget"),
                    ),
                    assistant_msg(
                        i * 2 + 1,
                        &format!("assistant reply {i} substantial content"),
                    ),
                ]
            })
            .collect();

        // Use a budget that produces windows of ≥3 events (well above overlap_events=2)
        // so the overlap invariant is always satisfiable.
        let overlap_events = 2;
        let config = SegmentationConfig::new(50, overlap_events);
        let episodes = segment_session(&events, &config);

        // Many windows.
        assert!(
            episodes.len() > 1,
            "flat transcript with tight budget must produce >1 windows; got {}",
            episodes.len()
        );

        // Every window is non-empty.
        for (i, ep) in episodes.iter().enumerate() {
            assert!(!ep.event_indices.is_empty(), "window {i} must be non-empty");
        }

        // Full coverage.
        let covered = all_covered_indices(&episodes);
        let expected = expected_indices(&events);
        assert_eq!(covered, expected, "all events must be covered (no drops)");

        // Consecutive overlap ≥ overlap_events for pairs where both windows are large
        // enough to carry the configured overlap. Tail windows that are smaller than
        // overlap_events are accepted as-is.
        for pair in episodes.windows(2) {
            let prev_len = pair[0].event_indices.len();
            let curr_len = pair[1].event_indices.len();
            // Only assert overlap when both windows are large enough to sustain it.
            if prev_len > overlap_events && curr_len > overlap_events {
                let prev_set: std::collections::HashSet<usize> =
                    pair[0].event_indices.iter().copied().collect();
                let overlap_count = pair[1]
                    .event_indices
                    .iter()
                    .filter(|idx| prev_set.contains(idx))
                    .count();
                assert!(
                    overlap_count >= overlap_events,
                    "consecutive large windows must overlap by ≥ {overlap_events} events; got {overlap_count}"
                );
            }
        }
    }
}
