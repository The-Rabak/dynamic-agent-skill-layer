---
name: reproduce-bug
description: Reproduce and investigate a bug using logs, console inspection, and browser screenshots
---

## Arguments
[issue number or description]

# Reproduce Bug Command

Look at github issue #$ARGUMENTS and read the issue description and comments.

## Phase 1: Log Investigation

Run the following agents in parallel to investigate the bug:

Before dispatching `repo-research-analyst` or `learnings-researcher`, use the platform's file-search tool against the bundled agent directory to look for `<agent-name>.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "<agent-name>"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.

1. Task repo-research-analyst(issue_description) - Search codebase for relevant code paths
2. Task learnings-researcher(issue_description) - Check if similar bugs have been solved before

Think about the places it could go wrong looking at the codebase. Look for logging output we can look for.

Run the agents again to find any logs that could help us reproduce the bug.

Keep running these agents until you have a good idea of what is going on.

## Phase 2: Visual Reproduction with agent-browser

If the bug is UI-related or involves user flows, use agent-browser to visually reproduce it:

### Step 1: Verify Server is Running

```bash
agent-browser open http://localhost-3000
agent-browser snapshot -i
```

If server not running, inform user to start the development server for the project.

### Step 2: Navigate to Affected Area

Based on the issue description, navigate to the relevant page:

```bash
agent-browser open "http://localhost:3000/[affected_route]"
agent-browser snapshot -i
```

### Step 3: Capture Screenshots

Take screenshots at each step of reproducing the bug:

```bash
agent-browser screenshot "bug-[issue]-step-1.png"
```

### Step 4: Follow User Flow

Reproduce the exact steps from the issue:

1. **Read the issue's reproduction steps**
2. **Execute each step using agent-browser:**
   - `agent-browser click @ref` for clicking elements
   - `agent-browser fill @ref "text"` for filling forms
   - `agent-browser snapshot -i` to see the current state
   - `agent-browser screenshot filename.png` to capture evidence

3. **Check for console errors** by inspecting the snapshot output for error indicators.

### Step 5: Capture Bug State

When you reproduce the bug:

1. Take a screenshot of the bug state
2. Document any console errors
3. Document the exact steps that triggered it

```bash
agent-browser screenshot "bug-[issue]-reproduced.png"
```

## Phase 3: Document Findings

**Reference Collection:**

- [ ] Document all research findings with specific file paths (e.g., `app/Services/ExampleService.php:42`)
- [ ] Include screenshots showing the bug reproduction
- [ ] List console errors if any
- [ ] Document the exact reproduction steps

## Phase 4: Report Back

Add a comment to the issue with:

1. **Findings** - What you discovered about the cause
2. **Reproduction Steps** - Exact steps to reproduce (verified)
3. **Screenshots** - Visual evidence of the bug (upload captured screenshots)
4. **Relevant Code** - File paths and line numbers
5. **Suggested Fix** - If you have one
