---
ticket_id: T01
title: Compose and domain foundation
kind: tracer-bullet
status: completed
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.1a"
feature_home: crates/domain/
depends_on: []
dependency_type: none
serves:
  - SC-7: graceful degrade foundation through local container topology
  - SC-8: V2 readiness through pure domain boundaries and shared traits
files:
  - docker-compose.yml
  - docker-compose.test.yml
  - docker-compose.override.yml
  - .env.example
  - scripts/run-t01-foundation-tests.sh
  - crates/domain/Cargo.toml
  - crates/domain/src/lib.rs
  - crates/domain/src/types.rs
  - crates/domain/src/traits.rs
  - crates/domain/src/errors.rs
  - crates/domain/src/config.rs
test_command: ./scripts/run-t01-foundation-tests.sh
tdd_mode: inherit
---

# Compose and domain foundation

## Serves

- SC-7 by making the whole stack runnable locally before any service logic lands.
- SC-8 by freezing the domain vocabulary and shared trait seams before adapters are added.

## Scope

Stand up the local Docker Compose topology and the pure `domain` crate that every downstream crate imports. The ticket is done when the workspace has a clean vocabulary for skills, scopes, subunits, extraction results, compiler inputs, and shared traits with no infrastructure leakage.

## Scope Fence

- Do not add concrete Ollama, PostgreSQL, Qdrant, Redis, or Claude clients here.
- Do not add service orchestration, MCP transport, retrieval, compilation, or graph logic.
- Keep `domain` free of `sqlx`, `qdrant-client`, `redis`, and `reqwest`.

## Acceptance Criteria

- `docker compose up` brings up the local topology cleanly.
- `cargo tree -p domain --depth 1` shows only lightweight domain dependencies.
- Domain types cover the plan vocabulary: skills, subunits, communities, scopes, lifecycle/status, extraction results, scored skills, and scope descriptors.
- Domain traits cover the architecture seams: embedding, transcript extraction, scope resolution, and context compilation.
- Typed config structs and shared domain errors exist without environment parsing or infrastructure coupling.
- Repeatable validation flow exists as `scripts/run-t01-foundation-tests.sh`.

## Shared / Global Notes

- Constitution local-first rules apply: all containers must stay local to Docker Compose.
- The architecture artifact freezes `domain` as the inner-most layer with zero infrastructure dependencies.
- This ticket sets the stable vocabulary consumed by every later ticket, so naming and trait shape changes after completion should be treated as expensive.

## Local Context

WHY link: the user story needs a zero-touch skill layer that multiple service crates can share without boundary drift. This ticket creates the vocabulary and container shell that make every later slice testable.

Work against the root Compose files plus `crates/domain/`. Keep the crate limited to data types, traits, config structs, and errors. Concrete decisions that matter now:

- `EmbeddingService`, `TranscriptSkillExtractionService`, `ScopeResolver`, and `ContextCompiler` live in `domain`.
- `docker-compose.yml` is the local-first deployment surface for Ollama, Qdrant, PostgreSQL, Redis, and service placeholders.
- Use `./scripts/run-t01-foundation-tests.sh` for repeatable ticket validation.
- If UUIDv7 support needs a decision early, prefer a contract-friendly placeholder that T02 can finalize without breaking the domain API.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 1.1a`
- WHY sections: `## Problem Narrative`, `## User Story`, `## Success Criteria`, `## TDD & Evidence Contract`

## Deeper-Dive Refs

- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/execution-shape.md`
- `.github/skills/workflows-to-issues/references/vertical-slice-architecture.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Compose topology and pure domain contracts stay together because they are the minimum honest tracer bullet foundation.
- Splitting the domain vocabulary away from the local runtime shell would force downstream tickets to guess about service shape and shared interfaces.
