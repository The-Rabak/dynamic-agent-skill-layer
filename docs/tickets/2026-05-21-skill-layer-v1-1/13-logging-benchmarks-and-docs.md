---
ticket_id: T13
title: Logging, benchmarks, and docs
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.2"
feature_home: crates/infrastructure/
depends_on:
  - T11
dependency_type: hard
serves:
  - SC-1: latency evidence for compile_context targets
  - SC-7: observable runtime behavior and documented degraded states
  - SC-8: contributor-ready documentation for the canonical architecture
files:
  - crates/infrastructure/src/logging.rs
  - crates/mcp-server/src/main.rs
  - crates/graph-builder/src/main.rs
  - crates/session-extractor/src/lib.rs
  - tests/bench/compile_context_bench.rs
  - README.md
  - docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
  - docs/reference/capability-catalog.md
  - docs/runbooks/degraded-state.md
  - docs/reference/transcript-ingress.md
  - CONTRIBUTING.md
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit && cargo bench
tdd_mode: inherit
---

# Logging, benchmarks, and docs

## Serves

- SC-1 by proving the latency target with benchmark evidence.
- SC-7 by surfacing structured runtime behavior and degraded-state meaning.
- SC-8 by turning the v1.1 contracts into contributor-facing documentation.

## Scope

Finish observability and operator-readiness: structured JSON logging, compile-context benchmarks, quickstart and architecture docs, capability reference, degraded-state runbook, and contributing guidance.

## Scope Fence

- Do not add OpenTelemetry, dashboards, or alerting stacks.
- Do not treat documentation as a place to invent new runtime behavior.
- Keep benchmark work focused on validating the existing latency target, not broad performance tuning.

## Acceptance Criteria

- All major services emit structured JSON logs to stdout with useful context.
- `compile_context` latency benchmarks produce repeatable p50 and p95 evidence against the plan target.
- README and contributing docs let a new developer stand the stack up and run the expected commands.
- Reference docs cover tool contracts, event catalog, lifecycle states, transcript ingress, and degraded reason codes.
- Documentation matches the hardened runtime and feature-home decisions already frozen elsewhere.

## Shared / Global Notes

- Logging setup remains shared infrastructure; service entry points only initialize and annotate it.
- Documentation must reinforce the local-first and filesystem-observable model rather than abstracting it away.
- The architecture artifact remains the deep reference; ticket output should update supporting docs without duplicating the whole plan.

## Local Context

WHY link: the system is only usable by its owner and future contributors if its behavior is observable and its contracts are discoverable without re-reading the whole plan.

This ticket joins observability code and docs because they are two sides of the same operator-facing outcome. Keep the focus on:

- structured log fields for compile-context and rebuild flows,
- benchmark evidence for the latency promise,
- docs that explain setup, runtime states, and maintenance expectations accurately.

Unknowns: benchmark fixture volume can be tuned, but the latency target and doc set are fixed.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 3.2`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Context Tiers`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Logging, latency evidence, and operator docs stay together because they all describe whether the finished system is understandable and supportable.
- Splitting docs away from observable runtime evidence would risk stale guidance and weak handoff.
