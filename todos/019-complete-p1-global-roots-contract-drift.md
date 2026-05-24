---
status: complete
priority: p1
issue_id: "019"
tags: [code-review, t04, configuration, constitution, drift-risk, documentation]
dependencies: []
---

# Reconcile SKILL_GLOBAL_ALLOWED_ROOTS requirement with frozen contracts

## Problem Statement

Implementation now hard-requires `SKILL_GLOBAL_ALLOWED_ROOTS`, but ticket/plan/architecture contract language for T04 centers on `SKILL_GLOBAL_PATHS`. This introduces contract drift without explicit waiver.

## Findings

- `EnvPathGlobalResolver` requires both path env vars and fails if allowed roots var is missing.
- T04 acceptance criteria and architecture seam docs do not currently codify this new hard requirement.
- `constitution_waivers` is empty for the plan.
- WHY impact: hidden config breakage can degrade first-prompt behavior and create governance drift.

## Proposed Solutions

### Option 1: Update contracts + docs + startup validation

**Approach:** Promote `SKILL_GLOBAL_ALLOWED_ROOTS` to explicit contract in ticket/architecture/runbook and fail fast with clear diagnostics.

**Pros:**
- Preserves stronger security posture.
- Removes ambiguity.

**Cons:**
- Requires synchronized doc and contract updates.

**Effort:** Small

**Risk:** Low

---

### Option 2: Backward-compatible default

**Approach:** If allowed roots is unset, derive safe default (e.g., repo root) and emit warning.

**Pros:**
- Reduces operational friction.

**Cons:**
- Implicit defaults can mask misconfiguration.

**Effort:** Small

**Risk:** Medium

---

### Option 3: Feature flag strict roots enforcement

**Approach:** Gate strict requirement behind explicit config mode.

**Pros:**
- Controlled rollout.

**Cons:**
- Adds configuration branching and possible drift.

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Adopt Option 1: update T04 ticket/architecture/plan contract language so docs explicitly require both `SKILL_GLOBAL_PATHS` and `SKILL_GLOBAL_ALLOWED_ROOTS` with no fallback, matching `EnvPathGlobalResolver` behavior.

## Technical Details

**Affected files:**
- `crates/infrastructure/src/scope.rs`
- `docs/tickets/2026-05-21-skill-layer-v1-1/04-dual-scope-retrieval-and-hooking.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- related runbook/config docs

## Resources

- Constitution: `docs/constitution.md`
- Plan frontmatter (`constitution_waivers: []`)

## Acceptance Criteria

- [x] Runtime contract for global scope env vars is explicit and documented.
- [x] Code and docs agree on required env vars and fallback behavior.
- [x] Review can trace either waiver or explicit contract amendment history.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Consolidated constitution-guardian and architecture findings about env contract drift.
- Logged as blocking governance/contract issue.

**Learnings:**
- Security hardening changes must be reflected in frozen architecture/ticket contracts to avoid drift.

## Notes

- WHY classification: 🏛️ CONSTITUTION VIOLATION + ⚠️ DRIFT RISK.

### 2026-05-23 - Contract drift resolved

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Updated T04 ticket acceptance criteria to require both `SKILL_GLOBAL_PATHS` and `SKILL_GLOBAL_ALLOWED_ROOTS` with explicit no-fallback semantics.
- Updated v1.1 architecture seam/global-context contract text to include `SKILL_GLOBAL_ALLOWED_ROOTS` as part of runtime config defaults and scope-resolution contract.
- Updated v1.1 plan Slice 1.3 build + acceptance text to match resolver behavior (required allowlist roots).
- Marked this todo complete with acceptance criteria checked.

**Learnings:**
- Security-driven env requirements must be reflected in frozen ticket/architecture/plan artifacts immediately to prevent governance and runtime contract drift.
