---
name: resolve-pr-parallel
description: Resolve all PR comments using parallel processing. Use when addressing PR review feedback, resolving review threads, or batch-fixing PR comments.
model: gpt-5.3-codex
---

# Resolve PR Comments in Parallel

Resolve all unresolved PR review comments by spawning parallel agents for each thread.

## Context Detection

Claude Code automatically detects git context:
- Current branch and associated PR
- All PR comments and review threads
- Works with any PR by specifying the number

## Workflow

### 1. Analyze

Gather unresolved review comments. Use one of these methods:

**Option A: User pastes comments**
Ask the user to paste the PR/MR review comments. This is the most reliable method and works with any Git platform.

**Option B: GitHub CLI (`gh`)**
```bash
bash ${CLAUDE_PLUGIN_ROOT}/skills/resolve-pr-parallel/scripts/get-pr-comments PR_NUMBER [OWNER/REPO]
```

**Option C: GitLab CLI (`glab`)**
```bash
glab mr view MR_NUMBER --comments
```

**Option D: Platform API (if access token is available)**

For GitHub:
```bash
gh api repos/OWNER/REPO/pulls/PR_NUMBER/reviews
gh api repos/OWNER/REPO/pulls/PR_NUMBER/comments
```

For GitLab:
```bash
curl --header "PRIVATE-TOKEN: $GITLAB_TOKEN" \
  "https://YOUR_GITLAB_HOST/api/v4/projects/PROJECT_ID/merge_requests/MR_IID/discussions"
```

For Bitbucket:
```bash
curl -u "$BITBUCKET_USER:$BITBUCKET_TOKEN" \
  "https://api.bitbucket.org/2.0/repositories/WORKSPACE/REPO/pullrequests/PR_ID/comments"
```

### 2. Plan

Create a TodoWrite list of all unresolved items grouped by type:
- Code changes requested
- Questions to answer
- Style/convention fixes
- Test additions needed

### 3. Implement (PARALLEL)

Spawn a `pr-comment-resolver` agent for each unresolved item in parallel.

Before dispatching `pr-comment-resolver`, use the platform's file-search tool against the bundled agent directory to look for `pr-comment-resolver.md`, then use the file-read tool to load the full template. Only if the bundled template cannot be loaded should you fall back to `ov_load_global_agent "pr-comment-resolver"`. Before dispatching, quote the first non-empty line of the loaded template and record the source used. If you cannot quote the template because it was not found or could not be read, stop execution, raise the missing-template issue, and do not dispatch. Never dispatch a named agent by name alone.

If there are 3 comments, spawn 3 agents:

1. Task pr-comment-resolver(comment1)
2. Task pr-comment-resolver(comment2)
3. Task pr-comment-resolver(comment3)

Always run all in parallel subagents/Tasks for each Todo item.

### 4. Commit & Resolve

- Commit changes with a clear message referencing the PR feedback
- Resolve each thread programmatically (GitHub):

```bash
bash ${CLAUDE_PLUGIN_ROOT}/skills/resolve-pr-parallel/scripts/resolve-pr-thread THREAD_ID
```

- Push to remote

### 5. Verify

Re-fetch comments to confirm all threads are resolved:

```bash
bash ${CLAUDE_PLUGIN_ROOT}/skills/resolve-pr-parallel/scripts/get-pr-comments PR_NUMBER
```

Should return an empty array `[]`. If threads remain, repeat from step 1.

## Scripts

- [scripts/get-pr-comments](scripts/get-pr-comments) - GraphQL query for unresolved review threads (GitHub)
- [scripts/resolve-pr-thread](scripts/resolve-pr-thread) - GraphQL mutation to resolve a thread by ID (GitHub)

## Success Criteria

- All unresolved review threads addressed
- Changes committed and pushed
- Threads resolved (marked as resolved on the Git platform)
- Empty result from get-pr-comments on verify
