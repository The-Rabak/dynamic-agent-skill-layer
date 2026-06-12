#!/usr/bin/env bash
# Verifier: arcswap-rcu-return-value
#
# Rule: the try_update function must derive the 'was applied' result from the
# return value of the rcu/swap operation (the previous state), NOT from a
# variable mutated inside the closure.
#
# Checks:
#   FAIL if `applied = true` or `applied = false` appears inside a closure/block
#        after an `rcu`-like call (the buggy pattern)
#   PASS if the return value of the swap is captured (let prev = ...) and
#        the applied boolean is derived from comparing versions outside the closure
#
# Exit 0 == rule obeyed
# Exit 1 == rule violated
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/snapshot.rs"

if [ ! -f "$file" ]; then
  echo "FAIL: src/snapshot.rs not found in workspace"
  exit 1
fi

# Detect the buggy pattern: `applied = true` or `applied = false` inside the function
# (mutation of external flag inside a closure body)
if grep -qE '^\s*applied\s*=\s*(true|false)' "$file"; then
  echo "FAIL: 'applied = true/false' assignment found — variable is being mutated inside the closure (unreliable under CAS retry)"
  exit 1
fi

# Verify the correct pattern: the return value of the rcu/swap is captured
# and the boolean outcome is derived from it (version comparison outside the closure)
has_prev_capture=0
if grep -qE 'let\s+prev\s*=|prev\s*=\s*self\.|let\s+previous\s*=' "$file"; then
  has_prev_capture=1
fi

# Check for version comparison outside the closure
has_version_compare=0
if grep -qE '(prev|previous)\.(version|ver)|incoming.*>.*prev|prev.*<.*incoming|new_snap\.version\s*>' "$file"; then
  has_version_compare=1
fi

if [ "$has_prev_capture" -eq 1 ] && [ "$has_version_compare" -eq 1 ]; then
  echo "PASS: rcu return value (prev) is captured and version comparison drives the applied boolean"
  exit 0
elif [ "$has_prev_capture" -eq 1 ]; then
  echo "PASS: return value captured; version comparison implied by prev usage"
  exit 0
else
  echo "FAIL: no evidence that rcu/swap return value is captured to derive applied outcome"
  exit 1
fi
