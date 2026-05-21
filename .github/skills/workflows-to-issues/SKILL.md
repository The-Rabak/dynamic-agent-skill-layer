---
name: workflows-to-issues
description: Convert a plan into local vertical-slice ticket artifacts with scoped execution context
---

## Arguments
[path to plan file] [optional: architecture artifact path]

# Ticketize a Plan into Local Execution Artifacts

This phase sits after `/workflows-plan` or `/deepen-plan` and before ticket-scoped execution. Its job is to convert a strategic plan into a **local kanban-style ticket set** with enough context for one focused execution run at a time.

The command is named `/workflows-to-issues` for continuity with the broader plan-to-issue pattern, but **v1 writes local artifacts only**. It does not publish tracker issues automatically.

## Plan File

<plan_path> #$ARGUMENTS </plan_path>

If the plan path above is empty:
1. Check for recent plans: `ls -t docs/plans/*-plan*.md 2>/dev/null | head -5`
2. Ask the user which plan should be ticketized.

Do not proceed until you have a valid plan path.

## Required reference contract

Before drafting tickets, load all of these references from `commands/workflows/references/` (or the generated platform-local equivalent):

- `ticketization-contract.md`
- `ticket-execution-contract.md`
- `execution-shape.md`
- `vertical-slice-architecture.md`
- `orchestration-protocol.md`

Follow this protocol:
1. Use the platform's file-search tool against the local `references/` directory bundled with this skill to find all five files.
2. Use the file-read tool to load all five files in full.
3. Before continuing, quote the first non-empty line of each reference and record which files you used.
4. If any reference cannot be loaded and quoted, stop and report the missing template instead of improvising.

Then locate `skills/focused-ticket-priming/SKILL.md` (or the generated platform-local equivalent), load it in full, quote its first non-empty line, and use it as the mandatory packet-packaging skill for every ticket candidate.

If the skill cannot be loaded and quoted, stop instead of inventing a packaging workflow.

Use those references and the skill as the mandatory contract for ticket generation.

## Required inputs

The ticketization phase requires:

- Problem Narrative
- User Story
- Success Criteria
- Architectural Context
- explicit `execution_shape`
- execution packets for that shape
- `tdd` frontmatter and `## TDD & Evidence Contract`

Prefer a real `architecture_ref`. If no architecture artifact exists, build an explicit architecture handoff contract from the plan and continue only when the feature-home and shared/global boundaries are still clear enough to avoid guesswork.

## Workflow

### 1. Read the plan and architecture context

Read the plan and extract:

- plan WHY artifacts
- `execution_shape` and execution packets
- `brainstorm_ref`, `architecture_ref`, `constitution_version`, `constitution_waivers`, and `source_docs` when present
- feature-home ownership and shared/global decisions from the architecture artifact or explicit handoff

If the selected `execution_shape` is missing or the packets are too vague to map cleanly into tickets, stop and send the user back to `/deepen-plan` instead of inventing ticket boundaries.

### 2. Convert execution packets into ticket candidates

Transform the plan's execution packets into local ticket candidates using the shared ticketization contract, the focused ticket-priming skill, and the ticket execution contract.

Rules:

- keep the selected execution shape honest
- preserve the plan's WHY tracing
- preserve the architecture handoff
- keep tickets small enough for one focused execution run
- size by coupling and boundary clarity, not by arbitrary task counts
- keep tracer bullets first when the mode is `vertical-slices`
- surface uncertainty instead of hiding it when ticketizing directly after `/workflows-plan`

Each ticket must include the required ticket-local context defined in `ticketization-contract.md`, and each ticket file must follow the exact frontmatter/body shape in `ticket-execution-contract.md`.

### 3. Package context without re-bloating it

Use the `focused-ticket-priming` skill on each execution packet. The skill is responsible for shaping the compact ticket packet.

Split context across the three tiers:

- **Global context** -- always-needed repo guardrails
- **On-demand context** -- architecture artifact and deeper supporting docs
- **Ticket-local context** -- the compact packet embedded directly in each ticket

Do not copy the entire plan into every ticket. Keep the packet compact, then link back to plan and architecture refs for deeper dives.

### 4. Write the local ticket set

Write the ticket set to:

```text
docs/tickets/YYYY-MM-DD-<topic>/
```

Required files:

- `index.md`
- one `NN-<ticket-slug>.md` file per ticket

Write every ticket using the exact schema from `ticket-execution-contract.md`, including its required frontmatter, section order, status lifecycle, and parent refs.

Then record `tickets_ref` back into the plan frontmatter when possible. If frontmatter cannot be updated safely, add the ticket-set path under `## Related Artifacts`.

### 5. Run the final ticket-set review sweep

Review the entire ticket set before finishing.

Apply the shared `Named Agent Dispatch` protocol from `orchestration-protocol.md` before dispatching `ticket-flow-auditor`.

Dispatch `ticket-flow-auditor` against:

- the plan WHY artifacts
- the architecture artifact or explicit handoff
- the generated `index.md`
- every generated ticket file
- the loaded `ticketization-contract.md` and `ticket-execution-contract.md`

Check for:

- feature-home drift
- shared/global drift
- missing blockers or bad dependency ordering
- oversized tickets
- tickets with weak WHY tracing
- missing acceptance criteria or evidence commands
- tickets whose local context is still too thin or too large

Require the reviewer to report findings in two buckets:

- **Blocking gaps**
- **Recommendations**

If blocking gaps remain, do not present the ticket set as execution-ready.

## Required outputs

A complete run must leave behind:

- a local ticket set under `docs/tickets/`
- `tickets_ref` or a labeled related-artifact link back into the plan
- explicit blocker/dependency ordering
- compact ticket-local context packs
- a final ticket-set review result from `ticket-flow-auditor`

## Handoff

When complete, summarize:

```text
Ticketization complete!

Plan: <plan_path>
Ticket set: docs/tickets/YYYY-MM-DD-<topic>/index.md

Execution readiness:
- Blocking gaps: <count>
- Recommendations: <count>

Recommended next step:
- Run `/workflows-work` on one ticket file once ticket-scoped execution is supported, or use the generated tickets as the scoped execution packet source for the next implementation pass.
```

NEVER CODE! This phase shapes execution artifacts and context packets. It does not implement the feature itself.
