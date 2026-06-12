#!/usr/bin/env bash
# Deterministic verifier — aether-language depth-4 sibling (Turbulence-Alert bug review).
#
# WHY: "Aether" is a fully invented language (context.md). Its bug-report format is the invented
# "Turbulence Alert" block (Cause / Fix / Corrected Code), and — the key discriminator — its
# assignment operator is "<<", NOT "=". The planted bug is `~average = ~total / ~count`, which
# uses `=` where Aether requires `<<`. An agent WITHOUT the Aether spec has no reason to flag `=`
# as an error (it is ordinary assignment in every mainstream language) and would never title an
# error report "Turbulence Alert" — so OFF cannot produce this. Checks compiled from the CL-bench
# rubrics for task b0807c2c (knowledge rubrics only; boldface-format rubric dropped from the core).
#
# Contract: `<this> <workspace_dir>`; exit 0 == rules obeyed; one-line reason to stdout; no net/LLM.
set -uo pipefail

ws="${1:?usage: aether-turbulence-review.sh <workspace_dir>}"
sol="$ws/solution.md"
if [[ -f "$sol" ]]; then text="$(cat "$sol")"; else
  text="$(find "$ws" -type f \( -name '*.md' -o -name '*.txt' \) -not -path '*/.git/*' -exec cat {} + 2>/dev/null)"
fi
[[ -n "${text// /}" ]] || { echo "LOSS: no answer text found in workspace ($ws)"; exit 1; }
lc="$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')"
fail() { echo "LOSS: $1"; exit 1; }

# 1 — invented bug-report section name.
echo "$lc" | grep -Eq 'turbulence alert' \
  || fail "missing the invented 'Turbulence Alert' section (OFF cannot know this format)"
# 2-4 — the three mandated subsections.
echo "$lc" | grep -Eq 'cause'          || fail "Turbulence Alert missing 'Cause' subsection"
echo "$lc" | grep -Eq 'fix'            || fail "Turbulence Alert missing 'Fix' subsection"
echo "$lc" | grep -Eq 'corrected code' || fail "Turbulence Alert missing 'Corrected Code' subsection"
# 5 — the operative invented rule: assignment is '<<', so the fix replaces '=' with '<<'.
echo "$text" | grep -Eq '~average[[:space:]]*<<' \
  || fail "Corrected Code does not use Aether's '<<' assignment for ~average (the planted '=' bug)"
# 6 — invented scoping rule: 'outer ~total' must be KEPT (rubric: outer enables non-local write
#     inside swirl). An agent that 'cleans up' by deleting outer violates the spec.
echo "$lc" | grep -Eq 'outer' \
  || fail "removed 'outer' — invented Aether scoping rule requires keeping 'outer ~total' in swirl"

echo "WIN: Turbulence Alert (Cause/Fix/Corrected Code) + '<<' assignment fix + 'outer' kept — 6/6 invented rules"
exit 0
