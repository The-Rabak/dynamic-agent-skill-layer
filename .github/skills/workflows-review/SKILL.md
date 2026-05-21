---
name: workflows-review
description: Perform exhaustive code reviews grounded in the user story. Filters technical findings through WHY context to protect purpose while improving quality.
---

## Arguments
[branch name, file path, or empty for current branch]

# Review Command

<command_purpose> Perform exhaustive code reviews using multi-agent analysis, ultra-thinking, and Git worktrees for deep local inspection. Ground every finding in the plan's WHY context (problem narrative, user story, success criteria) so technical improvements never drift from user purpose. </command_purpose>

## Introduction

<role>Senior Code Review Architect and WHY Guardian. Your dual mandate: (1) surface every technical issue that matters, and (2) protect the user story from well-meaning technical drift. You will be bombarded with findings from technically-minded subagents — your job is to distill them through the lens of "does this serve the user's actual need?" A technically superior suggestion that doesn't deliver the user story is a regression, not an improvement.</role>

## Prerequisites

<requirements>
- Git repository
- Clean main/master branch (for diff baseline)
- Proper permissions to create worktrees and access the repository
- For document reviews: Path to a markdown file or document
</requirements>

## Main Tasks

### 1. Determine Review Target & Setup (ALWAYS FIRST)

<review_target> #$ARGUMENTS </review_target>

<thinking>
First, I need to determine the review target type and set up the code for analysis.
</thinking>

#### Immediate Actions:

<task_list>

- [ ] Determine review type: branch name, file path (.md), or empty (current branch)
- [ ] Check current git branch
- [ ] Determine the default branch:
  ```bash
  default_branch=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@')
  [ -z "$default_branch" ] && default_branch=$(git rev-parse --verify origin/main >/dev/null 2>&1 && echo "main" || echo "master")
  ```
- [ ] If ALREADY on the target branch → proceed with analysis
- [ ] If DIFFERENT branch → offer to use worktree: "Use git-worktree skill for isolated analysis" or `git checkout [branch]`
- [ ] Get the list of changed files:
  ```bash
  git diff --name-only ${default_branch}...HEAD
  ```
- [ ] Get the full diff for review:
  ```bash
  git diff ${default_branch}...HEAD
  ```
- [ ] Get commit messages for context:
  ```bash
  git log --oneline ${default_branch}..HEAD
  ```
- [ ] Set up language-specific analysis tools
- [ ] Prepare security scanning environment

Ensure that the code is ready for analysis (either in worktree or on current branch). ONLY then proceed to the next step.

</task_list>

#### Load WHY + Constitution Context (BEFORE running any agents)

<thinking>
Before any review agent runs, I need to understand WHY this code was built. Without this, I'm reviewing HOW code works without knowing WHAT it's supposed to achieve for users. Technical reviewers will suggest changes that are "better" in isolation but could drift the implementation from its purpose.
</thinking>

**Step 1: Find the plan file.** Check these locations in order:

```bash
# Check commit messages for plan references
git log --oneline ${default_branch}..HEAD | grep -i "plan\|docs/plans"

# Check for execution session that references a plan
ls docs/execution-sessions/work-*/state.md 2>/dev/null

#check for recent plan files
ls -t docs/plans/*-plan*.md 2>/dev/null | head -5

# Check for architecture files
ls -t docs/architecture/*.md 2>/dev/null | head -5

# Check for ticket sets
ls -t docs/tickets/*/index.md 2>/dev/null | head -5

# Check for readme files
ls -t README.md 2>/dev/null | head -5
```

If a plan file is found, read it and extract:
- **Problem Narrative** — why this work exists, what pain it solves
- **User Story** — who benefits and what outcome they get
- **Architectural Context** — how the solution fits in the system
- **Success Criteria** — measurable conditions that define "done"
- **brainstorm_ref** — path to brainstorm document, if available
- **architecture_ref / Related Artifacts** — pointer to the architecture artifact or handoff document, if available
- **tickets_ref / Related Artifacts** — pointer to the ticket set, if available
- **constitution_version** / **constitution_waivers** — what repo-wide rules apply and which exceptions were explicitly approved

If an `architecture_ref` or matching `docs/architecture/*.md` artifact exists, read it and extract:
- Feature Homes and Ownership
- Shared / Global Decisions
- Deepening Candidates
- Context Tiers
- Deletion Test decisions
- Interfaces as test surfaces
- Seams, Adapters, and Contracts
- Drift Checks
- Recommendations for `/workflows-review`

If no architecture artifact exists, build an explicit architecture handoff contract from the plan's Architectural Context, Key Decisions, Constitution Alignment, brainstorm context, and STATE.md notes. Do not review architecture implicitly.

If a `tickets_ref` or matching `docs/tickets/*/index.md` artifact exists, read the ticket index and the relevant ticket files. Extract:
- ticket order and statuses
- dependency graph
- feature homes and scope fences
- acceptance criteria and evidence commands
- any visible blocker notes or ticket-set review findings

ALWAYS READ THE ARCHITECTURE ARTIFACT (or explicit handoff contract) AND README FILES FOR CONTEXT — they often contain critical information about architectural intent, constraints, and domain knowledge that is not in the plan.

If a STATE.md execution session exists, also read its WHY Context and Architecture Handoff sections.

If `docs/constitution.md` exists, read it too and extract:
- core principles
- review guardrails
- required approvals / allowed exceptions
- active version

**Step 2: If no plan exists**, construct minimal WHY from available signals:

- Read commit messages for intent ("feat: add user auth", "fix: email validation")
- Look at the files changed to infer domain and scope
- Check PR description if available
- **Ask the user**: "I couldn't find a plan file for this branch. In one sentence, what problem does this code solve and for whom?" — this grounds the entire review.

**Step 3: Summarize the WHY + constitution context** that will be passed to every agent:

```
WHY CONTEXT FOR REVIEWERS:
- Problem: [problem narrative]
- User Story: [user story]  
- Success Criteria: [list]
- Architectural Intent: [arch context summary]
- Architecture Artifact: [docs/architecture/... path or none]
- Ticket Set: [docs/tickets/.../index.md or none]
- Architecture Handoff: [feature homes, shared/global decisions, context tiers, deletion-test decisions, interfaces as test surfaces, seams/adapters/contracts, and drift checks the implementation must honor]
- Constitution Version: [version or none]
- Constitution Guardrails: [relevant principles, review baselines, approvals, waivers]
```

This context is passed to EVERY review agent below. It is not optional.

#### TDD Evidence Gate (BEFORE reviewer dispatch)

If a `docs/execution-sessions/work-*/state.md` file exists for this branch, read the completed execution unit session files before dispatching review agents and build a terse evidence ledger.

Apply `references/tdd-evidence-contract.md` as the source of truth for the Ralph evidence block and review-gate classifications. Verify the plan's approved exception contract instead of improvising replacement evidence rules.

Classify gate failures explicitly:
- **Missing behavior coverage** — treat as a spec blocker.
- **Missing cleanup after refactor** — treat as a quality failure, escalating to blocker when behavior may have changed without a rerun.

Keep the gate output terse and evidence-based. If the gate fails, carry that failure into the final summary even if no reviewer agent finds anything else.

#### Protected Artifacts

<protected_artifacts>
The following paths are compound-engineering pipeline artifacts and must never be flagged for deletion, removal, or gitignore by any review agent:

- `docs/plans/*.md` — Plan files created by `/workflows-plan`. These are living documents that track implementation progress (checkboxes are checked off by `/workflows-work`).
- `docs/architecture/*.md` — Architecture improvement artifacts and handoff contracts created or referenced between planning, deepening, execution, and review.
- `docs/tickets/**/*.md` — Local ticket artifacts created by `/workflows-to-issues` and updated by ticket-scoped execution.
- `docs/solutions/*.md` — Solution documents created during the pipeline.

If a review agent flags any file in these directories for cleanup or removal, discard that finding during synthesis. Do not create a todo for it.
</protected_artifacts>

#### Load Review Agents

Read `compound-engineering.local.md` in the project root. If found, use `review_agents` from YAML frontmatter. If the markdown body contains review context, pass it to each agent as additional instructions.

If no settings file exists, invoke the `setup` skill to create one. Then read the newly created file and continue.

`review_agents` only decides **which** named review agents `/workflows-review` coordinates. It does **not** authorize direct reviewer dispatch from other workflows or ad hoc prompts. If any other workflow or skill needs named review-agent analysis, route that request through `/workflows-review` instead of spawning the reviewers directly, except for `/workflows-to-issues`, which may run `ticket-flow-auditor` as its explicit ticket-set completion gate before code exists.

Regardless of `review_agents`, `/workflows-review` still adds the mandatory reviewers `agent-native-reviewer`, `learnings-researcher`, `uncle-bob`, and `ticket-flow-auditor`. `/workflows-architecture` likewise always runs `architecture-strategist` and `uncle-bob`.

#### Parallel Agents to review the branch changes:

<parallel_tasks>

Run all configured review agents in parallel using Task tool. For each agent in the `review_agents` list:

Apply the shared `Named Agent Dispatch` protocol from `references/orchestration-protocol.md` before every named reviewer dispatch.

- Start with the bundled agent directory and load the local template when it exists.
- Fall back to OpenViking/global context only when no bundled template can be loaded.
- Quote the first non-empty line of the loaded template and record which source you used before dispatching.
- Include the loaded template's rules in the delegated prompt without paraphrasing away mandatory constraints.
- If any configured or mandatory reviewer cannot be loaded and quoted, report that the review is incomplete and stop rather than substituting a different reviewer or silently reducing coverage.
- Never dispatch a named agent by name alone.

```
Task {agent-name}(branch diff content + review context from settings body + WHY context block)
```

**Every agent prompt MUST include the WHY context block and architecture handoff block** from the step above. This ensures agents evaluate fitness-for-purpose, not just technical quality. After loading the template, dispatch each reviewer with a prompt like:

```
Review this branch diff for security issues.

WHY CONTEXT FOR REVIEWERS:
- Problem: [problem narrative]
- User Story: [user story]
- Success Criteria: [criteria list]
- Architectural Intent: [arch context]
- Architecture Artifact: [artifact path or "plan-derived handoff"]
- Ticket Set: [ticket index path or "none"]
- Architecture Handoff: [deletion test, interfaces, seams/adapters/contracts, review checks]

When reporting findings, note whether each finding:
(a) THREATENS the user story or success criteria (highest priority)
(b) Is a general security concern independent of the user story
(c) Would require changes that ALTER the user's intended outcome (flag as DRIFT RISK)
(d) Introduces feature-home drift or shared/global drift against the architecture handoff
(e) Introduces ticket drift, dependency drift, or undocumented scope expansion against the ticket set

Branch diff:
[diff content]
```

Additionally, always run these regardless of settings. These mandatory reviewers cannot be disabled by `review_agents`:
- Apply the protocol above to `agent-native-reviewer`, then dispatch it with branch diff content + WHY context - Verify new features are agent-accessible
- Apply the protocol above to `learnings-researcher`, then dispatch it with branch diff content + WHY context - Search docs/solutions/ for past issues related to this PR's modules and patterns
- Apply the protocol above to `uncle-bob`, then dispatch it with branch diff content + WHY context - Pressure-test naming, cohesion, feature-home boundaries, shared/global extractions, side effects, and long-term changeability
- Apply the protocol above to `ticket-flow-auditor`, then dispatch it with branch diff content + WHY context + ticket artifacts - Verify plan/ticket/implementation alignment, scope fences, dependency honesty, and execution drift

</parallel_tasks>

#### Conditional Agents (Run if applicable):

<conditional_agents>

These agents are run ONLY when the branch changes match specific criteria. Check the changed files list to determine if they apply. **Pass the WHY context block to all conditional agents as well.**

Apply the same template-loading rule to every named conditional agent below. Never dispatch them by name alone, and do not silently skip a required conditional reviewer once its trigger conditions are met.

**MIGRATIONS: If PR contains database migrations or data backfills:**

- Apply the protocol above to `data-integrity-guardian`, then dispatch it with branch diff content - Reviews migration safety, constraint naming, and migration conventions
- Apply the protocol above to `data-migration-expert`, then dispatch it with branch diff content - Validates ID mappings match production, checks for swapped values, verifies rollback safety
- Apply the protocol above to `deployment-verification-agent`, then dispatch it with branch diff content - Creates Go/No-Go deployment checklist with SQL verification queries


**When to run:**
- Changed files include `database/migrations/*.php`
- Changes modify columns that store IDs, enums, or mappings
- Changes include data backfill scripts or artisan commands
- Commit messages mention: migration, backfill, data transformation, ID mapping

**What these agents check:**
- `data-integrity-guardian`: Reviews migration safety, project conventions (separate files for table/indexes/FKs, constraint naming with `unq_`/`fk_`/`idx_` prefixes)
- `data-migration-expert`: Verifies hard-coded mappings match production reality (prevents swapped IDs), checks for orphaned associations, validates dual-write patterns
- `deployment-verification-agent`: Produces executable pre/post-deploy checklists with SQL queries, rollback procedures, Horizon monitoring plans

</conditional_agents>

### 4. Ultra-Thinking Deep Dive Phases

<ultrathink_instruction> For each phase below, spend maximum cognitive effort. Think step by step. Consider all angles. Question assumptions. And bring all reviews in a synthesis to the user.</ultrathink_instruction>

<deliverable>
Complete system context map with component interactions
</deliverable>

#### Phase 3: Stakeholder Perspective Analysis

<thinking_prompt> ULTRA-THINK: Put yourself in each stakeholder's shoes. Use the WHY context to ground each perspective in the ACTUAL problem being solved, not generic questions. </thinking_prompt>

<stakeholder_perspectives>

1. **The User from the User Story** <questions>

   Using the actual user story extracted above:
   - Can this user achieve the stated outcome with the code as implemented?
   - Are the success criteria from the plan actually met by this code?
   - What could prevent the user from getting the value they were promised?
   - Are error states handled in a way that helps THIS user recover? </questions>

2. **Developer Perspective** <questions>

   - How easy is this to understand and modify?
   - Are the APIs intuitive?
   - Is debugging straightforward?
   - Can I test this easily? </questions>

3. **Operations Perspective** <questions>

   - How do I deploy this safely?
   - What metrics and logs are available?
   - How do I troubleshoot issues?
   - What are the resource requirements? </questions>

4. **Security Team Perspective** <questions>

   - What's the attack surface introduced by this specific feature?
   - Are there compliance requirements for the data this feature handles?
   - How is user data protected in the context of this user story?
   - What are the audit capabilities? </questions>

5. **Business Perspective** <questions>
   - Does this code actually solve the stated problem? (from the Problem Narrative)
   - Are there legal/compliance risks specific to this feature?
   - Does the implementation match the architectural intent, or has it drifted?
   - Is the scope contained — did implementation creep beyond the user story? </questions> </stakeholder_perspectives>

#### Phase 4: Scenario Exploration

<thinking_prompt> ULTRA-THINK: Explore edge cases and failure scenarios. What could go wrong? How does the system behave under stress? </thinking_prompt>

<scenario_checklist>

- [ ] **Happy Path**: Normal operation with valid inputs
- [ ] **Invalid Inputs**: Null, empty, malformed data
- [ ] **Boundary Conditions**: Min/max values, empty collections
- [ ] **Concurrent Access**: Race conditions, deadlocks
- [ ] **Scale Testing**: 10x, 100x, 1000x normal load
- [ ] **Network Issues**: Timeouts, partial failures
- [ ] **Resource Exhaustion**: Memory, disk, connections
- [ ] **Security Attacks**: Injection, overflow, DoS
- [ ] **Data Corruption**: Partial writes, inconsistency
- [ ] **Cascading Failures**: Downstream service issues </scenario_checklist>

### 6. Multi-Angle Review Perspectives

#### Technical Excellence Angle

- Code craftsmanship evaluation
- Engineering best practices
- Technical documentation quality
- Tooling and automation assessment

#### Purpose Delivery Angle

- **User story delivery**: Does the code enable the stated user outcome?
- **Success criteria coverage**: Which criteria are met, partially met, or unmet?
- **Scope containment**: Was anything built beyond what the user story requires?
- **Architectural fidelity**: Does implementation match the planned architecture, or has it drifted?
- **Architecture handoff fidelity**: Does implementation honor the artifact or explicit handoff decisions about deletion test, interfaces, seams, adapters, and contracts?

#### Risk Management Angle

- Security risk assessment (grounded in what data/flows THIS feature handles)
- Operational risk evaluation
- Compliance risk verification
- Technical debt accumulation
- **User story risk**: What could prevent the user from achieving their stated outcome?

#### Team Dynamics Angle

- Code review etiquette
- Knowledge sharing effectiveness
- Collaboration patterns
- Mentoring opportunities

### 4. Simplification and Minimalism Review

Apply the shared `Named Agent Dispatch` protocol above to `code-simplicity-reviewer`, then dispatch it to see if the code can be simplified. If the template cannot be quoted from a loaded source, stop and report that gap instead of dispatching blindly.

### 5. Findings Synthesis and Todo Creation Using file-todos Skill

<critical_requirement> ALL findings MUST be stored in the todos/ directory using the file-todos skill. Create todo files immediately after synthesis - do NOT present findings for user approval first. Use the skill for structured todo management. </critical_requirement>

#### Step 1: Synthesize All Findings Through WHY Filter

<thinking>
I am about to be hit with a firehose of technical findings from technically-minded agents. My job is NOT to pass them all through — it's to DISTILL them. Every finding must be evaluated: does acting on this finding serve or harm the user story? Technically superior suggestions that don't deliver the user story are regressions, not improvements.
</thinking>

<synthesis_tasks>

- [ ] Collect findings from all parallel agents
- [ ] Surface learnings-researcher results: if past solutions are relevant, flag them as "Known Pattern" with links to docs/solutions/ files
- [ ] Discard any findings that recommend deleting or gitignoring files in `docs/plans/` or `docs/solutions/` (see Protected Artifacts above)

**WHY-grounded classification (apply to EVERY finding before severity):**

For each finding, ask: "If we act on this finding, what happens to the user story?"

- **🎯 PROTECTS USER STORY** — Finding addresses something that could prevent the user from achieving the stated outcome. (e.g., security hole in the auth flow when the user story is about secure login). These get elevated priority.
- **🏛️ CONSTITUTION VIOLATION** — Implementation or recommendation conflicts with a repo-wide MUST / MUST NOT rule, or bypasses a required approval, without an explicit waiver. These should be treated as blocking unless the constitution is amended or the waiver is approved.
- **⚠️ DRIFT RISK** — Finding suggests a change that is technically valid but would ALTER the user's intended outcome or expand scope beyond the user story. (e.g., "refactor to microservices" when the plan says monolith, or "add OAuth support" when the story only mentions password login). **These must be flagged prominently and NEVER auto-applied.** Present to user with: "This suggestion is technically sound but would change what the feature delivers."
- **🔧 QUALITY IMPROVEMENT** — Finding improves code quality without affecting the user story positively or negatively. Standard review finding. Keep severity as-is.
- **📦 SCOPE EXPANSION** — Finding suggests adding functionality not in the user story or success criteria. Automatically downgrade to P3 regardless of agent-assigned severity, and tag as "Beyond current scope."

- [ ] Categorize by type: security, performance, architecture, quality, etc.
- [ ] Assign severity levels: 🔴 CRITICAL (P1), 🟡 IMPORTANT (P2), 🔵 NICE-TO-HAVE (P3)
  - **Override rule**: Findings classified as PROTECTS USER STORY get +1 severity bump (P3→P2, P2→P1)
  - **Override rule**: Findings classified as CONSTITUTION VIOLATION are blocking by default unless a valid waiver exists
  - **Override rule**: Findings classified as SCOPE EXPANSION get capped at P3
- [ ] Remove duplicate or overlapping findings
- [ ] Estimate effort for each finding (Small/Medium/Large)
- [ ] **User Story Delivery Assessment**: After classifying all findings, state:
  - "Does the implementation, AS REVIEWED, deliver the user story? YES / PARTIALLY / NO"
  - If PARTIALLY or NO, list which success criteria are unmet and why

</synthesis_tasks>

#### Step 2: Create Todo Files Using file-todos Skill

<critical_instruction> Use the file-todos skill to create todo files for ALL findings immediately. Do NOT present findings one-by-one asking for user approval. Create all todo files in parallel using the skill, then summarize results to user. </critical_instruction>

**Implementation Options:**

**Option A: Direct File Creation (Fast)**

- Create todo files directly using Write tool
- All findings in parallel for speed
- Use standard template from `.github/skills/file-todos/assets/todo-template.md`
- Follow naming convention: `{issue_id}-pending-{priority}-{description}.md`

**Option B: Sub-Agents in Parallel (Recommended for Scale)** For large PRs with 15+ findings, use sub-agents to create finding files in parallel:

```bash
# Launch multiple finding-creator agents in parallel
Task() - Create todos for first finding
Task() - Create todos for second finding
Task() - Create todos for third finding
etc. for each finding.
```

Sub-agents can:

- Process multiple findings simultaneously
- Write detailed todo files with all sections filled
- Organize findings by severity
- Create comprehensive Proposed Solutions
- Add acceptance criteria and work logs
- Complete much faster than sequential processing

**Execution Strategy:**

1. Synthesize all findings into categories (P1/P2/P3)
2. Group findings by severity
3. Launch 3 parallel sub-agents (one per severity level)
4. Each sub-agent creates its batch of todos using the file-todos skill
5. Consolidate results and present summary

**Process (Using file-todos Skill):**

1. For each finding:

   - Determine severity (P1/P2/P3), applying the WHY override rules from synthesis
   - **Tag with WHY classification**: 🎯 PROTECTS USER STORY / ⚠️ DRIFT RISK / 🔧 QUALITY IMPROVEMENT / 📦 SCOPE EXPANSION
   - **Note which success criterion** this finding affects (or "None — general quality")
   - Write detailed Problem Statement and Findings
   - For DRIFT RISK findings: explicitly state what would change about the user's outcome if the suggestion is followed
   - Create 2-3 Proposed Solutions with pros/cons/effort/risk
   - Estimate effort (Small/Medium/Large)
   - Add acceptance criteria and work log

2. Use file-todos skill for structured todo management:

   ```bash
   skill: file-todos
   ```

   The skill provides:

   - Template location: `.github/skills/file-todos/assets/todo-template.md`
   - Naming convention: `{issue_id}-{status}-{priority}-{description}.md`
   - YAML frontmatter structure: status, priority, issue_id, tags, dependencies
   - All required sections: Problem Statement, Findings, Solutions, etc.

3. Create todo files in parallel:

   ```bash
   {next_id}-pending-{priority}-{description}.md
   ```

4. Examples:

   ```
   001-pending-p1-path-traversal-vulnerability.md
   002-pending-p1-api-response-validation.md
   003-pending-p2-concurrency-limit.md
   004-pending-p3-unused-parameter.md
   ```

5. Follow template structure from file-todos skill: `.github/skills/file-todos/assets/todo-template.md`

**Todo File Structure (from template):**

Each todo must include:

- **YAML frontmatter**: status, priority, issue_id, tags, dependencies
- **Problem Statement**: What's broken/missing, why it matters
- **Findings**: Discoveries from agents with evidence/location
- **Proposed Solutions**: 2-3 options, each with pros/cons/effort/risk
- **Recommended Action**: (Filled during triage, leave blank initially)
- **Technical Details**: Affected files, components, database changes
- **Acceptance Criteria**: Testable checklist items
- **Work Log**: Dated record with actions and learnings
- **Resources**: Links to PR, issues, documentation, similar patterns

**File naming convention:**

```
{issue_id}-{status}-{priority}-{description}.md

Examples:
- 001-pending-p1-security-vulnerability.md
- 002-pending-p2-performance-optimization.md
- 003-pending-p3-code-cleanup.md
```

**Status values:**

- `pending` - New findings, needs triage/decision
- `ready` - Approved by manager, ready to work
- `complete` - Work finished

**Priority values:**

- `p1` - Critical (blocks merge, security/data issues)
- `p2` - Important (should fix, architectural/performance)
- `p3` - Nice-to-have (enhancements, cleanup)

**Tagging:** Always add `code-review` tag, plus: `security`, `performance`, `architecture`, `laravel`, `vue`, `quality`, etc.

#### Step 3: Summary Report

After creating all todo files, present comprehensive summary:

````markdown
## ✅ Code Review Complete

**Review Target:** Branch `[branch-name]` (vs `[default-branch]`)

### User Story Delivery Assessment

**User Story:** [user story from plan]
**Delivery Status:** ✅ DELIVERS / ⚠️ PARTIALLY DELIVERS / ❌ DOES NOT DELIVER
**Architecture Basis:** [docs/architecture/... path or "plan-derived handoff contract"]

### TDD Evidence Gate

- **Behavior coverage:** PASS / FAIL — [unit/session refs with weak or missing `Red`/`Green` evidence]
- **Cleanup after refactor:** PASS / FAIL — [unit/session refs with weak or missing `Post-Refactor Green` evidence]

[If PARTIALLY or NO:]
**Gaps:**
- [Success criterion X]: not met because [reason]
- [Success criterion Y]: partially met — [what's missing]

### WHY-Grounded Findings Summary:

- **Total Findings:** [X]
- **🎯 Protects User Story:** [count] — findings that address threats to the user's outcome
- **🏛️ Constitution Violations:** [count] — unwaived conflicts with repo-wide project rules
- **⚠️ Drift Risk:** [count] — suggestions that would ALTER what the feature delivers (review carefully)
- **🔧 Quality Improvements:** [count] — standard technical improvements
- **📦 Scope Expansion:** [count] — suggestions beyond current user story (consider for future work)

### Severity Breakdown:

- **🔴 CRITICAL (P1):** [count] - BLOCKS MERGE
- **🟡 IMPORTANT (P2):** [count] - Should Fix
- **🔵 NICE-TO-HAVE (P3):** [count] - Enhancements

### Created Todo Files:

**P1 - Critical (BLOCKS MERGE):**

- `001-pending-p1-{finding}.md` - {description}
- `002-pending-p1-{finding}.md` - {description}

**P2 - Important:**

- `003-pending-p2-{finding}.md` - {description}
- `004-pending-p2-{finding}.md` - {description}

**P3 - Nice-to-Have:**

- `005-pending-p3-{finding}.md` - {description}

### Review Agents Used:

- rabak-laravel-reviewer
- rabak-vue-reviewer
- security-sentinel
- performance-oracle
- architecture-strategist
- agent-native-reviewer
- [other agents]

### Next Steps:

1. **Address P1 Findings**: CRITICAL - must be fixed before merge

   - Review each P1 todo in detail
   - Implement fixes or request exemption
   - Verify fixes before merging PR

2. **Review Drift Risk Findings**: These require your decision — they suggest changes that would alter what the feature delivers. Accept, reject, or modify each one. If the drift is architectural, update the architecture artifact or explicit handoff contract before more implementation proceeds.

3. **Triage All Todos**:
   ```bash
   ls todos/*-pending-*.md  # View all pending todos
   /triage                  # Use slash command for interactive triage
   ```
````

4. **Work on Approved Todos**:

   ```bash
   /resolve_todo_parallel  # Fix all approved items efficiently
   ```

5. **Track Progress**:
   - Rename file when status changes: pending → ready → complete
   - Update Work Log as you work
   - Commit todos: `git add todos/ && git commit -m "refactor: add code review findings"`

```

### 7. End-to-End Testing (Optional)

<offer_testing>

After presenting the Summary Report, offer browser testing:

```markdown
**"Want to run browser tests on the affected pages?"**
1. Yes - run `/test-browser`
2. No - skip
```

</offer_testing>

#### If User Accepts Testing:

Spawn a subagent to run browser tests (preserves main context):

```
Use the general-purpose skill to: "Run /test-browser for the current branch. Test all affected pages, check for console errors, handle failures by creating todos and fixing."
```

The subagent will:
1. Identify pages affected by the PR
2. Navigate to each page and capture snapshots (using agent-browser CLI)
3. Check for console errors
4. Test critical interactions
5. Pause for human verification on OAuth/email/payment flows
6. Create P1 todos for any failures
7. Fix and retry until all tests pass

**Standalone:** `/test-browser`

### Important: P1 Findings and User Story Delivery Block Merge

Any **🔴 P1 (CRITICAL)** findings must be addressed before merging. Additionally, if the User Story Delivery Assessment is **❌ DOES NOT DELIVER**, the branch should not be merged regardless of P1 count — the code doesn't solve the stated problem.

Any **Missing behavior coverage** TDD gate failure should be treated as merge-blocking even if the implementation currently appears to work; review requires proof, not claims.

Any **Missing cleanup after refactor** TDD gate failure should block merge when cleanup/refactor happened without a trustworthy post-refactor rerun. Otherwise keep it as an important quality finding until the rerun evidence is repaired.

Any unwaived **🏛️ CONSTITUTION VIOLATION** findings should also block merge until the code is fixed, the waiver is explicitly approved, or the constitution is amended.

Any **⚠️ DRIFT RISK** findings must be explicitly reviewed by the user before acting on them. Never auto-resolve drift risk findings — they require a human decision about whether the user story should change.
```
