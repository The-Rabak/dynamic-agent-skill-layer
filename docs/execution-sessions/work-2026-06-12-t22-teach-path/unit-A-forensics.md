---
unit: "Unit A — Forensics (visibility map)"
unit_number: 1
unit_kind: infra-packet
serves: "Apportion the smoke extraction failure between Unit B (delivery) and Unit C (worldview); evidence base for both"
status: completed
attempt_count: 1
domains: [extraction, session-extractor, forensics]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md
session_id: work-2026-06-12-t22-teach-path
---

## What Was Implemented
A diagnostic `examples/` binary (`crates/session-extractor/examples/clband_visibility_map.rs`) that
replays the REAL extraction-input construction (`parse_session_events` → `mine_preamble` →
`segment_session` → `events_to_transcript` → `render_sanitized_transcript_lines`) over the two
captured smoke transcripts and persists a per-sentinel visibility map. No production code changed.

## Files Changed
- `crates/session-extractor/examples/clband_visibility_map.rs` — created (diagnostic, examples/ only)
- `tests/e2e/reports/efficacy/clband-smoke/visibility/{flywheel,aether}-visibility.json` — raw maps
- `tests/e2e/reports/efficacy/clband-smoke/visibility/VERDICT.md` — verdict

## Findings (evidence-backed; see VERDICT.md)
- **LOSS 1 (document invisibility):** `as_transcript_entry()` drops ToolResult/ToolCall/FileEdit, so
  the prose extractor sees only user+assistant message text. flywheel: only 1-2 sentinels lost (agent
  narrated rules → 8/9 operative visible). aether: severe (prose-visible 1338/38826 chars; spec read
  + answer write both invisible; 4/8 operative visible).
- **LOSS 2 (preamble eaten):** the synthetic `speaker:"system"` preamble is dropped by the
  suspicious-speaker filter on 1/1 windows of both contexts — CONFIRMED as the literal log line — but
  the preamble carries ZERO sentinels, so it is NOT the cause of fidelity failure. Real bug, off the
  critical path → cleanup note.
- **WORLDVIEW (dominant cause):** flywheel's prose extractor SAW 8/9 operative rules and still refused
  3× → the failure is the extractor's value system, not visibility. This is the core Unit C gap.

## Apportionment
- Unit B (delivery): load-bearing for **aether** (spec/answer lost in tool/file events).
- Unit C (worldview + retry): load-bearing for **both**; the SOLE remaining blocker for flywheel.
- Does NOT trigger decision-point #4 (not "refutes visibility AND C-alone fails"): B and C each own
  clear, independent, product-justified work.

## Test Results
- Command: `cargo build -p session-extractor --example clband_visibility_map` → PASS (10.45s)
- Ran on both transcripts; artifacts persisted. Workspace gates not yet re-run (no prod code changed).
- Attempts: 1
