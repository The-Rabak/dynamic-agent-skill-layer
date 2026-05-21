# Operations

This reference covers the operational side of swarms: agent types, teammate operations, task system usage, and messaging.

## Built-in agent types

- **Bash** -- command execution, git operations, system tasks
- **Explore** -- read-only exploration and codebase search
- **Plan** -- architecture and implementation planning
- **general-purpose** -- mixed research + action work
- **claude-code-guide** -- Claude Code usage and setup questions
- **statusline-setup** -- status-line configuration

Example:

```javascript
Task({
  subagent_type: "general-purpose",
  description: "Research and implement",
  prompt: "Research React Query best practices and implement caching for the user API"
})
```

## Plugin agent guidance

### Review agents
Do not spawn compound-engineering named review agents directly from ad hoc swarm prompts. Route that work through `/workflows-review`, which loads templates, injects WHY context, and enforces reviewer policy.

### Research agents
Use these when you need focused investigation:
- `best-practices-researcher`
- `framework-docs-researcher`
- `git-history-analyzer`
- `learnings-researcher`
- `repo-research-analyst`

### Design and workflow agents
Use plugin-specific design or workflow agents when their scope is narrower and more accurate than a generic worker.

### Execution workers
When implementation workers are needed, load the canonical `execution-agent-prompt.md` template from the workflow references and build workers from that source instead of improvising an execution prompt.

## Teammate operations

### Team creation
```javascript
Teammate({
  operation: "spawnTeam",
  team_name: "feature-auth",
  description: "Implementing OAuth2 authentication"
})
```

### Team discovery and join flow
- `discoverTeams`
- `requestJoin`
- `approveJoin`
- `rejectJoin`

### Messaging
- `write` for one teammate
- `broadcast` for all teammates

Important: teammate console output is not team communication. Use `write` or `broadcast`.

### Shutdown
- `requestShutdown`
- `approveShutdown`
- `rejectShutdown`
- `cleanup`

## Task system integration

Core operations:
- `TaskCreate` -- add work items
- `TaskList` -- inspect queue state
- `TaskGet` -- inspect one task
- `TaskUpdate` -- claim, update, or complete tasks

Use dependencies whenever the order matters. A swarm gets messy fast when workers rely on oral tradition instead of explicit task edges.

## Message formats

### Regular message
Plain text for ordinary coordination.

### Structured message
JSON-in-text for events like shutdown requests, approval flows, or other machine-readable handoffs.

Use structured messages only when the receiver benefits from a predictable schema.
