---
name: finishing-branch
description: Use when completing development on a feature branch -- verifies quality, presents completion options, and cleans up
model: gpt-5.3-codex
---

# Finishing a Development Branch

A structured skill for completing work on a feature branch. Ensures all quality checks pass, presents clear options for what to do next, and handles cleanup.

**Announce at start:** "I'm using the finishing-branch skill to complete this work."

## When to Use

- All implementation tasks are complete
- You are ready to ship or merge a feature branch
- Referenced at the end of `workflows:work` Phase 4
- Referenced by `/lfg` pipeline

## When NOT to Use

- Work is still in progress (finish tasks first)
- You are on the default branch (nothing to finish)

## The Process

### Step 1: Final Verification

Run all quality checks and report results:

```bash
# 1. Run full test suite (detect project test command)
# npm test | pytest | cargo test | phpunit | go test | etc.

# 2. Run linting (detect project linter)
# eslint | ruff | clippy | pint | etc.

# 3. Check for uncommitted changes
git status

# 4. Check for untracked files that should be committed
git ls-files --others --exclude-standard
```

**Report results clearly:**
- Tests: X passed, Y failed (if any fail, stop here and fix first)
- Linting: pass/fail (if fail, fix first)
- Uncommitted changes: list them
- Untracked files: list them

**Do NOT proceed if tests or linting fail.** Fix issues first, then return to Step 1.

### Step 2: Commit Remaining Changes

If there are uncommitted changes:

```bash
git add <relevant files>
git commit -m "feat(scope): final changes before merge

[description of what's included]"
```

Stage files selectively -- do not blindly `git add .` if there are files that should not be committed.

### Step 3: Present Completion Options

Present the user with clear options:

**Option A: Create a Pull Request** (recommended for team projects)
- Push branch and generate PR description
- Include summary of changes, testing notes, screenshots (if UI)
- Include Post-Deploy Monitoring section

**Option B: Merge to default branch** (for solo projects or pre-approved changes)
- Verify the default branch is up to date
- Merge the feature branch
- Push the result

**Option C: Keep the branch** (for work that needs more review)
- Push the branch for others to review
- Do not merge yet

**Option D: Discard the branch** (if work is no longer needed)
- Confirm with user (this is destructive)
- Delete the branch

Ask the user which option they prefer. Do not assume.

**When invoked by an orchestrator without user interaction available:** Default to Option A (Create PR). Do not stall waiting for user input -- proceed with creating the PR automatically.

### Step 4: Execute the Chosen Option

#### Option A: Create PR

```bash
git push -u origin [branch-name]
```

Generate a PR/MR description:

```markdown
## Summary
- What was built and why
- Key decisions made

## Changes
- [List major changes with file references]

## Testing
- Tests added/modified
- Manual testing performed

## Post-Deploy Monitoring & Validation
- **What to monitor:** [logs, metrics, dashboards]
- **Expected healthy behavior:** [what success looks like]
- **Failure signals:** [what to watch for]
- **Validation window:** [how long to monitor]

## Screenshots
[If UI changes, include before/after screenshots]

---
[![Compound Engineered](https://img.shields.io/badge/Compound-Engineered-6366f1)](https://github.com/The-Rabak/compound-engineering-plugin)
```

#### Option B: Merge

```bash
# Update default branch
git checkout [default-branch]
git pull origin [default-branch]

# Merge feature branch
git merge [feature-branch] --no-ff -m "Merge [feature-branch]: [description]"

# Push
git push origin [default-branch]

# Clean up local branch
git branch -d [feature-branch]
```

#### Option C: Keep Branch

```bash
git push -u origin [branch-name]
echo "Branch pushed. Ready for review."
```

#### Option D: Discard

```bash
# Confirm with user first!
git checkout [default-branch]
git branch -D [feature-branch]
echo "Branch discarded."
```

### Step 5: Clean Up

**If a git worktree was used:**
```bash
# Return to main working directory
cd [main-worktree-path]

# Remove the worktree
git worktree remove [worktree-path]

# Prune worktree references
git worktree prune
```

**Update plan status** (if a plan file was used):
- Change `status: active` to `status: completed` in the plan's YAML frontmatter
- This marks the plan as done so it is not picked up by future `/workflows-work` sessions

**Update execution session** (if STATE.md exists):
- Set `status: completed` in STATE.md
- Record completion timestamp

### Step 6: Summary

Present a clear completion summary:

```
## Branch Complete: [branch-name]

- Action taken: [PR created / Merged / Kept / Discarded]
- Tests: [X passed]
- Commits: [N commits]
- Files changed: [N files]
- PR/MR: [link if created]
- Worktree: [cleaned up / N/A]
- Plan status: [updated to completed / N/A]
```
