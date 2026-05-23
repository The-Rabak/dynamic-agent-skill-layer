---
status: complete
priority: p2
issue_id: "012"
tags: [code-review, architecture, compiler, retrieval]
dependencies: []
---

# Compiler is coupled to retrieval internals

## Problem Statement

`compiler` directly depends on `retrieval` crate types (`RetrievedSkill`, `RescueCue`). This weakens feature-home boundaries and increases change amplification: retrieval-internal changes can force compiler API and behavior changes.

## Findings

- `crates/compiler/Cargo.toml` includes `retrieval` dependency.
- `crates/compiler/src/lib.rs` imports retrieval-specific structs and exposes `compile_from_retrieval`.
- Architecture/ticket context expects retrieval and compiler to remain separately owned and transport-agnostic (`docs/tickets/.../03-single-scope-compile-context.md:69`).
- Reviewers also flagged drift risk from having multiple compile paths that can diverge over time.

## Proposed Solutions

### Option 1: Compiler-owned compile input seam (Recommended)

**Approach:** Define compiler-local (or domain-level) compile input DTOs. Map retrieval output to that seam in `mcp-server` orchestration.

**Pros:**
- Restores cleaner crate boundaries.
- Limits future cross-crate churn.

**Cons:**
- Requires mapper and test updates.

**Effort:** Medium

**Risk:** Low

---

### Option 2: Keep coupling temporarily with explicit documented waiver

**Approach:** Record temporary exception and defer seam cleanup to a follow-up ticket.

**Pros:**
- Fastest short-term path.

**Cons:**
- Preserves architecture debt.

**Effort:** Small

**Risk:** Medium

## Recommended Action


## Technical Details

**Affected files:**
- `crates/compiler/Cargo.toml`
- `crates/compiler/src/lib.rs`
- `crates/compiler/src/rescue.rs`
- `crates/mcp-server/src/tools/compile_context.rs`

**Related components:**
- Compiler/retrieval seam
- Feature-home ownership integrity

**Database changes (if any):**
- No

## Resources

- Ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/03-single-scope-compile-context.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`

## Acceptance Criteria

- [x] `compiler` no longer imports `retrieval` crate types directly.
- [x] Compile input contract is explicit and documented.
- [x] Existing `compile_context` behavior remains unchanged after decoupling.
- [x] Regression tests cover seam mapping and compiler output parity.

## Work Log

### 2026-05-23 - Review synthesis (full working tree)

**By:** Copilot CLI

**Actions:**
- Consolidated architecture/code-simplicity findings on cross-crate coupling.
- Verified coupling points in compiler dependency and type imports.
- Captured remediation options for seam hardening.

**Learnings:**
- Tracer bullets can meet behavior goals while still carrying architecture debt worth tracking immediately.

### 2026-05-23 - Implementation complete

**By:** Copilot CLI

**Actions:**
- Removed `retrieval` dependency from `crates/compiler/Cargo.toml`.
- Added compiler-owned seam DTOs in `crates/compiler/src/rescue.rs`.
- Switched compiler API to `compile_with_rescue` and updated unit tests.
- Mapped retrieval outputs to compiler seam in `crates/mcp-server/src/tools/compile_context.rs`.

**Learnings:**
- Explicit seam DTOs keep output behavior stable while reducing cross-crate coupling.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT
- Severity rationale: medium-term maintainability risk, not immediate user-visible breakage.
