#!/usr/bin/env bash
# Deterministic verifier for the 123corp-hr-policy measured sibling (T23 clband, depth-6).
#
# WHY: The '123 Corp' HR policy (context.md) is a fictional company document. The depth-6
# measured sibling asks about (a) whether a doctor's note is required for sick leave, and
# (b) how long it takes to accrue a week (40 hours) of sick pay. The key invented rules are:
#   - Section 7.02 states full-time employees accrue TWO (2) hours of sick leave per pay period.
#   - Medical attention cases require a letter stating DIAGNOSIS, PROGNOSIS, and work limitations.
#   - Medical certification (a written statement) may be required upon return from any sick leave.
#   - At 2 hours/pay-period over 26 bi-weekly periods/year, 20 pay periods (~10 months) reach 40h.
#   - The policy is in section 7.02 of the company document.
# These numeric constants and section codes are invented — OFF cannot produce them from
# general HR knowledge; the extracted skill carries them.
#
# Checks are compiled VERBATIM from the knowledge rubrics of task 361e77fa (depth-6, tasks.json).
# Persona/tone/format rubrics are dropped from the deterministic core (judge prompt secondary).
#
# Contract: invoked as `<this> <workspace_dir>`. Exit 0 == invented rules OBEYED. Non-zero == loss.
# Pure deterministic inspection of solution.md; no network, no LLM.
set -uo pipefail

ws="${1:?usage: 123corp-hr-policy.sh <workspace_dir>}"

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

# Check 1 — MEDICAL CERTIFICATION / DOCTOR'S NOTE (context.md:39, rubric 2):
# "While it is not explicitly mandatory, your manager may request a note" / "medical certification".
# The policy says a written statement of medical certification MAY be required; a correct ON-arm
# answer names this requirement rather than omitting it or stating none is ever needed.
echo "$lc" | grep -Eq 'medical certification|doctor.{0,4}s note|doctor note|note from.*health|health.*provider|written statement' \
  || fail "missing the section 7.02 medical-certification / doctor's-note rule (invented policy requirement)"

# Check 2 — DIAGNOSIS, PROGNOSIS, WORK LIMITATIONS (context.md:41, rubric 3):
# "In the event of an injury or illness that requires medical attention, the employee must furnish
# a letter ... that states the diagnosis, prognosis, and any work limitations."
# Both 'diagnosis' and 'prognosis' must appear; together they identify the specific invented form.
echo "$lc" | grep -iF 'diagnosis' \
  || fail "missing the section 7.02 diagnosis requirement for medical-attention sick leave (invented rule)"
echo "$lc" | grep -iF 'prognosis' \
  || fail "missing the section 7.02 prognosis requirement for medical-attention sick leave (invented rule)"

# Check 3 — TWO HOURS PER PAY PERIOD ACCRUAL (context.md:37, rubric 5):
# "Full-time employees will accrue two (2) hours of sick leave each pay period."
# The correct answer must state the 2-hour-per-pay-period rate to derive the 10-month answer.
echo "$lc" | grep -Eq '2 hours?|two.*hours?.*pay|two \(2\) hours?' \
  || fail "missing the section 7.02 sick-leave accrual rate of 2 hours per pay period (invented constant)"

# Check 4 — APPROXIMATELY 10 MONTHS / 20 PAY PERIODS (rubric 4 + 5):
# "As you accrue 2 hours of sick pay per pay period, it will take you 20 pay periods,
# approximately 10 months, to accumulate one full week."
# A correct answer must state ~10 months or explicitly cite 20 pay periods.
echo "$lc" | grep -Eq '10 months?|20 pay|twenty.*pay|20.*period|ten months?' \
  || fail "missing the ~10-months / 20-pay-periods derivation for one week of sick pay (invented rule)"

# Check 5 — SECTION 7.02 REFERENCE (rubric 6):
# "The information detailing the company's sick leave policy can be found in section 7.02."
# An ON-arm answer that read the extracted skill should cite section 7.02.
echo "$lc" | grep -Eq '7\.02|section 7' \
  || fail "missing section 7.02 reference for sick leave policy (invented section code)"

echo "WIN: all 5 invented 123 Corp HR-policy rules obeyed (medical-cert, diagnosis+prognosis, 2h/period, ~10mo/20pp, 7.02)"
exit 0
