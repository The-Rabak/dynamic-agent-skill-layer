#!/usr/bin/env bash
# T22 Unit D — clband smoke RE-RUN (the GO gate).
#
# Replays the two committed genuine teach transcripts through the FIXED pipeline (Unit B
# doc-delivery + Unit C taught-capture) into fresh isolated re-run scopes, then runs the
# two-tier fidelity gate (operative tier gates). Produces .pending drafts for the owner gate
# (DP-2) — NEVER auto-accepts.
#
# Replay (not re-capture) is deliberate: the teach->capture half already worked in the smoke
# (genuine sessions, faithful solution.md); the ONLY thing T22 changed is extraction (delivery +
# prompt), so replaying the genuine transcripts through the fixed extractor is the exact test of
# the fix. Re-capture is available (run_teach_session.py) if a fresh full run is wanted.
#
# Usage: run_smoke_rerun.sh            (both smoke contexts)
# Env:   EXTRACT_TEACH_CAPTURE=on|off  (default on — the intended T22 default)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
CLBAND="$ROOT/tests/e2e/efficacy/clband"
SMOKE="$ROOT/tests/e2e/reports/efficacy/clband-smoke"
RERUN="$ROOT/tests/e2e/reports/efficacy/clband-rerun"
export EXTRACT_TEACH_CAPTURE="${EXTRACT_TEACH_CAPTURE:-on}"
export CLBAND_TEACH_DELIVERY="${CLBAND_TEACH_DELIVERY:-on}"

mkdir -p "$RERUN/scopes"
echo "### clband smoke RE-RUN — EXTRACT_TEACH_CAPTURE=$EXTRACT_TEACH_CAPTURE CLBAND_TEACH_DELIVERY=$CLBAND_TEACH_DELIVERY ###"

# context_short  context_name                 transcript
run_one() {
  local short="$1" name="$2" transcript="$3"
  local scope="$RERUN/scopes/clband-$name"
  rm -rf "$scope"; mkdir -p "$scope/.git"; echo "ref: refs/heads/main" > "$scope/.git/HEAD"
  echo ""
  echo "===================== $name ($short) ====================="
  python3 "$CLBAND/clband_extract.py" "$name" "$transcript" "$scope" || { echo "EXTRACT FAILED for $name"; return 1; }
  echo "--- fidelity gate: $name ---"
  bash "$CLBAND/fidelity_gate.sh" "$short" "$scope"
  echo "gate_exit=$? ($name)"
}

rc=0
run_one 7833ca0b flywheel-assembly-agent "$SMOKE/transcripts/flywheel-assembly-agent.jsonl" || rc=1
run_one bc874bce aether-language        "$SMOKE/transcripts/aether-language.jsonl"        || rc=1
echo ""
echo "### re-run complete (rc=$rc). Drafts are .pending under $RERUN/scopes/*/.skills/ — owner gate (DP-2) pending. ###"
exit $rc
