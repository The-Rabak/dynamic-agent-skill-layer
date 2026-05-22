---
status: complete
priority: p2
issue_id: "007"
tags: [code-review, security, infrastructure, quality]
dependencies: []
---

# Harden security posture of infrastructure adapters

## Problem Statement

Adapter defaults and boundary handling leave avoidable security risk in credentials, input sizing, and diagnostics exposure.

## Findings

- Default DB credentials are weak and easy to misuse outside tests (`crates/infrastructure/src/persistence/postgres.rs:20`).
- Extraction paths do not enforce transcript size/token bounds before forwarding (`crates/infrastructure/src/extraction/claude.rs`, `.../ollama.rs`).
- Health checks return raw backend error strings (`crates/infrastructure/src/health.rs:60-63,79-82,86-89,107-110`).
- Global scope path env input is not trust-boundary constrained (`crates/infrastructure/src/scope.rs:77-99`).

## Proposed Solutions

### Option 1: Enforce fail-closed secure defaults
**Approach:** Require explicit DSN config, cap transcript payload size, sanitize health output, and validate scope paths against allowed roots.
**Pros:** Reduces exploitability and accidental exposure.
**Cons:** Requires config and docs updates.
**Effort:** Medium  
**Risk:** Low

---

### Option 2: Logging-only mitigation
**Approach:** Keep behavior but add warnings and docs.
**Pros:** Fast.
**Cons:** Does not eliminate core risk.
**Effort:** Small  
**Risk:** High

## Recommended Action

Implemented Option 1 (fail-closed defaults and boundary checks) across PostgreSQL config, transcript extraction limits, health diagnostics, and scope path validation.

## Acceptance Criteria

- [x] Runtime credentials are not silently defaulted to weak values.
- [x] Extraction requests enforce explicit payload limits.
- [x] Health endpoints expose sanitized diagnostics.
- [x] Scope resolver rejects out-of-bound paths.

## Work Log

### 2026-05-22 - Review capture

**By:** Copilot CLI

**Actions:**
- Consolidated security-sentinel findings into one hardening task.

**Learnings:**
- Most items are defensive hardening, not immediate exploit blockers, but should be addressed before wider rollout.

### 2026-05-22 - Execution

**By:** Copilot CLI

**Actions:**
- Changed `PostgresConfig::default()` to fail-closed (`database_url` empty) so runtime credentials are explicit.
- Added transcript payload bounds (`max_entries`, `max_entry_chars`, `max_total_chars`) and shared validation in extraction adapters.
- Sanitized health-check failure details to avoid raw backend error leakage.
- Added allowed-root enforcement for env-based global scope paths and tests that reject out-of-bound paths.

**Learnings:**
- Security hardening in adapters benefits from explicit boundary contracts more than warning-level logging.
