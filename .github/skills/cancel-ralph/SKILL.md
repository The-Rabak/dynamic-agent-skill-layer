---
name: cancel-ralph
description: Cancel an active ralph loop
---

# Cancel Ralph Loop

Check for and cancel an active ralph loop.

Use this when a Ralph-driven `/workflows-work` task or a manually started `/compound-engineering-ralph-loop` session needs to stop cleanly. This is an operational escape hatch for the default Ralph execution path.

```bash
STATE_FILE=".github/ralph-loop.local.md"

if [ -f "$STATE_FILE" ]; then
  ITERATION=$(grep "^iteration:" "$STATE_FILE" | sed 's/^iteration: *//')
  MAX=$(grep "^max_iterations:" "$STATE_FILE" | sed 's/^max_iterations: *//')
  echo "Cancelling ralph loop at iteration ${ITERATION}/${MAX}"
  rm -f "$STATE_FILE"
  echo "Ralph loop cancelled."
else
  echo "No active ralph loop found."
fi
```

Run the above script via Bash tool to cancel the loop.
