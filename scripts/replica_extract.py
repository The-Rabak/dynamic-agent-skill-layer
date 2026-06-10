#!/usr/bin/env python3
"""Replica multi-view extraction driver (T10) — REAL pipeline, no fakes.

Adapts the proven scripts/measure_214_extraction.py corpus-drain recipe for the
v1.7 multi-view prompt redesign + replica isolation:

  1. Read the FROZEN source manifest (tests/e2e/reports/replica-run/source_manifest.txt)
     so claude-code's own subprocess sessions (which land in ~/.claude/projects/-tmp)
     cannot pollute the run's input mid-flight.
  2. Neutralize the shared transcript_ingest_queue (mark spent rows processed) so the
     worker drains ONLY this batch.
  3. Ingest each transcript via the REAL mcp-server /ingest/transcript endpoint (:3001)
     -> rows land in transcript_ingest_queue (skill_layer_test).
  4. Run the REAL host maintenance-worker binary (RUN_ONCE + TRANSCRIPT_DRAIN) with
     EXTRACT_SESSION_PROVIDER=claude-code, draining the queue through the new
     multi-view prompts + grounding validator into the PERSISTENT replica .skills dir.
  5. Count + parse the produced .pending drafts (multi-view field population).

NO timeouts on the worker (drain-until-done; churners get no arbitrary caps).

Usage: replica_extract.py <N|all> [--neutralize]
"""
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RDIR = ROOT / "tests/e2e/reports/replica-run"
MANIFEST = Path(os.environ.get("REPLICA_MANIFEST", str(RDIR / "genuine_manifest.txt")))
SKILLS = RDIR / "skills"
WORKER = str(ROOT / "target/debug/maintenance-worker")
MCP = "http://127.0.0.1:3001"
PG = ["docker", "exec", "dynamic-agent-skill-layer-postgres-1", "psql", "-U", "skill_layer",
      "-d", "skill_layer_test", "-t", "-A"]

MULTIVIEW_KEYS = ["use_when", "avoid_when", "requires", "invariants", "tools",
                  "artifacts", "produces", "evidence", "type"]


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def neutralize_queue():
    # DELETE (not mark-processed): the ingest endpoint dedups by content_hash, so a
    # leftover processed row for the same transcript would make re-ingest a no-op
    # ("duplicate") and the worker would drain nothing. This queue is run scratch.
    psql("DELETE FROM transcript_ingest_queue;")
    pending = psql("SELECT count(*) FROM transcript_ingest_queue")
    print(f"[replica] purged queue; rows now: {pending}")


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def run_worker(logpath):
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        # Frontier provider — the multi-view redesign was approved/measured on claude-code.
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        # Persistent replica skills dir (NOT mktemp — corpus survives for approve+ingest).
        "CLAUDE_TRANSCRIPT_ROOT": str(SKILLS),
        "SKILL_GLOBAL_PATHS": str(SKILLS / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{SKILLS}/project,{SKILLS}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(SKILLS / "project"),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(SKILLS / "global"),
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    print(f"[replica] running maintenance-worker (claude-code); log: {logpath}")
    with open(logpath, "w") as lf:
        # No timeout — drain to completion.
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
    print(f"[replica] worker exited rc={proc.returncode}")
    return proc.returncode


def parse_draft(path: Path):
    text = path.read_text(errors="replace")
    fm = {}
    if text.startswith("---\n") and "\n---\n" in text[4:]:
        raw_fm, _ = text[4:].split("\n---\n", 1)
        cur = None
        for line in raw_fm.splitlines():
            if line and not line.startswith(" ") and not line.startswith("-") and ":" in line:
                k, _, v = line.partition(":")
                cur = k.strip()
                fm[cur] = v.strip()
            elif line.strip().startswith("-") and cur:
                fm[cur] = (fm.get(cur, "") + " " + line.strip()).strip()
    populated = {k: bool(fm.get(k, "").strip()) for k in MULTIVIEW_KEYS}
    return dict(name=fm.get("name", path.parent.name),
                description=fm.get("description", ""),
                skill_type=fm.get("type", ""),
                populated=populated, path=str(path))


def main():
    n_arg = sys.argv[1] if len(sys.argv) > 1 else "all"
    if "--neutralize" in sys.argv:
        neutralize_queue()
    SKILLS.joinpath("project").mkdir(parents=True, exist_ok=True)
    SKILLS.joinpath("global").mkdir(parents=True, exist_ok=True)

    files = [Path(l) for l in MANIFEST.read_text().splitlines() if l.strip()]
    if n_arg != "all":
        files = files[:int(n_arg)]
    print(f"[replica] ingesting {len(files)} transcripts -> {SKILLS}")
    for i, f in enumerate(files):
        sid = f"replica-{i:04d}-{f.stem[:8]}"
        try:
            out = ingest(sid, f.read_text(errors="replace"), str(SKILLS / "project"))
            status = out.get("status", "?")
        except Exception as e:
            status = f"ERR:{e}"
        print(f"   [{i}] {f.name} ({f.stat().st_size//1024}KB) -> {status}")

    ts = subprocess.run(["date", "+%H%M%S"], capture_output=True, text=True).stdout.strip()
    t0 = time.time()
    # RUN_ONCE drains at most DEFAULT_TRANSCRIPT_DRAIN_BATCH (16) rows per invocation
    # (runtime.rs hardcodes the batch). Loop the worker until the queue is fully
    # drained — drain-until-done, no arbitrary cap. Guard: stop if a sweep makes no
    # progress (pending unchanged) so failed/parked rows can't loop forever.
    rc = 0
    sweep = 0
    logpath = RDIR / "logs" / f"worker-{n_arg}-{ts}-s1.log"
    while True:
        sweep += 1
        pending_before = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending_before == 0:
            print(f"[replica] queue drained (sweep {sweep}): 0 pending")
            break
        logpath = RDIR / "logs" / f"worker-{n_arg}-{ts}-s{sweep}.log"
        print(f"[replica] sweep {sweep}: {pending_before} pending -> running worker")
        rc = run_worker(logpath)
        pending_after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending_after >= pending_before:
            print(f"[replica] no progress (pending {pending_before}->{pending_after}); stopping to avoid loop")
            break
    elapsed = time.time() - t0

    drafts = [parse_draft(p) for p in SKILLS.rglob("*.pending")]
    pop_counts = {k: sum(1 for d in drafts if d["populated"][k]) for k in MULTIVIEW_KEYS}
    result = dict(transcripts=len(files), drafts=len(drafts), worker_rc=rc,
                  elapsed_s=round(elapsed, 1), multiview_population=pop_counts,
                  worker_log=str(logpath), draft_detail=drafts)
    outp = RDIR / f"extract_result_{n_arg}.json"
    outp.write_text(json.dumps(result, indent=1))

    print(f"\n=== replica extract (N={n_arg}) ===")
    print(f"transcripts={len(files)} drafts={len(drafts)} elapsed={elapsed:.0f}s rc={rc}")
    print(f"multi-view population (of {len(drafts)} drafts):")
    for k in MULTIVIEW_KEYS:
        print(f"   {k:12s}: {pop_counts[k]}")
    print(f"report: {outp}\nworker log: {logpath}")


if __name__ == "__main__":
    main()
