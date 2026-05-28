---
ticket_id: T15
title: Extraction prompt review and unification
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "Follow-on hardening after T06"
feature_home: crates/session-extractor/
depends_on:
  - T06
dependency_type: hard
serves:
  - SC-3: extraction outputs stay contract-stable and provider-parity-safe
files:
  - crates/infrastructure/src/extraction/claude.rs
  - crates/infrastructure/src/extraction/ollama.rs
  - crates/session-extractor/src/providers/claude.rs
  - crates/session-extractor/src/providers/ollama.rs
  - crates/session-extractor/src/lib.rs
  - tests/integration/test_extract_session.rs
  - docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Extraction prompt review and unification

## Serves

- SC-3 by ensuring extraction prompt behavior is explicit, testable, and stable across providers.

## Scope

Review the extraction prompting strategy end-to-end, determine whether provider-specific prompting is required, and introduce a shared prompt contract where provider constraints do not require divergence.

## Scope Fence

- Do not change transcript trust-boundary rules (`transcript_ref`, root validation, traversal rejection).
- Do not change async extraction lifecycle semantics (`processing`, `job_id`, requested/completed/failed events).
- Do not change `.pending` lifecycle metadata or approval semantics.

## Acceptance Criteria

- Current prompt surfaces are inventoried (including provider-side versus local prompt ownership) with explicit rationale.
- A shared extraction prompt contract is implemented where possible; any provider-specific prompt logic is isolated and justified.
- Extraction contract parity remains intact (`ExtractionResult` candidate shape and required fields remain equivalent across providers).
- Integration coverage proves no regression to `extract_session` enqueue behavior, lifecycle events, and pending draft output.
- Architecture/ticket docs record the final decision: unified prompt path vs intentionally split provider prompts.

## Shared / Global Notes

- The question to answer is architectural, not just textual: prompt ownership can live in this repo or upstream provider adapters, but must be explicit.
- Provider-specific differences are acceptable only when tied to concrete API/response constraints.
- Prefer one canonical prompt builder/contract to reduce drift and prompt-quality skew.

## Local Context

Current implementation has provider asymmetry: Ollama builds a local natural-language prompt, while Claude sends structured transcript payload to an external endpoint that may own prompting logic. This ticket resolves whether that asymmetry is intentional and optimal, then codifies the result.

Unknowns to resolve during execution:

- Whether the Claude endpoint expects/benefits from a local prompt envelope in addition to transcript payload.
- Whether a shared prompt contract should be represented as a reusable builder in infrastructure extraction adapters.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Upstream extraction ticket: `docs/tickets/2026-05-21-skill-layer-v1-1/06-session-end-extraction-and-approval.md`

## Deeper-Dive Refs

- `docs/research/2026-05-26-llm-extraction-quality-map-reduce.md`
- `docs/constitution.md`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Prompt contract choice and provider adapters should ship together because split execution would create temporary parity drift.
- Contract tests stay coupled to this refactor so provider-equivalence guarantees are preserved as behavior changes.
