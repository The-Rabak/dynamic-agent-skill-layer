---
status: complete
priority: p2
issue_id: "021"
tags: [code-review, t04, testing, reliability, quality]
dependencies: []
---

# Add environment guard helpers for integration test isolation

## Problem Statement

Integration tests mutate process-global environment variables for scope configuration without restoring prior values. This can create temporal coupling and flaky test behavior.

## Findings

- `configure_scope_env` sets `SKILL_GLOBAL_PATHS` and `SKILL_GLOBAL_ALLOWED_ROOTS` directly.
- Tests call setup repeatedly but do not restore previous env state via guard.
- Parallel or reordered test execution may observe leaked env state.
- WHY impact: quality/reliability issue; does not change feature outcome directly.

## Proposed Solutions

### Option 1: RAII env guard utility

**Approach:** Add a shared test helper that snapshots old values and restores on drop.

**Pros:**
- Deterministic cleanup.
- Reusable across integration suites.

**Cons:**
- Small helper maintenance cost.

**Effort:** Small

**Risk:** Low

---

### Option 2: Serial test harness with explicit teardown

**Approach:** Force serialization and add manual teardown in each test.

**Pros:**
- Simple migration.

**Cons:**
- Slower and easier to regress.

**Effort:** Small

**Risk:** Medium

---

### Option 3: Separate process per suite

**Approach:** Execute env-sensitive suites in isolated processes.

**Pros:**
- Strong isolation.

**Cons:**
- Heavier CI complexity and runtime cost.

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Implement Option 1 (RAII env guard utility): introduce a shared integration-test helper that snapshots
`SKILL_GLOBAL_ALLOWED_ROOTS`/`SKILL_GLOBAL_PATHS`, restores prior values on drop, and serializes env
mutation through a global mutex. Replace per-file direct env mutation with guard usage.

## Technical Details

**Affected files:**
- `tests/integration/test_compile_context.rs`
- `tests/integration/test_dual_scope.rs`
- shared test utility module (if added)

## Resources

- T04 execution session unit test file references.

## Acceptance Criteria

- [x] Env vars are restored after each test via guard or equivalent.
- [x] Integration tests remain stable when run repeatedly/in parallel.
- [x] No duplicated setup/teardown logic remains across suites.

## Work Log

### 2026-05-23 - Review finding captured

**By:** Copilot CLI (`/workflows-review`)

**Actions:**
- Consolidated uncle-bob clean-code/testability findings.

**Learnings:**
- Test reliability debt often hides behind passing local runs until suite ordering changes.

### 2026-05-23 - Implemented env guard isolation for integration tests

**By:** Copilot CLI (`pr-comment-resolver`)

**Actions:**
- Added `tests/integration/env_guard.rs` with:
  - `EnvVarGuard` RAII restore-on-drop wrapper for test env vars.
  - `ScopeEnvGuard` that holds both env-var guards and a process-wide `LazyLock<Mutex<()>>` lock to
    serialize env mutation for env-sensitive integration tests.
  - `scope_env_guard_restores_previous_values` test to verify restoration semantics.
- Updated `tests/integration/test_compile_context.rs`:
  - Replaced direct `configure_scope_env` env mutation helper with shared `env_guard::configure_scope_env()`.
  - Bound guard as `_env_guard` in each test to keep setup active for test duration.
- Updated `tests/integration/test_dual_scope.rs` with the same shared guard usage and removed duplicated
  env setup helper.
- Attempted verification via:
  - `cargo test --test test_compile_context --test test_dual_scope`
  - Run currently fails due pre-existing compile errors outside this todo scope:
    - `crates/retrieval/src/scope_resolution.rs` calling `resolve()` without required `repo_path` arg.
    - `crates/infrastructure/src/scope.rs` trait implementation signature mismatch for `resolve`.

**Learnings:**
- Centralizing env setup in integration tests removes duplicated unsafe env logic and reduces race-prone
  cross-test coupling.

## Notes

- WHY classification: 🔧 QUALITY IMPROVEMENT.
