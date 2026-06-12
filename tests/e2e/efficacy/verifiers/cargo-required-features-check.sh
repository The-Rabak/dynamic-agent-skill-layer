#!/usr/bin/env bash
# Verifier: cargo-required-features-check
#
# Rule: the ci.sh script must pass --features test-utils (or equivalent) to
# cargo test when running the mcp-server integration test that requires it.
#
# Exit 0 == rule obeyed (--features flag present in the relevant cargo test call)
# Exit 1 == rule violated (cargo test called without --features for the required-features target)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/ci.sh"

if [ ! -f "$file" ]; then
  echo "FAIL: ci.sh not found in workspace"
  exit 1
fi

# Check that at least one cargo test invocation for test_live_data_plane_roundtrip
# includes --features (accepting any feature flag, not just test-utils, for flexibility)
if grep -E 'cargo test.*test_live_data_plane_roundtrip' "$file" | grep -q '\-\-features'; then
  echo "PASS: cargo test for test_live_data_plane_roundtrip includes --features flag"
  exit 0
fi

# Also accept the pattern where features come before the test target name
if grep -E 'cargo test.*--features.*test_live_data_plane_roundtrip' "$file" > /dev/null 2>&1; then
  echo "PASS: cargo test for test_live_data_plane_roundtrip includes --features flag"
  exit 0
fi

# Check if any line has both cargo test and --features test-utils (in any order) for mcp-server
if grep -E 'cargo test' "$file" | grep -E 'test_live_data_plane_roundtrip|mcp-server' | grep -q 'features'; then
  echo "PASS: cargo test invocation includes features flag covering the required-features target"
  exit 0
fi

echo "FAIL: cargo test for test_live_data_plane_roundtrip does not include --features; tests silently skip"
exit 1
