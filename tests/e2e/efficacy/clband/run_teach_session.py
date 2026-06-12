#!/usr/bin/env python3
"""Run ONE Session A teach session for the clband smoke (Unit 3), then locate its transcript.

Drives a genuine claude-code working session via the validated harness primitive
scripts/efficacy_ab.run_claude_solve (cwd = the teach workspace, prompt on stdin). The agent
reads the knowledge document already present in the workspace, works the teach task, and writes
solution.md. After the solve, finds the claude-code session transcript jsonl that this run
produced (newest jsonl under the munged project dir for the workspace cwd) so Unit 4 can ingest it.

Usage: run_teach_session.py <workspace_dir>
"""
from __future__ import annotations
import sys
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[4] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import efficacy_ab  # noqa: E402


def munged_project_dir(ws: Path) -> Path:
    # claude-code encodes the cwd path into a project dir name: every '/' (and '.') -> '-'.
    enc = str(ws.resolve()).replace("/", "-").replace(".", "-")
    return Path.home() / ".claude" / "projects" / enc


def main():
    ws = Path(sys.argv[1]).resolve()
    prompt = (ws / "prompt.txt").read_text()
    proj = munged_project_dir(ws)
    before = set(proj.glob("*.jsonl")) if proj.exists() else set()
    t0 = time.time()
    print(f"[teach] ws={ws}")
    print(f"[teach] running claude solve (sonnet, cwd=ws, prompt={len(prompt)} chars)...", flush=True)
    res = efficacy_ab.run_claude_solve(prompt=prompt, workspace_dir=ws, model="sonnet",
                                       max_turns=60, timeout_s=900)
    dt = time.time() - t0
    print(f"[teach] solve rc={res['exit_code']} timed_out={res['timed_out']} elapsed={dt:.0f}s")
    sol = ws / "solution.md"
    print(f"[teach] solution.md: {'WRITTEN ' + str(sol.stat().st_size) + ' bytes' if sol.exists() else 'MISSING'}")
    after = set(proj.glob("*.jsonl")) if proj.exists() else set()
    new = sorted(after - before, key=lambda p: p.stat().st_mtime)
    if not new and proj.exists():
        # No brand-new file (rare): take the most-recently-modified jsonl in the project dir.
        new = sorted(proj.glob("*.jsonl"), key=lambda p: p.stat().st_mtime)[-1:]
    if new:
        tr = new[-1]
        print(f"[teach] transcript: {tr} ({tr.stat().st_size} bytes)")
        print(f"TRANSCRIPT={tr}")
    else:
        print(f"[teach] WARNING: no transcript jsonl found under {proj}")
        print("TRANSCRIPT=")


if __name__ == "__main__":
    main()
