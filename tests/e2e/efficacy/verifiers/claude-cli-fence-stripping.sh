#!/usr/bin/env bash
# Verifier: claude-cli-fence-stripping
#
# Rule: the parse_claude_cli_output function in src/claude_parser.py must strip
# triple-backtick fences from the .result string before calling json.loads().
#
# Exit 0 == rule obeyed (fence stripping present)
# Exit 1 == rule violated (no fence stripping)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/claude_parser.py"

if [ ! -f "$file" ]; then
  echo "FAIL: src/claude_parser.py not found — function was not created"
  exit 1
fi

# Check that the file defines parse_claude_cli_output (not just the buggy stub)
if ! grep -q 'def parse_claude_cli_output' "$file"; then
  echo "FAIL: parse_claude_cli_output function not found in src/claude_parser.py"
  exit 1
fi

# Detect fence stripping: look for patterns that handle markdown code fences.
# Use python to check the file content (avoids backtick/quoting issues in shell).
python3 - "$file" << 'PYEOF'
import sys

filepath = sys.argv[1]
with open(filepath) as f:
    content = f.read()

# Indicators that fence stripping is implemented:
# 1. re.sub with a pattern containing backtick-related content
# 2. .strip() or .lstrip() or .rstrip() used with fence logic nearby
# 3. .replace() call targeting markdown fences
# 4. startswith check for fence characters
# 5. splitlines with index slicing (trim first/last line if fence)
fence_indicators = [
    're.sub',
    'strip',
    'replace',
    'startswith',
    'lstrip',
    'rstrip',
    'splitlines',
    'partition',
    'fence',
    'backtick',
]

has_any_fence_logic = any(ind in content for ind in fence_indicators)

if not has_any_fence_logic:
    print("FAIL: no fence-stripping logic found — direct json.loads on .result will fail on fenced output")
    sys.exit(1)

# Also verify there is no DIRECT json.loads(envelope["result"]) without any stripping
# The bad pattern: json.loads called on the raw result in a single expression
lines = content.split('\n')
for i, line in enumerate(lines):
    stripped = line.strip()
    # Detect: return json.loads(envelope["result"]) or json.loads(result_text) with no prior strip
    if 'json.loads' in stripped and ('["result"]' in stripped or "['result']" in stripped):
        # This is the direct parse — check if the line contains fence handling
        if 'strip' not in stripped and 'sub' not in stripped:
            print("FAIL: json.loads called directly on raw .result without fence stripping")
            sys.exit(1)

print("PASS: parse_claude_cli_output includes fence-stripping logic before json.loads")
sys.exit(0)
PYEOF
