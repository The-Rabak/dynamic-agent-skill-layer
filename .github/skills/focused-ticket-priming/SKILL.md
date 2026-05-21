---
name: focused-ticket-priming
description: Package one plan execution packet into a compact ticket-local execution packet with parent refs, scope fences, feature-home ownership, and evidence commands. Use when converting plans into local tickets or when execution needs one ticket-sized context pack without the full plan.
model: gpt-5.3-codex
---

# Focused Ticket Priming

Turn one execution packet into one execution-ready ticket without dragging the whole plan into the ticket body.

## Required contract

Before packaging a ticket, load `references/ticket-execution-contract.md` (or the platform-local equivalent), quote its first non-empty line, and use it as the output contract.

If the contract cannot be loaded, stop instead of inventing a ticket shape.

## Workflow

1. Read the source packet, the plan WHY artifacts, the architecture handoff, and the resolved TDD/evidence contract.
2. Keep the three context tiers separate:
   - global guardrails stay global
   - architecture and supporting docs stay on-demand
   - only the smallest safe execution packet goes into the ticket
3. Write the ticket using the exact frontmatter and section order from `ticket-execution-contract.md`.
4. Preserve feature-home ownership, scope fence, parent refs, dependency type, and evidence command exactly.
5. If the source packet is too vague to produce a compact ticket without guessing, stop and send the workflow back to `/deepen-plan`.

## Compactness rules

- Keep the `## Local Context` section short enough for one focused execution run.
- Embed only the files, interfaces, contracts, and WHY details that matter for this ticket.
- Link deeper design context through `## Deeper-Dive Refs` instead of copying it.
- Surface visible uncertainty instead of pretending the packet is settled.
- Size by coupling and boundary clarity, not by file-count symmetry.

## Ticket quality bar

A primed ticket is acceptable only when it:

- delivers one honest outcome
- preserves the source packet's WHY tracing
- keeps business logic in the declared feature home unless the architecture handoff justifies shared/global extraction
- includes a real validation command
- gives `/workflows-work` enough local context to execute one ticket safely

## Guardrails

- Do not expand a thin packet into a mini-plan.
- Do not remove parent refs just because the local context feels sufficient.
- Do not hide blockers or split dependencies across vague prose.
- Do not merge multiple outcomes into one ticket merely to reduce file count.
