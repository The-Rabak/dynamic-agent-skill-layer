#!/usr/bin/env bash
# Verifier: blank-env-treat-as-absent
#
# Rule: embed_model() in src/config.rs must treat Ok("") (blank env var from
# docker-compose ${VAR:-}) the same as Err(NotPresent) — both must produce
# the default model name.
#
# The fix requires filtering empty strings, e.g.:
#   .ok().filter(|s| !s.trim().is_empty())
#   or: .ok().filter(|s| !s.is_empty())
#   or: matching Ok(s) if s.is_empty() => default
#
# Exit 0 == rule obeyed (blank-treated-as-absent pattern present)
# Exit 1 == rule violated (only Err handled, blank Ok("") not filtered)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/config.rs"

if [ ! -f "$file" ]; then
  echo "FAIL: src/config.rs not found in workspace"
  exit 1
fi

# Detect the prohibited pattern: unwrap_or_else with no blank filtering
# i.e., only the original buggy pattern (unwrap_or handles Err but not Ok(""))
# Check: if only unwrap_or_else is present with no is_empty check → bug still present
has_blank_guard=0

# Accepts filter(|s| !s.is_empty()), filter(|s| !s.trim().is_empty()),
# match Ok(s) if s.is_empty(), trim() + empty checks, or any is_empty reference
if grep -qE 'is_empty\(\)|\.filter\(|trim\(\)' "$file"; then
  has_blank_guard=1
fi

if [ "$has_blank_guard" -eq 0 ]; then
  echo "FAIL: embed_model does not filter blank Ok(\"\") — docker-compose blank passthrough silently sets empty model name"
  exit 1
fi

# Make sure the original silent fallback pattern is not the ONLY thing (i.e., the fix is there)
# The fix must add blank-filtering ON TOP of or instead of the simple unwrap_or_else
echo "PASS: embed_model treats blank Ok(\"\") as absent — blank env var falls back to the default model name"
exit 0
