---
ticket_id: T14
title: Live data-plane E2E and stress suite
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.1, 2.2, 2.4, 3.1, 3.2"
feature_home: tests/
depends_on:
  - T07
  - T11
  - T13
dependency_type: hard
serves:
  - SC-1: full runtime context injection flow is validated against live dependencies
  - SC-4: PG-to-Qdrant durability and replay behavior are validated as one data-plane flow
  - SC-7: degraded semantics stay explicit under dependency loss and recovery
files:
  - tests/e2e/test_live_data_plane_roundtrip.rs
  - tests/e2e/test_concurrency_stress.rs
  - tests/e2e/test_watcher_churn_reconciliation.rs
  - tests/e2e/test_dream_state_contract.rs
  - scripts/run-e2e-tests.sh
  - scripts/run-t02-infrastructure-tests.sh
  - docker-compose.test.yml
test_command: ./scripts/run-e2e-tests.sh
tdd_mode: inherit
---

# Live data-plane E2E and stress suite

## Serves

- SC-1 by validating `compile_context` as a black-box runtime path, not only seeded or isolated test slices.
- SC-4 by proving graph mutations stay durable and replayable from filesystem change through PG and Qdrant consistency.
- SC-7 by verifying dependency outages produce explicit `degraded` behavior and clean recovery semantics.

## Scope

Add a dedicated end-to-end and stress harness that runs against temporary test containers (PostgreSQL, Qdrant, Ollama, Redis) and exercises the live runtime path: watcher/rebuild, outbox relay, retrieval, compile response, extraction, approval, degraded behavior, and recovery.

## Scope Fence

- Do not re-implement existing unit or integration assertions in this ticket; orchestrate full-flow behavior checks.
- Do not introduce distributed transactions or alternate persistence paths; validate the existing outbox and reconciliation contract.
- Keep stress scenarios deterministic and bounded so CI remains repeatable.

## Acceptance Criteria

- A roundtrip E2E test validates: skill filesystem change -> watcher/rebuild -> outbox drain -> retrieval visibility in `compile_context`.
- A session extraction E2E test validates inline/live extraction output goes to `.pending` and lifecycle events are emitted with no silent failure path.
- A degraded/recovery E2E test validates explicit reason-coded `degraded` results during dependency loss (for example Ollama or Qdrant unavailable) and healthy recovery after dependency restore.
- A watcher churn/reconciliation test validates rename/delete storms remain idempotent and converge to correct graph state.
- A bounded concurrency stress test validates parallel `compile_context` calls during rebuild/extraction activity with recorded latency/error evidence and no silent failure modes.
- A helper runner script executes realistic E2E suites in one command and can optionally execute ignored dream-state contract tests.

## Shared / Global Notes

- This ticket validates frozen contracts across crates; it does not redefine ownership or interfaces.
- The invalidation order (`PG commit -> outbox drain -> graph_version visibility -> graph.rebuilt`) remains canonical and must be asserted by test evidence.
- Degraded vs healthy-empty output remains a non-negotiable contract during failure tests.

## Local Context

WHY link: the autonomous loop is only trustworthy if the complete live path behaves correctly under realistic dependency conditions, not only in seeded or partially mocked slices.

This ticket intentionally couples black-box flow and stress coverage because both answer the same operational question: can the system sustain real runtime pressure without violating data integrity or response semantics? Keep focus on:

- live dependency containers and fixture lifecycles,
- cross-service event and invalidation sequencing,
- explicit failure and recovery semantics visible at MCP boundaries.

Unknowns: stress workload sizes may need tuning per CI capacity, but coverage targets and contract assertions are fixed.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.1, 2.2, 2.4, 3.1, 3.2`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Seams, Adapters, and Contracts`, `## Context Tiers`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#canonical-v11-contracts`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Full roundtrip E2E and stress scenarios stay together because both validate the same data-plane correctness boundary under different load/failure profiles.
- Splitting these surfaces into separate ownership would risk passing happy-path E2E while missing pressure-induced contract violations.
