---
name: brownfield-maintenance
description: Audit and fill brownfield AI-layer gaps on demand outside the main feature workflow
---

## Arguments
[repo area, workflow surface, or AI-layer problem statement]

# Brownfield AI-Layer Maintenance

Use this command when a repository already exists and the **AI layer** around it has drifted, stayed implicit, or never got properly established. This command runs **outside the canonical feature workflow** as an on-demand maintenance pass for brownfield repos.

The goal is to make an inherited or drifting repo easier for future agents to reason about by tightening the surrounding workflow surfaces:
- docs and handoff artifacts
- command / skill / agent prompts
- reviewer coverage
- vertical-slice boundaries
- generated surface consistency

## Scope

<maintenance_scope> #$ARGUMENTS </maintenance_scope>

If the scope is empty, default to the repository's overall AI layer and say so explicitly before continuing.

## Outcomes

A complete run should leave behind:

1. a clear map of the current AI layer
2. a prioritized list of gaps
3. direct fixes for safe, local gaps when they are obvious
4. a durable maintenance artifact at `docs/solutions/YYYY-MM-DD-<topic>-brownfield-ai-layer.md`
5. a clear recommendation for what command should run next, if any

## Operating mode

Start in **audit-first** mode.

- If the gaps are local, clear, and low-risk (docs drift, prompt drift, missing references, reviewer coverage gaps, stale generated outputs), fix them in this run.
- If the gaps imply broader product, architecture, or execution changes, stop at a maintenance artifact plus a prioritized backlog instead of making speculative edits.
- If a proposed fill-gap step would change product behavior, data boundaries, or repo-wide rules, ask the user before applying it.

## Phase 1: Map the current AI layer

Read the repo surfaces that shape how agents understand and modify the project:

- `AGENTS.md`, `CLAUDE.md`, and `README.md` when present
- `docs/constitution.md` when present
- local config such as `compound-engineering.local.md`
- recent `docs/brainstorms/`, `docs/plans/`, `docs/architecture/`, and `docs/solutions/` artifacts when relevant
- the relevant command, skill, and agent directories for the scoped area

Focus on whether the repo currently has:

- explicit WHY artifacts
- an architecture handoff or equivalent boundary contract
- vertical-slice feature homes with honest shared/global boundaries
- execution packets small enough for focused agent work
- review coverage aligned with the repo's actual risk
- docs and generated outputs that still match the portable source

## Phase 2: Research existing patterns and prior learnings

Before dispatching named agents, apply the shared `Named Agent Dispatch` protocol from `references/orchestration-protocol.md`.

Run these in parallel:

- `repo-research-analyst` -- map the current repo topology, conventions, hotspots, and workflow surfaces related to the scoped brownfield area
- `learnings-researcher` -- search `docs/solutions/` for prior fixes, recurring workflow gaps, and previous maintenance patterns that fit this scope

Pass both agents:

- the maintenance scope
- the relevant workflow surfaces already read
- a request to identify **missing, stale, contradictory, or oversized AI-layer context**

## Phase 3: Audit the brownfield gaps

Build a gap list under these headings:

### 1. WHY and handoff gaps
- missing or stale brainstorm / plan / architecture artifacts
- hidden intent that forces agents to guess

### 2. Boundary gaps
- missing feature-home ownership
- business logic dumped into horizontal shared directories
- shared/global abstractions duplicated across feature slices
- missing explanation for what stays shared versus local

### 3. Execution-context gaps
- plans or specs that are too large for focused execution
- missing ticket-sized context packets
- weak or missing validation / evidence commands

### 4. Reviewer gaps
- missing specialists for the actual risk profile
- reviewer prompts that do not enforce the feature-home / shared-global boundary
- stale review instructions that no longer match the workflow

### 5. Surface drift
- portable source and generated outputs no longer aligned
- docs, command tables, or counts that contradict the actual shipped surfaces

For each gap, record:

- **Symptom**
- **Why it hurts agent reasoning or maintenance**
- **Fix now vs plan later**
- **Smallest credible fix**

## Phase 4: Fill safe gaps

If a gap is local and high-confidence, fix it directly.

Good examples:
- missing prompt references
- stale workflow wording
- reviewer instructions that no longer match the architecture contract
- doc tables, counts, and changelog entries
- generated outputs that just need rebuilding

Do **not** silently fix broad product or architecture uncertainty under this command. Turn that into a focused follow-up plan instead.

## Phase 5: Write the maintenance artifact

Write a durable artifact to:

```text
docs/solutions/YYYY-MM-DD-<topic>-brownfield-ai-layer.md
```

Use this structure:

```markdown
# Brownfield AI-Layer Maintenance

## Scope
- <what was audited>

## Current Surface Map
- <what docs, prompts, skills, commands, reviewers, and artifacts shape the AI layer today>

## Filled Now
- <gaps fixed in this run, or `None`>

## Plan Later
- <gaps that need a larger workflow or architecture pass>

## Recommended Next Commands
1. <best next command, or `None`>
2. <optional second step>
```

## Validation

- If you changed portable workflow surfaces, run: `bun run build:platforms && bun run verify:generated && bun test`
- If you changed a normal application repo instead, run the repo's existing validation commands
- If you only produced an audit artifact, say so explicitly instead of claiming a fill-gap run

## Final summary

Report back with:

- the audited scope
- what was fixed now
- what remains as follow-up work
- the maintenance artifact path
- the recommended next command, if one is needed
