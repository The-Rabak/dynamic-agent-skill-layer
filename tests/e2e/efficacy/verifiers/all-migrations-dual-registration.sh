#!/usr/bin/env bash
# Verifier: all-migrations-dual-registration
#
# Rule: migration 012 must be wired into BOTH a const identifier AND the MIGRATIONS array
# in src/postgres.rs.
#
# Exit 0 == rule obeyed (both const and array entry present for 012)
# Exit 1 == rule violated (const missing, array entry missing, or both)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/postgres.rs"

if [ ! -f "$file" ]; then
  echo "FAIL: src/postgres.rs not found in workspace"
  exit 1
fi

# Check 1: a const for migration 012 exists (any form: M012, MIGRATION_012, etc.)
# Accept both: `const M012` and `include_str!` referencing 012
has_const=0
if grep -qE 'const\s+[A-Z0-9_]*012[A-Z0-9_]*\s*:' "$file"; then
  has_const=1
elif grep -qE 'include_str!.*012' "$file"; then
  has_const=1
fi

# Check 2: the MIGRATIONS array/slice entry for 012 exists
has_array_entry=0
if grep -qE '"012[_a-z_A-Z]*"' "$file"; then
  has_array_entry=1
fi

if [ "$has_const" -eq 0 ] && [ "$has_array_entry" -eq 0 ]; then
  echo "FAIL: migration 012 is wired into neither the const nor the MIGRATIONS array (both missing)"
  exit 1
elif [ "$has_const" -eq 0 ]; then
  echo "FAIL: migration 012 array entry present but const identifier missing (only half-wired)"
  exit 1
elif [ "$has_array_entry" -eq 0 ]; then
  echo "FAIL: migration 012 const present but MIGRATIONS array entry missing (only half-wired)"
  exit 1
else
  echo "PASS: migration 012 is registered in both the const and the MIGRATIONS array"
  exit 0
fi
