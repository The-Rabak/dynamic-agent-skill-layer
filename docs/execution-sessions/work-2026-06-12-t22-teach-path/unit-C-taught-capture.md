---
unit: "Unit C — Taught-knowledge candidate class + refusal≠malformed"
unit_number: 3
unit_kind: infra-packet
serves: "Make the real extractor capture taught knowledge verbatim; stop retrying reasoned refusals. The core."
status: completed
attempt_count: 1
domains: [extraction, prompt-contract, orchestrator, providers]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md
session_id: work-2026-06-12-t22-teach-path
---

## What Was Implemented
**Change 1 — taught-knowledge candidate class (prompt).** `prompt_contract.rs`: env-gated
`EXTRACT_TEACH_CAPTURE` (default ON; owner-approved 2026-06-12) injects a TAUGHT KNOWLEDGE section into
BOTH prompts (text/JSON + system) — capture idiosyncratic names/codes/constants/procedures VERBATIM;
recurrence NOT required; "explicitly stated in the document" is a capture TRIGGER not a rejection; plus
an abstraction EXCEPTION so taught literals are not {placeholder}-ed away. Additive: explicitly does not
lower the bar for organic/throwaway sessions. `=off` reproduces the pre-T22 prompt byte-for-byte.

**Change 2 — refusal≠malformed retry fix.** `ExtractionResult.assessment: Option<String>` added (domain)
and threaded from all three providers (ollama/claude/claude-code parse + return it). Orchestrator
`classify_prose_attempt` now: empty + reasoned assessment ⇒ accept WITHOUT retry (deliberate refusal);
empty + NO assessment on a substantive window ⇒ retry (cold-start `{}`); malformed JSON ⇒ retry. Helper
`has_reasoned_assessment`.

## Files Changed
- crates/domain/src/{types.rs (assessment field), lib.rs}
- crates/infrastructure/src/extraction/{prompt_contract.rs, claude.rs, claude_code.rs, ollama.rs}
- crates/session-extractor/src/{orchestrator.rs (classify + helper + new test + fakes), lib.rs, worker_pool.rs, writer.rs}
- tests/integration/{test_pending_lifecycle,test_pending_lifecycle_frontmatter_contract,test_skill_md_roundtrip,test_extract_session}.rs (assessment: None)
- tests/e2e/{test_concurrency_stress,test_live_data_plane_roundtrip}.rs (assessment: None)
- scripts/dogfood_regression.py (regression runner) + tests/e2e/reports/efficacy/dogfood-regression/* (artifacts)

## Fences honored
- No benchmark special-casing: ships as production behavior; the hard dogfood-regression gate is clean.
- No fakes/stubs; assessment threaded honestly (None where a provider/aggregate has no single assessment).

## Tests
- prompt_contract: taught_knowledge_section_present_by_default_in_both_prompts; teach_capture_off_reproduces_pre_t22_prompt; teach_capture_flag_parses_truthy_and_falsey_values — green.
- claude_code: parse_cli_output_surfaces_reasoned_assessment — green.
- orchestrator: prose_extractor_does_not_retry_reasoned_refusal_from_substantive_window (NEW) + the
  preserved retry-on-empty/parse-error tests — green.
- Full suites: domain 13, infrastructure 219(+new), session-extractor 184(+new) — all pass.
- Workspace gates: cargo fmt clean; clippy bare 0; clippy --features test-utils 0.

## Dogfood regression (HARD GATE) — PASS
3 organic sessions, OFF vs ON, real worker. Draft count delta +1 total; quality equivalent (same
skills, reworded names = run-to-run variance). No degradation. ANALYSIS.md + regression.json persisted.

## DP-1 (owner decision): default-ON — APPROVED 2026-06-12.

## Test Results
- All unit/lib suites green; dogfood regression clean; gates green. Attempts: 1.
