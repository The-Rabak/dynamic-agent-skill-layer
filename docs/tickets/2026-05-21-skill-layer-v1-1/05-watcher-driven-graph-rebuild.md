---
ticket_id: T05
title: Watcher-driven graph rebuild
kind: expansion
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.1"
feature_home: crates/graph-builder/
depends_on:
  - T04
dependency_type: hard
serves:
  - SC-4: incremental graph rebuild on filesystem changes
  - SC-5: filesystem-observable state feeds the graph
files:
  - crates/graph-builder/Cargo.toml
  - crates/graph-builder/src/main.rs
  - crates/graph-builder/src/watcher.rs
  - crates/graph-builder/src/watcher_recovery.rs
  - crates/graph-builder/src/extraction/mod.rs
  - crates/graph-builder/src/extraction/rules.rs
  - crates/graph-builder/src/extraction/ollama_fallback.rs
  - crates/graph-builder/src/extraction/dedup.rs
  - crates/graph-builder/src/graph/build.rs
  - crates/graph-builder/src/graph/communities.rs
  - crates/graph-builder/src/graph/embeddings.rs
  - crates/graph-builder/src/graph/rebuild.rs
  - tests/integration/test_watcher_rebuild.rs
  - tests/fixtures/test-skills/
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Watcher-driven graph rebuild

## Serves

- SC-4 by turning file changes into incremental graph rebuilds.
- SC-5 by making the filesystem the source of observable graph state changes.

## Scope

Build the offline graph-builder path that watches scope directories, recovers missed transitions, extracts subunits, generates embeddings, assigns communities, and emits rebuild events only when the graph is durably updated.

## Scope Fence

- Do not implement merge policy, retirement policy, cron maintenance, or admin tools here.
- Do not move retrieval, MCP transport, or approval workflows into graph-builder.
- Keep rebuild behavior aligned with the outbox and `graph_version` invalidation contract.

## Acceptance Criteria

- New and renamed skill files are detected in project and global scopes, including `.pending` to `.md` approvals.
- Reconciliation recovers missed rename/delete transitions idempotently.
- Graph rebuild extracts subunits with deterministic rules and falls back to Ollama JSON only when the structural path is too thin.
- Embeddings and community assignments are written durably, and rebuild completion publishes `graph.rebuilt` only after the durable invalidation point.
- Audit records capture graph mutations from file-driven changes.

## Shared / Global Notes

- Graph-builder owns graph construction only; merge/retire policy stays in `maintenance`, and online debug stays in `admin`.
- The event catalog and invalidation ordering are already frozen by the plan and architecture artifact.
- The watcher must preserve the filesystem-as-UI rule rather than replacing it with hidden state.

## Local Context

WHY link: the self-growing loop only becomes real when new or approved skills automatically reach the graph instead of being manually seeded forever.

This ticket stays inside `crates/graph-builder/` even though it touches extraction, embeddings, and communities, because those are all one offline graph-construction boundary. Important constraints:

- Use `notify-debouncer-full` plus reconciliation rather than trusting watcher events alone.
- Keep `skill.file_changed` and `graph.rebuilt` semantics aligned with the architecture's invalidation contract.
- If HDBSCAN implementation detail becomes contentious, resolve it inside this ticket without moving community ownership out of graph-builder.

Unknowns: the exact HDBSCAN implementation library may vary, but the graph-builder ownership and rebuild contract are fixed.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.1`
- Frozen contracts: `## Canonical V1.1 Contracts`, `## Drift Checks`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#feature-homes-and-ownership`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`

## Coupling Notes

- Watcher, extraction, embeddings, communities, and rebuild publication stay together because they all belong to the same graph-builder outcome.
- Splitting this packet earlier would blur ownership and invite hidden coupling across internal graph-builder modules.
