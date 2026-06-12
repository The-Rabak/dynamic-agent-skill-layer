#!/usr/bin/env bash
# Fidelity gate (plan §4 Step 3) — deterministic sentinel-coverage check for a clband context.
#
# Every manifest sentinel for the context MUST appear (case-insensitive) somewhere across the
# scope's skill texts (.pending drafts pre-acceptance, or SKILL.md post-acceptance). A missing
# sentinel == INSTRUMENT-FAILURE(extraction): the invented rule did not survive extraction, so the
# context yields NO Session B data point and no "layer doesn't help" reading. Fails loud.
#
# Usage: fidelity_gate.sh <context_short> <scope_dir>
#   context_short ∈ the manifest "short" field (e.g. 7833ca0b flywheel, bc874bce aether)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
MANIFEST="$ROOT/tests/e2e/efficacy/clband/manifest.json"
ctx="${1:?usage: fidelity_gate.sh <context_short> <scope_dir>}"
scope="${2:?usage: fidelity_gate.sh <context_short> <scope_dir>}"

# Pull the sentinels for this context from the manifest (jq if available, else python).
if command -v jq >/dev/null 2>&1; then
  mapfile -t sentinels < <(jq -r --arg s "$ctx" '.contexts[]|select(.short==$s)|.sentinels[]' "$MANIFEST")
  name=$(jq -r --arg s "$ctx" '.contexts[]|select(.short==$s)|.name' "$MANIFEST")
else
  mapfile -t sentinels < <(python3 -c "import json,sys;m=json.load(open('$MANIFEST'));c=[x for x in m['contexts'] if x['short']=='$ctx'][0];print('\n'.join(c['sentinels']))")
  name=$(python3 -c "import json;m=json.load(open('$MANIFEST'));print([x for x in m['contexts'] if x['short']=='$ctx'][0]['name'])")
fi
[[ ${#sentinels[@]} -gt 0 ]] || { echo "FATAL: no sentinels for context '$ctx' in manifest"; exit 2; }

# Gather the scope's skill texts (drafts + accepted), excluding the .git marker.
texts=$(find "$scope" -type f \( -name 'SKILL.md' -o -name 'SKILL.md.pending' \) -not -path '*/.git/*' 2>/dev/null)
n_skills=$(printf '%s\n' "$texts" | grep -c . || true)
echo "=== fidelity gate: $name ($ctx) — $n_skills skill file(s) under $scope ==="

missing=0
for s in "${sentinels[@]}"; do
  if [[ -z "$texts" ]]; then hit=""; else hit=$(printf '%s\n' "$texts" | xargs grep -ilF -- "$s" 2>/dev/null); fi
  if [[ -n "$hit" ]]; then
    echo "  PRESENT  '$s'  ($(printf '%s\n' "$hit" | grep -c .) file(s))"
  else
    echo "  MISSING  '$s'"
    missing=$((missing+1))
  fi
done

if [[ $missing -gt 0 ]]; then
  echo "FIDELITY FAIL: $missing/${#sentinels[@]} sentinels missing -> INSTRUMENT-FAILURE(extraction) for $name"
  exit 1
fi
echo "FIDELITY PASS: all ${#sentinels[@]} sentinels present across the $name scope"
exit 0
