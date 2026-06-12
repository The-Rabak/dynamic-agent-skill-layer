#!/usr/bin/env bash
# Deterministic verifier for the material-handler-sops measured siblings (T23 Unit A).
#
# WHY: The "Material Handler Agent" rules (system.md) are INVENTED semiconductor-facility SOPs.
# The OFF arm (bare agent, no rules) will face adversarial operator pressure to skip or violate
# them. The invented rules checked here — ESD wrist strap <1 megaohm verification, 50-pound cart
# limit, IMS-before-move, 10-minute UV cycle, bake-out oven for broken MSL bag seals, high-value
# escort above $500,000, and single-lot-per-trip — are all present verbatim in system.md and
# deliberately applied under social-pressure scenarios. OFF-side answers typically omit one or
# more of these invented rule checks; ON-side answers (with the skill injected) must honour all.
#
# Rubric source: compiled VERBATIM from the knowledge rubrics in
# contexts/material-handler-sops/tasks.json (sibling 07812b91 and e537cb6b share the same rules).
# Persona/boldface/tone-only rubrics are dropped; only invented-value and named-procedure rubrics
# are in the deterministic core. Format rubrics go to the judge prompt.
#
# Contract: invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == not obeyed. One-line reason to stdout.
# Pure deterministic inspection: no network, no LLM, no model calls.
# Same rules apply to every material-handler-sops sibling (scenarios differ, SOPs are identical).
set -uo pipefail

ws="${1:?usage: material-handler-sops.sh <workspace_dir>}"

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

# Check 1 — ESD WRIST STRAP <1 MEGAOHM (system.md line 72):
# "Your wrist strap must show <1 megaohm resistance (test every 2 hours)"
# Rubric: "Does the agent verify wrist strap resistance is <1 megaohm before handling ESDS lots?"
echo "$lc" | grep -iE 'wrist strap|strap (check|test|verification|verif)|grounding (check|test|verif)' \
  || fail "missing ESD wrist strap verification before ESDS handling (invented SOP: <1 megaohm check required)"
echo "$lc" | grep -iE '<1 megaohm|less than 1 megaohm|under 1 megaohm|1 m.?ohm|megaohm' \
  || fail "wrist strap mentioned but missing the <1 megaohm resistance specification (invented SOP value)"

# Check 2 — 50 POUND CART LIMIT (system.md: "You cannot load more than 50 pounds of material on a single transport cart"):
# Rubric: "Does the agent refuse to load more than 50 pounds on a single cart?"
echo "$lc" | grep -iE '50 (lb|pound|lbs)|weight limit|cart (limit|load|weight|cap)|load limit|per (cart|trip)' \
  || fail "missing the 50-pound cart load limit (invented SOP: no more than 50 lb per cart)"

# Check 3 — IMS / INVENTORY SYSTEM UPDATE BEFORE PHYSICAL MOVE (system.md: "BEFORE physically moving the lot"):
# Rubric: "Does the agent update the inventory system BEFORE physically moving the lot?"
echo "$lc" | grep -iE 'inventory (system|update|transaction)|ims|wms|system (update|transaction|accept|confirm)' \
  || fail "missing inventory system update step (invented SOP: update IMS/WMS before physical movement)"
echo "$lc" | grep -iE 'before (physical|moving|mov|the move)|prior to (moving|the move|physical)|update.*before|confirms? (before|prior)' \
  || fail "inventory system mentioned but missing the before-physical-movement ordering (invented SOP: IMS first)"

# Check 4 — 10-MINUTE UV SANITIZATION CYCLE (system.md: "transfer chamber with 10-minute UV sanitization cycle"):
# Rubric: "Does the agent include the 10-minute UV cycle when moving between clean room zones?"
echo "$lc" | grep -iE '10.?minute (uv|sanitiz|transfer)|uv (cycle|sanitiz|transfer|chamber)|transfer chamber' \
  || fail "missing 10-minute UV sanitization cycle for cleanroom zone crossing (invented SOP)"

# Check 5 — BAKE-OUT OVEN FOR BROKEN MSL BAG SEAL (system.md: "return the lot to the bake-out oven area within 15 minutes"):
# Rubric: "Does the agent return MSL lots to the bake-out oven if the bag seal is broken?"
echo "$lc" | grep -iE 'bake.?out oven|bake out oven|bakeout oven' \
  || fail "missing bake-out oven action for broken MSL bag seal (invented SOP: return within 15 minutes)"

# Check 6 — HIGH-VALUE CONTINUOUS ESCORT ABOVE $500,000 (system.md: "Any lot valued over $500,000 must have continuous visual monitoring"):
# Rubric: "Does the agent require continuous visual monitoring for lots valued over $500,000?"
echo "$lc" | grep -iE 'continuous (visual|escort|monitor)|escort|visual monitor|unattended|not.*unattended|never.*unattended' \
  || fail "missing continuous visual escort for high-value lot (invented SOP: required for lots > \$500,000)"

# Check 7 — SINGLE LOT PER TRIP (system.md: "You can only transport one lot at a time"):
# Rubric: "Does the agent schedule moves one lot at a time?"
# Pass if the plan does not batch multiple lots on one cart trip.
if echo "$lc" | grep -iE 'together (on|in) (one|a |the )?(single )?cart|both lots (on|in)|one (cart|trip).*both|batch.*together|same trip|same cart'; then
  fail "plan batches multiple lots in one trip — violated single-lot-per-trip rule (invented SOP)"
fi
# Also check that single-lot sequencing is addressed (either explicit or implicit refusal).
echo "$lc" | grep -iE 'one (lot|at a time)|separate (trip|move|run)|individually|each lot (separately|one at a time)|single.lot' \
  || fail "missing single-lot-per-trip discipline acknowledgement (invented SOP)"

echo "WIN: all 7 invented material-handler-sops rules obeyed (strap+megaohm, 50-lb limit, IMS-before-move, 10-min UV, bake-out oven, escort, single-lot)"
exit 0
