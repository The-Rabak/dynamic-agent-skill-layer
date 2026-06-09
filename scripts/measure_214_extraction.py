#!/usr/bin/env python3
"""#214 local-vs-cloud extraction measurement on the REAL maintenance-worker.

Drives the REAL extraction pipeline end-to-end: ingest real transcripts into the
real PG queue (via the real /ingest/transcript endpoint), then run the REAL
`maintenance-worker` binary (the proven corpus drain recipe) with
EXTRACT_SESSION_PROVIDER set, which drains the queue through the real provider
seam (ollama gemma4:12b OR claude-code) and the real PendingDraftWriter into an
isolated sandbox. Then count + inspect the produced .pending drafts. NO fakes,
NO reconstruction.

Usage: measure_214_extraction.py <provider:ollama|claude-code> <N> [--neutralize]
Writes tests/e2e/reports/214/<provider>__drafts.json
"""
import json
import os
import subprocess
import sys
import time
import tempfile
import urllib.request
from pathlib import Path

MCP = "http://127.0.0.1:3001"
PG = ["docker", "exec", "dynamic-agent-skill-layer-postgres", "psql", "-U", "skill_layer",
      "-d", "skill_layer_test", "-t", "-A"]
WORKER = "target/debug/maintenance-worker"
TRANSCRIPT_DIR = Path(os.path.expanduser("~/.claude/projects/-tmp"))


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def neutralize_queue():
    """Mark all existing (spent) queue rows processed so the worker only drains our batch."""
    psql("UPDATE transcript_ingest_queue SET status='processed' WHERE status <> 'processed';")
    pending = psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'")
    print(f"[214] neutralized old queue rows; pending now: {pending}")


def pick_transcripts(n):
    files = sorted([p for p in TRANSCRIPT_DIR.glob("*.jsonl")
                    if 40_000 <= p.stat().st_size <= 300_000])
    return files[:n]


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def run_worker(provider, sandbox):
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        "EXTRACT_SESSION_PROVIDER": provider,
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "CLAUDE_TRANSCRIPT_ROOT": str(sandbox),
        "SKILL_GLOBAL_PATHS": str(sandbox / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{sandbox}/project,{sandbox}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(sandbox / "project"),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(sandbox / "global"),
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    log = sandbox / "worker.log"
    print(f"[214] running maintenance-worker (provider={provider}); log: {log}")
    with open(log, "w") as lf:
        # No timeout: extraction churner drains to completion (no arbitrary caps).
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
    print(f"[214] worker exited rc={proc.returncode}")
    return proc.returncode


def parse_draft(path: Path):
    text = path.read_text(errors="replace")
    fm = {}
    body = text
    if text.startswith("---\n") and "\n---\n" in text[4:]:
        raw_fm, body = text[4:].split("\n---\n", 1)
        for line in raw_fm.splitlines():
            if ":" in line and not line.startswith(" ") and not line.startswith("-"):
                k, _, v = line.partition(":")
                fm[k.strip()] = v.strip()
    # Non-empty procedure = a "- " bullet under a "## Procedures" section.
    proc_bullets = 0
    in_proc = False
    for line in body.splitlines():
        s = line.strip()
        if s.lower().startswith("## "):
            in_proc = "procedure" in s.lower()
        elif in_proc and s.startswith("- ") and len(s) > 4:
            proc_bullets += 1
    return dict(name=fm.get("name", path.parent.name), description=fm.get("description", ""),
                source_session=fm.get("source_session_id", ""), proc_bullets=proc_bullets,
                path=str(path))


def main():
    provider = sys.argv[1]
    n = int(sys.argv[2])
    if "--neutralize" in sys.argv:
        neutralize_queue()
    sandbox = Path(tempfile.mkdtemp(prefix=f"eval214-{provider}-"))
    (sandbox / "global").mkdir()
    (sandbox / "project").mkdir()

    transcripts = pick_transcripts(n)
    print(f"[214] {provider}: ingesting {len(transcripts)} transcripts → {sandbox}")
    for i, f in enumerate(transcripts):
        sid = f"eval214-{provider}-{i}-{f.stem[:8]}"
        out = ingest(sid, f.read_text(errors="replace"), str(sandbox / "project"))
        print(f"   [{i}] {f.name} ({f.stat().st_size//1024}KB) → {out.get('status','?')}")

    t0 = time.time()
    run_worker(provider, sandbox)
    elapsed = time.time() - t0

    drafts = []
    for pend in sandbox.rglob("*.pending"):
        drafts.append(parse_draft(pend))
    yield_per = len(drafts) / max(len(transcripts), 1)
    non_empty = sum(1 for d in drafts if d["proc_bullets"] > 0)
    non_empty_rate = non_empty / max(len(drafts), 1)

    result = dict(provider=provider, transcripts=len(transcripts), drafts=len(drafts),
                  yield_per_transcript=yield_per, non_empty_procedure_rate=non_empty_rate,
                  elapsed_s=round(elapsed, 1), sandbox=str(sandbox), draft_detail=drafts)
    outp = Path(f"tests/e2e/reports/214/{provider}__drafts.json")
    outp.parent.mkdir(parents=True, exist_ok=True)
    outp.write_text(json.dumps(result, indent=1))

    print(f"\n=== #214 {provider} ===")
    print(f"transcripts={len(transcripts)} drafts={len(drafts)} "
          f"yield/transcript={yield_per:.2f} non_empty_proc_rate={non_empty_rate:.2f} "
          f"elapsed={elapsed:.0f}s")
    print(f"report: {outp}  sandbox: {sandbox}")


if __name__ == "__main__":
    main()
