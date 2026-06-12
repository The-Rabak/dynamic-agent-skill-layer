#!/usr/bin/env python3
"""T22 Unit B — teach-session document delivery (harness-side, NO extractor changes).

## Why this exists (Unit A evidence)

The orchestrated prose extractor's flat transcript only contains user + assistant MESSAGE
text: `SessionEvent::as_transcript_entry()` returns `None` for `ToolResult`/`ToolCall`/
`FileEdit` (crates/domain/src/types.rs). A teach session where the agent READS the knowledge
document (-> ToolResult) and WRITES its answer to solution.md (-> FileEdit) therefore delivers
almost none of the taught material to extraction. Unit A measured this: aether's prose-visible
text was 1,338 of 38,826 chars (4/8 operative sentinels invisible, lost in tool/file events).

## What it does

`materialize(context, raw_jsonl_text)` prepends the knowledge document into the captured
transcript as a leading USER turn — exactly how a real teaching session delivers a convention
document the user states/pastes into the chat. The verbatim rules then reach the prose
extractor through the user channel (which `as_transcript_entry` keeps).

## What it does NOT do (fences honored)

- It does NOT weaken or special-case the suspicious-speaker injection filter. The document is
  delivered as ordinary `role:"user"` content — the same trust level as every other user turn —
  and stays fenced inside the `<transcript>` block like all transcript content. The filter only
  drops *system*-impersonating speakers and jailbreak-prefixed content; a user turn carrying a
  task's rules is legitimate transcript data.
- It does NOT change the real extractor. This is the clband test harness only.

This is "replay/re-capture" delivery: it can transform an already-captured transcript (replay)
or be applied to a freshly captured one (re-capture) before ingest.
"""
from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CTX = ROOT / "contexts"

# Per-context knowledge document, matching setup_teach_workspaces.py:
#   flywheel (knowledge_home=system) -> system.md
#   aether   (knowledge_home=user)   -> context.md (spec + fused task)
_DOC_FILE = {
    "flywheel-assembly-agent": "system.md",
    "aether-language": "context.md",
}

_FRAMING = (
    "For this task you must follow the operational rules defined in the "
    "{name} knowledge document below. Apply these rules exactly — the invented "
    "names, codes, procedures, and constants are authoritative and must be used "
    "verbatim:\n\n{doc}"
)


def doc_path_for(context: str) -> Path | None:
    """Returns the knowledge-document path for a teach context, or None if unknown."""
    fname = _DOC_FILE.get(context)
    if not fname:
        return None
    p = CTX / context / fname
    return p if p.exists() else None


def materialize(context: str, raw_jsonl_text: str) -> str:
    """Prepends the context's knowledge document as a leading user turn.

    Returns the original text unchanged when the context has no known document (so the
    function is a safe no-op for non-teach contexts).
    """
    doc_path = doc_path_for(context)
    if doc_path is None:
        return raw_jsonl_text
    doc = doc_path.read_text(errors="replace")
    content = _FRAMING.format(name=context, doc=doc)
    user_turn = json.dumps(
        {"type": "user", "message": {"role": "user", "content": content}}
    )
    # Prepend as the first line; preserve the rest of the captured transcript verbatim.
    body = raw_jsonl_text.lstrip("\n")
    return f"{user_turn}\n{body}"


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 3:
        sys.exit("usage: teach_delivery.py <context> <raw_transcript.jsonl> [out.jsonl]")
    ctx, raw = sys.argv[1], Path(sys.argv[2])
    out = materialize(ctx, raw.read_text(errors="replace"))
    if len(sys.argv) >= 4:
        Path(sys.argv[3]).write_text(out)
        print(f"materialized {ctx}: {raw} -> {sys.argv[3]} (+{len(out) - raw.stat().st_size} bytes)")
    else:
        sys.stdout.write(out)
