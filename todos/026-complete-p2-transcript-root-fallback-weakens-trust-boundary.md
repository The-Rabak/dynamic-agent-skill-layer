---
status: complete
priority: p2
issue_id: "026"
tags: [code-review, security, contracts, session-extractor]
dependencies: []
---

# Remove implicit transcript root fallback

## Problem Statement

When `CLAUDE_TRANSCRIPT_ROOT` is unset, transcript loading silently falls back to current working directory, weakening the explicit transcript ingress trust boundary.

## Findings

- `crates/session-extractor/src/transcripts.rs:17-27` falls back to `current_dir()` if env var is missing.
- Ticket/architecture contract states transcript ingress should be rooted under mounted transcript root (`transcript_ref` trust boundary).

## Proposed Solutions

### Option 1: Require explicit `CLAUDE_TRANSCRIPT_ROOT`

**Approach:** Fail initialization when root is missing or invalid.

**Pros:**
- Strict trust boundary.
- Easier operational debugging of misconfiguration.

**Cons:**
- Requires explicit env setup in tests/dev scripts.

**Effort:** 1-2 hours

**Risk:** Low

---

### Option 2: Keep fallback but gate behind explicit dev-only flag

**Approach:** Allow fallback only when `EXTRACT_SESSION_ALLOW_CWD_FALLBACK=true`.

**Pros:**
- Preserves local ergonomics.

**Cons:**
- Retains optional insecure path if misused.

**Effort:** 1-2 hours

**Risk:** Medium

## Recommended Action

Implemented Option 1: require explicit `CLAUDE_TRANSCRIPT_ROOT` and fail fast when absent.

## Technical Details

**Affected files:**
- `crates/session-extractor/src/transcripts.rs`

**Database changes (if any):**
- No

## Resources

- `docs/tickets/2026-05-21-skill-layer-v1-1/06-session-end-extraction-and-approval.md`
- `crates/session-extractor/src/transcripts.rs`

## Acceptance Criteria

- [x] Missing transcript root is surfaced as explicit configuration error
- [x] No implicit cwd trust-boundary fallback in production mode
- [x] Focused tests cover missing-root behavior

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Audited transcript loader initialization path and contract expectations.

**Learnings:**
- Silent fallbacks hide broken trust-boundary configuration.

### 2026-05-25 - Unit execution

**By:** Copilot CLI

**Actions:**
- Removed `current_dir()` fallback from `TranscriptLoader::from_environment`.
- Added focused unit test proving missing `CLAUDE_TRANSCRIPT_ROOT` fails with `InvalidRoot`.
- Ran focused `session-extractor` test for missing-root behavior.

**Learnings:**
- Trust-boundary env configuration should fail closed and explicit during initialization.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
