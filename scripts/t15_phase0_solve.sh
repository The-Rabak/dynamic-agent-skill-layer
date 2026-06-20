#!/usr/bin/env bash
# T15 Phase-0 seed solve-runner — runs SWE-bench seed solves SERIALLY (one at a time)
# through the host claude CLI with the skill-layer SessionStart injection wired.
# Phase 0 does NOT verify seeds (transcripts are the deliverable), so containers are
# torn down after the transcript is captured.
#
# Usage: t15_phase0_solve.sh <repo> <org> <workspace> <id...>
#   e.g. t15_phase0_solve.sh django django /tmp/swebench-phase0-django 16046 13447
set -uo pipefail
repo=$1; org=$2; ws=$3; shift 3
settings=/home/rabak/projects/dynamic-agent-skill-layer/scripts/swebench/settings-swebench.json
for id in "$@"; do
  iid="${org}__${repo}-${id}"
  img="swebench/sweb.eval.x86_64.${org}_1776_${repo}-${id}:latest"
  echo "=== SOLVE ${iid} @ $(date +%H:%M:%S) ==="
  docker rm -f "swebench-${iid}" >/dev/null 2>&1
  docker run -d --name "swebench-${iid}" "$img" sleep 7200 >/dev/null
  {
    echo "Fix the following GitHub issue in the ${repo} codebase located at /testbed."
    echo "Run commands inside the target container, e.g.: docker exec swebench-${iid} bash -lc '<cmd>'"
    echo "Investigate the code, edit the source under /testbed, and run the relevant tests to verify."
    echo
    cat "${ws}/problems/${iid}.txt"
  } > "${ws}/prompt-${iid}.txt"
  ( cd "$ws" && claude --settings "$settings" --print --dangerously-skip-permissions \
      --max-turns 40 --add-dir "$ws" < "${ws}/prompt-${iid}.txt" > "${ws}/solve-${iid}.log" 2>&1 )
  echo "  solve rc=$?"
  docker exec "swebench-${iid}" bash -lc 'cd /testbed && git diff --stat 2>/dev/null | tail -2' 2>&1 || true
  docker rm -f "swebench-${iid}" >/dev/null 2>&1
done
echo "ALL SOLVES DONE @ $(date +%H:%M:%S)"
