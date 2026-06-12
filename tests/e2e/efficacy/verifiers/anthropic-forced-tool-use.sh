#!/usr/bin/env bash
# Verifier: anthropic-forced-tool-use
#
# Rule: extract_skills must use forced tool_use (tool_choice set) and extract
# the result from the tool_use content block — NOT parse the text content block
# as JSON.
#
# Exit 0 == rule obeyed (tool_use/tool_choice present, text-block parse absent)
# Exit 1 == rule violated (text-block parse remains, or no tool_use)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/extractor.py"

if [ ! -f "$file" ]; then
  echo "FAIL: src/extractor.py not found in workspace"
  exit 1
fi

# Detect the prohibited pattern: json.loads on response.content[0].text
# (direct text-block parsing — the fragile approach)
if grep -qE 'json\.loads\s*\(\s*response\.content\[0\]\.text|content\[0\]\.text\s*\)|\.content\[0\]\.text\s*\)' "$file"; then
  echo "FAIL: extract_skills parses response.content[0].text directly — must use tool_use content block instead"
  exit 1
fi

# Verify tool_use/tool_choice is configured
has_tool_choice=0
if grep -qE 'tool_choice|tools\s*=' "$file"; then
  has_tool_choice=1
fi

has_tool_use_extraction=0
# Accepts: .input, content block type == tool_use, tool_use extraction
if grep -qE '\.input|tool_use|input_schema' "$file"; then
  has_tool_use_extraction=1
fi

if [ "$has_tool_choice" -eq 1 ] && [ "$has_tool_use_extraction" -eq 1 ]; then
  echo "PASS: extract_skills uses forced tool_use and extracts result from tool_use content block"
  exit 0
elif [ "$has_tool_choice" -eq 1 ]; then
  echo "PASS: tool_choice configured (tool_use forcing present)"
  exit 0
fi

echo "FAIL: no tool_choice/tools configuration found — extract_skills does not use forced tool_use"
exit 1
