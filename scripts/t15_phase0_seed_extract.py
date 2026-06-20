#!/usr/bin/env python3
"""T15 Phase-0 seed extraction driver — REAL frontier pipeline, no fakes.

Per repo scope: ingest the SWE-bench seed-solve transcripts (that landed in
~/.claude/projects/<encoded-workspace>/) into the live mcp-server /ingest/transcript
with repo_path = the workspace (scopes drafts to <workspace>/.skills), then drain the
REAL host maintenance-worker binary with EXTRACT_SESSION_PROVIDER=claude-code (frontier)
until the queue is empty — drain-until-done, no caps. Mirrors scripts/replica_extract.py
but parameterized for an arbitrary workspace scope so django/sympy stay isolated.

Usage: t15_phase0_seed_extract.py --repo <django|sympy> --workspace /tmp/swebench-phase0-<repo>
"""
import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKER = str(ROOT / "target/debug/maintenance-worker")
MCP = "http://127.0.0.1:3001"
PG_CONTAINER = os.environ.get("PG_CONTAINER", "dynamic-agent-skill-layer-postgres")
PG = ["docker", "exec", PG_CONTAINER, "psql", "-U", "skill_layer", "-d", "skill_layer_test", "-t", "-A"]


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def encoded_project_dir(ws: Path) -> Path:
    # Claude Code stores per-project transcripts under ~/.claude/projects/<path with / -> ->.
    enc = str(ws).replace("/", "-")
    return Path.home() / ".claude" / "projects" / enc


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def run_worker(logpath, ws: Path):
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        # Frontier provider (host CLI) — best-shot extraction; gate is live in this binary.
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        # Scope drafts to THIS repo's workspace only.
        "CLAUDE_TRANSCRIPT_ROOT": str(ws),
        "SKILL_GLOBAL_PATHS": str(ws / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{ws},{ws}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(ws),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(ws / "global"),
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    (ws / "global").mkdir(parents=True, exist_ok=True)
    with open(logpath, "w") as lf:
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
    return proc.returncode


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--workspace", required=True)
    args = ap.parse_args()
    ws = Path(args.workspace)
    proj = encoded_project_dir(ws)
    transcripts = sorted(proj.glob("*.jsonl")) if proj.exists() else []
    print(f"[p0] repo={args.repo} ws={ws}")
    print(f"[p0] transcript dir {proj} -> {len(transcripts)} jsonl")
    if not transcripts:
        print("[p0] NO TRANSCRIPTS FOUND — did the solves run with cwd=workspace?", file=sys.stderr)
        sys.exit(2)

    for i, t in enumerate(transcripts):
        sid = f"t15p0-{args.repo}-{i:02d}-{t.stem[:8]}"
        try:
            out = ingest(sid, t.read_text(errors="replace"), str(ws))
            print(f"   [{i}] {t.name} ({t.stat().st_size//1024}KB) -> {out.get('status','?')}")
        except Exception as e:
            print(f"   [{i}] {t.name} -> ERR:{e}")

    logdir = ws / "logs"
    logdir.mkdir(exist_ok=True)
    t0 = time.time()
    sweep = 0
    rc = 0
    while True:
        sweep += 1
        before = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if before == 0:
            print(f"[p0] queue drained (sweep {sweep}): 0 pending")
            break
        log = logdir / f"worker-s{sweep}.log"
        print(f"[p0] sweep {sweep}: {before} pending -> worker (log {log})")
        rc = run_worker(log, ws)
        after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if after >= before:
            print(f"[p0] no progress ({before}->{after}); stopping")
            break
    elapsed = time.time() - t0

    drafts = sorted(ws.rglob("*.pending"))
    print(f"\n=== Phase-0 seed extract: repo={args.repo} ===")
    print(f"transcripts={len(transcripts)} drafts={len(drafts)} elapsed={elapsed:.0f}s rc={rc}")
    for d in drafts:
        print(f"   draft: {d.relative_to(ws)}")


if __name__ == "__main__":
    main()
