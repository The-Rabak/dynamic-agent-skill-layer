#!/usr/bin/env bash
# Verifier: cold-start-guard-retirement
#
# Rule: the retirement candidate query in src/retirement_worker.py must exclude
# items with zero total_usage_count (the cold-start guard).
#
# The query must have a condition like:
#   WHERE total_usage_count > 0 AND total_usage_count < threshold
# or equivalent. We check the actual SQL string / code logic for the > 0 guard,
# ignoring comments.
#
# Exit 0 == rule obeyed (cold-start guard present: > 0 condition on usage count)
# Exit 1 == rule violated (no > 0 guard; never-used items can be retired)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/retirement_worker.py"

if [ ! -f "$file" ]; then
  echo "FAIL: src/retirement_worker.py not found in workspace"
  exit 1
fi

# Use Python to parse the file, stripping comments, and check for the guard.
# This avoids false positives from docstrings/comments mentioning 0.
python3 - "$file" << 'PYEOF'
import sys, re

filepath = sys.argv[1]
with open(filepath) as f:
    content = f.read()

# Remove Python comments and docstrings for analysis
# Strip single-line comments
no_comments = re.sub(r'#[^\n]*', '', content)
# Strip triple-quoted strings (docstrings)
no_docstrings = re.sub(r'""".*?"""', '', no_comments, flags=re.DOTALL)
no_docstrings = re.sub(r"'''.*?'''", '', no_docstrings, flags=re.DOTALL)

code_only = no_docstrings

# Check for cold-start guard in code (not comments/docstrings):
# SQL-level guard: total_usage_count > 0 or >= 1 in a string literal
sql_guard = bool(re.search(r'total_usage_count\s*>\s*0|total_usage_count\s*>=\s*1|BETWEEN\s+1', code_only))

# Python-level guard: conditional check on usage count before/after the query
py_guard = bool(re.search(r'usage_count\s*>\s*0|count\s*>\s*0\b', code_only))

if not sql_guard and not py_guard:
    print("FAIL: no cold-start guard found — items with total_usage_count=0 can be retired (mass-deletion on first boot)")
    sys.exit(1)

print("PASS: cold-start guard present — never-used items (total_usage_count=0) excluded from retirement candidates")
sys.exit(0)
PYEOF
