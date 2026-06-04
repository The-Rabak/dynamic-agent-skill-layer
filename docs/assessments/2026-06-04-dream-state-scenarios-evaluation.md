---
artifact: dream-state-evaluation
date: 2026-06-04
evaluator: 5× opus subagents (one per scenario), synthesized by orchestrator
scope: DS-003..DS-007 (the runnable dream-state contract tests), fresh reports __20260604~0630
related: docs/tickets/2026-05-31-skill-layer-v1-5/index.md, tests/e2e/test_dream_state_contract.rs
---

# Dream-State Scenarios — Quality / Performance / Operational-Value Evaluation

**Context:** After the full live e2e suite ran GREEN (36/39 passed, 0 failed; `--include-dream`), five opus
evaluators independently assessed each runnable dream-state scenario against its own report JSON and test
definition. **Bottom line: the suite is green, but only DS-003 meaningfully proves its contract. DS-004/005/006/007
pass without exercising what their names claim — green ≠ proven.**

| Scenario | Verdict | One-line |
|---|---|---|
| DS-003 dependency_chaos_matrix (CQRS resilience) | **ADEQUATE** | Test `assert!`s genuinely prove Option-A (Qdrant-down→reads OK; Ollama-down→Degraded; recovery), but the JSON report is hollow (outcome hardcoded Passed, no recorded assertions/latencies). |
| DS-004 outbox_backlog_replay | **WEAK** | No real restart/crash, no backlog, tautological assertion (`graph_version before=4 after=5`). Proves a counter, not durability. |
| DS-005 qdrant_pg_drift | **WEAK** | Injects zero drift, never calls reconciliation, asserts only `graph_version>0`. A smoke test mislabeled as a drift-and-reconcile contract. |
| DS-006 watcher_extraction_saturation | **WEAK** | Fires 24 concurrent read calls, asserts `ok+no_match>0`; the fresh run is **ok=0, no_match=24** (zero successful retrievals) yet passes. No watcher/extraction/convergence logic. |
| DS-007 high_qps_compile_context | **WEAK** | Latency samples are a monotonic ramp → **serialized embedding bottleneck, not real concurrency**; p95=1046ms ≈ 2× the <500ms target; verdict is hardcoded `Passed` with no threshold assertion. |

## Cross-cutting findings (most important)

1. **Reports record `outcome: Passed` independent of assertions.** `build()` derives outcome from `sections`, which
   several tests never populate — so the JSON artifact cannot distinguish pass from fail. Trust currently rests on the
   test-process exit code (the `assert!` panics), not the report. (DS-003, DS-007 explicitly hardcode `Passed`.)
2. **Several assertions are tautological or too weak to fail** on the real failure mode: DS-004 (`before<after` counter),
   DS-005 (`version>0`), DS-006 (`ok+no_match>0` — passes on 100% misses), DS-007 (hardcoded `Passed`, no p95 threshold).
3. **DS-006 `ok=0/no_match=24`** echoes the same retrieval-not-matching signal seen in the activation demo: its seeding/
   prompts don't produce live matches, and the test masks it by counting `NoMatch` as success. (Note: the in-process
   `test_live_data_plane_roundtrip` DOES assert `Ok` with a real match — so retrieval works; DS-006's seeding/prompts are
   the weak link, not the engine.)
4. **DS-007 exposes a real perf risk it then hides:** the clean arithmetic latency ramp (210→1079ms, ~17ms/step,
   completions ~16-19ms apart) is the signature of a single serialized resource (Ollama embedding) draining a FIFO queue,
   not 4-way concurrency. p95≈1046ms vs the constitution's <500ms warm target — apples-to-oranges (cold/contended embedding
   vs warm template), but the test asserts neither regime.

## Per-scenario operational value
- **DS-003 — KEEP, INSTRUMENT.** Retires the single most important architecture risk (Qdrant outage doesn't break reads).
  Add `record_latency("recovery",…)`, populate `contract_assertions`, and harden the `qdrant_write_side` health check
  (currently `if let Some(...)` can silently skip). Also add Postgres/Redis fault coverage.
- **DS-004 — REWRITE.** Enqueue a known backlog into the real outbox/Redis stream, hard-kill+restart N times, assert
  replayed==enqueued, 0 lost, 0 duplicated, and seeded skills are retrievable (not `NoMatch`).
- **DS-005 — REWRITE.** Inject a known number of PG/Qdrant divergences, call `OutboxReconciler::reconcile_once`, assert
  post-reconcile store-count equality and `gaps_closed == gaps_injected`; cover both drift directions + scale.
- **DS-006 — REWRITE + FIX ASSERTION NOW.** At minimum require `ok_count>0` (current green hides a 24/24 zero-match run);
  drive real watcher/extraction churn over a sustained window and assert convergence (final count, no dupes, queue drained).
- **DS-007 — MAKE FAIL-ABLE + FIX HOT PATH.** Add `assert!(p95 < TARGET)` + error-budget recorded in `contract_assertions`;
  address the serialized embedding bottleneck (cache/pool embeddings or warm the session-start path).

## What this does NOT change
These are pre-existing dream-state contract tests, not part of T10b. T10b (activation proof) is complete and the live
data-plane suite is genuinely green (roundtrip asserts real `Ok` retrieval). This evaluation is forward-looking hardening
signal for V2, captured as follow-up todo #155. It does not retract the V1.5 green status; it scopes how much confidence
to place in each dream-state scenario today.
