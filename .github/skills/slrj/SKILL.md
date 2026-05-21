---
name: slrj
description: Full autonomous engineering workflow using swarm mode for parallel execution
---

## Arguments
[feature description]

Swarm-enabled LRJ. Run these steps in order, parallelizing where indicated.

## Sequential Phase

1. `/workflows-plan $ARGUMENTS`
2. `/compound-engineering-deepen-plan`
3. `/workflows-work` — **Use swarm mode**: Make a Task list and launch an army of agent swarm subagents to build the plan. This is the default Ralph-driven execution path and should emit red, green, and post-refactor green evidence unless the plan declares an explicit exception.

## Parallel Phase

After work completes, launch steps 4 and 5 as **parallel swarm agents** (both only need code to be written):

4. `/workflows-review` — spawn as background Task agent
5. `/compound-engineering-test-browser` — spawn as background Task agent

Wait for both to complete before continuing.

## Finalize Phase

6. `/compound-engineering-resolve_todo_parallel` — resolve any findings from the review
7. `/compound-engineering-feature-video` — record the final walkthrough and add to PR
8. Output `<promise>DONE</promise>` when video is in PR

Start with step 1 now.
