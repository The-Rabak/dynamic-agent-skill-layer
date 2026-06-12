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
INSTRUMENTS = ROOT / "instruments"

# Per-context knowledge document, matching setup_teach_workspaces.py:
#   flywheel (knowledge_home=system) -> system.md
#   aether   (knowledge_home=user)   -> context.md (spec + fused task)
# The two smoke contexts are pinned here; the 8 full-band contexts are data-driven from their
# instruments/<name>.json `doc_file` field (T23 Unit A authored these), so a new context needs no
# code change — only its committed instruments metadata. See `doc_path_for`.
_DOC_FILE = {
    "flywheel-assembly-agent": "system.md",
    "aether-language": "context.md",
}


def _doc_file_from_instruments(context: str) -> str | None:
    """Read the knowledge `doc_file` for a full-band context from its instruments metadata."""
    meta = INSTRUMENTS / f"{context}.json"
    if not meta.exists():
        return None
    try:
        data = json.loads(meta.read_text())
    except (json.JSONDecodeError, OSError):
        return None
    df = data.get("doc_file")
    return df if isinstance(df, str) and df else None

_FRAMING = (
    "For this task you must follow the operational rules defined in the "
    "{name} knowledge document below. Apply these rules exactly — the invented "
    "names, codes, procedures, and constants are authoritative and must be used "
    "verbatim:\n\n{doc}"
)


def doc_path_for(context: str) -> Path | None:
    """Returns the knowledge-document path for a teach context, or None if unknown.

    Resolution order: the pinned smoke map (flywheel/aether), then the full-band context's
    committed instruments/<name>.json `doc_file` (T23). A context with neither is a no-op.
    """
    fname = _DOC_FILE.get(context) or _doc_file_from_instruments(context)
    if not fname:
        return None
    p = CTX / context / fname
    return p if p.exists() else None


def doc_text_for(context: str) -> str | None:
    """The knowledge text delivered into extraction for a teach context, or None if unknown.

    Smoke contexts (flywheel/aether) deliver their single pinned doc (knowledge cleanly in one file).
    Full-band contexts (those with committed instruments) deliver the FULL COMMON context — the union
    of system.md + context.md — because some contexts split the invented rules across both files (e.g.
    dpms-agent-m: condensed system.md, specifics in context.md). Delivering the union is the faithful
    "here is the whole knowledge document" and a strict superset of doc_file, so the prose extractor
    always sees the operative rules regardless of which file holds them.
    """
    if context in _DOC_FILE:
        p = CTX / context / _DOC_FILE[context]
        return p.read_text(errors="replace") if p.exists() else None
    if (INSTRUMENTS / f"{context}.json").exists():
        parts = []
        for fn in ("system.md", "context.md"):
            p = CTX / context / fn
            if p.exists() and p.read_text(errors="replace").strip():
                parts.append(f"<!-- {fn} -->\n{p.read_text(errors='replace')}")
        return "\n\n".join(parts) if parts else None
    return None


def materialize(context: str, raw_jsonl_text: str) -> str:
    """Prepends the context's knowledge document as a leading user turn.

    Returns the original text unchanged when the context has no known document (so the
    function is a safe no-op for non-teach contexts).
    """
    doc = doc_text_for(context)
    if doc is None:
        return raw_jsonl_text
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
