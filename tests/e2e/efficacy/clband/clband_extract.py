#!/usr/bin/env python3
"""clband Session A -> .pending extraction driver (Unit 4) — REAL pipeline, no fakes.

Adapts scripts/replica_extract.py for the clband smoke: drives ONE teach-session transcript through
the real extraction pipeline into an ISOLATED per-context scope dir, producing .pending drafts for
the human gate (DP-1). Never auto-approves.

Flow (per context):
  1. Neutralize the shared transcript_ingest_queue (DELETE — scratch; the 262 corpus is already
     accepted and lives outside the queue, so this does not touch it).
  2. Ingest the teach transcript via the REAL mcp-server /ingest/transcript with repo_path = the
     context's isolated host scope dir (a .git marker there makes the scope resolver isolate it).
  3. Run the REAL host maintenance-worker (RUN_ONCE + TRANSCRIPT_DRAIN, EXTRACT_SESSION_PROVIDER=
     claude-code) with PROJECT_ROOT = the scope dir, draining the queue into <scope>/.skills/.
  4. Report the produced .pending drafts (paths + frontmatter). NO approval here — that is DP-1.

NO timeouts on the worker (drain-until-done). Usage: clband_extract.py <context> <transcript> <scope_dir>
"""
from __future__ import annotations
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[4]
WORKER = str(ROOT / "target/debug/maintenance-worker")
MCP = "http://127.0.0.1:3001"
PG = ["docker", "exec", "dynamic-agent-skill-layer-postgres-1", "psql", "-U", "skill_layer",
      "-d", "skill_layer_test", "-t", "-A"]
import urllib.request


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def neutralize_queue():
    psql("DELETE FROM transcript_ingest_queue;")
    print(f"[clband] purged queue; rows now: {psql('SELECT count(*) FROM transcript_ingest_queue')}")


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def run_worker(scope_dir: Path, logpath: Path):
    glob_dir = scope_dir.parent / "_global"
    glob_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        # Isolated per-context scope root: drafts land under <scope_dir>/.skills/.
        "SKILL_PROJECT_ROOT": str(scope_dir),
        "GRAPH_BUILDER_PROJECT_ROOT": str(scope_dir),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(glob_dir),
        "SKILL_GLOBAL_PATHS": str(glob_dir),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{scope_dir},{glob_dir}",
        "CLAUDE_TRANSCRIPT_ROOT": str(scope_dir),
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    print(f"[clband] running maintenance-worker (claude-code); scope={scope_dir}; log={logpath}")
    with open(logpath, "w") as lf:
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
    print(f"[clband] worker exited rc={proc.returncode}")
    return proc.returncode


def parse_draft(path: Path):
    text = path.read_text(errors="replace")
    fm = {}
    if text.startswith("---\n") and "\n---\n" in text[4:]:
        raw_fm, _ = text[4:].split("\n---\n", 1)
        try:
            parsed = yaml.safe_load(raw_fm)
            if isinstance(parsed, dict):
                fm = parsed
        except yaml.YAMLError:
            pass
    return dict(name=fm.get("name", path.parent.name), description=fm.get("description", ""),
                skill_type=fm.get("type", ""), path=str(path))


def main():
    ctx, transcript, scope = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]).resolve()
    logdir = scope.parent.parent / "logs"
    logdir.mkdir(parents=True, exist_ok=True)
    print(f"=== clband extract: {ctx} -> {scope} ===")
    neutralize_queue()
    sid = f"clband-teach-{ctx}"
    out = ingest(sid, transcript.read_text(errors="replace"), str(scope))
    print(f"[clband] ingest {transcript.name} ({transcript.stat().st_size//1024}KB) -> {out}")

    ts = subprocess.run(["date", "+%H%M%S"], capture_output=True, text=True).stdout.strip()
    t0 = time.time()
    rc, sweep = 0, 0
    while True:
        sweep += 1
        pending = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending == 0:
            print(f"[clband] queue drained (sweep {sweep})")
            break
        log = logdir / f"worker-{ctx}-{ts}-s{sweep}.log"
        print(f"[clband] sweep {sweep}: {pending} pending -> worker")
        rc = run_worker(scope, log)
        after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if after >= pending:
            print(f"[clband] no progress ({pending}->{after}); stopping")
            break
    elapsed = time.time() - t0

    drafts = [parse_draft(p) for p in scope.rglob("*.pending")]
    result = dict(context=ctx, transcript=str(transcript), scope=str(scope), worker_rc=rc,
                  elapsed_s=round(elapsed, 1), n_drafts=len(drafts), drafts=drafts)
    outp = scope.parent.parent / f"extract_{ctx}.json"
    outp.write_text(json.dumps(result, indent=1))
    print(f"\n=== {ctx}: {len(drafts)} .pending draft(s) in {elapsed:.0f}s rc={rc} -> {outp} ===")
    for d in drafts:
        print(f"   {d['name']}  [{d['skill_type']}]  {d['path']}")


if __name__ == "__main__":
    main()
