---
name: workflows-work
description: Execute work plans while maintaining WHY tracing from problem narrative through user story to implementation. Grounds every subagent in purpose.
---

## Arguments
[plan file, ticket index, ticket file, specification, or todo file path] [--review-mode bulk|inline|both]

# Work Plan Execution Command

Execute a work plan while maintaining WHY tracing from problem narrative through implementation.

**Ralph-first execution:** When the resolved TDD contract selects Ralph-driven execution, `/workflows-work` is the canonical Ralph red-green-refactor path. `/compound-engineering-ralph-loop` and `/compound-engineering-cancel-ralph` are operational helpers for that path, not a detached alternative workflow.

## Introduction

This command takes a work document (plan, ticket index, ticket, specification, or todo file) and executes it systematically using a **subagent orchestration model**. The orchestrator (this conversation) loads or adapts the source into execution units and delegates each unit to a focused subagent. `vertical-slices` is the default execution shape, but `infra-track` and `fix-batch` are also valid when declared by the plan. When the input is a ticket index, that index becomes the authoritative execution queue and `/workflows-work` selects the next safe batch from it. When the input is a ticket file, that ticket becomes the primary execution packet and the parent plan/architecture artifacts provide deeper context instead of re-expanding the whole backlog. Every implementation unit, retry, and regression repair in this workflow is delegated through the named `execution-agent`, which follows a standardized 4-phase protocol (understand, implement, self-review, report).

**WHY-grounded execution:** Every subagent receives the source plan's WHY context -- the problem narrative, user story, architectural context, the architecture handoff contract, and which success criterion their specific unit serves. When execution starts from a ticket index, the index decides the next batch while the selected tickets provide the local execution packets. When execution starts from a ticket file, the ticket's local context packet stays primary, while `plan_ref`, `tickets_ref`, and `architecture_ref` remain the deeper-dive path. This prevents implementation drift where technically correct code fails to deliver the user's actual need. The orchestrator is the guardian of WHY: it extracts purpose from the plan, threads it through every unit prompt, and validates that the combined output delivers the stated user story.

**Execution delegation rule:** Ticket execution must always go through the bundled `execution-agent`. Do not route ticket implementation, ticket fix loops, or ticket regression repairs through `general-purpose` or any ad hoc worker prompt.

### Review Mode

This command supports a `--review-mode` argument that controls when code review happens:

- **`bulk`** (default) -- Review happens after ALL tasks complete, using `/workflows-review`. This is the standard behavior and the only mode where named review agents run.
- **`inline`** -- After each task, a lightweight two-stage review (spec compliance then code quality) runs automatically using prompt templates and `general-purpose` subagents only. Inline mode must NOT spawn named review agents directly.
- **`both`** -- Inline review per task AND comprehensive `/workflows-review` at the end. Maximum quality assurance.

If no `--review-mode` is specified, check `compound-engineering.local.md` for a `review_mode` setting. If not found there either, default to `bulk`.

**Hard rule:** Named review agents belong to `/workflows-review`. `/workflows-work` may coordinate inline template-based checks, but it must never bypass `/workflows-review` by dispatching named reviewers directly.

## Input Document

<input_document> #$ARGUMENTS </input_document>

## Execution Workflow

### Phase 1: Quick Start

1. **Read the work document and extract WHY + guardrail context**

    - Read the work document completely
    - If the input is a ticket index or ticket file, first load `references/ticket-execution-contract.md` and verify the artifact includes the required index or ticket contract. Stop and send the user back to `/workflows-to-issues` if the contract is missing or malformed.
    - If the input is a ticket index, extract:
      - `plan_ref`, `architecture_ref`, `execution_shape`, `ticket_set_status`, `last_completed_batch`, and `total_batches`
      - the dependency graph and execution-batch table
      - the next unfinished batch and the ticket files it names
      - the file-overlap safety notes proving whether the batch is truly parallel-safe
    - If the input is a ticket index, use the index as the source of truth for ordering and batch selection. Load only the ticket files named in the next batch before continuing.
    - If the input is a ticket file, extract:
      - `plan_ref`, `tickets_ref`, `architecture_ref`, and `source_packet_ref`
      - `feature_home`, `depends_on`, `dependency_type`, `files`, `test_command`, and `status`
      - the compact packet in `## Local Context`
      - the parent trace in `## Parent Refs` and `## Deeper-Dive Refs`
    - If the input is a ticket index or ticket file, load the parent plan and architecture artifact from the recorded refs before continuing. The index chooses the batch; the ticket files remain the execution packets; the parent artifacts provide WHY and boundary context.
    - **Extract WHY artifacts** from the parent plan (these ground everything that follows):
      - **Problem Narrative** -- why this work exists, what pain it solves
      - **User Story** -- who benefits and what outcome they get
      - **Architectural Context** -- how the solution fits in the system
      - **Success Criteria** -- measurable conditions that define "done"
      - **Execution shape** -- resolve it using `references/execution-shape.md`
      - **Feature-home ownership** -- for `vertical-slices`, identify the feature home each unit changes and what remains shared/global
      - **Unit tracing** -- each packet's `Serves`, `Consumers`, or equivalent purpose line showing what outcome it delivers or unlocks
      - **Constitution alignment** -- relevant principles, required approvals, and any approved waivers
      - **`tdd` frontmatter + `## TDD & Evidence Contract`** -- resolve the effective TDD contract using `references/tdd-evidence-contract.md` (plan overrides local, `inherit` falls back, and no-local-config defaults to Ralph-driven `red-green-refactor` with unit + e2e evidence required)
    - Check for `handoff:` frontmatter in the parent plan. If present, verify all flags are `true` (problem_narrative, user_story, architectural_context, success_criteria). If any are `false`, warn the user that WHY context is incomplete and suggest running `/workflows-brainstorm` or `/workflows-plan` first.
   - If the resolved contract weakens Ralph/unit+e2e without a justified exception in the plan, stop and ask for the plan contract to be corrected before execution
   - If `docs/constitution.md` exists, read it and extract the active constitution version, applicable principles, execution baselines, and approval rules. If the plan lists `constitution_waivers`, honor only those explicit exceptions.
    - If the parent plan has a `brainstorm_ref:` path, read that brainstorm document too for richer WHY context
     - If the parent plan has an `architecture_ref:` path, the selected ticket or index has an `architecture_ref`, or `## Related Artifacts` points to `docs/architecture/`, read that artifact and extract feature homes, shared/global decisions, context tiers, drift checks, deletion tests, interfaces as test surfaces, seams, adapters, contracts, deepening candidates, and downstream work/review guidance
     - If no architecture artifact is recorded, assemble an explicit architecture handoff contract from the parent plan's Architectural Context, Key Decisions, Constitution Alignment, brainstorm context, execution constraints, and `references/vertical-slice-architecture.md`. Tell the user this is a fallback and recommend `/workflows-architecture` if boundaries are still unsettled.
    - Review any other references or links provided in the plan or ticket
   - If the constitution requires explicit approval for any part of the planned work (for example, risky writes, schema changes, auth changes, or scope expansions), surface that before execution starts
      - If the document is not already in a declared execution shape and is not a valid ticket index or ticket artifact, treat it as **legacy input**. Before spawning subagents, adapt it into execution units in STATE.md. If that adaptation materially changes scope or ordering, ask the user to approve the adapted unit backlog before proceeding.
    - If anything is unclear or ambiguous, ask clarifying questions now
   - Get user approval to proceed
   - **Do not skip this** - better to ask questions now than build the wrong thing

2. **Setup Environment**

   First, check the current branch:

   ```bash
   current_branch=$(git branch --show-current)
   default_branch=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@')

   # Fallback if remote HEAD isn't set
   if [ -z "$default_branch" ]; then
     default_branch=$(git rev-parse --verify origin/main >/dev/null 2>&1 && echo "main" || echo "master")
   fi
   ```

   **If already on a feature branch** (not the default branch):
   - Ask: "Continue working on `[current_branch]`, or create a new branch?"
   - If continuing, proceed to step 3
   - If creating new, follow Option A or B below

   **If on the default branch**, choose how to proceed:

   **Option A: Create a new branch**
   ```bash
   git pull origin [default_branch]
   git checkout -b feature-branch-name
   ```
   Use a meaningful name based on the work (e.g., `feat/user-authentication`, `fix/email-validation`).

   **Option B: Use a worktree (recommended for parallel development)**
   ```bash
   skill: git-worktree
   # The skill will create a new branch from the default branch in an isolated worktree
   ```

   **Option C: Continue on the default branch**
   - Requires explicit user confirmation
   - Only proceed after user explicitly says "yes, commit to [default_branch]"
   - Never commit directly to the default branch without explicit permission

   **Recommendation**: Use worktree if:
   - You want to work on multiple features simultaneously
   - You want to keep the default branch clean while experimenting
   - You plan to switch between branches frequently

3. **Preview Unit Breakdown**
   - Mentally identify the major execution units from the source document
   - If the input is a ticket index, preview the next batch from the index and whether it is sequential or parallel-safe
   - If the input is a ticket file, preview exactly one execution unit unless the user explicitly asks to re-split it
   - Note any questions about dependencies or scope
   - The formal unit decomposition happens in Phase 2 Step 4 (STATE.md), which is the persistent record of progress
   - TodoWrite can be used for in-conversation progress tracking if helpful, but STATE.md is the source of truth

### Phase 2: Orchestrated Execution

Phase 2 is where the orchestrator (this conversation) resolves the plan's execution shape, decomposes the work into execution units, and delegates each to a focused subagent. The orchestrator does NOT implement code itself -- it decomposes, delegates, records, and routes.

#### Step 1: Validate Plan Readiness

Before executing, validate four things: **structural readiness** (the selected execution shape or ticket contract is honest and its units are testable), **WHY readiness** (the plan carries purpose context), **TDD readiness** (the execution contract is explicit and enforceable), and **guardrail readiness** (repo-wide rules are visible and actionable).

**Structural readiness** -- first resolve `execution_shape` using `references/execution-shape.md`, then verify the units for that mode:

- **`vertical-slices`** -- slice type, serves, demo scenario, feature home, scope fence, files, success criteria, validation command, dependencies, dependency type
- **`infra-track`** -- capability enabled, consumers / downstream work unlocked, scope, files, risk / rollback, success criteria, validation command, dependencies
- **`fix-batch`** -- problem, repro / expected outcome, files, success criteria, validation command, dependencies
- **Ticket index input** -- valid `ticket-execution-contract.md` index contract, batch table, file-overlap safety notes, and progress pointer
- **Ticket input** -- valid `ticket-execution-contract.md` frontmatter/body, parent refs, feature home, scope fence, acceptance criteria, test command, and compact local context
- **Default rule** -- if `execution_shape` is missing, assume `vertical-slices`
- **Anti-coercion rule** -- do not force infra or fix-batch work into slices if that would create fake verticality

**Guardrail readiness** -- when the project has `docs/constitution.md`, the plan should make repo-wide rules visible:

- **Constitution alignment** -- relevant principles and baselines are surfaced
- **Constitution version** -- recorded when a constitution exists
- **Constitution waivers** -- explicit and limited; empty by default
- **Required approvals** -- called out before work begins

**TDD readiness** -- the plan and local defaults must resolve to one clear execution contract:

- **`tdd` frontmatter present** -- includes precedence, mode, loop, evidence, and exceptions
- **`## TDD & Evidence Contract` present** -- states the resolved execution path in plain language
- **Effective mode resolved** -- Ralph-driven by default unless the plan explicitly approves a standard-mode exception
- **Required evidence resolved** -- unit + e2e by default, or justified replacement evidence when explicitly waived
- **Report contract visible** -- Ralph-driven units must emit stable red, green, and post-refactor green evidence blocks

**WHY readiness** -- the plan should have:

- **Problem Narrative** -- present and non-empty
- **User Story** -- present with clear "As a... I want... So that..."
- **Architectural Context** -- present, describing system fit
- **Success Criteria** -- present at plan level (not just unit level)
- **Unit tracing** -- each execution unit has a purpose line connecting it to the user story or explicit enabling outcome

If the plan lacks structural details, the ticket index or ticket lacks the ticket execution contract, or no architecture artifact / handoff contract can explain the boundaries, refuse to proceed and suggest running `/workflows-architecture` first if the boundaries are still fuzzy, then `/deepen-plan` or `/workflows-to-issues`, or manually repairing the execution packet.

If the plan lacks the `tdd` block or `## TDD & Evidence Contract`, or if the resolved contract is ambiguous, refuse to proceed and suggest `/workflows-plan` or `/deepen-plan` to repair the execution contract before spawning subagents.

If the plan lacks WHY artifacts, the orchestrator should **construct minimal WHY context** before proceeding:
1. Ask the user: "This plan doesn't include a problem narrative or user story. In one sentence, what problem are we solving and for whom?"
2. Infer success criteria from the unit-level criteria
3. Infer architectural context from the file paths and technologies mentioned
4. Record these in STATE.md (see Step 3) so they're available for all units

#### Step 2: Check for Resumable Session

Before creating a new session, check for existing incomplete sessions for the same plan, ticket index, or ticket:

```bash
ls docs/execution-sessions/work-*/state.md 2>/dev/null
```

If a previous session exists for the same source artifact and has `status: in_progress`:

- Ask the user: "Found incomplete session `[session_id]` for this plan. Resume where you left off, or start fresh?"
- **If resume**: Read STATE.md, load the WHY Context section plus the Architecture Handoff section, skip completed units, load the learnings brief, and continue from `current_unit`
- **If fresh**: Archive the old session directory (rename with `-archived` suffix), then start a new session

If no resumable session exists, proceed to Step 3.

#### Step 3: Initialize Execution Session

Create a persistent execution session to track progress:

```bash
SESSION_ID="work-$(date +%Y-%m-%d-%H%M%S)"
mkdir -p docs/execution-sessions/${SESSION_ID}
```

Create a `STATE.md` file in the session directory:

```markdown
---
source_type: [plan | ticket-index | ticket | specification | todo]
plan_file: [path to plan]
ticket_index: [path to ticket index, if applicable]
ticket_file: [path to ticket, if applicable]
tickets_ref: [path to ticket index, if applicable]
source_packet_ref: [plan packet ref or ticket packet ref]
brainstorm_ref: [path to brainstorm, if available]
started: [ISO timestamp]
status: in_progress
execution_shape: [vertical-slices | infra-track | fix-batch]
current_unit: 0
total_units: [count]
session_id: [SESSION_ID]
---

## WHY Context

### Problem Narrative
[Extracted from plan -- why this work exists]

### User Story
[Extracted from plan -- who benefits and what outcome]

### Architectural Context
[Extracted from plan -- how this fits in the system]

### Success Criteria
[Extracted from plan -- measurable conditions for "done"]

### TDD Contract
- Effective mode: [Ralph-driven TDD | Standard implementation with approved exception]
- Effective loop: [Failing tests first -> minimal implementation -> refactor -> post-refactor rerun | Implementation-first]
- Required evidence: [Unit command/result], [E2E command/result], [Replacement evidence for any approved exception]
- Exceptions: [None, or explicit approved deviations from Ralph/unit+e2e]

### Constitution Context
[Relevant principles, baselines, approvals, and waivers extracted from plan and docs/constitution.md]

### Architecture Handoff
- Artifact: [docs/architecture/... path or "plan-derived handoff"]
- Feature homes: [primary feature homes and who owns them]
- Shared / global decisions: [what stays shared versus local]
- Context tiers: [global, on-demand, ticket-local]
- Deepening candidates to preserve: [list]
- Deletion test: [what stays concrete vs what may be abstracted later]
- Interfaces as test surfaces: [behavioral contracts]
- Seams / adapters / contracts: [boundaries this execution must honor]
- Review guidance: [what `/workflows-review` must verify later]

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | [unit title] | tracer-bullet | [which user story aspect or enabling outcome] | pending | -- | -- |
| 2 | [unit title] | expansion | [which user story aspect or enabling outcome] | pending | -- | -- |
...

## Learnings Brief
_No learnings yet._
```

#### Step 4: Load or Adapt Execution Units

The orchestrator parses the source artifact and creates a list of execution units. Each unit is a self-contained packet of work defined by the selected execution shape or the ticket contract. The orchestrator does the heavy lifting here:

- **Prefer plan-defined units directly** -- if the plan already declares a coherent execution shape, execute those packets as written
- **Prefer index-defined batches directly** -- if the input is a valid ticket index, execute the next unresolved batch as written and do not re-batch it unless the index is missing safety evidence or the user explicitly approves a change
- **Prefer ticket-defined unit directly** -- if the input is a valid ticket artifact, execute that one ticket as one unit and do not re-split it unless the user explicitly approves a change
- **Adapt legacy phase/task plans into units before coding** -- do not execute raw task lists directly once the shape contract is available
- **Break oversized units** into smaller units if needed (each unit should be completable in one subagent session)
- **Preserve WHY tracing** -- when splitting a unit, each resulting unit inherits or refines the parent unit's purpose line. Never create an orphan unit with no connection to the user story.
- **Identify file dependencies** between units
- **Determine parallelizable units** -- only units with non-overlapping file sets and compatible dependencies can run simultaneously. For ticket-index input, trust only the explicit batch partition recorded in `index.md`; if the index leaves overlap safety ambiguous, collapse the batch to sequential execution.
- **Ensure each unit has clear success criteria** -- if the plan already defines them, use them directly; otherwise, the orchestrator must create them
- **Map each unit to its purpose** -- record which success criterion or enabling outcome each unit delivers (this goes in STATE.md's "Serves / Unlocks" column)

Mode-specific rules:
- **`vertical-slices`** -- execute slices directly; keep the first unit a tracer bullet
- **`infra-track`** -- execute infrastructure work packets directly; do not coerce them into fake slices
- **`fix-batch`** -- execute fix items directly; keep each one narrow and independently verifiable

If the plan already has well-defined units with success criteria, use them directly. If not, the orchestrator must create them before proceeding.

#### Step 5: Execute Unit Loop

For each unit (or parallel batch of units), follow this cycle:

##### a. Build Scoped Prompt

For each unit, the orchestrator constructs a focused prompt for the named `execution-agent`.

Apply the shared `Named Agent Dispatch` protocol from `references/orchestration-protocol.md`, substituting `execution-agent`.

- Quote the first non-empty line of the loaded bundled agent template before continuing.
- Every execution, retry, fix, and regression-repair subagent in this workflow must start from a freshly loaded copy of that same agent template.
- Build `scoped_prompt` by injecting the full loaded `execution-agent` template plus the resolved context packet below. Do not summarize, abbreviate, or paraphrase the agent template.
- Apply the shared `Reference Template Loading` protocol from `references/orchestration-protocol.md`, substituting `execution-agent-prompt.md`, and quote the first non-empty line of that scaffold before continuing.
- Also load `references/execution-agent-prompt.md` as the scaffold for the context packet so the injected headings stay stable across retries and follow-up fixes.
- Fill the scaffold completely. Do not continue if any required section is missing or any `{{PLACEHOLDER}}` value is unresolved.

- **`## Your Unit`**
  - **{{UNIT_TITLE}}** and **{{UNIT_DESCRIPTION}}** -- from the plan packet or ticket artifact
  - **{{UNIT_KIND}}** -- from the plan or ticket (`tracer-bullet`, `expansion`, `hardening`, `infra-packet`, `fix-item`, etc.)
  - **{{OUTCOME_SCENARIO}}** -- the observable behavior or enabling outcome this unit proves
  - **{{FEATURE_HOME}}** -- the primary feature home or owning module
  - **{{UNIT_SCOPE}}** -- what the unit owns and excludes
  - **{{UNIT_SCOPE_FENCE}}** -- the boundary that keeps the unit thin
  - **{{FILE_LIST}}** -- files to create/modify from the plan or ticket
  - **{{SUCCESS_CRITERIA}}** -- checkboxes that define "done"
  - **{{VALIDATION_COMMAND}}** -- how to verify the unit works
  - **{{COMPLETED_DEPENDENCIES}}** -- list of already-completed units this depends on
  - **{{PARENT_REFS}}** -- plan, ticket set, architecture, and source packet refs that anchor this unit
- **`## Ticket-local context`**
  - **{{TICKET_LOCAL_CONTEXT}}** -- the ticket-local execution packet when the source is a ticket; otherwise a compact packet derived from the plan unit
- **`## Why This Unit Exists`**
  - **{{WHY_CONTEXT}}** -- the purpose grounding block (constructed by orchestrator):
  ```
  ## Why This Unit Exists
  **Problem:** [problem narrative from plan -- 1-2 sentences]
  **User Story:** [user story from plan]
  **This unit serves:** [the packet purpose line from this unit -- which user story aspect, success criterion, or enabling outcome this delivers]
  **Overall success criteria:** [plan-level success criteria list]
  **Guardrails:** [relevant constitution principles, approval rules, and approved waivers]
  ```
- **`## Architectural Context`**
  - **{{ARCHITECTURAL_CONTEXT}}** -- from the plan's Architectural Context section, filtered to what's relevant for this unit's files and domain
- **`## Architecture Handoff`**
  - **{{ARCHITECTURE_HANDOFF}}** -- from the `docs/architecture/` artifact or explicit plan-derived handoff contract; include deletion-test decisions, interfaces as test surfaces, seams, adapters, contracts, and downstream review guidance relevant to this unit
- **`## Learnings from Previous Units`**
  - **{{LEARNINGS_BRIEF}}** -- from previous units, filtered by domain relevance
- **`## Project Conventions`**
  - **{{PROJECT_CONVENTIONS}}** -- from CLAUDE.md plus relevant constitution baselines
- **`## TDD Execution Contract`**
  - **{{TDD_CONTRACT}}** -- the resolved execution contract: effective mode, Ralph/default loop, required unit/e2e evidence, any explicit exceptions, and any fix/regression context that the retried unit must address

The loaded `execution-agent` template instructs each subagent to follow a 4-phase protocol:
1. **Understand** -- review requirements, surface ambiguities, state assumptions before coding
2. **Implement** -- follow the resolved Ralph/default execution mode, retry on failure (up to 3 attempts)
3. **Self-review** -- check completeness, quality, discipline, testing, and evidence
4. **Report** -- return a structured execution report with stable red, green, and post-refactor green evidence when Ralph-driven

##### b. Spawn Subagent

Delegate the unit to a focused subagent:

```
Task(execution-agent, prompt=scoped_prompt)
```

The subagent prompt is constructed from the loaded bundled `execution-agent` template plus the fully injected context packet scaffold from `references/execution-agent-prompt.md`. Do not substitute a custom summary prompt for any execution worker, and do not dispatch ticket implementation through `general-purpose`:

1. Read referenced files and understand existing patterns
2. Follow the resolved Ralph/default execution contract
3. Capture stable red, green, and post-refactor green evidence when Ralph-driven
4. Run the test command
5. If tests fail: analyze failure, fix, and retry (up to 3 internal attempts)
6. Return a structured execution report containing:
   - Summary of what was implemented
   - Files created/modified (with paths)
   - Problems encountered and how they were fixed
   - Patterns discovered (naming conventions, architectural patterns, etc.)
   - TDD evidence (Red, Green, Post-Refactor Green) when Ralph-driven
   - Final test results (pass/fail)
   - Attempt count

**For parallel units**: Spawn multiple subagents simultaneously only when their file sets do not overlap and their dependency types allow parallel work. For ticket-index input, parallelize only when the selected batch is explicitly marked safe by the index's file-overlap notes. Before parallelizing, verify file sets do not overlap and no unit claims shared mutable state without an explicit guard. If there is doubt, execute sequentially.

**Example scoped prompt:**

```
You are implementing Unit 3 of a feature plan. Here is your scoped context:

## Why This Unit Exists
**Problem:** Users currently cannot authenticate, forcing manual session management that's error-prone and insecure.
**User Story:** As a user, I want to log in with my credentials so that I can access my personalized dashboard securely.
**This unit serves:** "Secure authentication flow" -- implementing the first thin end-to-end login path.
**Overall success criteria:**
- Users can log in and receive a JWT token
- Invalid credentials are rejected with clear error messages
- Tokens expire after the configured TTL

## Unit
Create the UserAuthService with JWT token generation and validation.

## Files to Create/Modify
- Create: `src/services/UserAuthService.ts`
- Create: `tests/UserAuthService.test.ts`
- Modify: `src/app.ts` (register service)

## Success Criteria
- [ ] UserAuthService instantiates correctly
- [ ] authenticate() returns JWT token for valid credentials
- [ ] authenticate() throws AuthenticationError for invalid credentials
- [ ] Token validation works for valid and expired tokens

## Validation Command
npm test -- --filter UserAuthService

## Architectural Context
JWT-based stateless auth. Tokens issued by UserAuthService, validated by middleware (Unit 4). No server-side session storage.

## TDD Execution Contract
- Effective mode: Ralph-driven TDD
- Effective loop: failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: unit command/result + e2e command/result
- Exceptions: none

## Conventions
- Use dependency injection pattern
- Variables are camelCase
- Type annotations on all parameters and return types

## Learnings from Previous Units
- [backend] Use jest.mock() for module mocking
- [backend] Factory pattern: createUser() helper not new User()
- [testing] Use expect().toThrow() for error assertions

## Instructions
1. Read the referenced files and understand existing patterns
2. Follow the Ralph-driven TDD contract: RED first, GREEN second, REFACTOR third, post-refactor rerun fourth
3. Capture stable report evidence for Red, Green, and Post-Refactor Green
4. Run the test command
5. If tests fail: analyze, fix, retry (up to 3 attempts)
6. Return a structured report: summary, files changed, problems encountered and fixes, patterns discovered, TDD evidence, test results, attempt count
```

##### c. Process Subagent Results

When the subagent returns, the orchestrator processes the results:

**0. Validate the execution contract evidence** -- audit the report against `references/tdd-evidence-contract.md`. If a Ralph-driven unit is missing stable `Red`, `Green`, and `Post-Refactor Green` evidence blocks, treat the report as incomplete and send it back for correction before marking the unit complete.

**1. Write session file** to `docs/execution-sessions/${SESSION_ID}/unit-{nn}-{slug}.md`:

```markdown
---
unit: "[unit title]"
unit_number: [n]
unit_kind: [tracer-bullet|expansion|hardening|infra-packet|fix-item]
serves: "[which user story aspect / success criterion / enabling outcome this unit delivers]"
status: [completed|failed]
attempt_count: [n]
domains: [backend, frontend, testing, database, etc.]
plan_file: [path]
ticket_file: [path, if applicable]
session_id: [SESSION_ID]
---

## What Was Implemented
[From subagent report]

## Files Changed
- `path/to/file.php` -- created/modified

## Problems Encountered
### Problem 1: [title]
- **Error:** [exact error message]
- **Root cause:** [analysis]
- **Fix:** [what was done]

### Problem 2: ...

## Patterns Discovered
- [pattern 1]
- [pattern 2]

## TDD Evidence
[Mirror the exact Ralph evidence block from `references/tdd-evidence-contract.md`. Preserve the `Red`, `Green`, and `Post-Refactor Green` headings with their command/result/evidence fields.]

## Test Results
- Command: `[test command]`
- Result: PASS/FAIL
- Attempts: [n]
```

**2. Inline Review (when `--review-mode inline` or `--review-mode both`)**

If the `--review-mode` argument is `inline` or `both`, perform a two-stage inline review before proceeding to the next unit. If `--review-mode` is `bulk` (the default), skip this step.

   **Stage 1: Spec Compliance Review**

   Apply the shared `Reference Template Loading` protocol from `references/orchestration-protocol.md`, substituting `spec-review-prompt.md`. If the template cannot be loaded and quoted, stop the inline review loop and report the missing template instead of improvising. Then fill in:
   - `{{UNIT_REQUIREMENTS}}` -- the unit description, outcome scenario, scope fence, and success criteria
   - `{{SUCCESS_CRITERIA}}` -- the success criteria checkboxes
   - `{{IMPLEMENTER_REPORT}}` -- the execution report from the subagent
   - `{{UNIT_PURPOSE}}` -- what user story aspect or enabling outcome this unit delivers (from the unit's purpose line)

   Spawn a spec reviewer subagent:
   ```
   Task(general-purpose, prompt=filled_spec_review_prompt)
   ```

   The spec reviewer should check not just checkbox compliance but whether the implementation actually delivers on the recorded purpose. A unit can pass all checkboxes but miss the intent.

   - If **PASS**: proceed to Stage 2
   - If **FAIL**: reload the bundled `execution-agent` template plus `execution-agent-prompt.md`, rebuild the scoped execution prompt with the review findings added as fix context, then spawn a new `execution-agent` run. Re-run the spec reviewer afterward (max 2 fix-review cycles). If still failing after 2 cycles, log the issues and ask the user how to proceed.

   **Stage 2: Code Quality Review** (only after spec compliance passes)

   Apply the shared `Reference Template Loading` protocol from `references/orchestration-protocol.md`, substituting `quality-review-prompt.md`. If the template cannot be loaded and quoted, stop the inline review loop and report the missing template instead of improvising. Then fill in:
   - `{{IMPLEMENTER_REPORT}}` -- the execution report
   - `{{FILES_CHANGED}}` -- list of files from the report

   Spawn a quality reviewer subagent:
   ```
   Task(general-purpose, prompt=filled_quality_review_prompt)
   ```

   - If **PASS**: proceed to next steps
   - If **FAIL** with Critical issues: reload the bundled `execution-agent` template plus `execution-agent-prompt.md`, rebuild the scoped execution prompt with the quality findings added as fix context, spawn an `execution-agent` fix run, then re-review (max 2 cycles)
   - If **FAIL** with only Important/Minor issues: log them for the orchestrator's attention but proceed to next task (these will also be caught by `/workflows-review` if run later)

   **Note:** Inline review is a lightweight per-unit check. It does NOT replace the comprehensive `/workflows-review` multi-agent review. When `--review-mode both` is active, inline review runs per-unit AND `/workflows-review` runs after all units complete.

**3. Update STATE.md** -- mark the unit status, increment `current_unit`, update the work status table

**4. Update learnings brief** -- add new learnings from this unit, tagged by domain, deduplicated against existing learnings

**5. Update source work artifact** -- keep the execution source honest:
   - plan input: check off completed items (`[ ]` to `[x]`) in the original plan document
   - ticket-index input: update the selected batch status in `index.md`, keep the ticket table honest, and increment `last_completed_batch` only after every ticket in that batch is `completed`
   - ticket input: update the ticket `status` field (`ready` -> `completed` or `blocked`) and preserve parent refs
   - if both an index and ticket files can be updated safely, update both without inventing new status fields
   - if both a ticket and plan backlog can be updated safely, update both without inventing new status fields

**6. Regression guard** -- run test commands from ALL previously completed tasks. If any regress:
   - Log the regression in the current task's session file
   - Reload the bundled `execution-agent` template plus `execution-agent-prompt.md`, rebuild the scoped execution prompt with context about what broke and why, and spawn an `execution-agent` fix run
   - Do not proceed to the next task until the regression is fixed

**7. Incremental commit** if appropriate (logical unit complete, tests pass):

   | Commit when... | Don't commit when... |
   |----------------|---------------------|
   | Logical unit complete (one observable outcome) | Small part of a larger unit |
   | Tests pass + meaningful progress | Tests failing |
   | About to switch contexts (backend to frontend) | Purely scaffolding with no behavior |
   | About to attempt risky/uncertain changes | Would need a "WIP" commit message |

   **Heuristic:** "Can I write a commit message that describes a complete, valuable change? If yes, commit. If the message would be 'WIP' or 'partial X', wait."

   ```bash
   # 1. Verify tests pass (use project's test command)
   # 2. Stage only files related to this logical unit (not `git add .`)
   git add <files related to this logical unit>
   # 3. Commit with conventional message
   git commit -m "feat(scope): description of this unit"
   ```

   **Note:** Incremental commits use clean conventional messages without attribution footers. The final Phase 4 commit/PR includes the full attribution.

##### d. Handle Failures

If a subagent fails after its internal retries:

1. **Reframe**: Can the unit be broken down differently? Try spawning a new subagent with a different approach or smaller scope.
2. **Ask user**: Use AskUserQuestion -- "Unit [name] failed after 3 attempts. [error summary]. How should I proceed?"
    - Options: "Retry with different approach", "Skip and continue", "Stop pipeline", "I'll fix it manually"
3. **Skip and continue**: Mark the unit as `skipped` in STATE.md. Note it as a blocker for any dependent units. Dependent units are also skipped automatically.
4. **Stop pipeline**: Save all state to STATE.md with `status: paused`, present a summary of what was completed and what remains.

### Phase 3: Quality Check

1. **Run Core Quality Checks**

   Always run before submitting:

   ```bash
   # Run full test suite (use project's test command)
   # Detect and run: npm test, pytest, cargo test, phpunit, go test, etc.

   # Run linting (per CLAUDE.md)
   # Use project-specific linter: eslint, ruff, clippy, pint, etc.
   ```

2. **Purpose Validation** (REQUIRED)

   Before mechanical quality checks, validate that the combined work delivers on the WHY:

      - **User story delivered?** -- Review the user story from STATE.md. Can a user actually achieve the stated outcome with what was built? If any success criterion is unmet or any unit was skipped, note the gap.
     - **Architectural integrity?** -- Does the implementation match the architectural context from the plan? Flag any deviations (e.g., plan said "stateless JWT" but implementation uses server sessions).
      - **Constitution honored?** -- Does the implementation respect the constitution baselines and approval rules captured in STATE.md? Flag any unwaived violations.
       - **Ralph evidence complete?** -- For Ralph-driven units, does every session file include Red, Green, and Post-Refactor Green evidence aligned to the resolved unit/e2e contract or an explicitly approved exception?
      - **No orphan code** -- Is there any implemented code that doesn't trace back to the user story or success criteria? This may indicate scope creep during execution.

   If purpose validation reveals gaps, present them to the user before proceeding to PR.

3. **Run `/workflows-review` when named reviewers are needed**

   Use `/workflows-review` for complex, risky, or large changes, and whenever `review_mode` requires final reviewer-agent coverage (`bulk` or `both`). Do not dispatch named review agents directly from `/workflows-work`.

   `/workflows-work` may only run the inline template-based reviewers described above. All named agents from `compound-engineering.local.md` frontmatter (`review_agents`) must be coordinated by `/workflows-review`, which owns the template-loading protocol, WHY-context injection, mandatory always-on reviewers, and conditional reviewer rules.

4. **Final Validation**
   - All units in STATE.md marked `completed` (or explicitly `skipped` with user approval)
   - All tests pass (including regression tests from every completed unit)
   - Linting passes
   - Code follows existing patterns
   - Purpose validation passed (user story deliverable, architecture intact)
   - Constitution validation passed (or waivers are explicit and approved)
   - Figma designs match (if applicable)
   - No console errors or warnings

4. **Prepare Operational Validation Plan** (REQUIRED)
   - Add a `## Post-Deploy Monitoring & Validation` section to the PR description for every change.
   - Include concrete:
     - Log queries/search terms
     - Metrics or dashboards to watch
     - Expected healthy signals
     - Failure signals and rollback/mitigation trigger
     - Validation window and owner
   - If there is truly no production/runtime impact, still include the section with: `No additional operational monitoring required` and a one-line reason.

### Phase 4: Ship It

Use the `finishing-branch` skill for structured branch completion. This skill handles final verification, commit, merge/PR options, worktree cleanup, and plan status updates.

```
skill: finishing-branch
```

If the `finishing-branch` skill is not available, follow the manual steps below:

1. **Create Commit**

   ```bash
   git add .
   git status  # Review what's being committed
   git diff --staged  # Check the changes

   # Commit with conventional format
   git commit -m "$(cat <<'EOF'
   feat(scope): description of what and why

   Brief explanation if needed.
   EOF
   )"
   ```

2. **Capture and Upload Screenshots for UI Changes** (REQUIRED for any UI work)

   For **any** design changes, new views, or UI modifications, you MUST capture and upload screenshots:

   **Step 1: Start dev server** (if not running)
   ```bash
   # Laravel: Docker containers should be running
   # Frontend: npm run dev
   ```

   **Step 2: Capture screenshots with agent-browser CLI**
   ```bash
   agent-browser open http://localhost:[port]/[route]
   agent-browser snapshot -i
   agent-browser screenshot output.png
   ```
   See the `agent-browser` skill for detailed usage.

   **Step 3: Upload using imgup skill**
   ```bash
   skill: imgup
   # Then upload each screenshot:
   imgup -h pixhost screenshot.png  # pixhost works without API key
   # Alternative hosts: catbox, imagebin, beeimg
   ```

   **What to capture:**
   - **New screens**: Screenshot of the new UI
   - **Modified screens**: Before AND after screenshots
   - **Design implementation**: Screenshot showing Figma design match

   **IMPORTANT**: Always include uploaded image URLs in MR description. This provides visual context for reviewers and documents the change.

3. **Push Branch and Create Merge Request**

   ```bash
   git push -u origin feature-branch-name
   ```

   After pushing, inform the user with the MR description template below. They can create the MR in GitLab's web UI or using `glab mr create` if available.

   **MR Description Template:**

   ```markdown
   ## Summary
   - **Problem:** [from plan's Problem Narrative]
   - **User Story:** [from plan's User Story]
   - **What was built:** [concrete description of implementation]
   - **Key decisions made:** [architectural or design choices]

   ## Success Criteria Status
    - [x] [criterion 1 from plan] -- delivered by Unit N
    - [x] [criterion 2 from plan] -- delivered by Unit N
   - [ ] [criterion 3 if skipped] -- skipped: [reason]

   ## Testing
   - Tests added/modified
   - Manual testing performed

   ## Post-Deploy Monitoring & Validation
   - **What to monitor/search**
     - Logs:
     - Metrics/Dashboards:
   - **Validation checks (queries/commands)**
     - `command or query here`
   - **Expected healthy behavior**
     - Expected signal(s)
   - **Failure signal(s) / rollback trigger**
     - Trigger + immediate action
   - **Validation window & owner**
     - Window:
     - Owner:
   - **If no operational impact**
     - `No additional operational monitoring required: <reason>`

   ## Before / After Screenshots
   | Before | After |
   |--------|-------|
   | ![before](URL) | ![after](URL) |

   ## Figma Design
   [Link if applicable]

   ---

   [![Compound Engineered](https://img.shields.io/badge/Compound-Engineered-6366f1)](https://github.com/The-Rabak/compound-engineering-plugin)
   ```

   **Save the MR description** to a file the user can copy from:
   ```bash
   # Write MR description to a temp file for easy copy-paste
   cat > /tmp/mr-description.md <<'MRDESC'
   [filled-in MR description from template above]
   MRDESC
   echo "MR description saved to /tmp/mr-description.md"
   ```

4. **Finalize Execution Session**

   Update the session STATE.md:
   - Set `status: completed`
   - Record final summary and completion timestamp

   If the input document has YAML frontmatter with a `status` field, update it to `completed`:
   ```
   status: active  →  status: completed
   ```

5. **Notify User**
   - Summarize what was completed
   - Link to PR
    - Highlight any units that were skipped and why
   - Reference the execution session directory for detailed logs
   - Note any follow-up work needed
   - Suggest next steps if applicable

---

## Swarm Mode (Optional)

For complex plans with multiple independent workstreams, enable swarm mode for parallel execution with coordinated agents.

### When to Use Swarm Mode

| Use Swarm Mode when... | Use Standard Mode when... |
|------------------------|---------------------------|
| Plan has 5+ independent units | Plan is linear/sequential |
| Multiple specialists needed (review + test + implement) | Single-focus work |
| Want maximum parallelism | Simpler mental model preferred |
| Large feature with clear phases | Small feature or bug fix |

### Enabling Swarm Mode

To trigger swarm execution, say:

> "Make a unit list and launch an army of agent swarm subagents to build the plan"

Or explicitly request: "Use swarm mode for this work"

### Swarm Workflow

When swarm mode is enabled, the workflow changes:

1. **Create Team**
   ```
   Teammate({ operation: "spawnTeam", team_name: "work-{timestamp}" })
   ```

2. **Create Unit List with Dependencies**
    - Parse plan into execution work items
   - Set up blockedBy relationships for sequential dependencies
   - Independent units have no blockers (can run in parallel)

3. **Spawn Specialized Teammates**
   ```
   Task({
     team_name: "work-{timestamp}",
     name: "implementer",
     subagent_type: "general-purpose",
      prompt: "Claim implementation units, execute, mark complete",
     run_in_background: true
   })

   Task({
     team_name: "work-{timestamp}",
     name: "tester",
     subagent_type: "general-purpose",
      prompt: "Claim testing units, run tests, mark complete",
     run_in_background: true
   })
   ```

4. **Coordinate and Monitor**
    - Team lead monitors unit completion
   - Spawn additional workers as phases unblock
   - Handle plan approval if required

5. **Cleanup**
   ```
   Teammate({ operation: "requestShutdown", target_agent_id: "implementer" })
   Teammate({ operation: "requestShutdown", target_agent_id: "tester" })
   Teammate({ operation: "cleanup" })
   ```

See the `orchestrating-swarms` skill for detailed swarm patterns and best practices.

**Note:** Swarm mode bypasses the orchestrated execution model (Phase 2). This means no execution session files, no learnings brief accumulation, no STATE.md, and no regression guard. Use swarm mode when speed and parallelism matter more than knowledge compounding. For features where execution learnings are valuable, use the standard orchestrated execution.

---

## Key Principles

### WHY Grounds Everything

- Every subagent knows why its unit exists, not just what to build
- The orchestrator is the guardian of WHY: it extracts, threads, and validates purpose
- Purpose drift is caught by inline reviews and Phase 3 validation, not just at the end
- If the combined work doesn't deliver the user story, passing tests don't matter

### Orchestrator is Lean, Subagents are Focused

- The orchestrator decomposes, delegates, records, and routes. It does NOT implement code itself.
- Each subagent gets only the context it needs. No conversation history pollution.
- Learnings compound: each unit benefits from everything learned in previous units.

### Start Fast, Execute Faster

- Get clarification once at the start, then execute
- Don't wait for perfect understanding - ask questions and move
- The goal is to **finish the feature**, not create perfect process

### The Plan is Your Guide

- Work documents should reference similar code and patterns
- Load those references and follow them
- Don't reinvent - match what exists

### Test As You Go

- Run tests after each change, not at the end
- Fix failures immediately
- Regression guard catches breakages early
- Continuous testing prevents big surprises

### Quality is Built In

- Follow existing patterns
- Write tests for new code
- Run linting before pushing
- Use reviewer agents for complex/risky changes only

### Ship Complete Features

- Mark all units completed before moving on
- Don't leave features 80% done
- A finished feature that ships beats a perfect feature that doesn't

### Failures are Handled Gracefully

- Escalation path (reframe, ask user, skip, stop) -- not infinite loops
- Progress is persistent: STATE.md means you can resume after crashes
- Regression is caught early: previous tests re-run after each unit
- When debugging unexpected errors, use the `systematic-debugging` skill for structured root-cause analysis instead of trial-and-error

## Quality Checklist

Before creating PR, verify:

- [ ] All clarifying questions asked and answered
- [ ] All units in STATE.md marked completed (or explicitly skipped with user approval)
- [ ] **User story deliverable** -- the combined work enables the stated user outcome
- [ ] **Success criteria met** -- every plan-level success criterion addressed (or gap documented)
- [ ] **Architecture intact** -- implementation matches the plan's architectural context
- [ ] Tests pass (run project's test command)
- [ ] Regression tests from all completed units pass
- [ ] Linting passes (use linting-agent)
- [ ] Code follows existing patterns
- [ ] Figma designs match implementation (if applicable)
- [ ] Before/after screenshots captured and uploaded (for UI changes)
- [ ] Commit messages follow conventional format
- [ ] PR description includes Post-Deploy Monitoring & Validation section (or explicit no-impact rationale)
- [ ] PR description includes summary, testing notes, and screenshots
- [ ] PR description includes Compound Engineered badge
- [ ] Execution session files saved in `docs/execution-sessions/`

## When to Use Reviewer Agents

**Don't use by default.** Use reviewer agents only when:

- Large refactor affecting many files (10+)
- Security-sensitive changes (authentication, permissions, data access)
- Performance-critical code paths
- Complex algorithms or business logic
- User explicitly requests thorough review

For most features: tests + linting + following patterns is sufficient.

## Common Pitfalls to Avoid

- **Losing the WHY** - Subagents build what's specified but miss the intent. Always pass WHY context.
- **Purpose drift** - Units individually pass but combined output doesn't deliver the user story. Validate at Phase 3.
- **Analysis paralysis** - Don't overthink, read the plan and execute
- **Skipping clarifying questions** - Ask now, not after building wrong thing
- **Ignoring plan references** - The plan has links for a reason
- **Testing at the end** - Test continuously or suffer later
- **Orchestrator doing implementation** - Delegate to subagents, don't implement inline
- **Skipping regression checks** - A passing unit that breaks previous work is not progress
- **Losing session state** - Always write to STATE.md before and after each unit
- **Dumping all session files into subagent context** - Use the learnings brief, filtered by domain
- **Over-reviewing simple changes** - Save reviewer agents for complex work
- **80% done syndrome** - Finish the feature, don't move on early
