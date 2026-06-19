#!/usr/bin/env python3
"""Honest field-backfill re-extraction driver (2026-06-18) — REAL pipeline, no fakes.

WHY: ~28 session_start gold skills lack `use_when`/`requires` (their e_task/e_needs
views are empty). They were extracted on 2026-06-10 with a prompt OLDER than the
current frontier `prompt_contract.rs` (re-architected 2026-06-12, commits 53b0156 +
f1647d5). This driver RE-EXTRACTS their genuine source transcripts with the current
frontier prompt to see whether grounded use_when/requires can be recovered HONESTLY
(no body-only synthesis — fields must come from the real session evidence).

It is a scratch-isolated variant of scripts/replica_extract.py:
  - writes fresh `.pending` drafts to a SCRATCH dir (NOT the live corpus dir), so the
    live 277-skill corpus is untouched during extraction;
  - drives the REAL host maintenance-worker binary (RUN_ONCE + TRANSCRIPT_DRAIN,
    EXTRACT_SESSION_PROVIDER=claude-code frontier) over the REAL mcp-server
    /ingest/transcript endpoint and the shared transcript_ingest_queue;
  - drain-until-done (no arbitrary caps), neutralizing the queue first so the worker
    drains ONLY this batch.

Usage:
  scripts/backfill_reextract.py <manifest.txt> [--label smoke]
  # manifest.txt = one absolute path per line to a genuine session .jsonl
"""
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
SCRATCH = Path(os.environ.get("BACKFILL_SCRATCH", "/tmp/backfill-reextract"))
SKILLS = SCRATCH / "skills"
LOGS = SCRATCH / "logs"
WORKER = str(ROOT / "target/debug/maintenance-worker")
MCP = "http://127.0.0.1:3001"
PG_CONTAINER = os.environ.get("PG_CONTAINER", "dynamic-agent-skill-layer-postgres")
PG = ["docker", "exec", PG_CONTAINER, "psql", "-U", "skill_layer", "-d", "skill_layer_test", "-t", "-A"]

MULTIVIEW_KEYS = ["use_when", "avoid_when", "requires", "invariants", "tools",
                  "artifacts", "produces", "evidence", "type"]


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def neutralize_queue():
    psql("DELETE FROM transcript_ingest_queue;")
    print(f"[backfill] purged queue; rows now: {psql('SELECT count(*) FROM transcript_ingest_queue')}")


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        return json.loads(resp.read())


def run_worker(logpath):
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "EMBEDDING_PROVIDER": "ollama",
        "OLLAMA_EMBED_MODEL": "qwen3-embedding:4b",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        # Frontier provider — the multi-view redesign was approved/measured on claude-code.
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        # SCRATCH skills dir — do NOT touch the live corpus during extraction.
        "CLAUDE_TRANSCRIPT_ROOT": str(SKILLS),
        "SKILL_GLOBAL_PATHS": str(SKILLS / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{SKILLS}/project,{SKILLS}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(SKILLS / "project"),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(SKILLS / "global"),
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
        "PATH": os.environ.get("PATH", "") + ":" + str(Path.home() / ".local/bin"),
    })
    print(f"[backfill] running maintenance-worker (claude-code frontier); log: {logpath}")
    with open(logpath, "w") as lf:
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
    print(f"[backfill] worker exited rc={proc.returncode}")
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
    return dict(name=fm.get("name", path.parent.name),
                description=fm.get("description", ""),
                use_when=fm.get("use_when") or [],
                avoid_when=fm.get("avoid_when") or [],
                requires=fm.get("requires") or [],
                invariants=fm.get("invariants") or [],
                produces=fm.get("produces") or [],
                tools=fm.get("tools") or [],
                artifacts=fm.get("artifacts") or [],
                skill_type=fm.get("type", ""),
                path=str(path))


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    manifest = Path(sys.argv[1])
    label = "run"
    if "--label" in sys.argv:
        label = sys.argv[sys.argv.index("--label") + 1]

    SKILLS.joinpath("project").mkdir(parents=True, exist_ok=True)
    SKILLS.joinpath("global").mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(parents=True, exist_ok=True)

    neutralize_queue()
    files = [Path(l.strip()) for l in manifest.read_text().splitlines() if l.strip()]
    print(f"[backfill] ingesting {len(files)} transcripts -> {SKILLS}")
    for i, f in enumerate(files):
        # Preserve the same session-id scheme as the original corpus build so the
        # 8-hex stem aligns with the existing skills' source_session_id.
        sid = f"backfill-{i:04d}-{f.stem[:8]}"
        try:
            out = ingest(sid, f.read_text(errors="replace"), str(SKILLS / "project"))
            status = out.get("status", "?")
        except Exception as e:
            status = f"ERR:{e}"
        print(f"   [{i}] {f.name} ({f.stat().st_size // 1024}KB) -> {status}")

    ts = subprocess.run(["date", "+%H%M%S"], capture_output=True, text=True).stdout.strip()
    t0 = time.time()
    rc, sweep = 0, 0
    while True:
        sweep += 1
        pending_before = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending_before == 0:
            print(f"[backfill] queue drained (sweep {sweep}): 0 pending")
            break
        logpath = LOGS / f"worker-{label}-{ts}-s{sweep}.log"
        print(f"[backfill] sweep {sweep}: {pending_before} pending -> running worker")
        rc = run_worker(logpath)
        pending_after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending_after >= pending_before:
            print(f"[backfill] no progress (pending {pending_before}->{pending_after}); stopping to avoid loop")
            break
    elapsed = time.time() - t0

    drafts = [parse_draft(p) for p in SKILLS.rglob("*.pending")]
    pop = {k: sum(1 for d in drafts if d.get(k)) for k in MULTIVIEW_KEYS}
    result = dict(transcripts=len(files), drafts=len(drafts), worker_rc=rc,
                  elapsed_s=round(elapsed, 1), multiview_population=pop, draft_detail=drafts)
    outp = SCRATCH / f"extract_result_{label}.json"
    outp.write_text(json.dumps(result, indent=1))
    print(f"\n=== backfill re-extract (label={label}) ===")
    print(f"transcripts={len(files)} drafts={len(drafts)} elapsed={elapsed:.0f}s rc={rc}")
    for k in MULTIVIEW_KEYS:
        print(f"   {k:12s}: {pop[k]}")
    print(f"report: {outp}")


if __name__ == "__main__":
    main()
