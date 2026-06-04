---
unit: "Fix #156 (rebuild idempotency/publish-atomicity) + #157 (ensure_collection 409)"
unit_number: 0.5
unit_kind: bugfix/prod
serves: "Unblocks the self-growing loop; golden-path RED→GREEN; prerequisite for all real DS scenarios"
status: completed
attempt_count: 1
domains: [rust, graph-builder, outbox, qdrant, postgres, live-e2e]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
**#156 (P1) — self-growing loop frozen:**
1. **Idempotent vector-outbox enqueue:** added `insert_outbox_event_idempotent` (`INSERT … ON CONFLICT (idempotency_key)
   DO NOTHING`) + `append_outbox_event_idempotent` inherent method on `PostgresGraphWriteCoordinator` (returns bool
   inserted/skipped). `PostgresDurableGraphState::persist_graph_mutation` now uses it, so re-emitting a vector event for an
   unchanged skill is a benign no-op instead of a cycle-failing UNIQUE conflict. **`append_outbox_event` (strict exactly-once,
   used by relay/DS-004) is UNTOUCHED** — conflict-tolerance scoped to the rebuild path only.
2. **Replay-safety:** added `maybe_replay_graph_rebuilt` in graph-builder `main.rs` — on each loop iteration / cold start, if
   PG `graph_state.graph_version` > last-published version, re-publish `graph.rebuilt` to Redis (mcp-server `swap_graph` is
   idempotent for same/older versions). `drain_published_events` now returns `Option<i64>`; main loop tracks
   `last_published_graph_version`. Snapshot can never permanently freeze behind PG again.

**#157 (P2) — cold-start 409 crash:** `QdrantAdapter::ensure_collection` now treats `409 Conflict` as benign success.
One fix covers both mcp-server + graph-builder (shared method).

**Bonus (real infra bug found while fixing):** `PostgresAdapter::connect` now sets `test_before_acquire(true)` on
`PgPoolOptions` — evicts "idle in transaction (aborted)" dirty connections (left by multi-statement migration SQL) before
checkout, which was otherwise blocking all pool ops.

## Files Changed (production crates)
- `crates/infrastructure/src/persistence/outbox.rs` — idempotent enqueue method
- `crates/infrastructure/src/persistence/postgres.rs` — `test_before_acquire(true)`
- `crates/infrastructure/src/vector/qdrant.rs` — 409-as-success + 2 tests
- `crates/graph-builder/src/graph/rebuild.rs` — use idempotent enqueue + idempotent-rebuild test
- `crates/graph-builder/src/main.rs` — `maybe_replay_graph_rebuilt`, drain returns version, track last-published

## TDD Evidence (LIVE)
- **Red:** golden-path FAIL live — "snapshot did not advance from v9 within 90s — see #156; PG=10 served=2".
- **Green:** after `docker compose build graph-builder mcp-server && up -d` → golden-path PASS (baseline gv=13;
  `loop_closes_after_seed: Passed`, `seeded_skill_retrievable: Passed`). graph-builder logs: replay fired (pg ahead → republish),
  `ON CONFLICT DO NOTHING ×6 rows_affected=0 no error`, `graph rebuilt graph_version=13`. New units: graph-builder 8, infrastructure 97.
- **Post-Refactor Green:** golden-path stable green (gv climbs 13→17→21 across runs — loop actually grows now); fmt clean; clippy clean.
- **#157 verified:** `docker compose rm -sf graph-builder mcp-server && up -d` → BOTH healthy with NO manual restart.

## Resolves
- todos/156-...md (P1) — RESOLVED. todos/157-...md (P2) — RESOLVED.
