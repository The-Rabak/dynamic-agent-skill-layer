---
name: workflows-brainstorm
description: Explore the WHY and WHAT of a feature through collaborative dialogue, producing a user story, architectural context, and design decisions that anchor all downstream phases
---

## Arguments
[feature idea or problem to explore]

# Brainstorm a Feature or Improvement

**Note: The current year is 2026.** Use this when dating brainstorm documents.

Brainstorming answers **WHY** we're building something and **WHAT** it is. It produces the lynchpin artifacts -- a problem narrative, user story, and architectural context -- that thread through every downstream phase (`/workflows-plan` -> `/workflows-architecture` -> `/deepen-plan` -> `/workflows-work` -> `/workflows-review`).

When `docs/constitution.md` exists, brainstorming must work **within** that project-level constitution. The brainstorm can propose a constitution amendment when a durable repo-wide rule needs to change, but it must not silently override project baselines.

Without a clear WHY, plans decompose into disconnected tasks, execution agents lose sight of purpose, and reviews evaluate code in a vacuum.

**Process knowledge:** Load the `brainstorming` skill for detailed question techniques, approach exploration patterns, user story construction, and architectural context mapping.

## Feature Description

<feature_description> #$ARGUMENTS </feature_description>

**If the feature description above is empty, ask the user:** "What would you like to explore? Please describe the feature, problem, or improvement you're thinking about."

Do not proceed until you have a feature description from the user.

## Execution Flow

### Phase 0: Assess Requirements Clarity

Evaluate whether brainstorming is needed based on the feature description.

**Clear requirements indicators:**
- Specific acceptance criteria provided
- Referenced existing patterns to follow
- Described exact expected behavior
- Constrained, well-defined scope

**If requirements are already clear:**
Use **AskUserQuestion tool** to suggest: "Your requirements seem detailed enough to proceed directly to planning. Should I run `/workflows-plan` instead, or would you like to explore the idea further?"

### Phase 1: Understand the Problem and the People

#### 1.1 Repository Research (Lightweight)

Run a quick repo scan to understand existing patterns, system topology, and any existing constitution:

Before spawning `repo-research-analyst`, use the platform's file-search tool against the bundled agent directory to look for `repo-research-analyst.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "repo-research-analyst"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.

- Use the repo-research-analyst skill to: "Understand existing patterns, system architecture, and component boundaries related to: <feature_description>. Report: (1 similar features and their structure, (2) services/modules this would touch or neighbor, (3) CLAUDE.md guidance, (4) data flow relevant to this area, (5) whether docs/constitution.md exists and which repo-wide principles or boundaries matter here.")

Focus on: similar features, established patterns, CLAUDE.md guidance, project constitution rules (if present), **and system boundaries this feature would interact with**.

#### 1.2 Collaborative Dialogue -- The WHY Layer

Use the **AskUserQuestion tool** to ask questions **one at a time**.

This phase has two goals: understand the **problem space** (why this matters) and the **user space** (who needs this and how they experience the problem today).

**Question progression (see `brainstorming` skill for detailed techniques):**

1. **The Problem** -- What pain or gap exists today? What triggers it? How are people working around it now?
2. **The People** -- Who experiences this problem? What's their context, role, skill level? Are there multiple personas?
3. **The Stakes** -- What happens if we don't solve this? What's the cost of the status quo? What's the business or user impact?
4. **Success Vision** -- What does "solved" look like from the user's perspective? How will they know it's working? What measurable outcome matters?
5. **Boundaries** -- What's explicitly out of scope? What adjacent problems should we be aware of but not solve now?
6. **Constraints** -- Technical limitations, timeline, dependencies, compliance requirements?

**Guidelines:**
- Prefer multiple choice when natural options exist
- Validate assumptions explicitly -- "I'm assuming X, is that right?"
- Don't move on from the WHY too quickly. The tendency is to jump to solutions. Resist it.

**Exit condition:** Continue until you can articulate the problem, the people, and the desired outcome clearly -- OR user says "proceed."

#### 1.3 Synthesize Problem & User Story

**This is the orchestrator's job -- do not delegate this to a subagent.**

After the collaborative dialogue, synthesize everything learned into two artifacts. Present them to the user for validation before proceeding.

**Problem Narrative** (2-4 sentences):
State the problem clearly: who has it, what triggers it, what the impact is, and why now is the right time to solve it. This is the WHY that every downstream phase traces back to.

**User Story** (structured):
```
As a [persona with relevant context],
I need to [action/capability]
so that [desired outcome/value],
because currently [pain point or gap]
which causes [concrete negative impact].
```

The user story is NOT a template checkbox. It's the north star. If a plan phase can't trace back to this story, it shouldn't exist. If a review finding doesn't connect to this story, its priority should be reassessed.

**Present both to the user** with AskUserQuestion: "Here's my understanding of the problem and user story. Does this capture the core of what we're solving?"

Iterate until the user confirms. This is the most important step in the entire brainstorm.

### Phase 2: Explore Approaches

Propose **2-3 concrete approaches** based on research and conversation.

**Critically: evaluate each approach against the problem narrative and user story, not in a vacuum.**

For each approach, provide:
- Brief description (2-3 sentences)
- How it addresses the user story (does it fully solve the problem, or partially?)
- Pros and cons
- When it's best suited
- Risks or trade-offs specific to the user's context

Lead with your recommendation and explain why **in terms of the user story and success criteria**. Apply YAGNI to implementation scope -- prefer simpler solutions -- but don't use YAGNI as an excuse to ignore understanding. You can understand broadly and build narrowly.

Use **AskUserQuestion tool** to ask which approach the user prefers.

### Phase 2.5: Architectural Context

**This is the orchestrator's job -- do not delegate this to a subagent.**

After the approach is chosen, construct a lightweight architectural context map. This is NOT implementation design (no file paths, no class names, no code). This is **structural understanding** -- where this feature lives in the system.

Use `references/vertical-slice-architecture.md` as the source of truth for feature-home thinking and shared/global boundary language.

**Architectural Context Map:**

1. **System Placement** -- Which service/module/layer does this feature live in? What's its feature home?
2. **Boundary Interactions** -- What other services, modules, or systems does it talk to? What data flows in and out?
3. **User Touchpoints** -- How does the user interact with this? What's the entry point (UI, API, CLI, event)?
4. **Data Considerations** -- What data does this feature create, read, update, or delete? Where does that data live?
5. **Dependency Direction** -- What does this feature depend on? What might depend on it in the future?
6. **Shared / Global Boundaries** -- Which parts look feature-local versus truly shared across multiple features?

**Output format:** A concise prose description (not a diagram). Think of it as explaining to a new team member where this feature sits and what it touches. 5-10 sentences max.

**Present to the user** with AskUserQuestion: "Here's where I see this feature sitting in the system. Does this match your understanding?"

Iterate until confirmed. This context map flows into every execution agent's `{{ARCHITECTURAL_CONTEXT}}` and every reviewer's evaluation frame.

### Phase 3: Capture the Design

Write a brainstorm document to `docs/brainstorms/YYYY-MM-DD-<topic>-brainstorm.md`.

Ensure `docs/brainstorms/` directory exists before writing.

**Document structure (mandatory sections for downstream handoff):**

```markdown
---
date: YYYY-MM-DD
topic: <kebab-case-topic>
status: complete
handoff:
  problem_narrative: true
  user_story: true
  architectural_context: true
  success_criteria: true
---

# <Topic Title>

## Problem Narrative

[The synthesized problem statement from Phase 1.3. WHY we're building this.
2-4 sentences: who has the problem, what triggers it, what the impact is.]

## User Story

[The structured user story from Phase 1.3. The north star for all downstream work.]

As a [persona],
I need to [action]
so that [outcome],
because currently [pain point]
which causes [impact].

## Success Criteria

[How we'll know this is working. Tied to the user story, not just technical correctness.]

- [Measurable outcome 1 -- linked to the user story's "so that"]
- [Measurable outcome 2]
- [Observable behavior that proves the problem is solved]

## Architectural Context

[The structural context map from Phase 2.5. WHERE this lives in the system.]

- **Lives in:** [service/module/layer]
- **Feature home:** [primary feature namespace or module home]
- **Interacts with:** [neighboring systems/modules]
- **User entry point:** [UI/API/CLI/event]
- **Data:** [what data flows, where it lives]
- **Dependencies:** [what this depends on, what may depend on it]
- **Shared / global notes:** [what should stay shared instead of being absorbed into the feature home]

## Chosen Approach

[Description of the selected approach and WHY it was chosen -- traced back to the
user story and problem narrative. Not just "it's simpler" but "it's simpler AND
it fully addresses the user's need because..."]

## Key Decisions

- [Decision 1]: [Rationale tied to problem/user story]
- [Decision 2]: [Rationale tied to problem/user story]

## Constitution Alignment

- **Relevant project rules:** [Which constitution principles or baselines matter here]
- **No amendment needed because:** [Why the feature fits existing rules]
- **Proposed amendment (if any):** [Only when this feature exposes a durable gap in the constitution]

## Approaches Considered

[Brief summary of alternatives and why they were not chosen, relative to the
user story and success criteria.]

## Stakeholder Impact

[Who is affected by this change and how. Populated from Phase 1.2 dialogue.]

- **End users:** [How their experience changes]
- **Developers:** [How this affects the codebase, patterns, maintenance]
- **Operations:** [Deployment, monitoring, infrastructure impact]
- **Business:** [Revenue, cost, compliance, timeline impact]

## Open Questions

- [Any unresolved questions for the planning phase]

## Resolved Questions

- [Questions that were resolved during brainstorming, with answers]
```

**IMPORTANT:** Before proceeding to Phase 4, check if there are any Open Questions listed in the brainstorm document. If there are open questions, YOU MUST ask the user about each one using AskUserQuestion before offering to proceed to planning. Move resolved questions to the "Resolved Questions" section.

**Validation:** Check that all `handoff` frontmatter fields are `true`. If any section is empty or missing, go back and fill it. The downstream phases depend on this contract.

### Phase 4: Handoff

Use **AskUserQuestion tool** to present next steps:

**Question:** "Brainstorm captured with problem narrative, user story, architectural context, and design decisions. What would you like to do next?"

**Options:**
1. **Review and refine** - Improve the document through structured self-review
2. **Proceed to planning** - Run `/workflows-plan` (will auto-detect this brainstorm, then route through `/workflows-architecture` before `/deepen-plan`)
3. **Ask more questions** - I have more questions to clarify before moving on
4. **Done for now** - Return later

**If user selects "Ask more questions":** Return to Phase 1.2 (Collaborative Dialogue) and continue probing deeper -- edge cases, constraints, preferences, or areas not yet explored. After new information emerges, re-synthesize the Problem Narrative and User Story (Phase 1.3) if they need updating. Continue until the user is satisfied, then return to Phase 4.

**If user selects "Review and refine":**

Load the `document-review` skill and apply it to the brainstorm document.

When document-review returns "Review complete", present next steps:

1. **Move to planning** - Continue to `/workflows-plan` with this document, then use `/workflows-architecture` as the supported next handoff before deepening
2. **Done for now** - Brainstorming complete. To start planning later: `/workflows-plan [document-path]`

## Output Summary

When complete, display:

```
Brainstorm complete!

Document: docs/brainstorms/YYYY-MM-DD-<topic>-brainstorm.md

Problem: [One-sentence problem narrative]
User Story: As a [persona], I need to [action] so that [outcome]
Architecture: Lives in [module], interacts with [neighbors]

Key decisions:
- [Decision 1]
- [Decision 2]

Next: Run `/workflows-plan`, then `/workflows-architecture` before
`/deepen-plan` or `/workflows-work`.
The plan will use this brainstorm's user story and architectural context
as its foundation, and the architecture phase will turn that into an
explicit downstream handoff.
```

## Important Guidelines

- **WHY first, WHAT second, never HOW** -- Implementation details belong in the plan. But the problem, the people, and the architectural context belong here.
- **Ask one question at a time** -- Don't overwhelm
- **YAGNI applies to building, not understanding** -- Explore the full problem space to make informed decisions. Build only what's needed now.
- **Keep outputs concise** -- 200-300 words per section max
- **The brainstorm document is a feature contract, not the constitution** -- Downstream phases (plan, work, review) will parse and reference its sections. The problem narrative, user story, and architectural context are not optional prose -- they're structured artifacts.

## Downstream Phase Integration

This brainstorm document is consumed alongside the project constitution:

- **`/workflows-plan`** -- Reads user story and architectural context to structure phases, while also checking constitution rules and recording any waivers or amendment proposals.
- **`/deepen-plan`** -- Uses problem narrative and success criteria to evaluate whether deepened tasks still serve the original intent.
- **`/workflows-work`** -- Feeds architectural context into each execution agent's `{{ARCHITECTURAL_CONTEXT}}` block via the canonical execution-agent template, while constitution rules act as guardrails for implementation and approvals.
- **`/workflows-review`** -- Uses problem narrative, user story, success criteria, and constitution baselines as the frame for evaluating whether the implementation actually solves the stated problem without policy drift. Named review agents are coordinated there, not dispatched ad hoc from other workflows.

NEVER CODE! Just explore, understand, and document the WHY, WHAT, and WHERE.
