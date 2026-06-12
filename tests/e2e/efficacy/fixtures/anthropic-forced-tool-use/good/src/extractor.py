"""Skill extractor using forced tool_use for reliable structured output."""
import json


EXTRACT_TOOL = {
    "name": "extract_skills",
    "description": "Extract skill candidates from the transcript.",
    "input_schema": {
        "type": "object",
        "properties": {
            "skills": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                    },
                    "required": ["name", "description"],
                },
            }
        },
        "required": ["skills"],
    },
}


def extract_skills(client, transcript: str) -> list:
    """
    Extract skill candidates from a transcript using forced tool_use.

    Uses tool_choice to force the model to return structured output via the
    tool_use content block, not via free-text that requires brittle parsing.
    """
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=4096,
        tools=[EXTRACT_TOOL],
        tool_choice={"type": "tool", "name": "extract_skills"},
        messages=[{"role": "user", "content": transcript}],
    )
    # Extract from tool_use content block, not text block
    for block in response.content:
        if block.type == "tool_use":
            return block.input["skills"]
    return []
