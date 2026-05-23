---
status: complete
priority: p1
issue_id: "013"
tags: [code-review, constitution, logging, mcp-server]
dependencies: []
---

# mcp-server startup logging violates structured logging constitution rule

## Problem Statement

The constitution requires structured events to stdout for every service. `mcp-server` previously logged startup via plain console print macros, which violated the unwaived repository rule.

## Findings

- Constitution mandates structured logging: `docs/constitution.md:74`.
- `crates/mcp-server/src/main.rs` previously used plain string startup printing (`main.rs:14-17`).
- No ticket/session waiver exists for this deviation (`docs/execution-sessions/work-2026-05-22-222602/STATE.md` shows constitution with no waivers).

## Proposed Solutions

### Option 1: Replace startup plain prints with structured logger event (Recommended)

**Approach:** Use repository-standard structured logging utility (or add minimal JSON log event) for startup/tool-registration output.

**Pros:**
- Satisfies constitution rule.
- Improves operability and auditability.

**Cons:**
- Requires choosing/aligning logger usage in this crate.

**Effort:** Small

**Risk:** Low

---

### Option 2: Document temporary waiver with explicit expiry

**Approach:** Record a short-lived constitution waiver in plan/ticket artifacts.

**Pros:**
- Allows merge with explicit exception handling.

**Cons:**
- Leaves compliance gap in runtime output.

**Effort:** Small

**Risk:** Medium

## Recommended Action


## Technical Details

**Affected files:**
- `crates/mcp-server/src/main.rs`
- `docs/constitution.md` (reference baseline only)

**Related components:**
- Service observability
- Review guardrail compliance

**Database changes (if any):**
- No

## Resources

- Constitution: `docs/constitution.md`
- Execution session: `docs/execution-sessions/work-2026-05-22-222602/STATE.md`

## Acceptance Criteria

- [x] `mcp-server` startup emits structured log event(s) to stdout.
- [x] No plain-text-only service lifecycle logs remain in this path.
- [x] Review confirms constitution logging requirement is met without waiver.

## Work Log

### 2026-05-23 - Review synthesis (full working tree)

**By:** Copilot CLI

**Actions:**
- Verified constitution logging rule and compared against runtime startup code.
- Captured compliance finding as blocking due unwaived MUST rule.

**Learnings:**
- Guardrail violations can hide in small bootstrap details.

### 2026-05-23 - Research + implementation complete

**By:** Copilot CLI

**Actions:**
- Researched 2026 Rust industry-standard logging/tracing stack and selected `tracing` + `tracing-subscriber` JSON as baseline, with OpenTelemetry-ready layering path.
- Expanded global infrastructure logging service with `ServiceLoggingConfig` and `init_service_logging` for shared structured bootstrap across crates.
- Replaced startup plain-print logging in `mcp-server` with structured `tracing::info!` event fields.
- Verified no plain console print macro usages remain in repository source (`rg` scan).

**Learnings:**
- A shared logging bootstrap in infrastructure crate gives consistent JSON logs and leaves a clean seam for future trace/metrics export layers.

## Notes

- WHY classification: 🏛️ CONSTITUTION VIOLATION
- Severity rationale: unwaived constitution MUST violation is merge-blocking by review policy.
