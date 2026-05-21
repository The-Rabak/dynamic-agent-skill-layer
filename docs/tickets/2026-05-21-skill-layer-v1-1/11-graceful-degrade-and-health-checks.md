---
ticket_id: T11
title: Graceful degrade and health checks
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.1"
feature_home: crates/infrastructure/
depends_on:
  - T08
dependency_type: hard
serves:
  - SC-7: explicit degraded behavior and service health
files:
  - crates/infrastructure/src/resilience.rs
  - crates/infrastructure/src/health.rs
  - crates/mcp-server/src/main.rs
  - crates/graph-builder/src/main.rs
  - crates/session-extractor/src/lib.rs
  - docker-compose.yml
  - tests/integration/test_resilience.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Graceful degrade and health checks

## Serves

- SC-7 by making infrastructure failure modes explicit, retryable, and inspectable instead of silent or crash-shaped.

## Scope

Add shared retry/backoff and circuit-breaker behavior, health endpoints, degrade guards for online paths, and Docker health-check definitions that describe the runtime honestly.

## Scope Fence

- Do not add dashboards, alerting, or metrics stacks.
- Do not collapse healthy `no_match` into degraded-empty behavior.
- Keep resilience logic reusable in infrastructure and consumed by services rather than reimplemented ad hoc.

## Acceptance Criteria

- MCP server returns explicit `degraded` outcomes when dependencies are unavailable.
- Graph builder and session extractor retry transient failures with bounded backoff.
- Circuit-breaker behavior is explicit and testable.
- Services expose `/health` with dependency-level status.
- Docker Compose health checks and startup order reflect the intended runtime topology.

## Shared / Global Notes

- The degraded vs healthy-empty distinction is a frozen top-level contract.
- Resilience helpers belong in `infrastructure`; services should apply them without re-owning the logic.
- This ticket hardens existing behavior; it should not reshape feature ownership.

## Local Context

WHY link: the user story requires zero-touch behavior that fails gracefully when local services are missing or restarting; brittle failure modes would break the session-start promise.

Work across the runtime entry points and shared resilience utilities only. Important now:

- Wrap `compile_context` in a degrade guard, not a fake success path.
- Keep retry behavior explicit for graph-builder and extractor flows.
- Use the same dependency names and reason-code semantics later documented in the runbook ticket.

Unknowns: none beyond threshold tuning for retry and circuit-breaker settings.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 3.1`
- Frozen contracts: `#### compile_context result contract`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#interfaces-as-test-surfaces`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Retry, degrade, and health semantics stay together because they all define what "safe failure" means across the runtime.
- Splitting runtime resilience from health reporting would make operator-facing status drift from actual fallback behavior.
