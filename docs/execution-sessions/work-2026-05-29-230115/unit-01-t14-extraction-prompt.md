---
unit: "T14 - Extraction prompt review and unification"
unit_number: 1
unit_kind: hardening
serves: "SC-3 — extraction outputs stay contract-stable and provider-parity-safe"
status: completed
attempt_count: 1
domains: [backend, extraction, prompt-engineering]
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/14-extraction-prompt-review-and-unification.md
session_id: work-2026-05-29-230115
---

## What Was Implemented

**Decision: Path A — Claude endpoint owns prompting, Ollama owns local prompt.** Provider asymmetry is architecturally intentional. Claude endpoint (`http://127.0.0.1:8080/extract`) is an external extraction service expected to apply Claude `strict: true` tool-calling. Ollama `/api/generate` is a raw model interface requiring a local prompt. Both providers conform to a shared semantic contract.

### Prompt Strategy
- Claude: Pure transport adapter — sends `{model, session_id, transcript}` to external endpoint. Endpoint owns prompt engineering (can evolve independently)
- Ollama: Enhanced local prompt ~80 lines covering 8 extraction targets, 5 quality dimensions (FME .30, actionable .25, correctness .20, conciseness .15, blacklist .10), 5 anti-patterns, output schema with example, confidence guidance
- Shared contract: `ExtractionPromptContract` type defines canonical extraction expectations, quality criteria, candidate validation

### Why not unified local prompt
1. Claude endpoint is standalone service, not API passthrough — injecting prompt would conflict
2. Research doc recommends Claude `strict: true` tool-calling for schema guarantees — best owned by endpoint
3. Endpoint can evolve prompting independently without client coordination
4. Ollama lacks `tool_choice` support — must embed schema/guidance in prompt text

## Files Changed
- `crates/infrastructure/src/extraction/mod.rs` — NEW module file with full decision rationale, prompt inventory, Path A justification
- `crates/infrastructure/src/extraction/prompt_contract.rs` — NEW canonical extraction contract with `ExtractionPromptContract`, `QualityDimension`, `ExtractionQualityCriteria`, `build_ollama_extraction_prompt()`, candidate validation, 6 unit tests
- `crates/infrastructure/src/extraction/ollama.rs` — enhanced prompt from 1-line to ~80-line production-quality prompt with quality rubric, schema, examples
- `crates/infrastructure/src/extraction/claude.rs` — doc comments explaining transport adapter design and why no local prompt is correct
- `crates/infrastructure/src/lib.rs` — `extraction` module changed from inline to file-based for `mod.rs` support
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md` — Seams section updated with T14 prompt strategy decision and rationale

## Problems Encountered
None. All changes correct on first attempt.

## Patterns Discovered
- Extraction provider asymmetry is a feature, not a bug — different LLM interfaces require different prompting strategies
- Semantic contract (what to extract) stays shared; syntactic contract (how to prompt) may differ per provider
- Ollama `format: "json"` requires embedding schema in prompt text — no `tool_choice` or `strict` support

## Test Results
- Command: `cargo test --workspace`
- Result: PASS (67 passed, 0 failed, 2 skipped — E2E requires PG/Qdrant containers)
- infrastructure: 41 passed (incl 6 new prompt_contract tests)
- session-extractor: 17 passed
- domain: 4 passed
- extract_session: 5 passed
- Attempts: 1