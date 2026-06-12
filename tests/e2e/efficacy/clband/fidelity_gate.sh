#!/usr/bin/env bash
# Fidelity gate (plan §4 Step 3; T22 two-tier) — deterministic sentinel-coverage check.
#
# TWO TIERS (T22):
#   sentinels_operative  — the constants/rules Session B actually needs (derived from each context's
#                          deterministic verifier). This is the GATING tier: every operative sentinel
#                          MUST appear (case-insensitive) across the scope's skill texts, or the gate
#                          FAILS LOUD (INSTRUMENT-FAILURE(extraction): the invented rule did not survive
#                          extraction, so the context yields no Session B data point).
#   sentinels_document   — system-name tier (personas, system titles). REPORTED for context, NOT gating:
#                          the preference/convention channel never emits these, and they are not what a
#                          Session B verifier checks.
# Back-compat: if sentinels_operative is absent, the legacy `sentinels` array is used as the gating tier.
#
# Usage: fidelity_gate.sh <context_short> <scope_dir>
#   context_short ∈ the manifest "short" field (e.g. 7833ca0b flywheel, bc874bce aether)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
MANIFEST="$ROOT/tests/e2e/efficacy/clband/manifest.json"
ctx="${1:?usage: fidelity_gate.sh <context_short> <scope_dir>}"
scope="${2:?usage: fidelity_gate.sh <context_short> <scope_dir>}"

read_tier() {  # read_tier <jq_field_expr> ; emits newline-separated values (empty if absent)
  local expr="$1"
  if command -v jq >/dev/null 2>&1; then
    jq -r --arg s "$ctx" ".contexts[]|select(.short==\$s)|${expr}" "$MANIFEST" 2>/dev/null
  else
    python3 -c "
import json
m=json.load(open('$MANIFEST'))
c=[x for x in m['contexts'] if x['short']=='$ctx'][0]
field='${expr}'.split('//')[0].strip().lstrip('.').split('[')[0]
print('\n'.join(c.get(field, []) or []))
"
  fi
}

# Gating tier: operative, falling back to legacy `sentinels`.
mapfile -t operative < <(read_tier '.sentinels_operative[]?')
if [[ ${#operative[@]} -eq 0 ]]; then
  mapfile -t operative < <(read_tier '.sentinels[]?')
  gating_tier="sentinels (legacy)"
else
  gating_tier="sentinels_operative"
fi
mapfile -t document < <(read_tier '.sentinels_document[]?')
if command -v jq >/dev/null 2>&1; then
  name=$(jq -r --arg s "$ctx" '.contexts[]|select(.short==$s)|.name' "$MANIFEST")
else
  name=$(python3 -c "import json;m=json.load(open('$MANIFEST'));print([x for x in m['contexts'] if x['short']=='$ctx'][0]['name'])")
fi
[[ ${#operative[@]} -gt 0 ]] || { echo "FATAL: no operative/legacy sentinels for context '$ctx' in manifest"; exit 2; }

# Gather the scope's skill texts (drafts + accepted), excluding the .git marker.
texts=$(find "$scope" -type f \( -name 'SKILL.md' -o -name 'SKILL.md.pending' -o -name '*.pending' \) -not -path '*/.git/*' 2>/dev/null | sort -u)
n_skills=$(printf '%s\n' "$texts" | grep -c . || true)
echo "=== fidelity gate: $name ($ctx) — $n_skills skill file(s) under $scope ==="

present_in_texts() {  # present_in_texts <needle> -> echoes matching files (empty if none)
  [[ -z "$texts" ]] && return 0
  printf '%s\n' "$texts" | xargs grep -ilF -- "$1" 2>/dev/null
}

# Document tier — reported only.
if [[ ${#document[@]} -gt 0 ]]; then
  echo "--- document tier (reported, NOT gating) ---"
  for s in "${document[@]}"; do
    hit=$(present_in_texts "$s")
    if [[ -n "$hit" ]]; then echo "  present  '$s'"; else echo "  absent   '$s'"; fi
  done
fi

# Operative tier — GATING.
echo "--- operative tier ($gating_tier, GATING) ---"
missing=0
for s in "${operative[@]}"; do
  hit=$(present_in_texts "$s")
  if [[ -n "$hit" ]]; then
    echo "  PRESENT  '$s'  ($(printf '%s\n' "$hit" | grep -c .) file(s))"
  else
    echo "  MISSING  '$s'"
    missing=$((missing+1))
  fi
done

if [[ $missing -gt 0 ]]; then
  echo "FIDELITY FAIL: $missing/${#operative[@]} OPERATIVE sentinels missing -> INSTRUMENT-FAILURE(extraction) for $name"
  exit 1
fi
echo "FIDELITY PASS: all ${#operative[@]} operative sentinels present across the $name scope"
exit 0
