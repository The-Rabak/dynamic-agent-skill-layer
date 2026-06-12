#!/usr/bin/env bash
# Deterministic verifier for the drywave-3000-manual measured sibling (T23 Unit A).
#
# WHY: The "DryWave 3000" product manual (context.md) is an INVENTED technical document
# containing fictional RH target ranges, mode prescriptions, and safety rules specific to
# this product. The measured sibling presents an annex scenario with a year-round RH that
# never exceeds 28% — well below the manual's explicit 35% Continuous-mode prohibition and
# its 40% lower-limit guidance for domestic living areas. The OFF arm cannot know:
#   (a) the 40%-floor rule with its exact consequences (human discomfort + wood contracting),
#   (b) the explicit ban on Continuous mode below 35% RH,
#   (c) that AUTO is the correct prescribed mode,
#   (d) the 45–50% target RH for normal living spaces,
#   (e) the recommendation to verify an anomalously low reading with an external calibrated hygrometer.
# All five checks are compiled VERBATIM from knowledge rubrics in tasks.json (depth-6 sibling
# b7dd8397). Persona / format-only rubrics (structured output, UK-English tone) are dropped
# from this deterministic core and handled by the judge-prompt secondary score.
#
# Contract: invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == loss. One-line reason to stdout.
# Pure deterministic inspection of solution.md; no network, no LLM.
set -uo pipefail

ws="${1:?usage: drywave-3000-manual.sh <workspace_dir>}"

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

# Check 1 — LOW-HUMIDITY FLOOR RULE (context.md:245): "Maintaining humidity below 40% for
# extended periods in domestic living areas is not recommended". The answer must warn that
# the annex at <28% RH must NOT be continuously dehumidified because the manual explicitly
# forbids sustained operation below 40% in living areas.
echo "$lc" | grep -iE 'below 40%|below 40 percent|humidity.*below.*40|40%.*not recommended|not recommended.*40%|prolonged.*low.moisture|low.moisture.*discomfort' \
  || fail "missing invented 40%-floor rule: the manual explicitly forbids extended operation below 40% in domestic living areas"

# Check 2 — HUMAN DISCOMFORT / WOOD CONTRACTING consequence (context.md:245): the manual states
# the specific harm: "human discomfort and certain wood materials contracting or cracking".
# The answer must name at least one of these invented consequences.
echo "$lc" | grep -iE 'human discomfort|wood.*contract|contract.*crack|wood.*crack|material.*crack|wood.*shrink|discomfort.*low|prolonged low' \
  || fail "missing invented consequence of sub-40% RH: human discomfort and/or wood materials contracting or cracking"

# Check 3 — CONTINUOUS MODE PROHIBITION BELOW 35% RH (context.md:731): "In extreme dryness
# (below 35% RH), the unit should not be operated in Continuous mode." The annex is at ≤28%,
# so Continuous mode must be explicitly ruled out or warned against.
echo "$lc" | grep -iE 'continuous mode.*not|not.*continuous mode|avoid.*continuous|continuous.*inappropriate|not.*continuous|should not.*continuous|continuous.*unsuitable|continuous.*inadvisable|extreme dryness|below 35' \
  || fail "missing invented rule: Continuous mode must not be used at below 35% RH (annex is at 28%)"

# Check 4 — AUTO MODE PRESCRIPTION (context.md sections 12, 20.2, 29): AUTO mode is the
# correct prescribed mode for general living-space conditioning; the answer must recommend AUTO.
echo "$lc" | grep -iE '\bauto\b.*mode|\bmode\b.*auto|auto mode|automatic mode' \
  || fail "missing AUTO mode prescription — the manual specifies AUTO for general domestic conditioning"

# Check 5 — 45–50% TARGET RH FOR NORMAL LIVING SPACES (context.md:224, 381): the manual
# specifies 45–55% for normal living spaces and ~45–50% for general conditioning; the answer
# must cite this invented target range for the annex.
echo "$lc" | grep -iE '45.5[05]%|45%.*50%|45.*50.*percent|45 to 5[05]|50%.*target|target.*4[5-9]%|rh.*4[5-9]|4[5-9].*rh' \
  || fail "missing invented target RH of 45–50%/45–55% for normal domestic living spaces"

# Check 6 — VERIFY ANOMALOUSLY LOW READING with external calibrated hygrometer (context.md:511):
# "For precise verification, users may use an external calibrated hygrometer." At 28% RH the
# manual implies measuring accuracy ±2%; an informed adviser must recommend verifying the reading.
# Matching on "hygrometer", "verify/check the reading/accuracy", or noting 28% is extremely/
# anomalously/unusually low as a prompt for verification. Deliberately excludes bare "lower side"
# phrasing that does not imply a verification action.
echo "$lc" | grep -iE 'hygrometer|verify.*reading|check.*reading|calibrat|confirm.*reading|verify.*humidity.*reading|check.*accuracy|independent.*measurement|second.*reading|question.*reading|unusual.*low|abnormally.*low|28%.*unusual|28%.*extremely|extremely.*low|anomalously|suspiciously.*low|low.*rh.*check|verify.*28|check.*28|28.*verify|28.*check|reading.*accurate|accurate.*reading|measurement.*accurate' \
  || fail "missing advice to verify or question the anomalously low 28% RH reading (invented rule: use external calibrated hygrometer)"

echo "WIN: all 6 invented DryWave 3000 rules obeyed (40%-floor warning, human-discomfort+wood consequence, Continuous-mode prohibition <35%, AUTO mode, 45-50% target, verify-28% reading)"
exit 0
