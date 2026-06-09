---
unit: "Slice 2.1 — DS-003 dependency_chaos_matrix — deepen real Option-A CQRS proof"
unit_number: 4
unit_kind: hardening
serves: "SC#8 (DS-003 recovery + faults)"
status: completed
attempt_count: 1
domains: [rust, e2e, resilience, fault-injection]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
Rewrote `dependency_chaos_matrix_preserves_degraded_semantics_and_fast_recovery` (test_dream_state_contract.rs:161).
Added `#[path = "support/mod.rs"] mod support;` (first Batch-2 slice). Closed all 4 DS-003 gaps:
1. **qdrant_write_side hard-assert** — replaced `if let Some(...)` silent-skip with present-AND-unhealthy assertions, both recorded via `assert_contract` (absent component ⇒ Failed).
2. **Recovery latency** — `Instant` from service-restored to first `Ok|NoMatch`; `record_latency("recovery", ms)` + 60s-budget assertion.
3. **Bounded polling** — removed fixed 2s sleep; `support::poll::poll_until` on real observables.
4. **PG + Redis fault phases** — `compose_stop_service("postgres"|"redis")` → assert `compile_context` stays `Ok|NoMatch` (write-side-only per CQRS doc) → restart → poll-recover. Recorded as contract assertions + degradation events.

## Files Changed
- `tests/e2e/test_dream_state_contract.rs` — `mod support` include + full DS-003 body rewrite

## Patterns Discovered
- `poll_until` closures capture by clone (`Fn`, called repeatedly). Recovery-latency needs explicit `for/break` loop (poll_until doesn't expose elapsed).
- PG/Redis: poll `InfrastructureHealthChecker` (matches prod health signal); Qdrant/Ollama: direct HTTP poll.
- PG & Redis are write-side-only → stopping them must NOT degrade reads (Option-A). Source: docs/reference/online-retrieval-cqrs.md.

## TDD Evidence
- **Red (by construction + compile):** documented fail-ability per assertion — absent write-side component ⇒ Failed (was silent skip); breached recovery budget ⇒ Failed; PG/Redis-down returning Degraded ⇒ Failed. Live Red pending stack.
- **Green:** `cargo test --test test_dream_state_contract -- --skip ignored` → 7 passed, 24 ignored; scenario compiles.
- **Post-Refactor Green:** same after fmt; compile incl. ignored clean (4 pre-existing harness dead_code warnings).

## Test Results
- `--skip ignored` green; full compile green; fmt clean. Live: `... dependency_chaos_matrix -- --include-ignored` PENDING-LIVE.
