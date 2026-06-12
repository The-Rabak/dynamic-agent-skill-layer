"""Claude CLI JSON output parser — BUGGY: no fence stripping."""
import json


def parse_claude_cli_output(raw_stdout: str) -> dict:
    """
    Parse the JSON envelope produced by `claude --print --output-format json`.

    BUG: directly parses .result as JSON without stripping code fences.
    This raises SyntaxError when the model wraps the reply in backtick fences.
    """
    envelope = json.loads(raw_stdout)
    # BUG: no fence stripping — will fail on fenced output
    return json.loads(envelope["result"])
