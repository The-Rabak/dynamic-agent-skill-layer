---
name: ralph-loop
description: Start a self-referential loop that continues until a completion promise is met
---

## Arguments
"<prompt>" --completion-promise "<text>" --max-iterations <n>

# Ralph Loop

Start a self-referential loop using the stop hook mechanism.

Ralph is the canonical red-green-refactor engine behind `/workflows-work` when the resolved TDD contract selects Ralph-driven execution. Use this command directly only when you need manual control outside the orchestrated work path; it is not a detached alternative workflow.

## Setup

Run the setup script via Bash tool:

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/setup-ralph-loop.sh" $ARGUMENTS
```

After setup, proceed with the prompt specified in the arguments. The stop hook will prevent exit and feed the prompt back until the completion promise is output or max iterations are reached.

Any Ralph-driven run should preserve this contract:

1. **RED first** -- run failing tests before implementation
2. **GREEN second** -- add the minimal implementation needed to pass
3. **REFACTOR third** -- clean up behavior-preserving code
4. **POST-REFACTOR RERUN** -- rerun the required tests and capture stable report evidence for:
   - `Red`
   - `Green`
   - `Post-Refactor Green`

To signal completion, output: `<promise>YOUR_PROMISE_TEXT</promise>`
