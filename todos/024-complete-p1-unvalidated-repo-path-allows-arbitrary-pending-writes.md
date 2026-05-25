---
status: complete
priority: p1
issue_id: "024"
tags: [code-review, security, session-extractor, filesystem]
dependencies: []
---

# Validate repo_path before writing pending drafts

## Problem Statement

`extract_session` accepts `repo_path` and writes `.skills/*.pending` under that path without trust-boundary validation, enabling arbitrary writable-path targeting.

## Findings

- `crates/session-extractor/src/writer.rs:71-79` directly returns `PathBuf::from(repo_path)` when request includes repo path.
- `crates/session-extractor/src/lib.rs:205-209` writes pending drafts using resolved path.
- No canonicalization + allowlist check is applied before filesystem mutation.

## Proposed Solutions

### Option 1: Enforce allowlisted canonical roots (recommended)

**Approach:** Canonicalize `repo_path`, require it to be under allowed roots (`SKILL_GLOBAL_ALLOWED_ROOTS` and/or validated project root), reject otherwise.

**Pros:**
- Prevents arbitrary file writes.
- Aligns with trust-boundary and constitution guardrails.

**Cons:**
- Requires explicit env/setup contract in local dev.

**Effort:** 2-4 hours

**Risk:** Medium

---

### Option 2: Ignore request repo_path and always use resolved project/global scope resolvers

**Approach:** Remove direct path acceptance from request and route through trusted resolver outputs only.

**Pros:**
- Strongest boundary control.
- Cleaner API contract.

**Cons:**
- Less flexible for tests/harness migrations.

**Effort:** 3-5 hours

**Risk:** Medium

## Recommended Action

Implemented Option 1 with bounded changes:
- Canonicalized and validated request `repo_path` against `SKILL_GLOBAL_ALLOWED_ROOTS`.
- Added explicit writer error reason-code mapping and pre-enqueue rejection path.
- Added integration coverage for both accepted and rejected `repo_path` behavior.

## Technical Details

**Affected files:**
- `crates/session-extractor/src/writer.rs`
- `crates/session-extractor/src/lib.rs`
- `tests/integration/test_extract_session.rs` (contract updates)

**Database changes (if any):**
- No

## Resources

- `docs/tickets/2026-05-21-skill-layer-v1-1/06-session-end-extraction-and-approval.md`
- `crates/session-extractor/src/writer.rs`

## Acceptance Criteria

- [x] `repo_path` is canonicalized and validated against trusted roots
- [x] Out-of-bound paths are rejected with explicit reason code
- [x] Integration tests cover traversal and absolute-path abuse cases
- [x] Pending drafts are only written within approved scope roots

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Traced extract request -> writer path resolution -> filesystem write path.
- Confirmed no allowlist enforcement on request-provided repo path.

**Learnings:**
- Current contract enables unintended write target control from tool input.

### 2026-05-25 - Implementation completed

**By:** Copilot CLI

**Actions:**
- Added `repo_path` canonicalization + allowlist enforcement in `PendingDraftWriter`.
- Added writer error reason-code mapping and enqueue-time scope validation for explicit `failed` responses.
- Added integration test proving out-of-bound `repo_path` is rejected with `invalid_repo_path`.
- Updated success-path integration test to run with an explicit allowlist root.

**Verification:**
- `cargo test --test test_extract_session`

## Notes

- WHY classification: 🎯 PROTECTS USER STORY.
- Security category: path trust-boundary violation.
