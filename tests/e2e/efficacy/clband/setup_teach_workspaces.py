#!/usr/bin/env python3
"""Set up the Session A teach workspaces for the clband smoke (Unit 3).

For each smoke context, create a fresh workspace containing the knowledge document (merged per
knowledge_home) and write the teach prompt to prompt.txt. The teach prompt = the teach task's
question, framed to make the agent WORK the task using the document (not paste-and-quit). The
agent then reads the doc, reasons, and writes solution.md — the transcript captures the rules
being used, which the real extraction pipeline (Unit 4) turns into .pending skills.

  flywheel (knowledge_home=system): doc = system.md; teach task = sibling #1 (407f5929).
  aether   (knowledge_home=user):   doc = context.md (spec + fused task); teach task = depth-2
                                     teach_only (7d9233cf, question fused into the document).
"""
from __future__ import annotations
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CTX = ROOT / "contexts"
TEACH = ROOT / "teach"
TEACH.mkdir(exist_ok=True)


def load(ctx):
    return json.loads((CTX / ctx / "tasks.json").read_text())


def task_by_id(data, short):
    return next(t for t in data["tasks"] if t["task_id"].startswith(short))


def setup_flywheel():
    ws = TEACH / "flywheel-assembly-agent"
    ws.mkdir(parents=True, exist_ok=True)
    doc = (CTX / "flywheel-assembly-agent" / "system.md").read_text()
    (ws / "flywheel-protocols.md").write_text(doc)
    teach = task_by_id(load("flywheel-assembly-agent"), "407f5929")
    prompt = (
        "You are Agent B, the Assembly Technician Agent in the Flywheel Manufacturing Multi-Agent "
        "System. The file `flywheel-protocols.md` in this directory contains your system's operational "
        "protocols (role, persona, workflow, workaround protocols, and operational boundaries). Read "
        "it carefully, then complete the task below by APPLYING those protocols — explicitly follow "
        "the numbered assembly workflow and the workaround procedures where the situation calls for "
        "them.\n\n"
        + teach["question"]
        + "\n\nWork the task using flywheel-protocols.md, and write your complete response to "
          "`solution.md` in this directory."
    )
    (ws / "prompt.txt").write_text(prompt)
    print(f"flywheel: ws={ws}  doc=flywheel-protocols.md ({len(doc)} chars)  prompt={len(prompt)} chars")


def setup_aether():
    ws = TEACH / "aether-language"
    ws.mkdir(parents=True, exist_ok=True)
    doc = (CTX / "aether-language" / "context.md").read_text()
    (ws / "aether-spec.md").write_text(doc)
    # depth-2 teach_only: the question is fused into the document (context.md). The document itself
    # ends with the concrete task (translate a Python sum function into Aether).
    prompt = (
        "The file `aether-spec.md` in this directory specifies the Aether programming language and, "
        "at the end, gives you a concrete task. Read the Aether specification carefully, then complete "
        "the task it describes by APPLYING the Aether language's rules (syntax, keywords such as "
        "conduit/flow/fork/swirl, the response-format standards, and error handling). Write your "
        "complete response — following the Aether response-format sections — to `solution.md` in this "
        "directory."
    )
    (ws / "prompt.txt").write_text(prompt)
    print(f"aether: ws={ws}  doc=aether-spec.md ({len(doc)} chars)  prompt={len(prompt)} chars")


if __name__ == "__main__":
    setup_flywheel()
    setup_aether()
