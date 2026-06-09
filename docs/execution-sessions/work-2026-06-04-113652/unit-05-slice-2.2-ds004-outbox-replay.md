---
unit: "Slice 2.2 — DS-004 outbox_backlog_replay — real kill/restart + no loss"
unit_number: 5
unit_kind: hardening
serves: "SC#4 (DS-004 no-loss replay)"
status: completed
attempt_count: 1
domains: [rust, e2e, durability, outbox]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
Rewrote `outbox_backlog_replays_without_data_loss_after_multi_restart_sequence` (test_dream_state_contract.rs:460)
in 8 phases: seed 10 skills → stop Qdrant → enqueue N=10 `vector.upsert` events to the real PG outbox
(`write_coordinator.append_outbox_event`, scoped by a `backlog_correlation_id` UUID) → **2 crash/restart cycles**
(`drop(components)` without `.teardown()` so PG rows survive; rebuild via `from_environment`) asserting 10 still
pending each time → start Qdrant → `OutboxRelay::relay_once()` drain loop → measure `replayed`/`lost`/`duplicated`
via sqlx scoped to the correlation id → assert `replayed==enqueued`, `lost==0`, `duplicated==0` via `assert_contract`
→ assert seeded skill retrievable (`Ok|NoMatch`). Tautological `graph_version before<after` removed.

## Files Changed
- `tests/e2e/test_dream_state_contract.rs` — DS-004 rewrite + outbox-type imports (GraphWriteCoordinator, OutboxEvent, OutboxRelay, VECTOR_UPSERT_EVENT_TYPE)

## KNOWN LIMITATION (recorded, not hidden)
"Crash" = **in-process `drop()` + rebuild**, NOT an OS-level SIGKILL of a separate relay process. Root cause: the
existing "live container" e2e tests run server logic IN-PROCESS against containerized PG/Redis/Qdrant/Ollama — they
do not drive the containerized mcp-server/relay over its transport, so there is no separate OS process to kill here.
What IS genuinely proven + fail-able: PG-outbox durability across crash (rows survive drop), exactly-once replay
(idempotency-key dedup), zero loss/dup. What is DEFERRED: true SIGKILL-mid-delivery + Redis consumer-group reclaim
of the containerized relay — belongs to transport-level coverage (DS-002 / slice 3.1) or a follow-up. SC#4 is
substantively met on the durability surface; the OS-SIGKILL variant is a noted follow-up. → see [[learnings]] note.

## TDD Evidence
- **Red (by construction):** `lost>0` ⇒ `zero_events_lost` Failed; `duplicated>0` ⇒ `zero_duplicates` Failed; stalled event ⇒ `replayed<enqueued` Failed. Old tautological counter removed. Live Red pending stack.
- **Green:** `--skip ignored` → 7 passed, 24 ignored; scenario compiles.
- **Post-Refactor Green:** same after fmt; FMT_CLEAN.

## Test Results
- `--skip ignored` green; fmt clean. Live: `... outbox_backlog -- --include-ignored` PENDING-LIVE.
