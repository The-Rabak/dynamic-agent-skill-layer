---
name: deepen-plan
description: Enhance a plan with parallel research agents grounded in user story, architectural context, and success criteria to add depth without losing purpose
---

## Arguments
[path to plan file]

# Deepen Plan - Power Enhancement Mode

## Introduction

**Note: The current year is 2026.** Use this when searching for recent documentation and best practices.

This command takes an existing plan (from `/workflows-plan`) and, when available, the architecture improvement artifact from `/workflows-architecture`. If no artifact exists yet, it must assemble an **explicit architecture handoff contract** from the plan instead of treating architecture as hidden context. Every enhancement is **grounded in the plan's WHY artifacts** -- the problem narrative, user story, architectural context, success criteria, the explicit architecture contract, and the plan's TDD/evidence contract -- so that deepening adds purpose-aligned depth, not generic complexity.

Each major element gets its own dedicated research sub-agent to find:
- Best practices and industry patterns **relevant to the user story**
- Performance optimizations **that matter for the stated success criteria**
- UI/UX improvements (if applicable) **aligned with the user's needs**
- Quality enhancements and edge cases **that could threaten success criteria**
- Real-world implementation examples **in similar architectural contexts**

The result is a deeply grounded, production-ready plan that remains tightly coupled to WHY we're building it while honoring the deletion-test, interface, seam, and adapter decisions captured in the architecture artifact or explicit architecture handoff contract. Deepening may add detail, remove unnecessary detail, or defer optional complexity when that yields a clearer and more executable plan.

## Plan File

<plan_path> #$ARGUMENTS </plan_path>

**If the plan path above is empty:**
1. Check for recent plans: `ls -la docs/plans/`
2. Ask the user: "Which plan would you like to deepen? Please provide the path (e.g., `docs/plans/2026-01-15-feat-my-feature-plan.md`)."

Do not proceed until you have a valid plan file path.

## Main Tasks

### 1. Parse and Analyze Plan Structure

<thinking>
First, read and parse the plan to extract the WHY artifacts (problem narrative, user story, architectural context, success criteria) and identify each major section that can be enhanced with research. The WHY artifacts are the lens through which all deepening is filtered.
</thinking>

**Read the plan file and extract WHY artifacts first:**

- [ ] **Problem Narrative** -- the synthesized WHY (who has the problem, what triggers it, impact)
- [ ] **User Story** -- the north star (As a [persona], I need to [action] so that [outcome])
- [ ] **Architectural Context** -- the WHERE map (lives in, interacts with, entry point, data, dependencies)
- [ ] **Success Criteria** -- the DONE definition (measurable outcomes tied to user story)
- [ ] **`handoff` frontmatter** -- check all fields are `true`; if any are `false` or missing, flag: "Plan is missing [X]. Deepening may add technically correct but purpose-misaligned enhancements. Consider running `/workflows-plan` to fill gaps first."
- [ ] **`tdd` frontmatter + `## TDD & Evidence Contract`** -- extract precedence, mode, loop, unit/e2e evidence expectations, and any exceptions
- [ ] Use `references/tdd-evidence-contract.md` to resolve the effective TDD contract: plan values override local defaults, `inherit` falls back, and no-local-config falls back to Ralph-driven `red-green-refactor` with unit + e2e evidence required
- [ ] If the plan weakens Ralph/unit+e2e without a justification, flag it and add a justified exception before continuing
- [ ] If the plan is missing the `tdd` block or the `## TDD & Evidence Contract` section, add them using the resolved local/fallback defaults before deepening other sections

**Check for brainstorm reference:**

- [ ] Read `brainstorm_ref` from plan frontmatter
- [ ] If a brainstorm path exists, read it and extract additional context:
  - Stakeholder Impact (who is affected and how)
  - Key Decisions and rationale
  - Approaches Considered and why they were rejected
  - Resolved Questions (context that informed decisions)
- [ ] This additional context helps research agents make purpose-aligned recommendations

**Check for architecture artifact or explicit handoff contract:**

- [ ] Read `architecture_ref` from plan frontmatter
- [ ] If an architecture path exists, read it and extract:
  - Feature Homes and Ownership
  - Shared / Global Decisions
  - Deepening Candidates
  - Context Tiers
  - Deletion Test decisions
  - Interfaces as test surfaces
  - Seams, Adapters, and Contracts
  - Drift Checks
  - Recommendations for `/deepen-plan`, `/workflows-work`, and `/workflows-review`
- [ ] If no `architecture_ref` exists, check `docs/architecture/*.md` for a recent artifact that matches the plan topic
- [ ] If no architecture artifact exists, build an explicit architecture handoff contract from the plan's Architectural Context, Key Decisions, Constitution Alignment, brainstorm context, and any `## Related Artifacts` section
- [ ] Record whether deepening used a real artifact or a plan-derived handoff contract so `/workflows-work` and `/workflows-review` inherit the same structural guidance
- [ ] If no architecture artifact exists, continue but flag: "No architecture artifact found. Consider running `/workflows-architecture` before deepening so structural decisions are explicit."

**Then extract plan structure:**

- [ ] Overview/Proposed Solution sections
- [ ] Technical Approach/Architecture
- [ ] `execution_shape` frontmatter + `## Execution Shape` section
- [ ] Execution packets / phase wrappers (noting which user story aspect each packet serves)
- [ ] Code examples and file references
- [ ] Acceptance criteria
- [ ] Any UI/UX components mentioned
- [ ] Technologies/frameworks mentioned (Laravel, Vue.js, Nuxt, Python, TypeScript, etc.)
- [ ] Domain areas (data models, APIs, UI, security, performance, etc.)

**Create a section manifest with WHY linkage:**
```
Section 1: [Title] - [Brief description of what to research] - Serves: [user story aspect / success criterion]
Section 2: [Title] - [Brief description of what to research] - Serves: [user story aspect / success criterion]
...
```

The "Serves" column ensures every deepening activity traces back to WHY we're building this.

### 1.1 Validate Execution Readiness

<thinking>
Check if the plan has sufficiently structured execution packets for the subagent orchestration model in `/workflows-work`. Use `references/execution-shape.md` as the source of truth. Plans need packets that are independently executable, testable, and traceable back to the user story without forcing fake verticality.
</thinking>

**Resolve execution shape first:**

- [ ] Read `execution_shape.mode`; if missing, default it to `vertical-slices`
- [ ] Read `execution_shape.rationale`; require it when the mode is not `vertical-slices`
- [ ] Ensure the body includes a matching `## Execution Shape` section
- [ ] If the mode is `vertical-slices`, also apply `references/vertical-slice-architecture.md` for feature-home and shared/global boundary checks
- [ ] If the chosen mode looks wrong for the real work, add a `### WHY Reassessment` note instead of silently changing it

**Scan each execution packet using the required fields from `references/execution-shape.md`:**

- [ ] **`vertical-slices`:** slice type, serves, demo scenario, feature home, scope + scope fence, files, depends on, dependency type, success criteria, test command
- [ ] **`infra-track`:** capability enabled, consumers / downstream work unlocked, scope, files, depends on, risk / rollback, validation command, success criteria
- [ ] **`fix-batch`:** problem, repro / expected outcome, files, depends on, validation command, success criteria
- [ ] **TDD alignment:** packet-level validation commands collectively satisfy the resolved unit/e2e evidence contract, or the plan records a justified exception with replacement evidence

**Validate WHY tracing:**

- [ ] **Each packet has a purpose line** (`Serves`, `Consumers`, or equivalent) tying it to user story value or explicit downstream unlocks
- [ ] **Success criteria trace to plan-level success criteria** -- packet criteria should be decomposed from the plan's success criteria, not invented independently
- [ ] **No orphan packets** -- every packet should trace to at least one success criterion or explicit enabling outcome
- [ ] **Phase wrappers stay optional** -- if the plan uses phases or tracks, confirm they are grouping containers only and do not replace packet-level tracing

**Expected packet format:**

```markdown
##### Slice N.1: [Slice Title]
**Slice type:** tracer-bullet | expansion | hardening
**Serves:** [Which aspect of the user story / which success criterion this slice delivers]
**Demo scenario:** [Smallest end-to-end behavior this slice proves]
**Feature home:** `path/to/feature-home/`
**Files:** `path/to/file1.php`, `path/to/file2.php`
**Depends on:** Slice N-1.2 (or "None")
**Dependency type:** real | stub-available | parallel-safe

###### Scope
- **Owns:** [What this slice changes]
- **Non-goals:** [What intentionally waits]
- **Scope fence:** [Boundary that keeps the slice thin]

###### Acceptance criteria
- [ ] Criterion 1
- [ ] Criterion 2

###### Evidence
- **Test command:** `command to run`
```

**Scoring:**

Count how many execution packets have the full structure. Report:

```
Execution Readiness: X/Y packets have complete structure (Z%)
```

**Actions based on score:**

| Score | Action |
|-------|--------|
| 80-100% | Plan is execution-ready. Proceed with deepening. |
| 50-79% | Flag incomplete packets. During deepening, add missing fields. |
| 0-49% | Plan needs significant restructuring. Add an "Execution Readiness" enhancement pass that decomposes vague phases/tasks into the packet shape required by the selected mode. **Note:** `/workflows-work` will refuse to execute plans that lack a coherent execution shape unless the user explicitly approves a mode change or legacy adaptation. |

**For packets missing structure, the deepening process should:**

1. Pick or confirm the execution shape that best matches the real work
2. Break vague phases or legacy tasks into specific packets for that mode
3. Identify the smallest honest outcome each packet proves or unlocks
4. Identify which files each packet will create or modify
5. Write concrete success criteria (not vague goals)
6. Determine the validation command (look at existing test patterns in the codebase)
7. Make it explicit whether the validation command contributes unit evidence, e2e evidence, or both
8. Map dependencies between packets
9. Add a suggested commit message per packet (conventional format: `feat(scope): description`)

### 1.2 Execution Shape Complexity Check

<thinking>
Check if any packets are too large, too vague, or shaped incorrectly for reliable subagent execution. Cross-layer work is allowed in `vertical-slices`; the failure mode is not "touches backend and frontend" but "tries to deliver multiple outcomes or no honest outcome at all."
</thinking>

**For each packet, check complexity against the selected mode:**

| Metric | Threshold | Action |
|--------|-----------|--------|
| Outcomes or unlocks | > 1 meaningful outcome | Flag for splitting |
| Files touched | > 6 files | Flag for review; confirm the packet is still thin |
| Success criteria | > 5 criteria | Flag for splitting |
| Scope fence | Missing or vague | Flag for clarification |
| Shape fit | `vertical-slices` used for horizontal-only work, or `infra-track` / `fix-batch` used to hide a real feature slice | Reassess mode |
| Risk controls | `Blast radius: high` with no rollback path | Add safety fields before execution |

**Important:** A packet that touches backend + frontend is **not automatically too large**. If the same thin slice needs a migration, service method, API handler, and tiny UI change to prove one observable behavior, keep it intact.

**If any slices exceed thresholds:**

Report:
```
Execution Shape Warning: X packets may be too large or incorrectly shaped for reliable subagent execution.

Slice 2.1: "User can complete first login tracer bullet" -- 2 demo scenarios, 7 success criteria
  Suggestion: Split into "User submits credentials and receives success state" and "User sees first authenticated dashboard shell"

Packet 3.2: "Create auth schema foundation" -- no demo scenario, horizontal-only outcome
  Suggestion: Either rewrite as "User can submit credentials and persist the first auth record" or switch this track to `infra-track` if it is truly enablement-only
```

Suggest splits that create self-contained packets with clear ownership and non-overlapping file sets where possible. **When splitting, ensure each new packet retains its tracing to the user story or enabling outcome.**

**This validation ensures the plan is ready for `/workflows-work`'s subagent orchestration model**, where each packet is delegated to a focused subagent with clear scope, proof, and termination criteria.

### 1.5 Re-fetch Source Documents (if available)

Check the plan's YAML frontmatter for `source_docs:`. If present, re-fetch the original documents for deeper analysis:

**For each source doc URL in `source_docs.tickets`, `source_docs.docs`, `source_docs.figma`:**

Launch parallel subagents to re-read the full documents (not just summaries this time):

```
Task general-purpose: "Re-read this source document in full detail for plan deepening.

URL: [url]
Type: [tickets|docs|figma]

Fetch the complete document content using this strategy:
1. Try ToolSearch to find any relevant MCP tools available
2. If MCP tools found, use them to fetch the document
3. If no MCP tools, try WebFetch on the URL
4. Last resort: output 'MANUAL_INPUT_NEEDED: Could not access [url]. Ask user to paste content.'

Focus on extracting:
- Detailed acceptance criteria and edge cases
- Technical constraints not captured in the summary
- Dependencies and integration points
- Any updates since the plan was created (check timestamps)
- Any user story or problem context that was missed or summarized too aggressively in the plan

Return the FULL content, not a summary. This will be used to ground the plan in source-of-truth documents."
```

Feed the full document contents to all subsequent deepening agents as additional context, alongside the WHY artifacts extracted in Step 1.

### 2. Discover and Apply Available Skills

<thinking>
Dynamically discover all available skills and match them to plan sections. Don't assume what skills exist - discover them at runtime.
</thinking>

**Step 1: Discover ALL available skills from ALL sources**

```bash
# 1. Project-local skills (highest priority - project-specific)
ls [project skill dir]

# 2. User's platform-global skills (for example ~/.copilot/skills or ~/.config/opencode/skills)
ls [platform global skills dir]

# 3. Installed plugin/package skills if the harness exposes them
ls [installed plugin skill dirs]

# 4. Broad fallback: search every discovered plugin/package location for skills
find [platform plugin roots] -type d -name "skills" 2>/dev/null

# 5. If the harness exposes plugin metadata, inspect it to find additional local skill locations
cat [installed plugin metadata file]
```

**Important:** Check EVERY source. Don't assume compound-engineering is the only plugin. Use skills from ANY installed plugin that's relevant.

**Step 2: For each discovered skill, read its SKILL.md to understand what it does**

```bash
# For each skill directory found, read its documentation
cat [skill-path]/skill.md
```

**Step 3: Match skills to plan content**

For each skill discovered:
- Read its SKILL.md description
- Check if any plan sections match the skill's domain
- If there's a match, spawn a sub-agent to apply that skill's knowledge

**Step 4: Spawn a sub-agent for EVERY matched skill**

**CRITICAL: For EACH skill that matches, spawn a separate sub-agent and instruct it to USE that skill.**

For each matched skill:
```
Task general-purpose: "You have the [skill-name] skill available at [skill-path].

YOUR JOB: Use this skill on the plan.

1. Read the skill: cat [skill-path]/skill.md
2. Follow the skill's instructions exactly
3. Apply the skill to this content:

[relevant plan section or full plan]

WHY CONTEXT (use this to ground the skill's recommendations):
- Problem: [problem narrative]
- User Story: [user story]
- Success Criteria: [success criteria]

4. Return the skill's full output. Filter recommendations that don't serve the user story.

The skill tells you what to do - follow it. Execute the skill completely."
```

Always use the discovered `[skill-path]` and read `SKILL.md` from that exact location. Do not hardcode Claude-specific paths when spawning skill subagents.

**Spawn ALL skill sub-agents in PARALLEL:**
- 1 sub-agent per matched skill
- Each sub-agent reads and uses its assigned skill
- All run simultaneously
- 10, 20, 30 skill sub-agents is fine

**Each sub-agent:**
1. Reads its skill's SKILL.md
2. Follows the skill's workflow/instructions
3. Applies the skill to the plan
4. Returns whatever the skill produces (code, recommendations, patterns, reviews, etc.)

**Example spawns:**
```
Task general-purpose: "Use the laravel-conventions skill at [discovered skill path]. Read SKILL.md and apply it to: [Laravel sections of plan]"

Task general-purpose: "Use the frontend-design skill at [discovered skill path]. Read SKILL.md and apply it to: [UI sections of plan]"

Task general-purpose: "Use the agent-native-architecture skill at [discovered skill path]. Read SKILL.md and apply it to: [agent/tool sections of plan]"

Task general-purpose: "Use the security-patterns skill at [discovered skill path]. Read SKILL.md and apply it to: [full plan]"
```

**No limit on skill sub-agents. Spawn one for every skill that could possibly be relevant.**

### 3. Discover and Apply Learnings/Solutions

<thinking>
Check for documented learnings from /workflows-compound. These are solved problems stored as markdown files. Spawn a sub-agent for each learning to check if it's relevant.
</thinking>

**LEARNINGS LOCATION - Check these exact folders:**

```
docs/solutions/           <-- PRIMARY: Project-level learnings (created by /workflows-compound)
├── performance-issues/
│   └── *.md
├── debugging-patterns/
│   └── *.md
├── configuration-fixes/
│   └── *.md
├── integration-issues/
│   └── *.md
├── deployment-issues/
│   └── *.md
└── [other-categories]/
    └── *.md
```

**Step 1: Find ALL learning markdown files**

Run these commands to get every learning file:

```bash
# PRIMARY LOCATION - Project learnings
find docs/solutions -name "*.md" -type f 2>/dev/null

# If docs/solutions doesn't exist, check alternate locations:
find .github/docs -name "*.md" -type f 2>/dev/null
find ~/.copilot/docs -name "*.md" -type f 2>/dev/null
```

**Step 2: Read frontmatter of each learning to filter**

Each learning file has YAML frontmatter with metadata. Read the first ~20 lines of each file to get:

```yaml
---
title: "N+1 Query Fix for Briefs"
category: performance-issues
tags: [eloquent, n-plus-one, eager-loading, with]
module: Briefs
symptom: "Slow page load, multiple queries in logs"
root_cause: "Missing eager loading with() on relationship"
---
```

**For each .md file, quickly scan its frontmatter:**

```bash
# Read first 20 lines of each learning (frontmatter + summary)
head -20 docs/solutions/**/*.md
```

**Step 3: Filter - only spawn sub-agents for LIKELY relevant learnings**

Compare each learning's frontmatter against the plan (both technical content AND WHY artifacts):
- `tags:` - Do any tags match technologies/patterns in the plan?
- `category:` - Is this category relevant? (e.g., skip deployment-issues if plan is UI-only)
- `module:` - Does the plan touch this module?
- `symptom:` / `root_cause:` - Could this problem occur with the plan?
- **WHY match** - Does the learning's domain relate to the user story or architectural context? (e.g., a caching learning is relevant if the user story involves performance even if the plan doesn't explicitly mention caching yet)

**SKIP learnings that are clearly not applicable:**
- Plan is frontend-only → skip `database-migrations/` learnings
- Plan is Python → skip `laravel-specific/` learnings
- Plan has no auth → skip `authentication-issues/` learnings

**SPAWN sub-agents for learnings that MIGHT apply:**
- Any tag overlap with plan technologies
- Same category as plan domain
- Similar patterns or concerns

**Step 4: Spawn sub-agents for filtered learnings**

For each learning that passes the filter:

```
Task general-purpose: "
LEARNING FILE: [full path to .md file]

1. Read this learning file completely
2. This learning documents a previously solved problem

Check if this learning applies to this plan:

USER STORY: [user story]
SUCCESS CRITERIA: [success criteria]

PLAN:
---
[full plan content]
---

If relevant:
- Explain specifically how it applies
- Quote the key insight or solution
- Note which success criterion or user story aspect it protects
- Suggest where/how to incorporate it

If NOT relevant after deeper analysis:
- Say 'Not applicable: [reason]'
"
```

**Example filtering:**
```
# Found 15 learning files, plan is about "Laravel API caching"

# SPAWN (likely relevant):
docs/solutions/performance-issues/n-plus-one-queries.md      # tags: [eloquent] ✓
docs/solutions/performance-issues/redis-cache-stampede.md    # tags: [caching, redis] ✓
docs/solutions/configuration-fixes/redis-connection-pool.md  # tags: [redis] ✓

# SKIP (clearly not applicable):
docs/solutions/deployment-issues/heroku-memory-quota.md      # not about caching
docs/solutions/frontend-issues/vue-reactivity-issue.md       # plan is API, not frontend
docs/solutions/authentication-issues/jwt-expiry.md           # plan has no auth
```

**Spawn sub-agents in PARALLEL for all filtered learnings.**

**These learnings are institutional knowledge - applying them prevents repeating past mistakes.**

### 4. Launch Per-Section Research Agents

<thinking>
For each major section in the plan, spawn dedicated sub-agents to research improvements. Ground each agent in the plan's WHY artifacts so research stays purpose-aligned.
</thinking>

**For each identified section, launch parallel research with WHY context:**

```
Task Explore: "Research best practices, patterns, and real-world examples for: [section topic].

CONTEXT -- WHY we're building this:
- Problem: [problem narrative summary]
- User Story: [user story]
- This section serves: [which success criterion / user story aspect]
- Architectural context: [relevant arch context for this section]

Find:
- Industry standards and conventions relevant to this user's problem
- Performance considerations that could affect the stated success criteria
- Common pitfalls that could threaten the user story outcome
- Documentation and tutorials for this architectural context
Return concrete, actionable recommendations. Filter out recommendations that don't serve the user story or success criteria."
```

**Also use Context7 MCP for framework documentation:**

For any technologies/frameworks mentioned in the plan, query Context7:
```
mcp__plugin_compound-engineering_context7__resolve-library-id: Find library ID for [framework]
mcp__plugin_compound-engineering_context7__query-docs: Query documentation for specific patterns
```

**Use WebSearch for current best practices:**

Search for recent (2024-2026) articles, blog posts, and documentation on topics in the plan.

### 5. Discover and Run ALL Review Agents

<thinking>
Dynamically discover every available agent and run them ALL against the plan. Don't filter, don't skip, don't assume relevance. 40+ parallel agents is fine. Use everything available. But give each agent the WHY context so their reviews are grounded.
</thinking>

**Step 1: Discover ALL available agents from ALL sources**

```bash
# 1. Project-local agents (highest priority - project-specific)
find [project agent dir] -name "*.md" 2>/dev/null

# 2. User's platform-global agents (for example ~/.copilot/agents or ~/.config/opencode/agents)
find [platform global agents dir] -name "*.md" 2>/dev/null

# 3. Installed plugin/package agents (all subdirectories) if the harness exposes them
find [installed plugin agent dirs] -name "*.md" 2>/dev/null

# 4. Broad fallback: search every discovered plugin/package location for agents
find [platform plugin roots] -path "*/agents/*.md" 2>/dev/null

# 5. If the harness exposes plugin metadata, inspect it to find additional local plugin locations
cat [installed plugin metadata file]

# 6. For local plugin/package entries, inspect their source directories directly
```

**Important:** Check EVERY source. Include agents from:
- Project-local agent directories
- User's platform-global agent directory
- compound-engineering plugin (but SKIP workflow/ agents - only use review/, research/, design/, docs/)
- ALL other installed plugins (agent-sdk-dev, frontend-design, etc.)
- Any local plugins

**For compound-engineering plugin specifically:**
- USE: `agents/review/*` (all reviewers)
- USE: `agents/research/*` (all researchers)
- USE: `agents/design/*` (design agents)
- USE: `agents/docs/*` (documentation agents)
- SKIP: `agents/workflow/*` (these are workflow orchestrators, not reviewers)

**Step 2: For each discovered agent, read its description**

Read the first few lines of each agent file to understand what it reviews/analyzes.

**Step 3: Launch ALL agents in parallel**

For EVERY agent discovered, launch a Task in parallel with WHY context:

Before dispatching any named agent discovered in this step, apply the shared `Named Agent Dispatch` protocol in `references/orchestration-protocol.md`. Pass the WHY context block from this workflow together with the loaded template.

```
Task [agent-name]: "Review this plan using your expertise.

WHY CONTEXT (use this to evaluate whether the plan solves the right problem):
- Problem Narrative: [problem narrative]
- User Story: [user story]
- Success Criteria: [success criteria list]
- Architectural Context: [arch context summary]

Apply all your checks and patterns. Flag anything that could prevent the user story from being achieved. Plan content: [full plan content]"
```

**CRITICAL RULES:**
- Do NOT filter agents by "relevance" - run them ALL
- Do NOT skip agents because they "might not apply" - let them decide
- Launch ALL agents in a SINGLE message with multiple Task tool calls
- 20, 30, 40 parallel agents is fine - use everything
- Each agent may catch something others miss
- The goal is MAXIMUM coverage, not efficiency

**Step 4: Also discover and run research agents**

Research agents (like `best-practices-researcher`, `framework-docs-researcher`, `git-history-analyzer`, `repo-research-analyst`) should also be run for relevant plan sections.

### 6. Wait for ALL Agents and Synthesize Everything

<thinking>
Wait for ALL parallel agents to complete - skills, research agents, review agents, everything. Then synthesize all findings through the lens of the plan's WHY artifacts. Prioritize enhancements that serve the user story and success criteria.
</thinking>

**Collect outputs from ALL sources:**

1. **Skill-based sub-agents** - Each skill's full output (code examples, patterns, recommendations)
2. **Learnings/Solutions sub-agents** - Relevant documented learnings from /workflows-compound
3. **Research agents** - Best practices, documentation, real-world examples
4. **Review agents** - All feedback from every reviewer (architecture, security, performance, simplicity, etc.)
5. **Context7 queries** - Framework documentation and patterns
6. **Web searches** - Current best practices and articles

**For each agent's findings, extract and classify by WHY alignment:**

- [ ] **Directly serves user story** -- enhancements that improve delivery of the stated user outcome (HIGH priority)
- [ ] **Protects success criteria** -- edge cases, security issues, performance concerns that could prevent success criteria from being met (HIGH priority)
- [ ] **Strengthens architecture** -- improvements aligned with the architectural context that make the implementation more robust (MEDIUM priority)
- [ ] **General best practices** -- technically sound improvements that don't directly trace to user story but improve overall quality (LOWER priority)
- [ ] **Scope warning** -- recommendations that would expand scope beyond the user story; flag these explicitly: "This enhancement is valuable but extends beyond the current user story. Consider adding to Future Considerations."

**For each finding also extract:**

- [ ] Concrete recommendations (actionable items)
- [ ] Code patterns and examples (copy-paste ready)
- [ ] Anti-patterns to avoid (warnings)
- [ ] Performance considerations (metrics, benchmarks)
- [ ] Security considerations (vulnerabilities, mitigations)
- [ ] Edge cases discovered (handling strategies)
- [ ] Documentation links (references)
- [ ] Skill-specific patterns (from matched skills)
- [ ] Relevant learnings (past solutions that apply - prevent repeating mistakes)

**Deduplicate, prioritize, and trace:**
- Merge similar recommendations from multiple agents
- Prioritize by WHY alignment (user story > success criteria > architecture > general)
- Flag conflicting advice for human review
- Group by plan section
- **For each recommendation, note which success criterion it serves or which risk it mitigates**

**Simplicity distillation pass (orchestrator responsibility):**
- Keep "run everything" agent coverage intact; filter only at synthesis time
- Prefer the least-complex change that satisfies the user story and success criteria
- Remove or defer recommendations that are speculative, redundant, or not required now
- Add complexity only when backed by concrete evidence from research, codebase constraints, or risk mitigation needs
- If complexity is retained, include a brief justification tied to a specific success criterion or risk

### 7. Enhance Plan Sections

<thinking>
Merge research findings back into the plan, adding depth where useful and reducing noise where needed. Critically: preserve all WHY sections untouched and ensure enhancements strengthen rather than dilute the connection to user story and success criteria.
</thinking>

**RULE: Never modify these WHY sections** (they are the contract from planning):
- Problem Narrative
- User Story
- Architectural Context
- Success Criteria
- Execution shape contract and packet tracing lines
- Handoff frontmatter

If research suggests changes to these, add a `### WHY Reassessment` note at the end of the plan for the user to review manually. Do not edit the originals.

**RULE: Do not silently weaken the TDD contract.**
- Preserve the plan's `tdd` frontmatter and `## TDD & Evidence Contract`
- You may clarify commands, add missing precedence notes, or add missing justifications
- Any relaxation from Ralph/unit+e2e must appear as an explicit justified exception with replacement evidence

**RULE: Simplicity over accretion.**
- You may redact or simplify non-essential implementation detail that does not materially serve the user story or success criteria.
- Keep packet structure and tracing intact while trimming unnecessary architectural ceremony.
- If simplification removes previously proposed complexity, record it in the enhancement summary with a reason.

**Enhancement format for each section:**

```markdown
## [Original Section Title]

[Original content preserved -- including any execution-shape and packet tracing lines]

### Research Insights

**Best Practices** (serves: [which success criterion]):
- [Concrete recommendation 1]
- [Concrete recommendation 2]

**Performance Considerations** (serves: [which success criterion]):
- [Optimization opportunity]
- [Benchmark or metric to target]

**Implementation Details:**
```[language]
// Concrete code example from research
```

**Edge Cases** (risk to: [which user story aspect]):
- [Edge case 1 and how to handle]
- [Edge case 2 and how to handle]

**References:**
- [Documentation URL 1]
- [Documentation URL 2]
```

### 8. Add Enhancement Summary

At the top of the plan, add a summary section:

```markdown
## Enhancement Summary

**Deepened on:** [Date]
**Sections enhanced:** [Count]
**Research agents used:** [List]

### WHY Integrity Check
- Problem Narrative: [preserved / flagged for reassessment]
- User Story: [preserved / flagged for reassessment]
- Architectural Context: [preserved / expanded / flagged for reassessment]
- Success Criteria: [preserved / flagged for reassessment]
- Execution shape: [preserved / flagged for reassessment]
- Packet tracing: [all packets still trace to user story or enabling outcome: yes/no]

### TDD Contract Check
- Precedence: [plan overrides local / inherit uses local / fallback default noted]
- Effective loop: [red-green-refactor / implementation-first]
- Evidence: [unit required?] [e2e required?]
- Exceptions: [none / justified and preserved]

### Key Improvements
1. [Major improvement 1] (serves: [success criterion])
2. [Major improvement 2] (serves: [success criterion])
3. [Major improvement 3] (serves: [success criterion])

### New Considerations Discovered
- [Important finding 1]
- [Important finding 2]

### Scope Warnings (if any)
- [Enhancement that was flagged as beyond current user story]

### Simplifications Applied
- [Removed/deferred complexity] (reason: [why it was unnecessary now])
- [Retained complexity with justification] (serves: [success criterion or risk])
```

### 9. Update Plan File

**Write the enhanced plan:**
- Preserve original filename
- Add `-deepened` suffix if user prefers a new file
- Update any timestamps or metadata

## Output Format

Update the plan file in place (or if user requests a separate file, append `-deepened` after `-plan`, e.g., `2026-01-15-feat-auth-plan-deepened.md`).

## Quality Checks

Before finalizing:

**Content integrity:**
- [ ] All original content preserved
- [ ] Research insights clearly marked and attributed
- [ ] Code examples are syntactically correct
- [ ] Links are valid and relevant
- [ ] No contradictions between sections
- [ ] Enhancement summary accurately reflects changes
- [ ] Execution packets have execution-ready structure for the selected mode
- [ ] TDD contract is explicit, precedence is documented, and unit/e2e evidence stays aligned with packet validation commands unless an exception says otherwise
- [ ] Simplification pass completed: non-essential complexity removed or deferred, and necessary complexity explicitly justified

**WHY integrity:**
- [ ] Problem Narrative, User Story, Success Criteria, and Architectural Context are unmodified from the original plan
- [ ] Handoff frontmatter is intact and still accurate
- [ ] `execution_shape` frontmatter and `## Execution Shape` still agree
- [ ] Every packet still has its tracing line
- [ ] No new packets were added without a tracing line connecting them to the user story or enabling outcome
- [ ] Enhancements tagged with which success criterion they serve
- [ ] Scope-expanding recommendations flagged in "Scope Warnings" rather than silently added to packets
- [ ] If WHY reassessment was needed, it's in a clearly marked section at the end (not inline edits)
- [ ] `tdd` frontmatter and `## TDD & Evidence Contract` still agree on precedence, effective loop, evidence, and exceptions

## Post-Enhancement Options

After writing the enhanced plan, use the **AskUserQuestion tool** to present these options:

**Question:** "Plan deepened at `[plan_path]`. What would you like to do next?"

**Options:**
1. **View diff** - Show what was added/changed
2. **Review and refine** - Improve the enhanced plan through structured document review
3. **Start `/workflows-work`** - Begin implementing this enhanced plan with its architecture artifact or explicit handoff contract
4. **Deepen further** - Run another round of research on specific sections
5. **Revert** - Restore original plan (if backup exists)

Based on selection:
- **View diff** → Run `git diff [plan_path]` or show before/after
- **Review and refine** → Load the `document-review` skill against the enhanced plan and the architecture artifact or explicit handoff contract that informed it
- **`/workflows-work`** → Call the /workflows-work command with the plan file path so execution loads the same architecture artifact or explicit handoff contract
- **Deepen further** → Ask which sections need more research, then re-run those agents
- **Revert** → Restore from git or backup

## Example Enhancement

**Before (from /workflows-plan):**
```markdown
## Technical Approach

Use React Query for data fetching with optimistic updates.
```

**After (from /workflows-deepen-plan):**
```markdown
## Technical Approach

Use React Query for data fetching with optimistic updates.

### Research Insights

**Best Practices:**
- Configure `staleTime` and `cacheTime` based on data freshness requirements
- Use `queryKey` factories for consistent cache invalidation
- Implement error boundaries around query-dependent components

**Performance Considerations:**
- Enable `refetchOnWindowFocus: false` for stable data to reduce unnecessary requests
- Use `select` option to transform and memoize data at query level
- Consider `placeholderData` for instant perceived loading

**Implementation Details:**
```typescript
// Recommended query configuration
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000, // 5 minutes
      retry: 2,
      refetchOnWindowFocus: false,
    },
  },
});
```

**Edge Cases:**
- Handle race conditions with `cancelQueries` on component unmount
- Implement retry logic for transient network failures
- Consider offline support with `persistQueryClient`

**References:**
- https://tanstack.com/query/latest/docs/react/guides/optimistic-updates
- https://tkdodo.eu/blog/practical-react-query
```

NEVER CODE! Just research and enhance the plan.
