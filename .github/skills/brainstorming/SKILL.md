---
name: brainstorming
description: "This skill should be used before implementing features, building components, or making changes. It guides exploring user intent, approaches, and design decisions before planning. Triggers on \"let's brainstorm\", \"help me think through\", \"what should we build\", \"explore approaches\", ambiguous feature requests, or when the user's request has multiple valid interpretations that need clarification."
model: gpt-5.3-codex
---

# Brainstorming

This skill provides detailed process knowledge for effective brainstorming sessions that establish **WHY** we're building something, clarify **WHAT** to build, and map **WHERE** it lives in the system -- before diving into **HOW** to build it.

The brainstorm produces three lynchpin artifacts that anchor all downstream phases:
1. **Problem Narrative & User Story** -- the WHY (consumed by plan, work, and review)
2. **Architectural Context Map** -- the WHERE (consumed by execution agents and reviewers)
3. **Design Decisions** -- the WHAT (consumed by plan for execution-slice decomposition)

## When to Use This Skill

Brainstorming is valuable when:
- Requirements are unclear or ambiguous
- Multiple approaches could solve the problem
- Trade-offs need to be explored with the user
- The user hasn't fully articulated what they want
- The feature scope needs refinement
- The problem statement is missing or vague (you know WHAT but not WHY)

Brainstorming can be skipped when:
- Requirements are explicit and detailed
- The user knows exactly what they want AND can articulate why
- The task is a straightforward bug fix or well-defined change

## Core Process

### Phase 0: Assess Requirement Clarity

Before diving into questions, assess whether brainstorming is needed.

**Signals that requirements are clear:**
- User provided specific acceptance criteria
- User referenced existing patterns to follow
- User described exact behavior expected
- Scope is constrained and well-defined
- Problem and motivation are explicitly stated

**Signals that brainstorming is needed:**
- User used vague terms ("make it better", "add something like")
- Multiple reasonable interpretations exist
- Trade-offs haven't been discussed
- User seems unsure about the approach
- WHY is missing -- user described a solution but not the problem it solves

If requirements are clear, suggest: "Your requirements seem clear. Consider proceeding directly to planning or implementation."

### Phase 1: Understand the Problem and the People

Ask questions **one at a time** to understand the user's problem space and the people affected. The goal is not just to understand WHAT they want but WHY they need it.

**Question Techniques:**

1. **Start with the problem, not the solution**
   - Good: "What problem are you trying to solve? What happens today that's painful?"
   - Avoid: "What feature do you want?" (this skips the WHY)

2. **Prefer multiple choice when natural options exist**
   - Good: "Should the notification be: (a) email only, (b) in-app only, or (c) both?"
   - Avoid: "How should users be notified?"

3. **Probe for stakes and impact**
   - "What happens if we don't solve this?"
   - "How are people working around this today?"
   - "What's the cost of the status quo?"

4. **Validate assumptions explicitly**
   - "I'm assuming users will be logged in. Is that correct?"

5. **Ask about success criteria early and tie them to outcomes**
   - "How will you know this feature is working well -- from the user's perspective?"
   - "What measurable change would tell you the problem is solved?"

**Key Topics to Explore:**

| Topic | Example Questions | Purpose |
|-------|-------------------|---------|
| Problem | What pain exists? What triggers it? What's the workaround? | Establishes the WHY |
| People | Who experiences this? What's their context and skill level? | Defines the persona |
| Stakes | What's the cost of not solving this? What's the business impact? | Validates priority |
| Success | How will users know it's working? What outcome matters? | Defines done |
| Boundaries | What's explicitly out of scope? What adjacent problems exist? | Prevents scope creep |
| Constraints | Technical limitations? Timeline? Dependencies? Compliance? | Grounds the approach |
| Existing Patterns | Are there similar features in the codebase to follow? | Informs architecture |

**Exit Condition:** Continue until you can articulate the problem, the people, and the desired outcome clearly -- OR user says "proceed" or "let's move on."

### Phase 1.5: Synthesize Problem Narrative & User Story

**This is the critical synthesis step.** After the Q&A, the orchestrator constructs two artifacts from everything learned.

**Problem Narrative** (2-4 sentences):
Clearly state: who has the problem, what triggers it, what the impact is, and why now is the right time to solve it. This is the WHY that every downstream phase traces back to.

Example: *"Support agents currently spend 20+ minutes per ticket manually cross-referencing customer data across three systems. This leads to longer resolution times and frequent errors in billing adjustments. The upcoming Q3 volume spike will make this unsustainable."*

**User Story** (structured format):
```
As a [persona with relevant context],
I need to [action/capability]
so that [desired outcome/value],
because currently [pain point or gap]
which causes [concrete negative impact].
```

Example:
```
As a support agent handling billing disputes,
I need to see all customer data consolidated in one view
so that I can resolve tickets in under 5 minutes,
because currently I have to switch between three systems
which causes 20+ minute resolution times and frequent billing errors.
```

**Validation:** Present both artifacts to the user and ask for confirmation. This is the most important step. If the problem narrative and user story are wrong, everything downstream will be misaligned.

### Phase 2: Explore Approaches

After understanding the problem, propose 2-3 concrete approaches.

**Critically: evaluate each approach against the problem narrative and user story.**

**Structure for Each Approach:**

```markdown
### Approach A: [Name]

[2-3 sentence description]

**How it addresses the user story:**
[Direct connection to the problem and desired outcome]

**Pros:**
- [Benefit 1]
- [Benefit 2]

**Cons:**
- [Drawback 1]
- [Drawback 2]

**Best when:** [Circumstances where this approach shines]
**Risk:** [What could go wrong, specific to the user's context]
```

**Guidelines:**
- Lead with a recommendation and explain why **in terms of the user story**
- Be honest about trade-offs
- YAGNI applies to building scope, not understanding scope
- Reference codebase patterns when relevant
- Don't just list pros/cons in a vacuum -- tie them back to the problem

### Phase 2.5: Architectural Context Mapping

After the approach is chosen, construct a lightweight architectural context map. This is NOT implementation design (no file paths, no class names, no code). This is structural understanding.

**What to map:**

| Aspect | Question | Example |
|--------|----------|---------|
| System Placement | Where does this feature live? | "New endpoint in the Billing API service" |
| Boundary Interactions | What does it talk to? | "Reads from CustomerDB, calls Payment Gateway, emits events to Notification service" |
| User Touchpoints | How do users interact with it? | "Support agents access via the admin dashboard" |
| Data Considerations | What data does it create/read/update? | "Creates consolidated views from three data sources, caches in Redis" |
| Dependency Direction | What depends on what? | "Depends on Customer service and Payment service; Notification service will consume its events" |

**Output:** A concise prose description (5-10 sentences). Think of it as explaining to a new team member where this feature sits and what it touches. Avoid code-level details.

**Present to the user for validation.** This context map flows into:
- Every execution agent's `{{ARCHITECTURAL_CONTEXT}}` block in `/workflows-work`
- The plan's phase decomposition in `/workflows-plan`
- The review's evaluation frame in `/workflows-review`

### Phase 3: Capture the Design

Summarize everything in a structured brainstorm document.

**Document Template:**

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

[2-4 sentences: who has the problem, what triggers it, what the impact is, why now.]

## User Story

As a [persona],
I need to [action]
so that [outcome],
because currently [pain point]
which causes [impact].

## Success Criteria

- [Measurable outcome tied to the user story]
- [Observable behavior that proves the problem is solved]

## Architectural Context

- **Lives in:** [service/module/layer]
- **Interacts with:** [neighboring systems]
- **User entry point:** [UI/API/CLI/event]
- **Data:** [what data flows, where it lives]
- **Dependencies:** [depends on X; Y may depend on this]

## Chosen Approach

[What we're building and WHY this approach -- tied to the problem narrative]

## Key Decisions

- [Decision 1]: [Rationale tied to problem/user story]
- [Decision 2]: [Rationale tied to problem/user story]

## Approaches Considered

[Alternatives and why they were not chosen, relative to the user story]

## Stakeholder Impact

- **End users:** [experience change]
- **Developers:** [codebase/maintenance impact]
- **Operations:** [deployment/monitoring impact]
- **Business:** [revenue/cost/compliance impact]

## Open Questions

- [Unresolved questions for the planning phase]

## Resolved Questions

- [Questions resolved during brainstorming, with answers]

## Next Steps

-> `/workflows-plan` for implementation details
```

**Output Location:** `docs/brainstorms/YYYY-MM-DD-<topic>-brainstorm.md`

**Handoff Contract:** The `handoff` frontmatter fields signal to downstream phases which lynchpin sections are present. All four should be `true` before handing off to `/workflows-plan`.

### Phase 4: Handoff

Present clear options for what to do next:

1. **Proceed to planning** -> Run `/workflows-plan` (will auto-detect and consume this brainstorm's lynchpin artifacts)
2. **Refine further** -> Continue exploring the design
3. **Done for now** -> User will return later

## YAGNI Principles -- Applied Correctly

During brainstorming, YAGNI applies to **building scope**, not **understanding scope**:

- **DO** explore the full problem space, adjacent problems, and future touchpoints -- this informs architecture
- **DO** understand broadly, even if you build narrowly
- **DON'T** commit to building features for hypothetical future requirements
- **DO** choose the simplest approach that solves the stated problem
- **DO** prefer boring, proven patterns over clever solutions
- **DO** ask "Do we really need to BUILD this?" when complexity emerges
- **DON'T** ask "Do we need to UNDERSTAND this?" -- yes, you always do

## Incremental Validation

Keep sections short -- 200-300 words maximum. After each section of output, pause to validate understanding:

- "Does this match what you had in mind?"
- "Any adjustments before we continue?"
- "Is this the direction you want to go?"

This prevents wasted effort on misaligned designs.

## Anti-Patterns to Avoid

| Anti-Pattern | Better Approach |
|--------------|-----------------|
| Asking 5 questions at once | Ask one at a time |
| Jumping to solutions before understanding the problem | Establish WHY before exploring WHAT |
| Evaluating approaches in a vacuum | Evaluate against the user story |
| Proposing overly complex solutions | Start simple, add complexity only if needed |
| Ignoring existing codebase patterns | Research what exists first |
| Making assumptions without validating | State assumptions explicitly and confirm |
| Creating lengthy design documents | Keep it concise -- details go in the plan |
| Skipping architectural context | Map where it lives before handing to plan |
| Treating the doc as disposable prose | It's a contract consumed by downstream phases |

## Integration with Downstream Phases

The brainstorm document is the **feature-level spec and handoff contract** for downstream work. The project constitution, when present, remains the repo-wide governing artifact:

**`/workflows-plan` consumes:**
- Problem narrative and user story -> structures execution slices around the WHY
- Architectural context -> informs slice decomposition, file mapping, dependencies
- Success criteria -> becomes the plan's acceptance criteria foundation
- Key decisions -> preserved and enriched, not re-decided

**`/deepen-plan` consumes:**
- Problem narrative -> evaluates whether deepened slices still serve the original intent
- Success criteria -> grounds best-practice research in actual goals

**`/workflows-work` consumes:**
- Architectural context -> populates `{{ARCHITECTURAL_CONTEXT}}` for every execution agent
- User story -> orchestrator validates each slice contributes to the story
- Problem narrative -> included in scoped prompts so agents understand purpose

**`/workflows-review` consumes:**
- Problem narrative + user story -> evaluates whether implementation solves the stated problem
- Architectural context -> validates structural decisions match the planned placement
- Success criteria -> confirms the feature delivers measurable outcomes
- Stakeholder impact -> frames review from multiple perspectives (not invented post-hoc)
