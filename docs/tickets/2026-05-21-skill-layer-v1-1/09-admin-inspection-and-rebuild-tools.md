---
ticket_id: T09
title: Admin inspection and rebuild tools
kind: expansion
status: completed
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.3"
feature_home: crates/admin/
depends_on:
  - T05
dependency_type: hard
serves:
  - SC-4: operable graph inspection and manual rebuild trigger
  - SC-5: read-only visibility into graph state
files:
  - crates/admin/Cargo.toml
  - crates/admin/src/lib.rs
  - crates/admin/src/tools.rs
  - crates/mcp-server/Cargo.toml
  - crates/mcp-server/src/lib.rs
  - crates/mcp-server/src/protocol.rs
  - tests/integration/test_admin_tools.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Admin inspection and rebuild tools

## Serves

- SC-4 by giving the operator a manual rebuild trigger and graph inspection surface.
- SC-5 by exposing graph state through read-only or trigger-only MCP tools instead of hidden internals.

## Scope

Create the `admin` feature home for online debug tools: `rebuild_graph`, `inspect_skill`, and `list_communities`.

## Scope Fence

- Do not move merge/retire policy or graph construction into the admin crate.
- Keep tools read-only or trigger-only; no approval or mutation authority beyond rebuild initiation.
- Do not duplicate existing extraction approval workflows in admin tooling.
- Do not implement authn/authz in this phase; access-control work is explicitly deferred.

## Deferred Access-Control Decision

- Auth/access control for admin tools is deferred by scope decision for this phase.
- Admin tools are currently unauthenticated and therefore MUST be deployed only on localhost or private network surfaces.
- Public exposure is out of bounds until a dedicated auth/access-control unit lands.

## Acceptance Criteria

- `rebuild_graph` triggers a full rebuild against the existing graph-builder workflow.
- `inspect_skill` returns a skill's neighborhood, subunits, and community context.
- `list_communities` returns communities with member counts.
- The admin surface composes into the online binary without bloating the main MCP feature path.

## Shared / Global Notes

- The architecture artifact explicitly keeps `admin` separate from `mcp-server` and `graph-builder` even if it is router-composed into the same online binary.
- These tools exist for visibility and manual triggering, not to replace filesystem approval flows.

## Local Context

WHY link: once the graph is self-updating, the operator still needs a clean way to inspect state and recover manually without breaching architecture boundaries.

Stay inside the admin crate and wire it to existing graph-builder and persistence contracts. The key boundary to preserve is that admin remains a thin online surface; it should not inherit maintenance or graph construction logic.

Unknowns: none beyond response-shape details for the read-only tool payloads.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.3`
- Feature-home guidance: `## Feature Homes and Ownership`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#resolved-operational-choices`
- `.github/skills/workflows-to-issues/references/vertical-slice-architecture.md`

## Coupling Notes

- These tools stay together because they form one online debug surface with the same read-only or trigger-only boundary.
- Keeping them separate from maintenance preserves feature-home clarity and keeps policy code out of the online path.
