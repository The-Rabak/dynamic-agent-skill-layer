# SWE-bench Lite Spike — Dynamic Agent Skill Layer Integration

This directory contains the SWE-bench Lite runner and hook-wiring groundwork for
**#218 Phase 2A** (feasibility spike). It satisfies Acceptance Criterion #1: "A spike
confirms SWE-bench Lite can be run through Claude Code with our hooks wired."

## Files

| File | Purpose |
|------|---------|
| `run-spike.sh` | Main spike script: dry-validates hooks + verifies images + prints live-proof command |
| `settings-swebench.json` | Claude Code settings with hooks wired to the live MCP server |
| `fetch-problem-statement.py` | Fetches a problem statement from HuggingFace datasets (no API key needed) |
| `README.md` | This file |

## Quick start (spike only — no model-driven solves)

```bash
# Prerequisite: skill-layer stack must be running
docker ps | grep mcp-server  # should show dynamic-agent-skill-layer-mcp-server-1

# Run dry-validate only (no image pulls needed)
scripts/swebench/run-spike.sh --dry-validate-only

# Full spike (dry-validate + image verification)
scripts/swebench/run-spike.sh
```

## Integration path chosen

### How hooks wire in

Claude Code runs from a **swebench-lite workspace** directory (`/tmp/swebench-lite-workspace`
by default). The `settings-swebench.json` file is passed via `--settings` and wires:

- **SessionStart**: `compile_context` MCP tool — pulls relevant skills from the skill corpus
  before the solve starts. `repo_path` is forwarded from the hook payload (= workspace dir).
- **UserPromptSubmit**: `compile_context` — re-fires on each mid-session user prompt.
- **SessionEnd**: `capture-transcript.sh` command hook — reads the transcript from disk and
  POSTs its content to `/ingest/transcript`. The `cwd` field in the hook payload becomes
  `repo_path` in the ingest request, scoping extracted skills to the swebench-lite workspace.

### Project scope "swebench-lite"

The skill layer's scope system is **path-based**: when the ingest POST includes
`repo_path = <swebench-workspace>`, the maintenance worker's `PendingDraftWriter` writes
`.pending` drafts into `<swebench-workspace>/.skills/`. This isolates SWE-bench-derived
skills from the global pool until they are manually approved (renamed to `.md`).

The swebench-lite workspace must exist as a directory before the solve starts. For project
scope resolution to succeed (so the writer doesn't fall back to the global scope root),
either:
- Initialize `.git` in the workspace: `git init /tmp/swebench-lite-workspace`
- Or set `SKILL_PROJECT_ROOT=/tmp/swebench-lite-workspace` before running `claude`

### SWE-bench Docker container interaction

Each SWE-bench instance has a pre-built Docker image with `/testbed` containing the
target repository at the base commit. The intended solve pattern:

1. Start the container: `docker run -d --name swebench-<id> <image> sleep 3600`
2. Run `claude` with `--add-dir /testbed` or by SSH/exec into the container via Bash tool
3. The agent uses Bash tool calls to run tests inside the container

**Practical approach for the live proof**: Run `claude` from the swebench-lite workspace
directory with the Docker container name as context in the prompt. Claude uses its Bash tool
to `docker exec` into the container, run tests, and apply patches.

## Selected instances (3 smallest by Docker image, test split)

| Instance ID | Docker Image | Image Size | Problem Summary |
|-------------|-------------|------------|-----------------|
| `psf__requests-863` | `swebench/sweb.eval.x86_64.psf_1776_requests-863:latest` | 2.34 GB | Allow lists in hook dict values |
| `pallets__flask-4045` | `swebench/sweb.eval.x86_64.pallets_1776_flask-4045:latest` | 2.57 GB | Raise error for blueprint name with dot |
| `sympy__sympy-20590` | `swebench/sweb.eval.x86_64.sympy_1776_sympy-20590:latest` | 2.58 GB | Symbol.__dict__ regression since 1.7 |

Pull commands:
```bash
docker pull swebench/sweb.eval.x86_64.psf_1776_requests-863:latest
docker pull swebench/sweb.eval.x86_64.pallets_1776_flask-4045:latest
docker pull swebench/sweb.eval.x86_64.sympy_1776_sympy-20590:latest
```

## Image naming convention

Docker images: `swebench/sweb.eval.x86_64.<org>_1776_<repo>-<issue>:latest`
SWE-bench instance IDs: `<org>__<repo>-<issue>`

The `_1776_` component is a dataset version marker in the image tag, not part of the instance ID.

## Live 3-instance proof command (for orchestrator)

See the output of `run-spike.sh` (Step 5) for the exact command. Summary:

```bash
# Setup workspace
mkdir -p /tmp/swebench-lite-workspace
git init /tmp/swebench-lite-workspace

# Per-instance (example: psf__requests-863)
INSTANCE_ID=psf__requests-863
IMAGE=swebench/sweb.eval.x86_64.psf_1776_requests-863:latest
PROBLEM=$(python3 scripts/swebench/fetch-problem-statement.py $INSTANCE_ID)

docker run -d --name swebench-${INSTANCE_ID} ${IMAGE} sleep 3600

time claude \
  --settings scripts/swebench/settings-swebench.json \
  --print \
  --dangerously-skip-permissions \
  --max-turns 40 \
  --add-dir /tmp/swebench-lite-workspace \
  "Fix the following issue in the codebase at /testbed (accessible via docker exec swebench-${INSTANCE_ID}):

${PROBLEM}" \
  2>&1 | tee /tmp/swebench-${INSTANCE_ID}.log

docker stop swebench-${INSTANCE_ID} && docker rm swebench-${INSTANCE_ID}
```

Run from directory: `/tmp/swebench-lite-workspace` (so cwd = workspace for hook scope).

## Per-instance cost estimate

- Docker container start: ~15-30 seconds (warm image)
- Claude Code solve (Sonnet): ~5-20 minutes per instance
- API cost estimate: ~$0.10-$0.50 per instance (rough; depends on turns)
- 3-instance total: ~35-65 minutes wall time, ~$0.30-$1.50 API cost

Budget $2.00 for the 3-instance proof.

## Scope fence

This spike does NOT:
- Run the full 20-instance or 300-instance experiment
- Claim any efficacy result
- Solve instances with claude-code (orchestrator does the live proof, serialized)
- Modify retrieval scoring, extraction, or ingestion logic
