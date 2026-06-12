#!/usr/bin/env python3
"""T22 Unit C — dogfood extraction-regression gate (REAL pipeline, no fakes).

Hard guardrail: the taught-knowledge prompt change (EXTRACT_TEACH_CAPTURE) produces the SAME 262
organic corpus and may not degrade it. This runner re-extracts N organic session transcripts through
the REAL maintenance-worker under BOTH arms — EXTRACT_TEACH_CAPTURE=off (the pre-T22 prompt,
byte-for-byte) and =on (the candidate default) — into ISOLATED scratch scopes (never the 262 corpus),
and diffs draft count + draft identity/quality per session. The only variable between arms is the
flag, so any delta is attributable to the prompt change.

Usage: dogfood_regression.py <out.json> <transcript1.jsonl> [<transcript2.jsonl> ...]
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKER = str(ROOT / "target/debug/maintenance-worker")
MCP = "http://127.0.0.1:3001"
SCRATCH = ROOT / "tests/e2e/reports/efficacy/dogfood-regression"
PG = ["docker", "exec", "dynamic-agent-skill-layer-postgres-1", "psql", "-U", "skill_layer",
      "-d", "skill_layer_test", "-t", "-A"]


def psql(sql):
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def ingest(session_id, content, repo_path):
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": repo_path}).encode()
    req = urllib.request.Request(f"{MCP}/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def run_worker(scope_dir: Path, teach: str, logpath: Path):
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
        "EXTRACT_TEACH_CAPTURE": teach,  # the ONLY variable between arms
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
    with open(logpath, "w") as lf:
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT)
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
                skill_type=fm.get("type", ""), body_chars=len(text))


def extract_arm(transcript: Path, arm: str, idx: int):
    """One arm (off|on) for one transcript, into a fresh isolated scope."""
    scope = SCRATCH / f"s{idx}-{arm}"
    if scope.exists():
        shutil.rmtree(scope)
    scope.mkdir(parents=True)
    (scope / ".git").mkdir()  # marker so the scope resolver isolates this dir
    (scope / ".git" / "HEAD").write_text("ref: refs/heads/main\n")
    logdir = SCRATCH / "logs"
    logdir.mkdir(parents=True, exist_ok=True)

    psql("DELETE FROM transcript_ingest_queue;")
    sid = f"dogfood-regress-{idx}-{arm}"
    out = ingest(sid, transcript.read_text(errors="replace"), str(scope))
    print(f"[regress] s{idx} {arm}: ingest {transcript.name} ({transcript.stat().st_size//1024}KB) -> {out}")

    t0 = time.time()
    sweep, rc = 0, 0
    while True:
        sweep += 1
        pending = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if pending == 0:
            break
        rc = run_worker(scope, arm, logdir / f"s{idx}-{arm}-sweep{sweep}.log")
        after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
        if after >= pending:
            print(f"[regress] s{idx} {arm}: no progress ({pending}->{after}); stopping")
            break
    elapsed = round(time.time() - t0, 1)
    drafts = sorted((parse_draft(p) for p in scope.rglob("*.pending")), key=lambda d: d["name"])
    print(f"[regress] s{idx} {arm}: {len(drafts)} drafts in {elapsed}s")
    return dict(arm=arm, scope=str(scope), n_drafts=len(drafts), elapsed_s=elapsed, drafts=drafts)


def diff_arms(off, on):
    off_names = {d["name"] for d in off["drafts"]}
    on_names = {d["name"] for d in on["drafts"]}
    return dict(
        n_off=off["n_drafts"], n_on=on["n_drafts"], delta=on["n_drafts"] - off["n_drafts"],
        only_off=sorted(off_names - on_names), only_on=sorted(on_names - off_names),
        shared=len(off_names & on_names),
    )


def main():
    out_path = Path(sys.argv[1])
    transcripts = [Path(p) for p in sys.argv[2:]]
    if not transcripts:
        sys.exit("usage: dogfood_regression.py <out.json> <transcript...>")
    SCRATCH.mkdir(parents=True, exist_ok=True)
    results = []
    for idx, tr in enumerate(transcripts):
        off = extract_arm(tr, "off", idx)
        on = extract_arm(tr, "on", idx)
        d = diff_arms(off, on)
        print(f"=== s{idx} {tr.name}: off={d['n_off']} on={d['n_on']} delta={d['delta']} "
              f"only_off={d['only_off']} only_on={d['only_on']} ===")
        results.append(dict(transcript=str(tr), off=off, on=on, diff=d))
    out_path.write_text(json.dumps(dict(results=results), indent=1))
    print(f"\nwrote {out_path}")
    # Summary verdict.
    total_delta = sum(r["diff"]["delta"] for r in results)
    print(f"SUMMARY: total draft delta (on - off) across {len(results)} sessions = {total_delta:+d}")


if __name__ == "__main__":
    main()
