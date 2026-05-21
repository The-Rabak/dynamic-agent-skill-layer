# Foundations

This reference covers the conceptual model behind swarms: primitives, lifecycle, and the difference between short-lived Task workers and persistent teammates.

## Primitives

| Primitive | What it is | Typical location |
|---|---|---|
| Agent | A Claude instance that can use tools. You are an agent; spawned workers are agents too. | Process/runtime |
| Team | Named group of collaborating agents with a leader and teammates. | `~/.copilot/teams/{name}/config.json` |
| Teammate | Persistent team member spawned with `team_name` and `name`. | Team config |
| Leader | Agent that creates the team and remains responsible for synthesis and approvals. | Team config |
| Task | Shared work item with owner, status, and dependencies. | `~/.copilot/tasks/{team}/n.json` |
| Inbox | Message store used for teammate communication. | `~/.copilot/teams/{name}/inboxes/{agent}.json` |
| Backend | Runtime for teammates: `in-process`, `tmux`, or `iterm2`. | Auto-detected/configured |

## Team lifecycle
1. Create a team.
2. Define tasks and dependencies.
3. Spawn teammates or subagents.
4. Coordinate work and unblock dependencies.
5. Gather results through inboxes or task state.
6. Request shutdown and clean up team artifacts.

## Task vs teammate

### Task worker
Use a plain `Task(...)` call when the worker is one-shot:

```javascript
Task({
  subagent_type: "Explore",
  description: "Find auth files",
  prompt: "Find all authentication-related files in this codebase",
  model: "claude-haiku-4-5-20251001"
})
```

Best when:
- work is focused and self-contained
- result should return directly to the caller
- no inbox messaging or shared queue is needed

### Persistent teammate
Use `team_name` + `name` when the worker should join a team and collaborate over time:

```javascript
Teammate({ operation: "spawnTeam", team_name: "my-project" })

Task({
  team_name: "my-project",
  name: "security-reviewer",
  subagent_type: "security-sentinel",
  prompt: "Review auth code and send findings to the leader",
  run_in_background: true
})
```

Best when:
- work runs in parallel with other workers
- results should flow through inboxes
- workers need access to a shared task list
- the team will coordinate over multiple steps

## Core architecture

Typical mental model:
- **Leader** creates the plan, keeps the big picture, and decides when work is "done."
- **Workers** execute scoped tasks and report back.
- **Task list** makes dependencies and status visible.
- **Inboxes** carry direct coordination and findings.

## File structure

```text
~/.copilot/teams/{team-name}/
├── config.json
└── inboxes/
    ├── team-lead.json
    ├── worker-1.json
    └── worker-2.json

~/.copilot/tasks/{team-name}/
├── 1.json
├── 2.json
└── 3.json
```

## Team config sketch

```json
{
  "name": "my-project",
  "description": "Working on feature X",
  "leadAgentId": "team-lead@my-project",
  "members": [
    {
      "agentId": "team-lead@my-project",
      "name": "team-lead",
      "agentType": "team-lead",
      "backendType": "in-process"
    },
    {
      "agentId": "worker-1@my-project",
      "name": "worker-1",
      "agentType": "Explore",
      "backendType": "in-process"
    }
  ]
}
```
