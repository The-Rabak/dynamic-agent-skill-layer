---
status: complete
priority: p2
issue_id: "027"
tags: [code-review, graph-builder, contract-drift, extraction]
dependencies: []
---

# Implement real Ollama extraction fallback

## Problem Statement

The code path named `extract_with_ollama_fallback` does not call an Ollama/provider adapter and instead returns a deterministic summary line, drifting from the ticket contract.

## Findings

- `crates/graph-builder/src/extraction/ollama_fallback.rs:5-18` generates local summary text only.
- `crates/graph-builder/src/extraction/mod.rs:41-44` marks this path as fallback behavior when structural extraction is thin.
- T05 acceptance criteria calls for fallback to Ollama JSON when deterministic extraction is insufficient.

## Proposed Solutions

### Option 1: Wire fallback through extraction provider seam

**Approach:** Use infrastructure-backed provider adapter and parse canonical JSON candidate/subunit output.

**Pros:**
- Aligns implementation with ticket/architecture contract.
- Improves extraction quality.

**Cons:**
- Adds provider-call failure handling in graph-builder path.

**Effort:** 4-6 hours

**Risk:** Medium

---

### Option 2: Rename current behavior and defer true Ollama fallback to explicit follow-up

**Approach:** Rename to deterministic fallback and update ticket/session evidence to reflect partial delivery.

**Pros:**
- Honest contract documentation.
- Lower immediate implementation cost.

**Cons:**
- Leaves intended behavior unmet.

**Effort:** 1-2 hours

**Risk:** Medium

## Recommended Action

Implemented Option 1: wire fallback through the extraction provider seam and preserve
explicit unavailable-provider semantics when Ollama extraction cannot be used.

## Technical Details

**Affected files:**
- `crates/graph-builder/src/extraction/ollama_fallback.rs`
- `crates/graph-builder/src/extraction/mod.rs`
- `tests/integration/test_watcher_rebuild.rs` (or dedicated extraction tests)

**Database changes (if any):**
- No

## Resources

- `docs/tickets/2026-05-21-skill-layer-v1-1/05-watcher-driven-graph-rebuild.md`
- `crates/graph-builder/src/extraction/ollama_fallback.rs`

## Acceptance Criteria

- [x] Fallback path uses provider-backed extraction rather than static line summary
- [x] Fallback output shape is deterministic and validated in tests
- [x] Failure modes are surfaced without breaking rebuild ordering contracts

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Audited fallback implementation and compared to ticket acceptance contract.

**Learnings:**
- Current naming overpromises behavior and can mislead maintenance/review.

### 2026-05-25 - Unit execution

**By:** Copilot CLI

**Actions:**
- Replaced the static fallback summary implementation with a provider-backed Ollama extraction path in `crates/graph-builder/src/extraction/ollama_fallback.rs`.
- Added explicit unavailable-provider fallback semantics (summary + evidence subunit) so rebuild paths stay safe without silent panics.
- Added focused fallback-path tests in `crates/graph-builder/src/extraction/ollama_fallback.rs` and extraction orchestration tests in `crates/graph-builder/src/extraction/mod.rs`.
- Ran focused validation for the graph-builder extraction module.

**Learnings:**
- Explicit degraded-mode subunits preserve deterministic graph shape while keeping provider failures visible.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
