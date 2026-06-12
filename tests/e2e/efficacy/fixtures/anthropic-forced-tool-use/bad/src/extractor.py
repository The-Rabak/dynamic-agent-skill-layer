"""Skill extractor — BUGGY: parses text content block as JSON."""
import json


def extract_skills(client, transcript: str) -> list:
    """
    Extract skill candidates from a transcript.

    BUG: relies on parsing the text content block as JSON — fails when the
    model wraps the reply in markdown or adds explanation.
    """
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=4096,
        system="Extract skills as a JSON array. Return only the JSON array.",
        messages=[{"role": "user", "content": transcript}],
    )
    return json.loads(response.content[0].text)
