#!/usr/bin/env bash
# Deterministic verifier for the flywheel-assembly-agent measured siblings (T14 clband smoke).
#
# WHY: The "Flywheel Manufacturing Multi-Agent System" rules (system.md) are an INVENTED
# operational SOP. The OFF arm (bare agent, no rules) cannot know the system's two signature
# WORKAROUND PROTOCOLS — both are deliberately unusual procedures a competent model would NOT
# independently recommend — nor the mandatory-spin-test gate or the Agent-C/Agent-D routing.
# These checks are compiled VERBATIM from the CL-bench rubrics shared across the 12 siblings
# (system.md lines 30-32, 53-65). Knowledge rubrics only; persona/boldface format rubrics are
# dropped from the deterministic core (judge-prompt secondary).
#
# Contract (tests/e2e/efficacy/CONTRACT.md): invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == not obeyed. One-line reason to stdout.
# Pure deterministic inspection: no network, no LLM. Same rules for every flywheel sibling
# (the siblings differ only by scenario; the invented SOP they exercise is identical).
set -uo pipefail

ws="${1:?usage: flywheel-assembly.sh <workspace_dir>}"

# Concatenate the agent's produced answer. Prefer solution.md; else all text files.
sol="$ws/solution.md"
if [[ -f "$sol" ]]; then
  text="$(cat "$sol")"
else
  text="$(find "$ws" -type f \( -name '*.md' -o -name '*.txt' \) -not -path '*/.git/*' -exec cat {} + 2>/dev/null)"
fi
if [[ -z "${text// /}" ]]; then
  echo "LOSS: no answer text found in workspace ($ws) — agent produced no solution.md/.txt"
  exit 1
fi
lc="$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')"

fail() { echo "LOSS: $1"; exit 1; }

# Check 1 — WRENCH WORKAROUND (system.md:31): "next size up" + "extra torque".
echo "$lc" | grep -Eq 'next size up' \
  || fail "missing wrench workaround 'next size up' (invented WORKAROUND PROTOCOL)"
echo "$lc" | grep -Eq 'extra torque|more torque|additional torque|apply (more|extra) torque' \
  || fail "wrench workaround present but missing the 'extra torque' half of the invented rule"

# Check 2 — WOBBLE WORKAROUND (system.md:32): firm shake + retest.
echo "$lc" | grep -Eq 'firm shake|good shake|firm, ?good shake|good,? ?firm shake|give it a (good |firm )?shake|shake (it|the unit)' \
  || fail "missing wobble workaround 'firm shake' (invented WORKAROUND PROTOCOL)"
echo "$lc" | grep -Eq 'retest|re-test|spin again|test again' \
  || fail "wobble workaround present but missing the 'retest' half of the invented rule"

# Check 3 — MANDATORY SPIN TEST, not skipped (system.md:35,61).
echo "$lc" | grep -Eq 'spin test|hand spin|spin check by hand|basic spin' \
  || fail "missing the mandatory basic spin test (invented hard quality gate)"
if echo "$lc" | grep -Eq 'skip (the )?(basic )?(hand )?spin|omit (the )?spin|no spin test|without (a )?spin test'; then
  fail "the mandatory spin test is SKIPPED — invented rule forbids skipping under any pressure"
fi

# Check 4 — NOTE routed to the Validation Engineer / Agent C (system.md:51,64).
echo "$lc" | grep -Eq 'validation engineer|agent c\b|agent \(c\)|to validation' \
  || fail "Spin Check Note not routed to the Validation Engineer (Agent C)"

# Check 5 — TRANSPORT handoff via the Forklift Operator / Agent D (system.md:65).
echo "$lc" | grep -Eq 'forklift|agent d\b|agent \(d\)' \
  || fail "missing the transport handoff to the Forklift Operator (Agent D)"

echo "WIN: all 5 invented flywheel rules obeyed (wrench-up+torque, shake+retest, spin-test, ->Agent C, ->Agent D)"
exit 0
