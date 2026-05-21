---
name: ideate
description: Generate and critically evaluate grounded improvement ideas for the current project before selecting one to brainstorm
model: gpt-5.3-codex
---

# Ideate

Generate improvement ideas that are grounded in the actual repository, reject weak ones aggressively, and preserve the strongest survivors in `docs/ideation/` before handing one off to `/workflows-brainstorm`.

Use this when the user wants the AI to suggest what to improve rather than refine an idea they already chose.

- `ideate` answers: "What are the strongest ideas worth exploring?"
- `/workflows-brainstorm` answers: "What exactly should one chosen idea mean?"
- `/workflows-plan` answers: "How should it be built?"

This workflow produces a ranked ideation artifact in `docs/ideation/`. It does not produce requirements, plans, or code.

## Interaction Method

Use the platform's blocking question tool when available (`AskUserQuestion` in Claude Code, `request_user_input` in Codex, `ask_user` in Gemini). Otherwise, present numbered options in chat and wait for the user's reply before proceeding.

Ask one question at a time. Prefer concise single-select choices when natural options exist.

## Focus Hint

Interpret any provided argument as optional context. It may be:

- a concept such as `DX improvements`
- a path such as `plugins/compound-engineering/skills/`
- a constraint such as `low-complexity quick wins`
- a volume hint such as `top 3`, `100 ideas`, or `raise the bar`

If no argument is provided, proceed with open-ended ideation.

## Core Principles

1. **Ground before ideating** - Scan the actual codebase first. Do not generate abstract product advice detached from the repository.
2. **Diverge before judging** - Generate the full idea set before evaluating any individual idea.
3. **Use adversarial filtering** - The quality mechanism is explicit rejection with reasons, not optimistic ranking.
4. **Preserve the original prompt mechanism** - Generate many ideas, critique the whole list, then explain only the survivors in detail.
5. **Use agent diversity to improve the candidate pool** - Parallel sub-agents are a support mechanism for richer idea generation and critique, not the core workflow itself.
6. **Preserve the artifact early enough** - Before any handoff or session end, write the ideation document so the work survives interruptions.
7. **Route action into brainstorming** - Ideation identifies promising directions; `/workflows-brainstorm` defines the selected one precisely enough for planning.

## Execution Flow

### Phase 0: Resume and Scope

#### 0.1 Check for Recent Ideation Work

Look in `docs/ideation/` for ideation documents created within the last 30 days.

Treat a prior ideation doc as relevant when:
- the topic matches the requested focus
- the path or subsystem overlaps the requested focus
- the request is open-ended and there is an obvious recent open ideation doc
- the issue-grounded status matches: do not offer to resume a repo-wide issue-theme ideation when the current request is not issue-driven, or vice versa

If a relevant doc exists, ask whether to:
1. continue from it
2. start fresh

If continuing:
- read the document
- summarize what has already been explored
- preserve previous idea statuses and session log entries
- update the existing file instead of creating a duplicate

#### 0.2 Interpret Focus and Volume

Infer three things from the argument:

- **Focus context** - concept, path, constraint, or open-ended
- **Volume override** - any hint that changes candidate or survivor counts
- **Issue-tracker intent** - whether the user wants issue or bug data as an input source

Issue-tracker intent triggers when the argument's primary intent is about analyzing issue patterns: `bugs`, `github issues`, `open issues`, `issue patterns`, `what users are reporting`, `bug reports`, `issue themes`.

Do not trigger on arguments that merely mention bugs as a focus: `bug in auth`, `fix the login issue`, `the signup bug`.

Default volume:
- each ideation sub-agent generates about 7-8 ideas (yielding 30-40 raw ideas across agents, about 20-30 after dedupe)
- keep the top 5-7 survivors

Honor clear overrides such as:
- `top 3`
- `100 ideas`
- `go deep`
- `raise the bar`

### Phase 1: Codebase Scan

Before generating ideas, gather codebase context.

#### 1.1 Load Local Guidance

Read the local guidance files in this order before ideating:
1. `AGENTS.md`
2. `CLAUDE.md` (if present)
3. `README.md`
4. `docs/constitution.md` (if present)

Use them to preserve the repository's workflow language, OpenViking guidance, docs conventions, artifact locations, and any repo-wide principles or boundaries already ratified in the constitution.

#### 1.2 Gather Grounding in Parallel

Run these in parallel in the foreground:

**Named-agent dispatch rule:** Before dispatching any named specialist agent below (for example `learnings-researcher` or `issue-intelligence-analyst`), use the platform's file-search tool against the bundled agent directory to look for `<agent-name>.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "<agent-name>"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.

1. **Quick context scan** - dispatch a general-purpose or explore-style sub-agent to do a shallow repository scan.

   Prompt it to:
   - read `AGENTS.md`, or `CLAUDE.md` only as fallback, then `README.md` if neither exists
   - discover the top-level directory layout using native file-search tools
   - return a concise summary covering:
     - project shape (language, framework, top-level layout)
     - notable patterns or conventions
     - obvious pain points or gaps
     - likely leverage points for improvement

   Keep the scan shallow. Do not do deep code search yet.

2. **Learnings search** - dispatch `learnings-researcher` with a brief summary of the ideation focus so recent `docs/solutions/` learnings can shape the candidate pool.

3. **Issue intelligence** (conditional) - if issue-tracker intent was detected, dispatch `issue-intelligence-analyst` with the focus hint.
   - If it errors because GitHub tooling or auth is unavailable, log a warning and continue.
   - If it reports fewer than 5 total issues, note that issue signal is too weak for theme analysis and continue with default ideation frames.

4. **OpenViking context** (optional) - if OpenViking project context or memories are available in this repository, use them as supplemental grounding only. They should enrich repo-specific context, not override direct codebase evidence.

Consolidate the results into a short grounding summary with distinct sections:
- **Codebase context**
- **Past learnings**
- **Issue intelligence** (when present)
- **OpenViking context** (when present)
- **Constitution context** (when `docs/constitution.md` exists)

Do not do external research in this workflow.

### Phase 2: Divergent Ideation

Follow this mechanism exactly:

1. Generate the full candidate list before critiquing any idea.
2. Each sub-agent targets about 7-8 ideas by default. Adjust when volume overrides apply.
3. Push past the safe obvious layer. The first few ideas tend to be generic.
4. Ground every idea in the Phase 1 scan.
5. Preserve the backbone pattern:
   - generate many ideas first
   - challenge them systematically second
   - explain only the survivors in detail third
6. Use sub-agents to improve diversity in the candidate pool rather than to replace the core mechanism.
7. Give each ideation sub-agent the same:
   - grounding summary
   - focus hint
   - per-agent volume target
   - instruction to generate raw candidates only, not critique
8. Assign each ideation sub-agent a different starting bias, not a hard constraint.

#### Frame selection

**When issue-tracker intent is active and themes were returned:**
- each theme with meaningful signal becomes an ideation frame
- if fewer than 4 frames emerge, pad with these defaults:
  - leverage and compounding effects
  - assumption-breaking or reframing
  - inversion, removal, or automation of a painful step
- cap at 6 total frames

**When issue-tracker intent is not active:**
- user or operator pain and friction
- unmet need or missing capability
- inversion, removal, or automation of a painful step
- assumption-breaking or reframing
- leverage and compounding effects
- extreme cases, edge cases, or power-user pressure

Ask each ideation sub-agent to return a compact structured object for each idea with:
- title
- summary
- why_it_matters
- evidence or grounding hooks
- optional local signals such as boldness or focus_fit

Then:
1. merge and dedupe the outputs
2. synthesize cross-cutting combinations when two weaker ideas suggest a stronger combined direction
3. preserve the strongest 20-30 candidate set before critique

### Phase 3: Adversarial Filtering

Review every generated idea critically.

Prefer a two-layer critique:
1. One or more skeptical sub-agents attack the merged list from distinct angles.
2. The orchestrator synthesizes those critiques, applies one rubric consistently, scores the survivors, and decides the final ranking.

For each rejected idea, write a one-line reason.

Use rejection criteria such as:
- too vague
- not actionable
- duplicates a stronger idea
- not grounded in the current codebase
- conflicts with the constitution without a compelling amendment case
- too expensive relative to likely value
- already covered by existing workflows or docs
- better handled as a brainstorm variant, not a product improvement

Use a consistent survivor rubric that weighs:
- groundedness in the current repo
- constitution fit
- expected value
- novelty
- pragmatism
- leverage on future work
- implementation burden
- overlap with stronger ideas

Target output:
- keep 5-7 survivors by default
- if too many survive, run a second stricter pass
- if fewer than 5 survive, report that honestly rather than lowering the bar

### Phase 4: Present the Survivors

Present only the surviving ideas in structured form:
- title
- description
- rationale
- downsides
- confidence score
- estimated complexity

Then include a brief rejection summary so the user can see what was considered and cut.

Keep this presentation concise. The durable artifact holds the full record.

Allow brief follow-up questions and lightweight clarification before writing the artifact.

Write the ideation doc before any handoff, explicit preserve request, or session end.

### Phase 5: Write the Ideation Artifact

To write the artifact:

1. Ensure `docs/ideation/` exists.
2. If `.gitignore` exists, ensure `docs/ideation/` is ignored alongside `docs/plans/` and `docs/brainstorms/` unless the user explicitly wants ideation docs committed.
3. Choose the file path:
   - `docs/ideation/YYYY-MM-DD-<topic>-ideation.md`
   - `docs/ideation/YYYY-MM-DD-open-ideation.md` when no focus exists
4. Write or update the ideation document.

Use this structure and omit clearly irrelevant fields only when necessary:

```markdown
---
date: YYYY-MM-DD
topic: <kebab-case-topic>
focus: <optional focus hint>
---

# Ideation: <Title>

## Codebase Context
[Grounding summary from Phase 1]

## Ranked Ideas

### 1. <Idea Title>
**Description:** [Concrete explanation]
**Rationale:** [Why this improves the project]
**Downsides:** [Tradeoffs or costs]
**Confidence:** [0-100%]
**Complexity:** [Low / Medium / High]
**Status:** [Unexplored / Explored]

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | <Idea> | <Reason rejected> |

## Session Log
- YYYY-MM-DD: Initial ideation - <candidate count> generated, <survivor count> survived
```

If resuming:
- update the existing file in place
- append to the session log
- preserve explored markers

### Phase 6: Refine or Hand Off

After presenting the results, ask what should happen next.

Offer these options:
1. brainstorm a selected idea
2. refine the ideation
3. end the session

#### 6.1 Brainstorm a Selected Idea

If the user selects an idea:
- write or update the ideation doc first
- mark that idea as `Explored`
- note the brainstorm date in the session log
- invoke `/workflows-brainstorm` with the selected idea as the seed

Do not skip brainstorming and go straight to planning from ideation output.

#### 6.2 Refine the Ideation

Route refinement by intent:
- `add more ideas` or `explore new angles` -> return to Phase 2
- `re-evaluate` or `raise the bar` -> return to Phase 3
- `dig deeper on idea #N` -> expand only that idea's analysis

After each refinement:
- update the ideation document before any handoff or session end
- append a session log entry

#### 6.3 End the Session

When ending:
- ensure the ideation doc is written or updated
- summarize where it lives
- remind the user that the next step is `/workflows-brainstorm [chosen idea]`

## Quality Bar

Before finishing, check:
- the idea set is grounded in the actual repo
- the candidate list was generated before filtering
- the many-ideas -> critique -> survivors mechanism was preserved
- rejected ideas have reasons
- survivors are materially better than a naive "give me ideas" list
- the artifact was written before any handoff or session end
- acting on an idea routes to `/workflows-brainstorm`, not directly to implementation
