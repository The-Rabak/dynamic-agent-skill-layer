---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "T22 (NEW 2026-06-12 from clband smoke INSTRUMENT-FAILURE(extraction) + 3-component re-diagnosis)"
brainstorm_ref: ""
started: 2026-06-12
status: in_progress
execution_shape: infra-track
current_unit: 1
total_units: 4
session_id: work-2026-06-12-t22-teach-path
---

## WHY Linkage
- Canonical WHY source: docs/assessments/2026-06-12-t14-clband-smoke.md (+ 2026-06-12 addendum) + T22 ticket
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: make the REAL extraction pipeline able to capture TAUGHT knowledge (one-shot,
  document-grounded, idiosyncratic rules) verbatim, then re-run the clband smoke as the GO gate for the
  T14 8-context acquisition band. The whole efficacy chapter is blocked behind this.
- Success-criteria focus: T22 acceptance criteria (visibility map; doc delivery; taught-knowledge class +
  refusal≠malformed; dogfood regression clean; two-tier sentinels; smoke re-run green; GO/NO-GO recorded).

### TDD Contract
- Effective mode: Ralph-driven TDD (ticket tdd_mode: ralph) for the Rust Unit C change (unit tests first).
- Required evidence: unit tests (cargo test -p infrastructure / -p session-extractor) + live e2e (dogfood
  regression diff; smoke re-run fidelity_gate.sh). Measurement drives the REAL stack over HTTP.
- Exceptions: Unit A is forensics (no fixes); Unit B is harness Python (pytest/live re-extract evidence).

### Constitution Context
- No docs/constitution.md in repo; governing rules = machine-wide CLAUDE.md (no stubs/fakes — fail loud)
  + project standing rules (measurement drives real app; heavy actions serialized by orchestrator;
  subagents on sonnet, forbidden from cargo build/clippy/test + model-call storms; never delete this
  session's outputs; workspace gates green).

### Architecture Handoff (plan-derived)
- Feature homes: crates/infrastructure/src/extraction (prompt contract + sanitizer) ; crates/session-extractor
  (orchestrator + preamble + segmentation) ; tests/e2e/efficacy/clband (harness: delivery, gate, manifest).
- Injection defense (suspicious-speaker filter) is a trust boundary — MUST NOT be weakened (Unit B fence).
- Seams: prose extractor sees ONLY the flat transcript (events_to_transcript → render_sanitized_transcript_lines).
- Deletion test: taught-knowledge capture ships as production behavior (no benchmark special-casing); the
  hard dogfood-regression gate guards the 262 corpus.

## Forensic model (established by reading, pre-Unit-A; Unit A persists the measured artifact)
- LOSS 1 (document invisibility): SessionEvent::as_transcript_entry() returns None for ToolResult/ToolCall/
  FileEdit (domain/types.rs:420). The flywheel doc is delivered as a workspace file read via `Read` → its
  verbatim content lands in a ToolResult and NEVER reaches the prose extractor's flat transcript. The prose
  pass sees only user+assistant message text.
- LOSS 2 (preamble eaten): orchestrator.rs:703 prepends the mined preamble as speaker="system"; the
  suspicious-speaker filter (prompt_contract.rs:252 SUSPICIOUS_SPEAKERS) drops every entry whose speaker
  contains "system" → the preamble is dropped on EVERY window. That is the literal worker-log line.
- Worldview (Unit C): the prose prompt demands DURABLE/REUSABLE/"FUTURE, DIFFERENT task" knowledge and an
  anti-pattern explicitly penalizes copying literals verbatim → taught one-shot rules are rejected/abstracted.
- Retry burn (Unit C): classify_prose_attempt treats a substantive-window zero-candidate (a reasoned refusal)
  as EmptyOrMalformed → retried 3× identically (orchestrator.rs:642).
- Operative sentinels (Unit D) derived from the committed verifiers:
  - flywheel: "next size up", "extra torque", "firm shake", "retest", "spin test", "Validation Engineer"/
    "Agent C", "Forklift"/"Agent D".
  - aether: "Turbulence Alert", "Cause/Fix/Corrected Code", "<<" assignment, "outer" kept; translate
    conduit→def/flow→return/swirl→for/fork→if/Len→len.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| A | Forensics — visibility map | infra-packet | Apportion blame B vs C; evidence base | completed (671b412) | 1 | unit-A-forensics.md |
| B | Harness document delivery | infra-packet | Doc verifiably reaches extraction windows | completed (360f7cd) | 1 | unit-B-delivery.md |
| C | Taught-knowledge candidate class + refusal≠malformed | infra-packet | Verbatim capture; the core | code+tests green; dogfood regression running; DP-1 next | 1 | unit-C-taught-capture.md |
| D | Two-tier sentinels + full smoke re-run | infra-packet | GO gate; GO/NO-GO recommendation | manifest+gate ready; smoke re-run pending DP-1 | -- | -- |

## Unit C implementation status (pre-regression)
- Change 1 (taught-knowledge prompt): `EXTRACT_TEACH_CAPTURE` (default ON) injects a TAUGHT KNOWLEDGE
  section into both prompts (text/JSON + system) + an abstraction exception for taught literals.
  `=off` reproduces the pre-T22 prompt byte-for-byte. prompt_contract.rs.
- Change 2 (refusal≠malformed): `ExtractionResult.assessment` threaded (ollama/claude/claude-code);
  orchestrator `classify_prose_attempt` accepts a reasoned refusal (empty + assessment) WITHOUT retry;
  cold-start empty (no assessment) + malformed JSON still retry. New test
  `prose_extractor_does_not_retry_reasoned_refusal_from_substantive_window` green.
- Unit tests: infrastructure 219 + 4 new ; session-extractor 184 + 1 new ; domain 13 — all green.
- Unit D ready: manifest two-tier sentinels (operative derived from verifiers) + fidelity_gate.sh
  gates on operative / reports document. Gate mechanics validated: OLD flywheel drafts FAIL 7/7
  operative (the exact T22 failure). run_smoke_rerun.sh written (replay-based).

## Owner decision points (STOP and ask)
1. Unit C default: EXTRACT_TEACH_CAPTURE default-ON vs env-gated (after regression diff is in).
2. Human gate on re-run .pending drafts (present with operative-sentinel coverage).
3. GO/NO-GO for the 8-context band (recommend with evidence; owner decides).
4. If Unit A refutes document-visibility AND Unit C alone still fails fidelity — stop & present options.

## Learnings Brief
_Unit A in progress._
