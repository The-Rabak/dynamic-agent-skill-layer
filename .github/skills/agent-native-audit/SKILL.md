---
name: agent-native-audit
description: Run a comprehensive scored audit of agent-native architecture principles
model: gpt-5.3-codex
---

# Agent-Native Audit

Run a comprehensive review of a codebase against agent-native architecture principles, split the work into scored principle-specific audit tracks, and produce a prioritized summary report.

**Announce at start:** "I'm using the agent-native-audit skill to run a scored agent-native architecture review."

## When to Use

- After adding agent tools, prompts, commands, or shared-workspace behavior
- Before shipping major agent-facing product changes
- When you need a numeric baseline for how agent-native a system really is
- When you want prioritized recommendations instead of a vague architectural opinion

## Core Principles to Audit

1. **Action Parity** - "Whatever the user can do, the agent can do"
2. **Tools as Primitives** - "Tools provide capability, not behavior"
3. **Context Injection** - "System prompt includes dynamic context about app state"
4. **Shared Workspace** - "Agent and user work in the same data space"
5. **CRUD Completeness** - "Every important entity has full CRUD (Create, Read, Update, Delete)"
6. **UI Integration** - "Agent actions are reflected in the product experience"
7. **Capability Discovery** - "Users can discover what the agent can do"
8. **Prompt-Native Features** - "Features are prompts defining outcomes, not rigid workflow code"

## Workflow

### Step 1: Load the Local Reference Skill

First, invoke `agent-native-architecture` to refresh the local vocabulary, principles, checklist, and anti-patterns used in this plugin.

Focus especially on:

- Core principles
- Architecture Review Checklist
- Tool Design
- Files & Workspace
- Context Injection
- UI Integration
- Capability Discovery
- Anti-Patterns

Use the local skill name directly. Do not rely on Every-specific namespaced invocations.

### Step 2: Decide Audit Scope

If `$ARGUMENTS` names a single principle, audit only that principle.

Otherwise, audit all 8 principles.

When auditing a plugin, CLI, or developer-tooling codebase, map "user actions" and "UI" broadly:

- Slash commands
- Prompts
- Agents
- Skills
- CLI entrypoints
- Generated artifacts
- Shared files, docs, and workspace state

### Step 3: Launch Parallel Audit Tracks

Launch up to 8 parallel audit tracks using the platform's task/subagent tool.

- On Claude-style platforms, this is typically `Task` with `subagent_type`
- On Copilot-style platforms, this is typically `task` with `agent_type`

Prefer discovery-oriented agents for evidence gathering and stronger review agents for synthesis when available. In this plugin, `agent-native-reviewer`, `architecture-strategist`, `explore`, and `general-purpose` are all reasonable choices depending on platform and available agents.

Each audit track should:

1. Enumerate all relevant instances in the codebase
2. Check compliance against the principle
3. Produce a specific numeric score in `X/Y` format
4. List concrete gaps
5. Recommend the highest-impact fixes first

## Audit Track Prompts

### Track 1: Action Parity

Audit for **ACTION PARITY** - "Whatever the user can do, the agent can do."

Tasks:

1. Enumerate all meaningful user actions across product surfaces:
   - UI actions
   - Slash commands
   - CLI actions
   - Form submissions
   - API endpoints
   - File and workspace operations
2. Check which actions have corresponding agent tools, commands, or capabilities
3. Score: "Agent can do X out of Y user actions"

Format:

```markdown
## Action Parity Audit
### User Actions Found
| Action | Location | Agent Equivalent | Status |
|--------|----------|------------------|--------|
### Score: X/Y (percentage%)
### Missing Agent Capabilities
### Recommendations
```

### Track 2: Tools as Primitives

Audit for **TOOLS AS PRIMITIVES** - "Tools provide capability, not behavior."

Tasks:

1. Find and read all agent tools, commands, or callable integration surfaces
2. Classify each as:
   - **PRIMITIVE (good)** - Exposes a capability without embedding decision-making
   - **WORKFLOW (risky)** - Encodes business logic, orchestration, or branching that should live in prompts
3. Score: "X out of Y tools are proper primitives"

Format:

```markdown
## Tools as Primitives Audit
### Tool Analysis
| Surface | File | Type | Reasoning |
|---------|------|------|-----------|
### Score: X/Y (percentage%)
### Workflow-Heavy Surfaces
### Recommendations
```

### Track 3: Context Injection

Audit for **CONTEXT INJECTION** - "System prompt includes dynamic context about app state."

Tasks:

1. Find context injection code, system prompts, and agent setup logic
2. Enumerate what is injected versus what should be injected:
   - Available resources
   - User preferences
   - Recent activity
   - Workspace or session state
   - Available capabilities
   - Relevant history
3. Score: "X out of Y important context categories are injected"

Format:

```markdown
## Context Injection Audit
### Context Types Analysis
| Context Type | Injected? | Location | Notes |
|--------------|-----------|----------|-------|
### Score: X/Y (percentage%)
### Missing Context
### Recommendations
```

### Track 4: Shared Workspace

Audit for **SHARED WORKSPACE** - "Agent and user work in the same data space."

Tasks:

1. Identify all important data stores and shared artifacts:
   - Files
   - Databases
   - Documents
   - Session stores
   - Queues
   - Generated outputs
2. Check whether agents and users operate on the same state or separate shadow state
3. Score: "X out of Y important data spaces are truly shared"

Format:

```markdown
## Shared Workspace Audit
### Data Space Analysis
| Data Space | User Access | Agent Access | Shared? |
|------------|-------------|--------------|---------|
### Score: X/Y (percentage%)
### Isolated State (anti-pattern)
### Recommendations
```

### Track 5: CRUD Completeness

Audit for **CRUD COMPLETENESS** - "Every important entity has full CRUD."

Tasks:

1. Identify all important entities, resources, or managed objects
2. For each, check whether the agent can:
   - Create
   - Read
   - Update
   - Delete
3. Score per entity and overall

Format:

```markdown
## CRUD Completeness Audit
### Entity CRUD Analysis
| Entity | Create | Read | Update | Delete | Score |
|--------|--------|------|--------|--------|-------|
### Overall Score: X/Y entities with full CRUD (percentage%)
### Incomplete Entities
### Recommendations
```

### Track 6: UI Integration

Audit for **UI INTEGRATION** - "Agent actions are reflected in the product experience."

Tasks:

1. Check how agent actions propagate to user-visible surfaces
2. Look for:
   - Immediate UI refresh
   - Shared state updates
   - Eventing or streaming
   - File watching
   - Regeneration steps
   - Clear feedback in CLI or docs
3. Identify silent-action anti-patterns where the agent changes something but the user cannot see it promptly

Format:

```markdown
## UI Integration Audit
### Agent Action -> User Visibility Analysis
| Agent Action | Surface Update Mechanism | Immediate? | Notes |
|--------------|--------------------------|------------|-------|
### Score: X/Y (percentage%)
### Silent Actions
### Recommendations
```

### Track 7: Capability Discovery

Audit for **CAPABILITY DISCOVERY** - "Users can discover what the agent can do."

Tasks:

1. Check for these discovery mechanisms:
   - Onboarding or getting-started guidance
   - Help documentation
   - Capability hints in UI or command descriptions
   - Agent self-description
   - Suggested prompts or workflows
   - Empty-state guidance
   - Slash commands or discoverable command lists
2. Score against the mechanisms that are relevant to the product

Format:

```markdown
## Capability Discovery Audit
### Discovery Mechanism Analysis
| Mechanism | Exists? | Location | Quality |
|-----------|---------|----------|---------|
### Score: X/Y (percentage%)
### Missing Discovery Paths
### Recommendations
```

### Track 8: Prompt-Native Features

Audit for **PROMPT-NATIVE FEATURES** - "Features are prompts defining outcomes, not code."

Tasks:

1. Read prompts, agents, skills, commands, and orchestration code
2. Classify each important behavior as defined primarily in:
   - **PROMPT (good)** - Outcome described in natural language
   - **CODE (risky)** - Business logic hardcoded in conditionals or rigid orchestration
3. Check whether changing behavior usually requires prompt edits or code edits

Format:

```markdown
## Prompt-Native Features Audit
### Feature Definition Analysis
| Feature | Defined In | Type | Notes |
|---------|------------|------|-------|
### Score: X/Y (percentage%)
### Code-Defined Behaviors
### Recommendations
```

## Step 4: Compile the Summary Report

After all audit tracks finish, compile a single report:

```markdown
## Agent-Native Architecture Review: [Project Name]

### Overall Score Summary

| Core Principle | Score | Percentage | Status |
|----------------|-------|------------|--------|
| Action Parity | X/Y | Z% | ✅/⚠️/❌ |
| Tools as Primitives | X/Y | Z% | ✅/⚠️/❌ |
| Context Injection | X/Y | Z% | ✅/⚠️/❌ |
| Shared Workspace | X/Y | Z% | ✅/⚠️/❌ |
| CRUD Completeness | X/Y | Z% | ✅/⚠️/❌ |
| UI Integration | X/Y | Z% | ✅/⚠️/❌ |
| Capability Discovery | X/Y | Z% | ✅/⚠️/❌ |
| Prompt-Native Features | X/Y | Z% | ✅/⚠️/❌ |

**Overall Agent-Native Score: X%**

### Status Legend
- ✅ Excellent (80%+)
- ⚠️ Partial (50-79%)
- ❌ Needs Work (<50%)

### Top 10 Recommendations by Impact

| Priority | Action | Principle | Effort |
|----------|--------|-----------|--------|

### What's Working Excellently

[List top 5 strengths]
```

## Success Criteria

- [ ] All requested audit tracks complete
- [ ] Each principle has a specific numeric score in `X/Y` format
- [ ] Summary table includes all audited principles and status indicators
- [ ] Recommendations are prioritized by impact
- [ ] Report identifies both strengths and gaps

## Optional: Single Principle Audit

If `$ARGUMENTS` specifies a single principle, run only that audit track and provide detailed findings for that principle.

Valid arguments:

- `action parity` or `1`
- `tools` or `primitives` or `2`
- `context` or `injection` or `3`
- `shared` or `workspace` or `4`
- `crud` or `5`
- `ui` or `integration` or `6`
- `discovery` or `7`
- `prompt` or `features` or `8`
