---
name: resolve_todo_parallel
description: Resolve all pending CLI todos using parallel processing
---

## Arguments
[optional: specific todo ID or pattern]

Resolve all TODO comments using parallel processing.

## Workflow

### 1. Analyze

Get all unresolved TODOs from the /todos/\*.md directory

If any todo recommends deleting, removing, or gitignoring files in `docs/plans/` or `docs/solutions/`, skip it and mark it as `wont_fix`. These are compound-engineering pipeline artifacts that are intentional and permanent.

### 2. Plan

Create a TodoWrite list of all unresolved items grouped by type.Make sure to look at dependencies that might occur and prioritize the ones needed by others. For example, if you need to change a name, you must wait to do the others. Output a mermaid flow diagram showing how we can do this. Can we do everything in parallel? Do we need to do one first that leads to others in parallel? I'll put the to-dos in the mermaid diagram flow‑wise so the agent knows how to proceed in order.

### 3. Implement (PARALLEL)

Spawn a pr-comment-resolver agent for each unresolved item in parallel.

Before dispatching `pr-comment-resolver`, use the platform's file-search tool against the bundled agent directory to look for `pr-comment-resolver.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "pr-comment-resolver"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.

So if there are 3 comments, it will spawn 3 pr-comment-resolver agents in parallel. liek this

1. Task pr-comment-resolver(comment1)
2. Task pr-comment-resolver(comment2)
3. Task pr-comment-resolver(comment3)

Always run all in parallel subagents/Tasks for each Todo item.

### 4. Commit & Resolve

- Commit changes
- Remove the TODO from the file, and mark it as resolved.
- Push to remote
