#!/usr/bin/env bash
# Verifier: backend-selector-fail-loud
#
# Rule: the QdrantHybrid arm of the backend selector must NOT silently clone/delegate
# to the dense results — it must return an Err, call unreachable!(), or otherwise
# fail loud.
#
# Exit 0 == rule obeyed (QdrantHybrid arm has explicit error/unreachable)
# Exit 1 == rule violated (silent passthrough/clone remains)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/retrieval.rs"

if [ ! -f "$file" ]; then
  echo "FAIL: src/retrieval.rs not found in workspace"
  exit 1
fi

# Detect the silent passthrough pattern: QdrantHybrid arm followed by clone/Ok(dense)
# This covers: `QdrantHybrid => dense_results.clone()` or `QdrantHybrid => Ok(dense_results.clone())`
if grep -A3 'QdrantHybrid' "$file" | grep -qE '\.clone\(\)|Ok\(dense'; then
  echo "FAIL: QdrantHybrid arm silently delegates via clone/passthrough — must return Err or unreachable!"
  exit 1
fi

# Verify that the QdrantHybrid arm uses an explicit failure mode
if grep -A5 'QdrantHybrid' "$file" | grep -qE 'Err\(|unreachable!|todo!|panic!|unimplemented!'; then
  echo "PASS: QdrantHybrid arm uses explicit error/unreachable instead of silent delegation"
  exit 0
fi

# QdrantHybrid arm exists but has no recognizable failure mode
if grep -q 'QdrantHybrid' "$file"; then
  echo "FAIL: QdrantHybrid arm present but no explicit Err/unreachable/panic found — likely still silent"
  exit 1
fi

echo "FAIL: QdrantHybrid arm not found in src/retrieval.rs"
exit 1
