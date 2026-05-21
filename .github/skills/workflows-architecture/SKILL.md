---
name: workflows-architecture
description: Produce a dedicated architecture improvement artifact between planning and deepening, using deletion tests, interfaces, seams, and adapters as the shared contract
---

## Arguments
[path to plan file]

# Create an architecture improvement artifact

This phase sits **after** `/workflows-plan` and **before** `/deepen-plan`.

Its job is to turn the plan's architectural context into a **consumable artifact** that downstream phases can read instead of relying on hidden oral tradition. The output is not commentary. It is a concrete architecture improvement document that names the deepening candidates, deletion-test results, interfaces, seams, adapters, and contracts that should guide deeper execution hardening.

**Process knowledge:** Load the `agent-native-architecture` skill for design heuristics around deletion tests, interface-as-test-surface thinking, seams, adapters, and contracts.

## Plan File

<plan_path> #$ARGUMENTS </plan_path>

**If the plan path above is empty:**
1. Check for recent plans: `ls -t docs/plans/*-plan*.md 2>/dev/null | head -5`
2. Ask the user: "Which plan should I improve architecturally? Please provide the path (for example `docs/plans/2026-01-15-feat-my-feature-plan.md`)."

Do not proceed until you have a valid plan file path.

## Required inputs

The architecture phase requires these inputs from the plan or linked artifacts:

- **Problem Narrative** -- why this work exists
- **User Story** -- who needs what outcome
- **Success Criteria** -- what must be true for the work to be done
- **Architectural Context** -- where the change lives and what it touches
- **Implementation phases/tasks** -- the proposed execution shape
- **`brainstorm_ref` / constitution alignment / waivers / source docs** -- when present

Use `references/execution-shape.md` to interpret the plan's packet structure before deciding whether a boundary belongs in a feature home, shared/global scope, or a different execution mode entirely.

If any required WHY or WHERE inputs are missing, stop and tell the user exactly what is missing. Do not invent architectural context.

## Required reference contract

Before drafting the artifact, load both of these references from `commands/workflows/references/` (or the generated platform-local equivalent):

- `references/architecture-improvement-prompt.md`
- `references/vertical-slice-architecture.md`

Follow this protocol:
1. Use the platform's file-search tool against the local `references/` directory bundled with this skill to look for both files.
2. Use the file-read tool to load both files in full.
3. Before continuing, quote the first non-empty line of each loaded reference and record which files you used.
4. If you cannot load and quote both references, stop and report the missing template instead of improvising.

Use those references as the **mandatory artifact contract**.

## Mandatory architecture reviewers

Before finalizing the artifact, apply the shared `Named Agent Dispatch` protocol from `references/orchestration-protocol.md`.

Always run these reviewers regardless of repo config:

- `architecture-strategist` -- pressure-test dependency direction, boundary ownership, seams, adapters, and contract shape.
- `uncle-bob` -- pressure-test naming, responsibility slicing, feature-home ownership, shared/global extractions, side-effect visibility, local reasoning, and whether the proposed structure stays easy to change.

Pass both reviewers the plan's WHY/WHERE context plus the current architecture notes or draft artifact content. Fold their findings into the final artifact instead of treating them as optional commentary.

If either mandatory reviewer cannot be loaded and quoted, stop and report the architecture phase as incomplete rather than silently proceeding with reduced scrutiny.

## Workflow

### 1. Read the plan and linked context

Read the plan file and extract:
- Problem Narrative
- User Story
- Success Criteria
- Architectural Context
- Key Decisions / Approaches Considered (when present)
- Task list and dependencies
- Current or likely feature homes
- `brainstorm_ref`, `constitution_version`, `constitution_waivers`, `source_docs`, and any existing `architecture_ref`

If `brainstorm_ref` exists, read it for stakeholder impact, resolved questions, and architectural context that should not be lost.

If an existing `architecture_ref` already exists, read it first and decide whether to update it in place or replace it with a newer artifact. Do not create duplicate artifacts without explaining why.

### 2. Run the architecture improvement pass

Use the reference contract to produce explicit architectural guidance:

1. **Name the deepening candidates** -- which parts of the plan need deeper architectural treatment before execution hardening.
2. **Run the deletion test** -- what can be removed, avoided, or kept concrete before adding a new interface, seam, or adapter.
3. **Define interfaces as test surfaces** -- what behavior downstream tests and callers should rely on.
4. **Map seams, adapters, and contracts** -- where the system should flex, what translates external concerns, and what promises must stay stable.
5. **Use design-it-twice only where leverage is high** -- compare at least two structural options for risky boundaries, not for every detail.
6. **Confirm feature homes and shared/global ownership** -- name what belongs in feature-local scope versus true shared/global scope so DRY and SOLID stay intact.
7. **Split post-plan context tiers** -- make global, on-demand, and ticket-local context explicit so later ticketization and execution stay smaller.
8. **Translate findings into downstream guidance** -- what `/deepen-plan`, `/workflows-work`, and `/workflows-review` should preserve or verify.

If you cannot explain a proposed abstraction in terms of deletion test, interface, seam, or adapter, it is not ready to include.

### 2.5 Run mandatory architecture reviewers

Dispatch `architecture-strategist` and `uncle-bob` using the protocol above.

- `architecture-strategist` should challenge structural choices, coupling, and boundary integrity.
- `uncle-bob` should challenge readability, responsibility boundaries, naming, side effects, long-term changeability, and whether the feature-home versus shared/global split is actually honest.

Resolve meaningful conflicts between the two perspectives explicitly in the artifact instead of picking one silently.

### 3. Write the artifact

Write the architecture artifact to:

```text
docs/architecture/YYYY-MM-DD-<topic>-architecture.md
```

Ensure `docs/architecture/` exists before writing.

After writing the artifact:
1. Add or update `architecture_ref: <artifact path>` in the plan frontmatter when possible.
2. If frontmatter cannot be safely updated, add a clearly labeled `## Related Artifacts` section to the plan with the artifact path.
3. Do not silently move the artifact elsewhere.

## Required outputs

A complete run must leave behind all of the following:

- **Architecture artifact** in `docs/architecture/`
- **Explicit artifact path** recorded back into the plan via `architecture_ref` or `## Related Artifacts`
- **Feature-home ownership decisions** for the main slices or modules
- **Shared/global boundary decisions** that keep DRY and SOLID honest
- **Context tiers** that separate global, on-demand, and ticket-local context
- **Deepening candidates** the next phase can act on
- **Deletion-test decisions** that justify which abstractions stay or go
- **Interface / seam / adapter / contract guidance** stated in plain language
- **Clear next step**: run `/deepen-plan` with the updated plan

## Handoff

When complete, summarize:

```text
Architecture improvement complete!

Plan: <plan_path>
Artifact: docs/architecture/YYYY-MM-DD-<topic>-architecture.md

Key deepening candidates:
- <candidate 1>
- <candidate 2>

Deletion test:
- Keep: <what stays concrete>
- Add later only if needed: <what failed the deletion test>

Next: Run `/deepen-plan <plan_path>` so execution hardening uses this architecture artifact.
```

NEVER CODE! This phase produces architecture guidance and artifact contracts, not implementation changes.
