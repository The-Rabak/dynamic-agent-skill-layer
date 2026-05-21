---
name: workflows-constitution
description: Create or update the project constitution for downstream workflows
---

## Arguments
[optional: project standards, constraints, or amendment goal]

# Create or Update a Project Constitution

Create or update the repo-level constitution artifact that governs `ideate -> brainstorm -> plan -> work -> review`.

This command writes a **living project artifact** at `docs/constitution.md`. It is not a feature brief, not a brainstorm, and not a plan. It defines the durable repo-wide rules that downstream workflows must honor.

Do **not** implement product code in this workflow. Research, ask questions, synthesize principles, and write or amend the constitution only.

## Interaction Method

Use the platform's blocking question tool when available (`AskUserQuestion` in Claude Code, `ask_user` in Copilot CLI, or the equivalent blocking question tool on other platforms). Otherwise, present numbered options in chat and wait for the user's reply before proceeding.

Ask **one question at a time**. Prefer concise single-select choices with a recommended option when natural options exist. Ask at most **5 high-signal clarification questions** unless the user explicitly asks to go deeper.

## Goal

<constitution_goal> #$ARGUMENTS </constitution_goal>

If the goal is empty, proceed with open-ended constitution discovery for the current repository.

## Artifact Contract

This workflow owns `docs/constitution.md`, a versioned, durable project artifact that captures:

- repo purpose and scope boundaries
- non-negotiable engineering principles
- agent execution and portability rules
- phase guardrails for plan, work, and review
- amendment governance

Downstream feature artifacts may refine how a feature fits within the constitution, but they must not silently override the constitution.

## Execution Flow

### Phase 0: Detect Existing Constitution

Look for an existing constitution at `docs/constitution.md`.

**If found:**

1. Read it completely.
2. Determine whether the user wants to:
   - review and reaffirm it
   - amend it
   - replace it
3. Ask only the delta questions needed for the chosen change.
4. Preserve amendment history and versioning.

**If not found:**

Proceed with first-time constitution creation.

### Phase 1: Gather Local Grounding

Before asking the user about project rules, gather repo evidence in parallel:

1. Read local guidance:
   - `AGENTS.md`
   - `CLAUDE.md` (if present)
   - `README.md`
   - `compound-engineering.local.md` (if present)

2. Run lightweight repo research in parallel:
   - Before dispatching `repo-research-analyst` or `learnings-researcher`, use the platform's file-search tool against the bundled agent directory to look for `<agent-name>.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "<agent-name>"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.
   - Task `repo-research-analyst` to summarize project shape, recurring conventions, architecture boundaries, quality patterns, and docs conventions.
   - Task `learnings-researcher` to surface recurring lessons in `docs/solutions/` that look durable enough to become constitution material.

3. Consolidate findings into a short grounding brief:
   - **Project shape**
   - **Existing conventions**
   - **Repeated quality expectations**
   - **Repeated failure modes / learnings**

The constitution should reflect **durable project baselines**, not one-off implementation preferences.

### Phase 2: Ask High-Signal Constitution Questions

Use the blocking question tool to ask **one question at a time**. Prioritize the smallest set of questions that materially shapes repo-wide policy.

Recommended question sequence:

1. **Purpose and scope**
   - What is this repository for?
   - What is explicitly out of scope?

2. **Non-negotiable principles**
   - What rules must all work obey?
   - Examples: portable-source-first, CLI-first, security review before external writes, keep agent/user parity where relevant

3. **Quality and testing bar**
   - What must happen before work is considered done?
   - Examples: tests required, lint required, doc updates required, no silent failures

4. **Approval boundaries**
   - Which changes require explicit human approval?
   - Examples: production writes, migrations, auth changes, external integrations, scope expansions

5. **Amendment process**
   - How should this constitution evolve?
   - Who can amend it, and what review cadence or trigger should apply?

Guidelines:

- Prefer multiple choice when natural options exist.
- State inferred defaults explicitly and ask the user to confirm them.
- Convert vague answers into observable rules.
- If a rule sounds feature-specific, ask whether it belongs in the constitution or in a brainstorm/plan instead.

### Phase 3: Draft the Constitution

Synthesize the constitution using durable, testable language.

Prefer RFC-style requirement words where useful:

- **MUST**
- **MUST NOT**
- **SHOULD**
- **MAY**

Phrase rules as observable guardrails, not aspirations.

Good:

- "Plans MUST record any constitution waivers explicitly."
- "Review MUST treat unwaived constitution violations as blocking."

Weak:

- "We care about quality."
- "Agents should probably ask questions."

### Phase 4: Validate the Draft

Present a concise summary of the proposed constitution to the user and ask for confirmation before writing:

- repo purpose and boundaries
- top principles
- quality and approval baselines
- downstream workflow guardrails
- amendment process

If the user changes a principle, update the draft before writing the file.

### Phase 5: Write `docs/constitution.md`

Ensure `docs/` exists, then write or update `docs/constitution.md`.

This file **should be committed**. Do not suggest adding it to `.gitignore`.

If updating an existing constitution:

- preserve the amendment log
- update `last_amended`
- bump version according to the scope of the change:
  - **MAJOR** -- principle removed or redefined
  - **MINOR** -- new principle or section
  - **PATCH** -- clarification only

Validate that all handoff frontmatter flags are `true`.

### Phase 6: Explain Downstream Impact and Handoff

After writing the constitution, summarize what downstream workflows must now do:

- **`/workflows-ideate`** -- score ideas for constitution fit and avoid ideas that conflict with repo baselines unless they are framed as explicit amendment candidates
- **`/workflows-brainstorm`** -- read the constitution before exploring approaches, and record any proposed amendment instead of silently drifting
- **`/workflows-plan`** -- record `constitution_version` and any `constitution_waivers`, then translate relevant rules into acceptance criteria and approvals
- **`/workflows-work`** -- inject constitution guardrails into execution prompts and stop for approval when the constitution requires it
- **`/workflows-review`** -- treat unwaived constitution violations as blocking

Then offer next steps:

1. Proceed to `/workflows-ideate`
2. Proceed to `/workflows-brainstorm`
3. Proceed to `/workflows-plan`
4. Done for now

## Constitution Template

Write `docs/constitution.md` using this structure:

```markdown
---
artifact: project-constitution
status: active
version: 1.0.0
ratified: YYYY-MM-DD
last_amended: YYYY-MM-DD
owners:
  - [team-or-maintainer]
review_cycle: [monthly|quarterly|on-major-changes]
applies_to:
  - ideate
  - brainstorm
  - plan
  - work
  - review
handoff:
  purpose: true
  principles: true
  phase_guardrails: true
  agent_rules: true
  amendment_process: true
---

# Project Constitution

## Purpose

[What this repository exists to do]

## Scope Boundaries

- **In scope:** [...]
- **Out of scope:** [...]

## Core Principles

### 1. [Principle name]

- MUST / SHOULD / MAY rules
- Rationale

### 2. [Principle name]

...

## Agent Execution Rules

- Question-asking expectations
- Portability requirements
- Traceability requirements
- Completion / blocked-state reporting expectations

## Phase Guardrails

### Ideation Guardrails
- What ideate must honor

### Brainstorm Guardrails
- What brainstorm must honor

### Planning Guardrails
- What plan must honor

### Execution Guardrails
- What work must honor

### Review Guardrails
- What review must honor

## Allowed Exceptions

- How waivers are recorded
- What requires explicit approval

## Amendment Process

- Who can propose changes
- How amendments are reviewed
- Version bump policy

## Amendment Log

- v1.0.0 - Initial ratification
```

## Important Rules

- The constitution is **project-level**, not per-feature.
- Brainstorms and plans may propose amendments, but they do not ratify them by themselves.
- If the same waiver or review finding recurs more than once, recommend revisiting the constitution.
- Keep the constitution concise enough to be read every time a new plan starts.

## Success Criteria

- [ ] `docs/constitution.md` exists and is complete
- [ ] Purpose, principles, agent rules, guardrails, exceptions, and amendment process are all present
- [ ] The constitution uses durable, observable language
- [ ] Downstream workflow expectations are explicit
- [ ] Versioning and amendment handling are defined
