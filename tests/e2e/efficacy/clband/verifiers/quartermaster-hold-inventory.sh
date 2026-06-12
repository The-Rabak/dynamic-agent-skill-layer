#!/usr/bin/env bash
# Deterministic verifier for the quartermaster-hold-inventory measured siblings (T23 clband).
#
# WHY: The "Hold Inventory Agent — The Quartermaster" rules (system.md) are an INVENTED climbing-gym
# SOP. The OFF arm (bare agent, no rules) cannot know the system's exact fixed consumption constants
# (500 holds per Main Wall, 100 holds for Wall 5), the three-tier status-code taxonomy
# (HOLD_OK / HOLD_LOW / HOLD_CRITICAL_LOW) with their precise numeric boundaries (≥150% / 100-149%
# / <100%), nor the hard-stop constraint that HOLD_CRITICAL_LOW blocks plan authorization. These
# checks are compiled VERBATIM from the CL-bench rubrics shared across the siblings (system.md).
# Knowledge rubrics only; persona/format-only rubrics are dropped (judge-prompt secondary).
#
# Contract: invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == loss. One-line reason to stdout.
# Pure deterministic inspection: no network, no LLM. Same rules for every quartermaster sibling.
set -uo pipefail

ws="${1:?usage: quartermaster-hold-inventory.sh <workspace_dir>}"

# Read the agent's produced answer. Prefer solution.md; else all text files.
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

# Check 1 — FIXED MAIN WALL CONSUMPTION: exactly 500 holds (system.md: "every Main Wall requires
# exactly 500 holds"). OFF arms know no such fixed figure; 500 for a wall is an invented constant.
echo "$lc" | grep -Eq '500\b' \
  || fail "missing the invented fixed Main Wall consumption of exactly 500 holds"

# Check 2 — FIXED WALL 5 CONSUMPTION: exactly 100 holds (system.md: "the Boulder of the Week
# (Wall 5) requires exactly 100"). OFF arms have no basis for this specific number.
echo "$lc" | grep -Eq '\b100\b' \
  || fail "missing the invented fixed Wall 5 (Boulder of the Week) consumption of exactly 100 holds"

# Check 3 — STATUS CODE PRESENCE: one of the three invented status codes must appear. The codes
# HOLD_OK / HOLD_LOW / HOLD_CRITICAL_LOW are not standard inventory jargon — they are invented
# constants whose exact strings must appear verbatim (system.md: "I report `HOLD_OK`", "I report
# `HOLD_LOW`", "I must return `HOLD_CRITICAL_LOW`").
echo "$lc" | grep -Eq 'hold_ok|hold_low|hold_critical_low' \
  || fail "missing the invented status code (HOLD_OK / HOLD_LOW / HOLD_CRITICAL_LOW) in the Inventory Status report"

# Check 4 — HOLD_CRITICAL_LOW IS A HARD STOP: when HOLD_CRITICAL_LOW is assigned/returned as the
# status, the plan must be blocked (system.md: "This is a hard stop condition … must be escalated
# or reworked"; "I MUST NOT authorize any plan when inventory is below 100 percent of requirement").
# Only fires when the agent affirmatively assigns the code (not when they say it "does not apply").
# Pattern: "status: hold_critical_low", "**hold_critical_low**", "assigned: hold_critical_low",
# "return hold_critical_low", "report: hold_critical_low", or the code on its own line/header.
if echo "$lc" | grep -Eq '(status|assigned|code|result|return)[^.]{0,30}hold_critical_low|hold_critical_low[^.]{0,30}(assigned|status|code|hard stop)|^\s*hold_critical_low\s*$'; then
  # Critical-low was explicitly assigned as the status — confirm it is treated as a stop.
  echo "$lc" | grep -Eq 'hard stop|cannot proceed|stop.{0,40}setting|setting stops|plan (is )?(blocked|cannot|must not|halted)|halt.{0,30}critical|critical.{0,30}halt|escalat|rework|impossible' \
    || fail "HOLD_CRITICAL_LOW assigned as status but hard-stop language missing — invented hard-stop rule requires plan escalation/halt"
fi

# Check 5 — INVENTORY STATUS REPORT OUTPUT: the agent must produce an "Inventory Status report"
# (system.md: "I produce an Inventory Status report for the Setting Manager. This report includes
# the assigned status … and a numerical breakdown showing exactly how many holds are required
# versus how many are available").
echo "$lc" | grep -Eq 'inventory status( report)?|status report|holds (required|needed|available)|required.*available|available.*required' \
  || fail "missing the Inventory Status report (required vs available holds breakdown)"

# Check 6 — HOLD_OK BOUNDARY: exactly 150% available → HOLD_OK (system.md: "at least 150 percent
# of what we need, I report `HOLD_OK`"). This is an invented numeric boundary; OFF has no basis.
# Only fire when the text mentions a 150% figure or both 150 and HOLD_OK together.
if echo "$lc" | grep -Eq '\b150\b|150%|150 percent'; then
  echo "$lc" | grep -Eq 'hold_ok' \
    || fail "150% figure mentioned but HOLD_OK not assigned — invented ≥150% → HOLD_OK boundary violated"
fi

# Check 7 — HOLD_LOW BOUNDARY: 100-149% → HOLD_LOW (system.md: "between 100 and 149 percent of
# requirement, I report `HOLD_LOW`"). Only fire when text references the 100-149% window or
# explicitly discusses the HOLD_LOW tier.
if echo "$lc" | grep -Eq '\b14[0-9]\b|149%|100.{0,20}149|between 100|100 percent.{0,30}hold_low|hold_low.{0,30}100'; then
  echo "$lc" | grep -Eq 'hold_low' \
    || fail "100-149% range mentioned but HOLD_LOW not assigned — invented boundary rule violated"
fi

echo "WIN: all deterministic quartermaster rules obeyed (500 Main/100 Wall5 constants, status code present, hard-stop honoured, Inventory Status report present)"
exit 0
