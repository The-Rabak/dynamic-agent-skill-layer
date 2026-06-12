"""Claude CLI JSON output parser with fence stripping."""
import json
import re


def parse_claude_cli_output(raw_stdout: str) -> dict:
    """
    Parse the JSON envelope produced by `claude --print --output-format json`.

    The outer envelope has shape:
      {type: "result", subtype: "success", result: "...", ...}

    The .result string is wrapped in triple-backtick json fences by the CLI.
    These fences must be stripped before parsing the inner payload.
    """
    envelope = json.loads(raw_stdout)
    result_text = envelope["result"]
    # Strip triple-backtick fences (```json ... ``` or ``` ... ```)
    stripped = re.sub(r'^```(?:json)?\s*\n?', '', result_text.strip())
    stripped = re.sub(r'\n?```\s*$', '', stripped)
    return json.loads(stripped)
