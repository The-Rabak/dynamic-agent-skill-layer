//! T22 Unit A — Forensic visibility map for the clband smoke extraction failure.
//!
//! This is a DIAGNOSTIC harness (an `examples/` binary), NOT production logic. It reuses the
//! REAL production functions end to end — `parse_session_events` (real parser),
//! `mine_preamble` (real preamble miner), `segment_session` (real episodic segmenter),
//! `events_to_transcript` + `render_sanitized_transcript_lines` (the exact flat-transcript the
//! prose extractor receives) — so what it reports is byte-for-byte what the orchestrated prose
//! extractor actually sees for each window.
//!
//! It answers the T22 Unit A questions for a captured smoke transcript:
//!   1. What does the suspicious-speaker filter drop on each window? (count + content class)
//!   2. Does the knowledge document (SOP / spec) text reach ANY prose-extraction window?
//!   3. Per sentinel, is it visible to the prose extractor, or only present in the wider
//!      session haystack (tool results, tool calls) that the flat transcript discards?
//!
//! The mechanism under test: `SessionEvent::as_transcript_entry()` returns `None` for
//! ToolResult/ToolCall/FileEdit (domain/types.rs), and the orchestrator prepends the mined
//! preamble as `speaker:"system"` (orchestrator.rs) which the suspicious-speaker filter then
//! drops. Both losses are reproduced here against the real captured transcripts.
//!
//! Usage:
//!   cargo run -p session-extractor --example clband_visibility_map -- \
//!     <flywheel|aether> <transcript.jsonl> <out.json>

use std::collections::BTreeMap;
use std::env;

use domain::{DomainId, SessionEvent, TranscriptEntry, events_to_transcript};
use infrastructure::render_sanitized_transcript_lines;
use serde_json::json;
use session_extractor::preamble::mine_preamble;
use session_extractor::segmentation::{SegmentationConfig, segment_session};
use session_extractor::transcripts::parse_session_events;

/// Two-tier sentinels per smoke context.
/// `document` = the system-name tier the current manifest gates on (NOT emitted by the
/// preference channel). `operative` = the constants/rules Session B actually needs, derived
/// VERBATIM from the committed deterministic verifiers (tests/e2e/efficacy/clband/verifiers/).
fn sentinels(context: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    match context {
        "flywheel" => (
            // document tier (current manifest)
            vec![
                "Flywheel Manufacturing Multi-Agent System",
                "Scatterbrained Improviser",
                "spin test",
                "WORKAROUND",
            ],
            // operative tier (from flywheel-assembly.sh verifier)
            vec![
                "next size up",
                "extra torque",
                "firm shake",
                "retest",
                "spin test",
                "Validation Engineer",
                "Agent C",
                "Forklift",
                "Agent D",
            ],
        ),
        "aether" => (
            vec!["conduit", "swirl", "Turbulence Alert", "Fracture"],
            // operative tier (from aether-turbulence-review.sh + aether-python-translate.sh)
            vec![
                "Turbulence Alert",
                "Cause",
                "Corrected Code",
                "outer",
                "conduit",
                "swirl",
                "fork",
                "flow",
            ],
        ),
        other => panic!("unknown context '{other}' (expected flywheel|aether)"),
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: clband_visibility_map -- <flywheel|aether> <transcript.jsonl> <out.json>"
        );
        std::process::exit(2);
    }
    let context = args[1].as_str();
    let transcript_path = &args[2];
    let out_path = &args[3];
    let (doc_sentinels, op_sentinels) = sentinels(context);

    let payload = std::fs::read_to_string(transcript_path)
        .unwrap_or_else(|e| panic!("read {transcript_path}: {e}"));

    // ── REAL parse ───────────────────────────────────────────────────────────
    let parsed = parse_session_events(&payload);
    let events = parsed.events;
    let session_id = DomainId::new_unchecked("clband-visibility");

    // Event-type distribution.
    let mut type_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for ev in &events {
        let t = match ev {
            SessionEvent::UserMessage { .. } => "UserMessage",
            SessionEvent::AssistantMessage { .. } => "AssistantMessage",
            SessionEvent::ToolCall { .. } => "ToolCall",
            SessionEvent::ToolResult { .. } => "ToolResult",
            SessionEvent::FileEdit { .. } => "FileEdit",
            SessionEvent::Metadata { .. } => "Metadata",
        };
        *type_counts.entry(t).or_default() += 1;
    }

    // ── Session haystacks ────────────────────────────────────────────────────
    // Full haystack: every event's grounding text (INCLUDES ToolResult.output, ToolCall input).
    let full_haystack: String = events
        .iter()
        .filter_map(SessionEvent::grounding_text)
        .collect::<Vec<_>>()
        .join("\n");

    // Prose-visible haystack: ONLY what events_to_transcript keeps (UserMessage + AssistantMessage),
    // i.e. what the flat transcript the prose extractor receives can ever contain.
    let prose_only_transcript = events_to_transcript(session_id.clone(), &events);
    let prose_only_haystack = render_sanitized_transcript_lines(&prose_only_transcript);

    // ── REAL preamble (mined once over the full event stream, as the orchestrator does) ──
    let preamble = mine_preamble(&events, None)
        .await
        .expect("preamble mining must not fail with no normalizer");

    // ── REAL segmentation (frontier tier budget = 40960, matches the smoke routing log) ──
    let config = SegmentationConfig::new(40_960, 3);
    let windows = segment_session(&events, &config);
    let event_by_index: BTreeMap<usize, &SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    // Per-window: reproduce EXACTLY what extract_prose_window builds and sanitizes.
    let mut window_reports = Vec::new();
    let mut prose_window_union = String::new();
    let mut total_dropped_system_entries = 0usize;
    for (idx, window) in windows.iter().enumerate() {
        let window_events: Vec<SessionEvent> = window
            .event_indices
            .iter()
            .filter_map(|i| event_by_index.get(i).copied().cloned())
            .collect();

        let mut transcript = events_to_transcript(session_id.clone(), &window_events);

        // Prepend the synthetic preamble EXACTLY like extract_prose_window (orchestrator.rs).
        if !preamble.text.is_empty() {
            transcript.entries.insert(
                0,
                TranscriptEntry {
                    speaker: "system".to_owned(),
                    content: format!("[Session context]\n{}", preamble.text),
                },
            );
        }

        let entries_before = transcript.entries.len();
        let rendered = render_sanitized_transcript_lines(&transcript);
        // A dropped entry == an entry whose `speaker: ` line is absent from the render.
        let rendered_entry_lines = transcript
            .entries
            .iter()
            .filter(|e| rendered.contains(&format!("{}: ", e.speaker)))
            .count();
        let dropped = entries_before.saturating_sub(rendered_entry_lines);
        // Did the preamble (speaker "system") survive?
        let preamble_present = rendered.contains("[Session context]");
        if !preamble.text.is_empty() && !preamble_present {
            total_dropped_system_entries += 1;
        }

        // Window haystack (full grounding text of the window's events).
        let window_full: String = window_events
            .iter()
            .filter_map(SessionEvent::grounding_text)
            .collect::<Vec<_>>()
            .join("\n");

        prose_window_union.push('\n');
        prose_window_union.push_str(&rendered);

        window_reports.push(json!({
            "window": idx,
            "event_indices": window.event_indices.len(),
            "entries_before_sanitize": entries_before,
            "entries_after_sanitize": rendered_entry_lines,
            "dropped_entries": dropped,
            "preamble_prepended": !preamble.text.is_empty(),
            "preamble_survived_sanitizer": preamble_present,
            "window_full_haystack_chars": window_full.len(),
            "window_prose_visible_chars": rendered.len(),
        }));
    }

    // ── Sentinel visibility (the headline) ───────────────────────────────────
    let sentinel_report = |tier: &str, list: &[&str]| -> Vec<serde_json::Value> {
        list.iter()
            .map(|s| {
                // Per-event-type carrier counts (where does this sentinel physically live?).
                let mut carriers: BTreeMap<&str, usize> = BTreeMap::new();
                for ev in &events {
                    if let Some(txt) = ev.grounding_text()
                        && contains_ci(&txt, s)
                    {
                        let t = match ev {
                            SessionEvent::UserMessage { .. } => "UserMessage",
                            SessionEvent::AssistantMessage { .. } => "AssistantMessage",
                            SessionEvent::ToolCall { .. } => "ToolCall",
                            SessionEvent::ToolResult { .. } => "ToolResult",
                            SessionEvent::FileEdit { .. } => "FileEdit",
                            SessionEvent::Metadata { .. } => "Metadata",
                        };
                        *carriers.entry(t).or_default() += 1;
                    }
                }
                let in_full = contains_ci(&full_haystack, s);
                let in_prose = contains_ci(&prose_window_union, s);
                json!({
                    "tier": tier,
                    "sentinel": s,
                    "in_full_session_haystack": in_full,
                    "visible_to_prose_extractor": in_prose,
                    "invisible_lost_in_flat_transcript": in_full && !in_prose,
                    "carrier_event_types": carriers,
                })
            })
            .collect()
    };

    let mut sentinels_json = sentinel_report("document", &doc_sentinels);
    sentinels_json.extend(sentinel_report("operative", &op_sentinels));

    let doc_visible = doc_sentinels
        .iter()
        .filter(|s| contains_ci(&prose_window_union, s))
        .count();
    let op_visible = op_sentinels
        .iter()
        .filter(|s| contains_ci(&prose_window_union, s))
        .count();

    let report = json!({
        "context": context,
        "transcript": transcript_path,
        "unit": "T22-A-forensics",
        "events_total": events.len(),
        "malformed_lines": parsed.malformed_count,
        "event_type_distribution": type_counts,
        "windows": windows.len(),
        "segmentation_token_budget": 40_960,
        "preamble": {
            "text_chars": preamble.text.len(),
            "approx_tokens": preamble.approximate_tokens,
            "preferences_detected": preamble.preferences.len(),
            "carries_any_document_sentinel": doc_sentinels.iter().any(|s| contains_ci(&preamble.text, s)),
            "carries_any_operative_sentinel": op_sentinels.iter().any(|s| contains_ci(&preamble.text, s)),
            "text": preamble.text,
        },
        "preamble_drop": {
            "windows_with_preamble_dropped_by_suspicious_speaker_filter": total_dropped_system_entries,
            "windows_total": windows.len(),
        },
        "haystack_chars": {
            "full_session_grounding_text": full_haystack.len(),
            "prose_visible_session_text": prose_only_haystack.len(),
        },
        "sentinel_visibility": sentinels_json,
        "summary": {
            "document_tier_visible_to_prose": format!("{}/{}", doc_visible, doc_sentinels.len()),
            "operative_tier_visible_to_prose": format!("{}/{}", op_visible, op_sentinels.len()),
        },
        "per_window": window_reports,
    });

    std::fs::write(out_path, serde_json::to_string_pretty(&report).unwrap())
        .unwrap_or_else(|e| panic!("write {out_path}: {e}"));

    // Human summary.
    println!("== clband visibility map: {context} ==");
    println!("transcript: {transcript_path}");
    println!(
        "events: {}  windows: {}  event_types: {:?}",
        events.len(),
        windows.len(),
        type_counts
    );
    println!(
        "preamble: {} chars, {} prefs; carries_document_sentinel={} carries_operative_sentinel={}",
        preamble.text.len(),
        preamble.preferences.len(),
        doc_sentinels.iter().any(|s| contains_ci(&preamble.text, s)),
        op_sentinels.iter().any(|s| contains_ci(&preamble.text, s)),
    );
    println!(
        "preamble dropped by suspicious-speaker filter on {}/{} windows",
        total_dropped_system_entries,
        windows.len()
    );
    println!(
        "full-session haystack: {} chars ; prose-visible: {} chars",
        full_haystack.len(),
        prose_only_haystack.len()
    );
    println!("-- sentinel visibility (prose extractor) --");
    for s in sentinels_json.iter() {
        println!(
            "  [{}] {:<42} full={} prose={} {}",
            s["tier"].as_str().unwrap(),
            s["sentinel"].as_str().unwrap(),
            s["in_full_session_haystack"],
            s["visible_to_prose_extractor"],
            if s["invisible_lost_in_flat_transcript"].as_bool().unwrap() {
                "<< INVISIBLE (lost in flat transcript)"
            } else {
                ""
            },
        );
    }
    println!(
        "summary: document-tier {}/{} visible ; operative-tier {}/{} visible -> {out_path}",
        doc_visible,
        doc_sentinels.len(),
        op_visible,
        op_sentinels.len()
    );
}
