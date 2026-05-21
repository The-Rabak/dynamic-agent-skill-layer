# Workflows and Recipes

This reference keeps the end-to-end examples and practical habits that are useful once you already know you need a swarm.

## Example workflows

### Full code review with parallel specialists
1. Leader creates the review goal and diff scope.
2. Spawn specialists by domain.
3. Collect findings through inboxes.
4. Synthesize duplicates and conflicts centrally.
5. Shut down workers and summarize.

### Research -> plan -> implement -> test pipeline
1. Research worker gathers constraints and best practices.
2. Planning worker turns findings into an execution shape.
3. Implementation worker executes against that plan.
4. Validation worker confirms behavior and regressions.

### Self-organizing code review swarm
1. Create a task queue per module or review slice.
2. Spawn teammates that claim tasks independently.
3. Leader monitors queue health and synthesis quality.
4. Use only when queue ownership is clearer than fixed worker assignment.

## Best practices

1. **Always clean up** once the job is done.
2. **Use meaningful names** for teams and workers.
3. **Write clear prompts** with scope and expected outputs.
4. **Use task dependencies** instead of implicit sequencing.
5. **Check inboxes and task state** before assuming workers are idle or stuck.
6. **Handle worker failures explicitly** instead of spawning duplicates.
7. **Prefer `write` over `broadcast`** when only one teammate needs the message.
8. **Match agent type to task** instead of defaulting to the biggest worker.

## Quick reference snippets

### Spawn subagent
```javascript
Task({
  subagent_type: "Explore",
  description: "Find auth files",
  prompt: "Find all authentication-related files in this codebase"
})
```

### Spawn teammate
```javascript
Task({
  team_name: "feature-auth",
  name: "security-reviewer",
  subagent_type: "security-sentinel",
  prompt: "Review auth code and report findings to the leader",
  run_in_background: true
})
```

### Message teammate
```javascript
Teammate({
  operation: "write",
  target_agent_id: "security-reviewer",
  value: "Please prioritize the authentication module."
})
```

### Create task pipeline
```javascript
TaskCreate({ title: "Research", description: "Gather auth constraints" })
TaskCreate({ title: "Implement", description: "Build auth flow", depends_on: [1] })
TaskCreate({ title: "Test", description: "Validate auth flow", depends_on: [2] })
```

### Shutdown team
```javascript
Teammate({ operation: "requestShutdown", target_agent_id: "security-reviewer" })
Teammate({ operation: "cleanup", team_name: "feature-auth" })
```
