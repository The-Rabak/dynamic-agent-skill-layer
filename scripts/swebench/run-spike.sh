#!/usr/bin/env bash
#
# run-spike.sh — SWE-bench Lite feasibility spike for the Dynamic Agent Skill Layer.
#
# Satisfies #218 Acceptance Criterion #1: "A spike confirms SWE-bench Lite can be run
# through Claude Code with our hooks wired (or documents the integration path chosen)."
#
# SCOPE FENCE: This spike does NOT run the full measured experiment.
# - It DRY-validates hook wiring (compile_context, find_skill, capture-transcript).
# - It pulls the 3 smallest test-split SWE-bench Lite instances and verifies they are usable.
# - It documents the exact command to run the live 3-instance proof (orchestrator's job).
# - It does NOT invoke claude-code to solve any instance (model-driven proof is serialized,
#   run by the orchestrator after this spike, per CRITICAL EXECUTION BOUNDARY).
#
# Hook wiring overview:
#   SessionStart → compile_context (skill context injected before each solve)
#   UserPromptSubmit → compile_context (re-fires on each mid-session prompt)
#   SessionEnd → capture-transcript.sh → POST /ingest/transcript (enqueues for extraction)
#
# Project scope labeling:
#   All sessions run with cwd = SWEBENCH_WORKSPACE (default: /tmp/swebench-lite-workspace).
#   Claude Code sets cwd in hook payloads; capture-transcript.sh forwards it as repo_path
#   in the ingest POST. The maintenance worker writes .pending drafts into
#   <SWEBENCH_WORKSPACE>/.skills/ — isolating extracted skills under the swebench-lite
#   project scope and keeping them out of the global skill pool until manually approved.
#
# SWE-bench Lite instances (3 smallest by Docker image, all in test split):
#   1. psf__requests-863   (2.34 GB)  — requests hooks argument bug
#   2. pallets__flask-4045 (2.57 GB)  — blueprint name dot validation
#   3. sympy__sympy-20590  (2.58 GB)  — Symbol.__dict__ regression
#
# Usage:
#   scripts/swebench/run-spike.sh [--dry-validate-only]
#
#   --dry-validate-only  Run only the hook-wiring dry validation. Skip image pulls and the
#                        live proof readiness check.
#
# Configuration (env overrides):
#   SWEBENCH_WORKSPACE   Directory used as the swebench-lite project scope root.
#                        Must exist or will be created. (default: /tmp/swebench-lite-workspace)
#   MCP_SERVER_PORT      Port of the running MCP server (default: 3001)
#   SKILL_LAYER_INGEST_SECRET  Optional ingest auth secret (default: empty)
#
# Requirements:
#   - The skill-layer stack must be running (postgres, redis, qdrant, ollama, mcp-server).
#     Check: docker ps (look for dynamic-agent-skill-layer-mcp-server-1).
#   - claude CLI must be on PATH for the live proof step.
#   - Docker must be running and have network access to DockerHub.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# ---------------------------------------------------------------------------
# CLI flags
# ---------------------------------------------------------------------------
DRY_VALIDATE_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --dry-validate-only) DRY_VALIDATE_ONLY=1 ;;
        *)
            echo "Unknown option: $arg" >&2
            echo "Usage: $0 [--dry-validate-only]" >&2
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Configuration constants
# ---------------------------------------------------------------------------
MCP_SERVER_PORT="${MCP_SERVER_PORT:-3001}"
MCP_URL="http://127.0.0.1:${MCP_SERVER_PORT}/mcp"
INGEST_URL="http://127.0.0.1:${MCP_SERVER_PORT}/ingest/transcript"
SKILL_LAYER_INGEST_SECRET="${SKILL_LAYER_INGEST_SECRET:-}"

# The swebench-lite project scope root: all sessions run with cwd = this directory.
# .pending drafts land in ${SWEBENCH_WORKSPACE}/.skills/ — isolated from global scope.
SWEBENCH_WORKSPACE="${SWEBENCH_WORKSPACE:-/tmp/swebench-lite-workspace}"

CAPTURE_SCRIPT="${REPO_ROOT}/config/claude-code/capture-transcript.sh"
HOOKS_CONFIG="${SCRIPT_DIR}/settings-swebench.json"

# The 3 smallest SWE-bench Lite instances from the test split, by Docker image size.
# Image naming format: swebench/sweb.eval.x86_64.<org>_1776_<repo>-<issue>:latest
# Instance ID format (for dataset lookup):  <org>__<repo>-<issue>
declare -A INSTANCE_IMAGES=(
    ["psf__requests-863"]="swebench/sweb.eval.x86_64.psf_1776_requests-863:latest"
    ["pallets__flask-4045"]="swebench/sweb.eval.x86_64.pallets_1776_flask-4045:latest"
    ["sympy__sympy-20590"]="swebench/sweb.eval.x86_64.sympy_1776_sympy-20590:latest"
)

declare -A INSTANCE_SIZES=(
    ["psf__requests-863"]="2.34GB"
    ["pallets__flask-4045"]="2.57GB"
    ["sympy__sympy-20590"]="2.58GB"
)

declare -A INSTANCE_PROBLEMS=(
    ["psf__requests-863"]="Allow lists in the dict values of the hooks argument (requests/hooks.py)"
    ["pallets__flask-4045"]="Raise error when blueprint name contains a dot"
    ["sympy__sympy-20590"]="Symbol instances have __dict__ since 1.7 (regression)"
)

SPIKE_START_EPOCH=$(date +%s)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
log_step() { echo ""; echo "==> $*"; }
pass() { echo "  [PASS] $*"; }
fail_loud() { echo "  [FAIL] $*" >&2; exit 1; }
warn() { echo "  [WARN] $*"; }

# ---------------------------------------------------------------------------
# Step 1: Verify the skill-layer stack is reachable
# ---------------------------------------------------------------------------
log_step "Stack connectivity check"

HEALTH_URL="http://127.0.0.1:${MCP_SERVER_PORT}/health"
HEALTH_BODY="$(curl -sSf --max-time 10 "$HEALTH_URL" 2>/dev/null || true)"
if [ -z "$HEALTH_BODY" ]; then
    fail_loud "MCP server at ${HEALTH_URL} did not respond. Start the stack first:
    docker compose -f docker-compose.test.yml up -d
    (or use the default docker-compose.yml for the production stack)"
fi

STACK_HEALTHY="$(echo "$HEALTH_BODY" | python3 -c "
import json, sys
r = json.load(sys.stdin)
print('ok' if r.get('healthy', False) else 'degraded')
" 2>/dev/null || echo "degraded")"

GRAPH_VERSION="$(echo "$HEALTH_BODY" | python3 -c "
import json, sys
r = json.load(sys.stdin)
print(r.get('graph_version', 0))
" 2>/dev/null || echo "0")"

pass "MCP server reachable (healthy=${STACK_HEALTHY}, graph_version=${GRAPH_VERSION})"

# ---------------------------------------------------------------------------
# Step 2: DRY-VALIDATE hook wiring
#
# Proves each hook path responds correctly WITHOUT model-driven solves:
#   a. compile_context responds over MCP HTTP
#   b. find_skill responds over MCP HTTP
#   c. capture-transcript.sh → POST /ingest/transcript returns "enqueued"
#   d. hooks-config JSON is syntactically valid (wired to fire)
#
# NOTE: These calls use a synthetic/dry-run session ID. No real SWE-bench
# session is started; this is a connectivity and wiring proof only.
# ---------------------------------------------------------------------------
log_step "DRY-VALIDATE hook wiring"

DRY_SESSION_ID="swebench-drywire-$(date +%s)"

# (a) compile_context
echo "  Testing compile_context (MCP tool, SessionStart hook)"
CC_PAYLOAD="$(python3 -c "
import json, os
print(json.dumps({
    'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
    'params': {
        'name': 'compile_context',
        'arguments': {
            'prompt': '[SWEBENCH-DRYWIRE] Fix import error in Python module',
            'session_id': '${DRY_SESSION_ID}',
            'repo_path': '${SWEBENCH_WORKSPACE}',
        }
    }
}))
")"

CC_RESPONSE="$(curl -sSf --max-time 15 -X POST "$MCP_URL" \
    -H "Content-Type: application/json" -d "$CC_PAYLOAD" 2>/dev/null || true)"

CC_STATUS="$(echo "$CC_RESPONSE" | python3 -c "
import json, sys
try:
    r = json.load(sys.stdin)
    print(r.get('result', {}).get('status', 'error'))
except Exception:
    print('parse_error')
" 2>/dev/null || echo "error")"

CC_GV="$(echo "$CC_RESPONSE" | python3 -c "
import json, sys
try:
    r = json.load(sys.stdin)
    print(r.get('result', {}).get('graph_version', '?'))
except Exception:
    print('?')
" 2>/dev/null || echo "?")"

if [ "$CC_STATUS" = "error" ] || [ "$CC_STATUS" = "parse_error" ]; then
    fail_loud "compile_context failed: status=${CC_STATUS} response=${CC_RESPONSE}"
fi
pass "compile_context responded: status=${CC_STATUS} graph_version=${CC_GV}"

# (b) find_skill
echo "  Testing find_skill (MCP tool, mid-session retrieval)"
FS_PAYLOAD="$(python3 -c "
import json
print(json.dumps({
    'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call',
    'params': {
        'name': 'find_skill',
        'arguments': {
            'prompt': '[SWEBENCH-DRYWIRE] How to fix Python import error in requests library',
        }
    }
}))
")"

FS_RESPONSE="$(curl -sSf --max-time 15 -X POST "$MCP_URL" \
    -H "Content-Type: application/json" -d "$FS_PAYLOAD" 2>/dev/null || true)"

FS_STATUS="$(echo "$FS_RESPONSE" | python3 -c "
import json, sys
try:
    r = json.load(sys.stdin)
    result = r.get('result', {})
    # find_skill returns {status, matches, reason_code} not wrapped in content
    print(result.get('status', 'error'))
except Exception:
    print('parse_error')
" 2>/dev/null || echo "error")"

if echo "$FS_RESPONSE" | grep -q '"error"'; then
    fail_loud "find_skill returned error: ${FS_RESPONSE}"
fi
pass "find_skill responded: status=${FS_STATUS}"

# (c) capture-transcript.sh → /ingest/transcript
echo "  Testing capture-transcript.sh → POST /ingest/transcript (SessionEnd hook path)"

if [ ! -x "$CAPTURE_SCRIPT" ]; then
    fail_loud "capture-transcript.sh not found or not executable at ${CAPTURE_SCRIPT}"
fi

# Write a clearly-labeled synthetic transcript to a temp file (wiring check only, NOT corpus).
# SYNTHETIC transcript — labeled [SWEBENCH-DRYWIRE] so it is NOT a real benchmark session.
SYNTHETIC_TRANSCRIPT_FILE="$(mktemp)"
trap 'rm -f "$SYNTHETIC_TRANSCRIPT_FILE"' EXIT

python3 - "$SYNTHETIC_TRANSCRIPT_FILE" <<'PYWRITE'
import json, sys

events = [
    {'type': 'user',      'message': {'role': 'user',      'content': '[SWEBENCH-DRYWIRE] Fix the import error in requests/adapters.py'}},
    {'type': 'assistant', 'message': {'role': 'assistant',  'content': '[SWEBENCH-DRYWIRE] I will fix the import error by updating the import statement in adapters.py to use the correct module path.'}},
]
out_path = sys.argv[1]
with open(out_path, 'w') as f:
    for e in events:
        f.write(json.dumps(e) + '\n')
PYWRITE

# Build hook payload (exactly how Claude Code invokes command hooks)
HOOK_PAYLOAD="$(python3 -c "
import json
print(json.dumps({
    'transcript_path': '${SYNTHETIC_TRANSCRIPT_FILE}',
    'session_id': 'swebench-capture-drywire-$(date +%s)',
    'cwd': '${SWEBENCH_WORKSPACE}',
}))
")"

export SKILL_LAYER_INGEST_URL="$INGEST_URL"
export SKILL_LAYER_INGEST_SECRET="$SKILL_LAYER_INGEST_SECRET"

# The capture script reads stdin (hook payload) and detaches the POST to a background worker.
echo "$HOOK_PAYLOAD" | bash "$CAPTURE_SCRIPT" session_end
CAPTURE_EXIT=$?
if [ "$CAPTURE_EXIT" -ne 0 ]; then
    fail_loud "capture-transcript.sh exited non-zero: ${CAPTURE_EXIT}"
fi

# Also verify the endpoint directly (synchronous, for the wiring proof)
INGEST_PAYLOAD="$(python3 -c "
import json, os
try:
    content = open('${SYNTHETIC_TRANSCRIPT_FILE}').read()
except Exception:
    content = '[SWEBENCH-DRYWIRE] fallback'
print(json.dumps({
    'session_id': 'swebench-ingest-drywire-$(date +%s)',
    'source': 'session_end',
    'content': content,
    'repo_path': '${SWEBENCH_WORKSPACE}',
}))
")"

INGEST_RESPONSE="$(curl -sSf --max-time 10 -X POST "$INGEST_URL" \
    -H "Content-Type: application/json" \
    -d "$INGEST_PAYLOAD" 2>/dev/null || true)"

INGEST_STATUS="$(echo "$INGEST_RESPONSE" | python3 -c "
import json, sys
try:
    r = json.load(sys.stdin)
    print(r.get('status', 'error'))
except Exception:
    print('parse_error')
" 2>/dev/null || echo "error")"

if [ "$INGEST_STATUS" != "enqueued" ] && [ "$INGEST_STATUS" != "duplicate" ]; then
    fail_loud "/ingest/transcript returned unexpected status=${INGEST_STATUS} (expected 'enqueued')"
fi
pass "capture-transcript.sh → /ingest/transcript: status=${INGEST_STATUS}"

# (d) hooks-config JSON syntactic validation
echo "  Validating hooks settings JSON (config for SWE-bench sessions)"
if [ ! -f "$HOOKS_CONFIG" ]; then
    fail_loud "hooks settings file not found at ${HOOKS_CONFIG} — check run-spike.sh setup"
fi
python3 -c "
import json, sys
try:
    with open('${HOOKS_CONFIG}') as f:
        cfg = json.load(f)
    hooks = cfg.get('hooks', {})
    mcp = cfg.get('mcpServers', {})
    assert 'SessionStart' in hooks, 'SessionStart hook missing'
    assert 'SessionEnd' in hooks, 'SessionEnd hook missing'
    assert 'skill-layer' in mcp, 'skill-layer MCP server missing'
    print('ok')
except Exception as e:
    print(f'FAIL: {e}')
    sys.exit(1)
" 2>/dev/null
pass "hooks settings JSON is valid and wired (SessionStart + SessionEnd + skill-layer MCP)"

# ---------------------------------------------------------------------------
# Early exit if dry-validate-only
# ---------------------------------------------------------------------------
if [ "$DRY_VALIDATE_ONLY" -eq 1 ]; then
    echo ""
    echo "================================================================"
    echo "  Dry-validate complete (--dry-validate-only flag set)"
    echo "================================================================"
    echo "  compile_context: ${CC_STATUS} (graph_version=${CC_GV})"
    echo "  find_skill: ${FS_STATUS}"
    echo "  capture-transcript → /ingest/transcript: ${INGEST_STATUS}"
    echo "  hooks config: valid"
    echo ""
    echo "RESULT: dry-validate-ok"
    exit 0
fi

# ---------------------------------------------------------------------------
# Step 3: Verify the 3 SWE-bench Lite Docker images are present and usable
# ---------------------------------------------------------------------------
log_step "SWE-bench Lite Docker images (3 smallest test-split instances)"

ALL_IMAGES_OK=1
for INSTANCE_ID in "${!INSTANCE_IMAGES[@]}"; do
    IMAGE="${INSTANCE_IMAGES[$INSTANCE_ID]}"
    SIZE="${INSTANCE_SIZES[$INSTANCE_ID]}"
    PROBLEM="${INSTANCE_PROBLEMS[$INSTANCE_ID]}"

    echo "  Instance: ${INSTANCE_ID} (${SIZE})"
    echo "  Image:    ${IMAGE}"
    echo "  Problem:  ${PROBLEM}"

    # Verify the image is locally present
    if docker images --format "{{.Repository}}:{{.Tag}}" | grep -qF "${IMAGE%%:*}:latest"; then
        TESTBED_DIR="$(docker run --rm --entrypoint bash "$IMAGE" \
            -c 'ls /testbed/ 2>/dev/null | head -3 | tr "\n" " "' 2>/dev/null || echo "(failed)")"
        pass "image present: ${IMAGE}"
        echo "    /testbed contents: ${TESTBED_DIR}"
    else
        warn "image not found locally: ${IMAGE}"
        echo "    Pull it with: docker pull ${IMAGE}"
        ALL_IMAGES_OK=0
    fi
    echo ""
done

if [ "$ALL_IMAGES_OK" -ne 1 ]; then
    fail_loud "One or more SWE-bench images are missing locally. Pull them first:
    docker pull swebench/sweb.eval.x86_64.psf_1776_requests-863:latest
    docker pull swebench/sweb.eval.x86_64.pallets_1776_flask-4045:latest
    docker pull swebench/sweb.eval.x86_64.sympy_1776_sympy-20590:latest"
fi

# ---------------------------------------------------------------------------
# Step 4: Print per-instance wall-time and cost estimate
# ---------------------------------------------------------------------------
log_step "Per-instance estimate (wall-time + cost)"

SPIKE_END_EPOCH=$(date +%s)
ELAPSED=$((SPIKE_END_EPOCH - SPIKE_START_EPOCH))

# Estimates based on: typical claude-code solve time for small SWE-bench instances
# Container overhead (image start): ~15-30s per instance (images are warm after first pull)
# Claude-code solve: ~5-20 min per instance (Sonnet 4.5, hard limit recommended: 30 min)
# Transcript ingest + extraction: background, no wall-time cost on the solve path
cat <<'EOF'
  Estimate (LABELED AS ESTIMATE — based on spike setup time, not live model runs):

  Per-instance:
    Docker container start:     ~15-30 seconds (image already pulled/warm)
    Claude Code solve (Sonnet): 5-20 minutes per instance (expect ~10 min average)
    Transcript capture/ingest:  background, no wall-time overhead
    Image pull (first time):    ~2-5 minutes per image (shared base layers speed up 2nd/3rd)

  3-instance proof estimate:
    Wall time:  ~35-65 minutes total (excluding image pulls)
    API cost:   ~$0.10-$0.50 per instance on claude-sonnet-4-5 (rough estimate)
                (input tokens: problem statement + context ≈ 5k-50k; output: patch ≈ 0.5k-5k)
    Total cost: ~$0.30-$1.50 for 3 instances

  NOTE: Cost depends heavily on problem complexity and how many turns the agent uses.
  Budget $2.00 to be safe for the 3-instance proof. Monitor with --output-format=stream-json.
EOF

# ---------------------------------------------------------------------------
# Step 5: Print the exact command for the live 3-instance proof
# ---------------------------------------------------------------------------
log_step "Live 3-instance proof — exact command for orchestrator"

cat <<LIVEPROOF

  The orchestrator should run this script AFTER the corpus build completes:

    SWEBENCH_WORKSPACE=/tmp/swebench-lite-workspace \\
    MCP_SERVER_PORT=3001 \\
    scripts/swebench/run-3instance-proof.sh

  That script (to be created by the orchestrator, NOT this spike script) should:
    1. Create \${SWEBENCH_WORKSPACE} with a .git dir (for scope resolution) or set
       SKILL_PROJECT_ROOT=\${SWEBENCH_WORKSPACE} before each claude invocation.
    2. For each instance:
         INSTANCE_ID=psf__requests-863
         IMAGE=swebench/sweb.eval.x86_64.psf_1776_requests-863:latest
         a. Start container: docker run -d --name swebench-\${INSTANCE_ID} \${IMAGE} sleep 3600
         b. Fetch problem statement from HuggingFace dataset (see fetch_problem_statement.py).
         c. Run claude-code NON-INTERACTIVELY:
              time claude \\
                --settings "${SCRIPT_DIR}/settings-swebench.json" \\
                --print \\
                --dangerously-skip-permissions \\
                --max-turns 40 \\
                "Fix the following issue in /testbed (running in container swebench-\${INSTANCE_ID}):\\n\$PROBLEM_STATEMENT" \\
              2>&1 | tee /tmp/swebench-\${INSTANCE_ID}.log
         d. Stop container: docker stop swebench-\${INSTANCE_ID} && docker rm swebench-\${INSTANCE_ID}
         e. Record: exit_code, wall_time, log path
    3. After all 3 instances: confirm .pending files landed in \${SWEBENCH_WORKSPACE}/.skills/
    4. Report: instance_id, solved/unsolved (by inspecting container test results), wall_time, cost_estimate.

  HOOKS: The settings-swebench.json config (${SCRIPT_DIR}/settings-swebench.json) wires:
    - SessionStart → compile_context (pulls relevant skills before solve starts)
    - UserPromptSubmit → compile_context (refreshes context mid-session)
    - SessionEnd → capture-transcript.sh → /ingest/transcript (enqueues for extraction)

  PROJECT SCOPE: All claude invocations use cwd=${SWEBENCH_WORKSPACE}.
  The hook payload's cwd field = ${SWEBENCH_WORKSPACE}.
  The ingest POST's repo_path = ${SWEBENCH_WORKSPACE}.
  .pending drafts land in ${SWEBENCH_WORKSPACE}/.skills/ — isolated to swebench-lite scope.

LIVEPROOF

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
SPIKE_END_EPOCH=$(date +%s)
ELAPSED=$((SPIKE_END_EPOCH - SPIKE_START_EPOCH))

echo ""
echo "================================================================"
echo "  run-spike.sh complete"
echo "================================================================"
echo "  compile_context : ${CC_STATUS} (graph_version=${CC_GV})"
echo "  find_skill      : ${FS_STATUS}"
echo "  ingest POST     : ${INGEST_STATUS}"
echo "  hooks config    : valid"
echo "  images present  : 3/3"
echo "  elapsed         : ${ELAPSED}s"
echo ""
echo "RESULT: spike-ok — ready for live 3-instance proof"
