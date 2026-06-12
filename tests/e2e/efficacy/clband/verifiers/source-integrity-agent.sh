#!/usr/bin/env bash
# Deterministic verifier for the source-integrity-agent measured siblings (T14 clband band).
#
# WHY: The "Source Integrity Agent (SIA)" rules (system.md) are an INVENTED multi-agent
# news-verification pipeline. Its rigid JSON output schema, the four-component Domain Trust
# Score formula, and the exact enum literals for status/risk_level fields are all fabricated
# constants — an unguided model has no basis for knowing them. The checks below are compiled
# VERBATIM from CL-bench rubrics shared across the measured siblings (tasks.json rubrics):
#
#   Check 1 — required JSON top-level keys (domain_trust_score with score/calculation_basis/
#             justification; next_agent_notes with to_FCA / to_AO) [rubrics: "domain_trust_score
#             object include 'score', 'calculation_basis', and 'justification'"; "next_agent_notes
#             object with keys 'to_FCA' and 'to_AO'"]
#   Check 2 — metadata_validation.status enum: exactly "PASS | FAIL | INCOMPLETE"
#             [rubric: "metadata_validation.status uses exactly 'PASS | FAIL | INCOMPLETE'"]
#   Check 3 — EXIF_integrity enum: exactly "INTACT | TAMPERED | ABSENT"
#             [rubric: "EXIF_integrity result uses exactly 'INTACT | TAMPERED | ABSENT'"]
#   Check 4 — spoofing_detection.risk_level enum: exactly "NONE | LOW | MEDIUM | HIGH | CRITICAL"
#             [rubric: "spoofing_detection.risk_level uses exactly 'NONE | LOW | MEDIUM | HIGH |
#             CRITICAL'"]
#   Check 5 — WHOIS privacy protection deduction: -15 points explicitly stated
#             [rubric: "If WHOIS privacy protection is detected, does SIA reduce the trust score
#             by 15 points?"]
#   Check 6 — Missing metadata sentinel: uses "MISSING" (not "ABSENT" or empty)
#             [rubric: "Does SIA explicitly state 'MISSING' for any unavailable metadata field?"]
#   Check 7 — Trust Score formula components present: Domain_Age_Score and SSL_Validity
#             [rubric: "Does SIA use the exact formula for the Domain Trust Score calculation?";
#             system.md: "Trust Score = (Domain_Age_Score × 0.4) + (SSL_Validity × 0.3) + ..."]
#   Check 8 — timestamp_match enum: exactly "MATCH | MISMATCH | MISSING"
#             [rubric: "timestamp_match result uses exactly 'MATCH | MISMATCH | MISSING'"]
#
# Contract (tests/e2e/efficacy/CONTRACT.md): invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == loss. One-line reason to stdout.
# Pure deterministic inspection of solution.md; no network, no LLM.
# Same rules for every source-integrity-agent sibling (siblings differ by scenario; the
# invented schema + formula they exercise is identical).
set -uo pipefail

ws="${1:?usage: source-integrity-agent.sh <workspace_dir>}"

# Read the agent's produced answer. Prefer solution.md; else all *.md/*.txt not under .git.
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

# Check 1 — Required JSON top-level schema keys: domain_trust_score with score/calculation_basis/
# justification, and next_agent_notes with to_FCA / to_AO.
# (Rubrics: "domain_trust_score object include score, calculation_basis, and justification";
#  "next_agent_notes object with keys to_FCA and to_AO" — both in every sibling's rubric set.)
echo "$lc" | grep -iF '"domain_trust_score"' \
  || fail "missing required top-level key 'domain_trust_score' (invented SIA JSON schema)"
echo "$lc" | grep -iF '"calculation_basis"' \
  || fail "missing 'calculation_basis' sub-key inside domain_trust_score (invented SIA schema)"
echo "$lc" | grep -iF '"justification"' \
  || fail "missing 'justification' sub-key inside domain_trust_score (invented SIA schema)"
echo "$lc" | grep -iF '"next_agent_notes"' \
  || fail "missing required key 'next_agent_notes' (invented SIA JSON schema)"
echo "$lc" | grep -iF '"to_fca"' \
  || fail "next_agent_notes missing 'to_FCA' key (invented SIA routing schema)"
echo "$lc" | grep -iF '"to_ao"' \
  || fail "next_agent_notes missing 'to_AO' key (invented SIA routing schema)"

# Check 2 — metadata_validation.status must use exactly "PASS", "FAIL", or "INCOMPLETE".
# (Rubric: "metadata_validation.status uses exactly 'PASS | FAIL | INCOMPLETE'")
echo "$lc" | grep -iE '"status"\s*:\s*"(pass|fail|incomplete)"' \
  || fail "metadata_validation.status not one of exactly PASS|FAIL|INCOMPLETE (invented SIA enum)"

# Check 3 — EXIF_integrity result must use exactly "INTACT", "TAMPERED", or "ABSENT".
# (Rubric: "EXIF_integrity result uses exactly 'INTACT | TAMPERED | ABSENT'")
echo "$lc" | grep -iE '"exif_integrity"' \
  || fail "missing 'EXIF_integrity' check in metadata_validation findings (invented SIA schema)"
echo "$lc" | grep -iE '"result"\s*:\s*"(intact|tampered|absent)"' \
  || fail "EXIF_integrity result not one of exactly INTACT|TAMPERED|ABSENT (invented SIA enum)"

# Check 4 — spoofing_detection.risk_level must use exactly "NONE", "LOW", "MEDIUM", "HIGH",
# or "CRITICAL". (Rubric: "spoofing_detection.risk_level uses exactly 'NONE|LOW|MEDIUM|HIGH|CRITICAL'")
echo "$lc" | grep -iF '"spoofing_detection"' \
  || fail "missing required key 'spoofing_detection' (invented SIA JSON schema)"
echo "$lc" | grep -iE '"risk_level"\s*:\s*"(none|low|medium|high|critical)"' \
  || fail "spoofing_detection.risk_level not one of NONE|LOW|MEDIUM|HIGH|CRITICAL (invented SIA enum)"

# Check 5 — WHOIS privacy protection must trigger a -15 point trust score deduction.
# (Rubric: "If WHOIS privacy protection is detected, does SIA reduce the trust score by 15 points?";
#  system.md line 202: "Proceed with reduced trust score (-15 points)")
echo "$lc" | grep -iE '15\s*points?|[-−]\s*15|deduct\w*\s*15|reduc\w*\s*15' \
  || fail "missing WHOIS privacy protection -15 point trust score deduction (invented SIA rule)"

# Check 6 — Unavailable metadata fields must be represented as "MISSING" (not "ABSENT", null,
# or empty string). (Rubric: "Does SIA explicitly state 'MISSING' for any unavailable metadata
# field?"; system.md line 110: "MUST explicitly state 'MISSING' for any unavailable metadata field")
echo "$lc" | grep -iF '"missing"' \
  || fail "missing 'MISSING' sentinel for unavailable metadata fields (invented SIA constraint)"

# Check 7 — Trust Score formula must reference the invented component names Domain_Age_Score
# and SSL_Validity from the spec formula:
#   Trust Score = (Domain_Age_Score × 0.4) + (SSL_Validity × 0.3) + ...
# (Rubric: "Does SIA use the exact formula for the Domain Trust Score calculation?";
#  system.md lines 135-138 define the four named components.)
echo "$lc" | grep -iE 'domain.?age.?score|domain age score' \
  || fail "missing Domain_Age_Score component in trust score calculation (invented SIA formula)"
echo "$lc" | grep -iE 'ssl.?validity' \
  || fail "missing SSL_Validity component in trust score calculation (invented SIA formula)"

# Check 8 — timestamp_match result must use exactly "MATCH", "MISMATCH", or "MISSING".
# (Rubric: "timestamp_match result uses exactly 'MATCH | MISMATCH | MISSING'")
echo "$lc" | grep -iE '"timestamp_match"' \
  || fail "missing 'timestamp_match' check in metadata_validation findings (invented SIA schema)"
echo "$lc" | grep -iE '"result"\s*:\s*"(match|mismatch|missing)"' \
  || fail "timestamp_match result not one of exactly MATCH|MISMATCH|MISSING (invented SIA enum)"

echo "WIN: all 8 invented SIA rules obeyed (schema keys, status/EXIF/risk_level enums, -15 WHOIS deduction, MISSING sentinel, formula components, timestamp_match enum)"
exit 0
