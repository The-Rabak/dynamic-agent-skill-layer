---
ticket_id: T06
title: SkillDAG-style agent retrieval tools
kind: expansion
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 6"
feature_home: "crates/mcp-server/src/tools and crates/retrieval"
depends_on:
  - T04
  - T05
dependency_type: hard
serves:
  - Agent-facing matches, neighbors, conflicts, and show-body workflow
files:
  - crates/mcp-server/src/protocol.rs
  - crates/mcp-server/src/tools/
  - crates/retrieval/src/
  - crates/compiler/src/
test_command: "cargo test -p mcp-server --features test-utils --test test_skill_graph_tools -- --ignored && cargo test -p retrieval"
tdd_mode: ralph
---

# SkillDAG-style agent retrieval tools

## Serves

Expose retrieval as an agent-callable typed graph surface instead of only opaque ranked context injection.

## Scope

- Add or evolve MCP tools around `search_skill_graph` and `show_skill`.
- Return separate `matches`, `neighbors`, and `conflicts`.
- Include why/rationale fields, score components, latency, and graph version.
- Keep full skill bodies on demand.

## Scope Fence

- Do not blindly inject graph neighbors into `compile_context`.
- Do not bloat session-start context.
- Do not hide conflict signals inside positive match scores.
- Do not remove existing `find_skill` compatibility unless explicitly migrated.

## Acceptance Criteria

- Tool output separates matches, neighbors, and conflicts.
- `show_skill(skill_id)` or equivalent returns full `SKILL.md` bodies on demand.
- Output includes enough score/rationale detail for an agent to decide what to read.
- Real-server MCP/HTTP test proves the surface works against a live graph.
- Any `crates/compiler/src/` change is explicitly justified as a measured `compile_context` change; otherwise compiler behavior stays unchanged.

## Shared / Global Notes

This is user-facing protocol work. Preserve backward compatibility where possible and keep compiler changes conservative until efficacy data says automatic injection should change.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: give agents SkillDAG-style agency over retrieval while keeping prompt size under control.
- Existing `find_skill` is the sharp mid-session path; this ticket should either extend it carefully or add a companion tool with a clear contract.
- Important unknown: exact MCP tool naming should minimize migration cost for existing harnesses.
- `crates/compiler/src/` is listed only for conditional measured `compile_context` integration. It is not permission to silently inject graph neighbors.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/reference/retrieval-contract.md`
- `/tmp/SkillDAG/skills/skilldag-retriever/SKILL.md` if the local research checkout remains available
- https://github.com/Ericbai06/SkillDAG

## Coupling Notes

Depends on T04 scores and T05 edge semantics. Any compiler/context injection change should be explicit and measured, not incidental.
