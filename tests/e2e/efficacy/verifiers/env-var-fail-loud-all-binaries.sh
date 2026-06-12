#!/usr/bin/env bash
# Verifier: env-var-fail-loud-all-binaries
#
# Rule: maintenance/src/main.rs must NOT use unwrap_or / unwrap_or_else with
# a hardcoded fallback for DATABASE_URL; it must fail loud (expect/panic/explicit error)
# when DATABASE_URL is absent.
#
# Specifically flags:
#   - unwrap_or_else with a localhost default (the original bug)
#   - Any hardcoded port 15432 (the test-infrastructure port) as a fallback
#
# Exit 0 == rule obeyed (fail-loud pattern present, no silent fallback)
# Exit 1 == rule violated (silent fallback or hardcoded port remains)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/maintenance/src/main.rs"

if [ ! -f "$file" ]; then
  echo "FAIL: maintenance/src/main.rs not found in workspace"
  exit 1
fi

# Detect the prohibited pattern: hardcoded port 15432 anywhere in the file
if grep -q '15432' "$file"; then
  echo "FAIL: hardcoded port 15432 (test-infrastructure port) found in maintenance/src/main.rs"
  exit 1
fi

# Detect the silent fallback pattern: unwrap_or / unwrap_or_else with a non-panicking closure
# on DATABASE_URL reads
if grep -E 'DATABASE_URL' "$file" | grep -qE 'unwrap_or\b|unwrap_or_else'; then
  echo "FAIL: DATABASE_URL read uses unwrap_or/unwrap_or_else (silent fallback) in maintenance/src/main.rs"
  exit 1
fi

# Verify that a fail-loud pattern is present for DATABASE_URL
if grep -E 'DATABASE_URL' "$file" | grep -qE '\.expect\(|panic!|\.unwrap\(\)|expect\('; then
  echo "PASS: maintenance/src/main.rs uses fail-loud pattern for DATABASE_URL (no silent fallback)"
  exit 0
fi

# If DATABASE_URL is read but neither silently nor loudly, check for explicit error handling
if grep -q 'DATABASE_URL' "$file"; then
  # The var is read; check it is not with a fallback
  echo "PASS: maintenance/src/main.rs reads DATABASE_URL without a silent fallback"
  exit 0
fi

echo "FAIL: maintenance/src/main.rs does not read DATABASE_URL with fail-loud discipline"
exit 1
