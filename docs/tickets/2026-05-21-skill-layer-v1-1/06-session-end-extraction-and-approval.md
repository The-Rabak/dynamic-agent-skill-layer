---
ticket_id: T06
title: Session-end extraction and approval
kind: expansion
status: completed
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.2"
feature_home: crates/session-extractor/
depends_on:
  - T05
dependency_type: hard
serves:
  - SC-3: session-end skill extraction with human approval
files:
  - crates/session-extractor/Cargo.toml
  - crates/session-extractor/src/lib.rs
  - crates/session-extractor/src/transcripts.rs
  - crates/session-extractor/src/providers/claude.rs
  - crates/session-extractor/src/providers/ollama.rs
  - crates/session-extractor/src/writer.rs
  - crates/mcp-server/src/tools/extract_session.rs
  - tests/integration/test_extract_session.rs
  - tests/fixtures/sample-transcript.jsonl
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Session-end extraction and approval

## Serves

- SC-3 by turning session transcripts into human-gated `.pending` skill drafts.

## Scope

Implement the `extract_session` path, transcript trust boundary, provider routing, and `.pending` draft generation so that a session can grow the skill graph without bypassing human approval.

## Scope Fence

- Do not auto-approve, auto-merge, or perform real-time in-session transcript analysis.
- Do not accept raw host filesystem paths outside `CLAUDE_TRANSCRIPT_ROOT`.
- Do not add non-Claude transcript formats in v1.1.

## Acceptance Criteria

- `extract_session(transcript_ref)` returns immediately with a processing response and background job identity.
- Transcript refs are validated relative to the mounted root and reject traversal or raw absolute host paths.
- Claude and Ollama providers both emit the same extraction JSON contract.
- `.pending` skill drafts are written to the correct scope with the expected metadata and suggested tags.
- `skill.extraction_requested`, `extraction.completed`, and `extraction.failed` events reflect the async lifecycle.

## Shared / Global Notes

- The filesystem remains the approval UI; `.pending` rename-to-approve is mandatory.
- `TranscriptSkillExtractionService` is a domain seam; providers stay behind it.
- `get_pending_extractions` remains read-only convenience, not an approval path.

## Local Context

WHY link: the system only becomes self-growing when session output turns into reviewable skill drafts automatically.

Key contract details to preserve:

- `transcript_ref` is the primary ingress contract.
- `extract_session` is asynchronous by design and must not hold up the session-end hook.
- Writer output must be friendly to the watcher/rebuild flow introduced in T05 so approvals turn into active graph content without extra tooling.

Unknowns: none beyond provider implementation details already hidden behind the extraction trait.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 2.2`
- Frozen contracts: `#### extract_session transcript contract`, `## Canonical V1.1 Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#seams-adapters-and-contracts`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- The tool handler, transcript parser, provider router, and draft writer stay together because they deliver one honest outcome: approved skill drafts after session end.
- Pulling the writer or provider routing into another ticket would weaken the trust-boundary and approval workflow contract.
