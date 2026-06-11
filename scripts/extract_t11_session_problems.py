#!/usr/bin/env python3
"""Extract genuine developer problem statements from the 24 source transcripts
and the multi-view corpus inventory for the T11 anti-circularity fixture.

WHY (T11 anti-circularity rule, owner decision 2026-06-11): headline held-out
queries must be drawn from material the skill text was NOT generated from — the
*problem statements* the developer actually typed in the source sessions — not
from the skills' own `use_when`/description (that measures self-recall, not
retrieval). This script pulls the real user-typed problem statements out of each
transcript (dropping slash-command templates, injected context, tool results,
and harness boilerplate) so the fixture builder can phrase queries from THEM and
only LABEL gold from the corpus.

Outputs (under tests/e2e/reports/t11/):
  - session_problems.json : {session_key: {transcript, problems: [text, ...]}}
  - corpus_inventory.json : [{name, type, description, use_when, tools,
                              artifacts, invariants, produces, source_session_id,
                              session_key}]

No LLM, no network — pure deterministic parsing. Fail loud if a manifest
transcript is missing.
"""
import json
import os
import re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SKILLS_ROOT = REPO / "tests/e2e/reports/replica-run/skills"
MANIFEST = REPO / "tests/e2e/reports/replica-run/genuine_manifest.txt"
OUT_DIR = REPO / "tests/e2e/reports/t11"

# Markers that identify a user turn as harness boilerplate / injected context
# rather than a genuine developer problem statement.
_BOILERPLATE_MARKERS = (
    "<command-name>", "<command-message>", "<command-args>",
    "<local-command-stdout>", "<local-command-caveat>",
    "# Work Plan Execution Command", "Execute a work plan",
    "Caveat: The messages below were generated",
    "This command takes a work document",
    "<system-reminder>", "DO NOT respond to these messages",
    "## Introduction", "## Execution Workflow", "## Input Document",
)
# A turn that is mostly one of these tags is injected context, not a problem.
_INJECTED_PREFIXES = ("<", "[Request interrupted", "[Tool ")


def _frontmatter(text: str) -> dict:
    """Parse a minimal YAML frontmatter block (scalar + simple list fields)."""
    if not text.startswith("---"):
        return {}
    end = text.find("\n---", 3)
    if end < 0:
        return {}
    block = text[3:end]
    out, key = {}, None
    for line in block.splitlines():
        if not line.strip():
            continue
        m = re.match(r"^([a-zA-Z_][\w]*):\s*(.*)$", line)
        if m and not line.startswith((" ", "-", "\t")):
            key = m.group(1)
            val = m.group(2).strip()
            if val and val not in ("|", ">"):
                out[key] = val.strip("'\"")
            else:
                out[key] = []
        elif key is not None and re.match(r"^\s*-\s+", line):
            if not isinstance(out.get(key), list):
                out[key] = []
            out[key].append(re.sub(r"^\s*-\s+", "", line).strip().strip("'\""))
    return out


def load_corpus_inventory() -> list[dict]:
    inv = []
    for dirpath, _, files in os.walk(SKILLS_ROOT):
        if "SKILL.md" not in files:
            continue
        text = (Path(dirpath) / "SKILL.md").read_text(encoding="utf-8", errors="replace")
        fm = _frontmatter(text)
        ssid = fm.get("source_session_id")
        skey = None
        if isinstance(ssid, str):
            m = re.match(r"(replica-\d+-[0-9a-f]+)", ssid)
            skey = m.group(1) if m else ssid
        inv.append({
            "name": fm.get("name"),
            "type": fm.get("type"),
            "description": fm.get("description", ""),
            "use_when": fm.get("use_when", []),
            "tools": fm.get("tools", []),
            "artifacts": fm.get("artifacts", []),
            "invariants": fm.get("invariants", []),
            "produces": fm.get("produces", []),
            "source_session_id": ssid,
            "session_key": skey,
        })
    return inv


def _user_text(rec: dict) -> str | None:
    if rec.get("type") != "user":
        return None
    msg = rec.get("message")
    if not isinstance(msg, dict) or msg.get("role") != "user":
        return None
    content = msg.get("content")
    if isinstance(content, list):
        parts = []
        for blk in content:
            if isinstance(blk, dict) and blk.get("type") == "text":
                parts.append(blk.get("text", ""))
            elif isinstance(blk, dict) and blk.get("type") == "tool_result":
                return None  # tool result echo, not a problem statement
        text = "\n".join(parts).strip()
    elif isinstance(content, str):
        text = content.strip()
    else:
        return None
    return text or None


def _is_genuine(text: str) -> bool:
    if not text or len(text) < 15:
        return False
    if any(text.lstrip().startswith(p) for p in _INJECTED_PREFIXES):
        return False
    if any(m in text for m in _BOILERPLATE_MARKERS):
        return False
    # Drop turns that are almost entirely a pasted file/diff/log (low prose ratio).
    if text.count("\n") > 40 and len(text) > 4000:
        return False
    return True


def extract_problems(transcript: Path, max_keep: int = 60) -> list[str]:
    """Drain ALL genuine user-typed turns (deduped) up to max_keep.

    Earlier versions broke after the first 25 turns, which missed problem
    statements in long agentic sessions where the genuine asks come late and are
    interleaved with slash-command turns. We now scan the whole transcript and
    keep every distinct genuine prose ask.
    """
    problems, seen = [], set()
    with transcript.open(encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            text = _user_text(rec)
            if text and _is_genuine(text):
                clean = re.sub(r"\s+", " ", text).strip()[:1500]
                key = clean[:120].lower()
                if key in seen:
                    continue
                seen.add(key)
                problems.append(clean)
            if len(problems) >= max_keep:
                break
    return problems


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    manifest = [l.strip() for l in MANIFEST.read_text().splitlines() if l.strip()]
    if len(manifest) != 24:
        raise SystemExit(f"expected 24 manifest transcripts, got {len(manifest)}")

    inv = load_corpus_inventory()
    by_session: dict[str, list[str]] = {}
    for s in inv:
        by_session.setdefault(s["session_key"], []).append(s["name"])

    # Map manifest uuid -> session_key (replica-NNNN-<suffix>) via the suffix.
    suffix_to_key = {}
    for key in by_session:
        m = re.match(r"replica-\d+-([0-9a-f]+)", key or "")
        if m:
            suffix_to_key[m.group(1)] = key

    sessions = {}
    missing = []
    for path in manifest:
        p = Path(path)
        if not p.exists():
            missing.append(path)
            continue
        suffix = p.stem.split("-")[0]
        key = suffix_to_key.get(suffix)
        if key is None:
            # transcript present but no skills mapped to it (shouldn't happen for the 24)
            key = f"unmapped-{suffix}"
        problems = extract_problems(p)
        sessions[key] = {
            "transcript": p.name,
            "uuid_suffix": suffix,
            "skills_in_session": by_session.get(key, []),
            "problems": problems,
        }
    if missing:
        raise SystemExit(f"FATAL: manifest transcripts missing on disk: {missing}")

    (OUT_DIR / "session_problems.json").write_text(json.dumps(sessions, indent=1))
    (OUT_DIR / "corpus_inventory.json").write_text(json.dumps(inv, indent=1))

    total_problems = sum(len(s["problems"]) for s in sessions.values())
    print(f"sessions: {len(sessions)}  skills: {len(inv)}  "
          f"problem-statements captured: {total_problems}")
    print(f"  -> {OUT_DIR/'session_problems.json'}")
    print(f"  -> {OUT_DIR/'corpus_inventory.json'}")
    # Sanity: show per-session problem counts.
    for key in sorted(sessions):
        s = sessions[key]
        print(f"  {key}: {len(s['problems'])} problems, "
              f"{len(s['skills_in_session'])} gold skills")


if __name__ == "__main__":
    main()
