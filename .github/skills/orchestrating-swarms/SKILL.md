---
name: orchestrating-swarms
description: This skill should be used when orchestrating multi-agent swarms using Claude Code's TeammateTool and Task system. It applies when coordinating multiple agents, running parallel code reviews, creating pipeline workflows with dependencies, building self-organizing task queues, or any task benefiting from divide-and-conquer patterns.
model: gpt-5.3-codex
---

# Claude Code Swarm Orchestration

Use Claude Code swarms deliberately. The goal is not "more agents"; it is faster throughput with cleaner task boundaries, explicit coordination, and better synthesis than one overloaded agent could deliver.

## When to use
- Coordinating several independent investigations, reviews, or implementation tracks that can run in parallel.
- Building leader/worker flows where tasks, dependencies, ownership, and handoffs must stay explicit.
- Choosing between one-off Task workers and persistent teammates for longer-running collaboration.
- Running pipelines where research, planning, implementation, and validation should be separated but still coordinated.

## Workflow
1. Decide whether the work needs **one-shot Task workers**, **persistent teammates**, or a **hybrid** model.
2. Define the swarm shape before spawning anything: objective, task graph, ownership, communication path, and completion signal.
3. Load only the reference docs that match the current need instead of dragging the entire swarm manual into the live prompt.
4. Spawn the minimum useful set of workers, then keep the leader responsible for prioritization, synthesis, and shutdown.
5. Track progress through explicit tasks, direct messages, and dependency updates rather than implicit "everyone knows the plan" assumptions.
6. Clean up the team when the job is done.

## Reference map
Use the detailed references as a router, not as mandatory reading every time:

| Need | Read |
|---|---|
| Core primitives, team lifecycle, Task vs teammate model | `references/foundations.md` |
| Built-in agent types, plugin-agent guidance, teammate operations, task system, message formats | `references/operations.md` |
| Orchestration patterns, environment variables, backends, failure handling | `references/patterns-and-backends.md` |
| End-to-end example workflows, best practices, quick command snippets | `references/workflows-and-recipes.md` |

### Fast decision rules
- Use a **Task subagent** when the worker should return a result and disappear.
- Use a **teammate** when the worker needs inbox communication, shared tasks, or a longer-lived role in a team.
- Use a **pipeline** when ordering matters.
- Use a **parallel specialist swarm** when the work splits cleanly by expertise.
- Use a **self-organizing swarm** only when workers genuinely benefit from claiming from a shared queue.

### Minimal planning template
Before spawning, write down:
- **Goal:** what outcome the swarm must deliver.
- **Leader role:** what only the leader is allowed to decide.
- **Workers:** names, specialties, and stop conditions.
- **Task graph:** parallel-safe work, dependencies, and unblock conditions.
- **Communication rule:** when to use direct messages, broadcast, or task status.
- **Shutdown rule:** what completion or failure condition ends the team.

## Output
- A swarm plan with clear worker roles, dependency ordering, communication rules, and completion criteria.
- Correct use of Task and teammate primitives grounded in the reference docs above.
- A coordination model that is easier to supervise than ad hoc parallel prompts.

## Guardrails
- Do not spawn a swarm when one focused agent would be simpler.
- Do not create workers without explicit ownership, task boundaries, and a return path for results.
- Do not leave synthesis to the workers; the leader must merge findings and decide next steps.
- Do not improvise teammate semantics when the documented primitives already cover the need.
- Prefer loading the smallest relevant reference file over pasting a giant operational manual into the active prompt.
