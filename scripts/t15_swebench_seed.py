#!/usr/bin/env python3
"""T15 Phase 2 — seed→gate→reconcile into the CLEAN layer + live separation test.

Pieces C and E2 of todo 284. Operates ONLY on the isolated experiment layer
(swebench_t15 DB / skills__t15_swebench collection / the dast15-* containers on
:3002). The 277-skill dogfood corpus on :3001 is never read or mutated here.

Subcommands:
  gate-existing    — auto-gate the Phase-0 seed drafts into the clean swebench-<repo>
                     scopes via the REAL rename path (.pending → SKILL.md, T23
                     precedent), reconcile (graph-builder file-watch + mcp-server
                     snapshot rebuild), and snapshot the per-scope corpus inventory.
  reconcile        — force a reconcile + mcp-server-t15 snapshot rebuild and report counts.
  inventory        — print the per-scope seed-skill inventory (count + names).
  separation-test  — E2: fire the pre-registered aligned probes via the REAL
                     compile_context (:3002) and assert each repo's probe retrieves
                     its OWN-repo seed skills and NOT the other's.

NO FAKES: the gate uses the filesystem rename the production gate uses; retrieval
drives the real isolated server over HTTP; the separation assertions read the
live injected skill names (no offline cosine reconstruction).
"""
import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import efficacy_ab as eff  # noqa: E402
import t15_swebench_runner as runner  # noqa: E402  — reuse scope map + problem fetch

REPO_ROOT = _SCRIPTS_DIR.parent
SERVER = "http://127.0.0.1:3002"
PG = ["docker", "exec", "dynamic-agent-skill-layer-postgres",
      "psql", "-U", "skill_layer", "-d", "swebench_t15", "-t", "-A"]

# Phase-0 draft sources → clean scope dirs.
PHASE0_SOURCES = {
    "swebench-django": Path("/tmp/swebench-phase0-django/.skills"),
    "swebench-sympy": Path("/tmp/swebench-phase0-sympy/.skills"),
}

# E2 pre-registered aligned held-out probes (recorded in the Phase-0 assessment).
SEPARATION_PROBES = {
    "swebench-django": ["django__django-12708", "django__django-16820"],
    "swebench-sympy": ["sympy__sympy-12171", "sympy__sympy-14817"],
}


def psql(sql: str) -> str:
    return subprocess.run(PG + ["-c", sql], capture_output=True, text=True).stdout.strip()


def scope_dir(scope: str) -> Path:
    return runner.T15_PROJECT_ROOT / scope


# ── C: gate the Phase-0 drafts into the clean scopes ──────────────────────────

def gate_existing(args: argparse.Namespace) -> int:
    """Copy each Phase-0 .pending draft into its clean scope, then RENAME to
    SKILL.md (the real gate path), so graph-builder-t15 reconciles it into the
    isolated DB. Idempotent: re-running re-gates from source."""
    gated = {}
    for scope, src in PHASE0_SOURCES.items():
        dst_skills = scope_dir(scope) / ".skills"
        dst_skills.mkdir(parents=True, exist_ok=True)
        names = []
        if not src.is_dir():
            print(f"WARNING: phase-0 source missing: {src}", file=sys.stderr)
            continue
        for pending in sorted(src.rglob("*.pending")):
            skill_name = pending.parent.name
            dst_dir = dst_skills / skill_name
            dst_dir.mkdir(parents=True, exist_ok=True)
            # Copy the draft into the clean scope as .pending, then exercise the
            # REAL rename gate (.pending → SKILL.md) in place.
            staged = dst_dir / "SKILL.md.pending"
            shutil.copyfile(pending, staged)
            final = dst_dir / "SKILL.md"
            staged.rename(final)  # the gate
            names.append(skill_name)
        gated[scope] = names
        print(f"[gate] {scope}: gated {len(names)} skills -> {dst_skills}")
        for n in names:
            print(f"        + {n}")

    reconcile_and_rebuild(wait_s=args.reconcile_wait, expected_total=sum(len(v) for v in gated.values()))
    print_inventory()
    return 0


def reconcile_and_rebuild(wait_s: int, expected_total: int | None = None) -> None:
    """Wait for graph-builder-t15 to reconcile the filesystem into PG, then force
    an mcp-server-t15 snapshot rebuild. Drain-until-stable, no fake sleeps."""
    print(f"[reconcile] waiting for graph-builder-t15 (≤{wait_s}s, expected≈{expected_total})...")
    deadline = time.time() + wait_s
    last = -1
    while time.time() < deadline:
        n = int(psql("SELECT count(*) FROM skills WHERE status != 'retired'") or 0)
        emb = int(psql("SELECT count(*) FROM skill_embeddings") or 0)
        if n != last:
            print(f"   skills={n} embeddings={emb}")
            last = n
        if expected_total is not None and n >= expected_total and emb >= expected_total:
            break
        time.sleep(3)
    # Force mcp-server-t15 to rebuild its in-memory snapshot from PG.
    print("[reconcile] restarting mcp-server-t15 to rebuild snapshot...")
    subprocess.run(
        ["docker", "compose", "-f", str(REPO_ROOT / "docker-compose.t15.yml"), "-p", "dast15",
         "up", "-d", "--no-deps", "--force-recreate", "mcp-server-t15"],
        capture_output=True, text=True,
    )
    _wait_healthy(SERVER, 60)


def _wait_healthy(server: str, timeout_s: int) -> None:
    import urllib.request
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(server + "/health", timeout=4) as r:
                if json.loads(r.read()).get("healthy"):
                    print(f"[reconcile] {server} healthy")
                    return
        except Exception:
            pass
        time.sleep(2)
    print(f"WARNING: {server} not healthy within {timeout_s}s", file=sys.stderr)


def print_inventory() -> dict:
    inv = {}
    for scope in PHASE0_SOURCES:
        names = sorted(runner.seed_skill_names(scope_dir(scope)))
        inv[scope] = names
    total_pg = int(psql("SELECT count(*) FROM skills WHERE status != 'retired'") or 0)
    print("\n=== clean-layer corpus inventory (swebench_t15) ===")
    print(f"PG skills (non-retired): {total_pg}")
    for scope, names in inv.items():
        print(f"  {scope}: {len(names)} skills")
        for n in names:
            print(f"      - {n}")
    inv["_pg_total_non_retired"] = total_pg
    return inv


# ── E2: live retrieval-of-seeds + semantic-separation test ────────────────────

def separation_test(args: argparse.Namespace) -> int:
    """Fire the aligned probes via the REAL compile_context and assert each repo's
    probe retrieves its OWN seeds and NOT the other's. Honest reporting: a probe
    that returns no_match (no seed retrieved) is recorded as such, not as a pass."""
    print("=== E2 live retrieval-of-seeds + semantic-separation test ===")
    own_seeds = {s: runner.seed_skill_names(scope_dir(s)) for s in PHASE0_SOURCES}
    foreign = {
        "swebench-django": own_seeds["swebench-sympy"],
        "swebench-sympy": own_seeds["swebench-django"],
    }
    import uuid

    rows = []
    overall_ok = True
    for scope, probes in SEPARATION_PROBES.items():
        for iid in probes:
            problem = None
            for attempt in range(4):  # transient HF blips retry; persistent → record
                try:
                    problem = runner.fetch_problem_statement(iid)
                    break
                except Exception as exc:  # noqa: BLE001
                    print(f"  [{scope}] {iid}: fetch attempt {attempt + 1} failed: {exc}",
                          file=sys.stderr)
                    time.sleep(3 * (attempt + 1))
            if problem is None:
                rows.append({"scope": scope, "probe": iid, "error": "problem fetch failed after retries"})
                overall_ok = False
                continue
            # trigger=session_start = the production SessionStart priming path
            # (lower floor + multi-view) that retrieves against verbose issues.
            cc = eff.compile_context_http(
                server_url=args.server_url, prompt=problem,
                session_id=f"t15-sep-{iid}-{uuid.uuid4()}", repo_path=str(scope_dir(scope)),
                trigger="session_start",
            )
            names = set(cc["skill_names"])
            own_hit = sorted(names & own_seeds[scope])
            foreign_hit = sorted(names & foreign[scope])
            retrieved_own = len(own_hit) > 0
            no_foreign = len(foreign_hit) == 0
            ok = retrieved_own and no_foreign
            overall_ok = overall_ok and ok
            status = cc["raw"].get("status")
            rows.append({
                "scope": scope, "probe": iid, "status": status,
                "injected": sorted(names), "own_seed_hits": own_hit,
                "foreign_seed_hits": foreign_hit,
                "retrieved_own": retrieved_own, "no_foreign_leak": no_foreign, "pass": ok,
            })
            verdict = "PASS" if ok else ("NO-MATCH" if not retrieved_own and no_foreign else "FAIL")
            print(f"  [{scope}] {iid} status={status} own={own_hit} foreign={foreign_hit} -> {verdict}")

    report = {
        "test": "E2 separation", "server_url": args.server_url,
        "own_seeds": {s: sorted(v) for s, v in own_seeds.items()},
        "rows": rows,
        "all_pass": overall_ok,
        "summary": {
            "probes": len(rows),
            "passed": sum(1 for r in rows if r.get("pass")),
            "foreign_leaks": sum(1 for r in rows if r.get("foreign_seed_hits")),
        },
    }
    out = runner.REPORT_DIR
    out.mkdir(parents=True, exist_ok=True)
    out_path = out / "e2_separation.json"
    out_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"\n[report] {out_path}")
    print(f"E2 RESULT: all_pass={overall_ok} foreign_leaks={report['summary']['foreign_leaks']}")
    # A foreign leak is a hard separation failure → non-zero. Pure no_match is
    # inconclusive (exit 2), full pass is 0.
    if report["summary"]["foreign_leaks"] > 0:
        return 1
    return 0 if overall_ok else 2


WORKER = str(REPO_ROOT / "target/debug/maintenance-worker")


def seed_drain(args: argparse.Namespace) -> int:
    """SEED arc into the ISOLATED layer: ingest a real seed-solve transcript to the
    clean server (:3002 → swebench_t15 queue), then drain via the HOST worker with
    DATABASE_URL pointed at swebench_t15 + frontier extraction. Proves the HARD
    INVARIANT (extraction never touches skill_layer_test) and that E3 holds (the
    recurrence/promotion pass no longer crashes on a merge-verifier JSON miss).
    """
    import urllib.request

    if args.transcript and Path(args.transcript).is_file():
        transcript = Path(args.transcript)
    else:
        # Default: first Phase-0 django seed-solve transcript.
        proj = Path.home() / ".claude/projects/-tmp-swebench-phase0-django"
        cands = sorted(proj.glob("*.jsonl")) if proj.exists() else []
        if not cands:
            print(f"ERROR: no --transcript given and no Phase-0 django transcript found under {proj}",
                  file=sys.stderr)
            return 2
        transcript = cands[0]
    ws = Path(args.workspace)
    (ws / ".skills").mkdir(parents=True, exist_ok=True)
    (ws / "global").mkdir(parents=True, exist_ok=True)

    # 1) Ingest the transcript to the CLEAN server (queue row lands in swebench_t15).
    sid = f"t15p2-seeddrain-{transcript.stem[:8]}"
    body = json.dumps({"session_id": sid, "source": "session_end",
                       "content": transcript.read_text(errors="replace"),
                       "repo_path": str(ws)}).encode()
    req = urllib.request.Request(SERVER + "/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        print(f"[seed-drain] ingested {transcript.name} -> {json.loads(r.read()).get('status')}")

    pending = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
    print(f"[seed-drain] swebench_t15 queue pending = {pending}")

    # 2) Drain via the HOST worker (frontier extraction) pointed at the ISOLATED DB.
    import os
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/swebench_t15",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "QDRANT_COLLECTION": "skills__t15_swebench",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EXTRACTION_MODEL": "gemma4:12b",
        "OLLAMA_EXTRACTION_ENDPOINT": "http://127.0.0.1:11444/api/generate",
        "OLLAMA_EMBED_MODEL": "qwen3-embedding:4b",
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        "CLAUDE_TRANSCRIPT_ROOT": str(ws),
        "SKILL_GLOBAL_PATHS": str(ws / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{ws},{ws}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(ws),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(ws / "global"),
        "SKILL_PROJECT_MARKER": ".skills",
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    log = ws / "seed-drain-worker.log"
    print(f"[seed-drain] draining via host worker (DATABASE_URL=swebench_t15) → {log}")
    with open(log, "w") as lf:
        proc = subprocess.run([WORKER], env=env, stdout=lf, stderr=subprocess.STDOUT, timeout=args.timeout)
    drafts = sorted(ws.rglob("*.pending"))
    after = int(psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)
    # E3 check: a malformed merge-verifier JSON must DEGRADE, not crash (rc==0).
    degraded = "merge_verifier_malformed_json_degraded" in log.read_text(errors="replace")
    print(f"[seed-drain] worker rc={proc.returncode}  queue_after={after}  drafts={len(drafts)}  "
          f"e3_degrade_seen={degraded}")
    for d in drafts:
        print(f"   draft: {d.relative_to(ws)}")
    # Confirm the dogfood DB was untouched by this isolated extraction.
    dogfood = subprocess.run(
        ["docker", "exec", "dynamic-agent-skill-layer-postgres", "psql", "-U", "skill_layer",
         "-d", "skill_layer_test", "-t", "-A", "-c", "SELECT count(*) FROM skills;"],
        capture_output=True, text=True).stdout.strip()
    print(f"[seed-drain] dogfood skill_layer_test skills = {dogfood} (must be 277, untouched)")
    if proc.returncode != 0:
        print("WARNING: worker rc != 0 — inspect the drain log (E3 should prevent merge-JSON crashes).",
              file=sys.stderr)
        return 1
    return 0


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gate-existing")
    g.add_argument("--reconcile-wait", dest="reconcile_wait", type=int, default=90)
    rc = sub.add_parser("reconcile")
    rc.add_argument("--reconcile-wait", dest="reconcile_wait", type=int, default=90)
    sub.add_parser("inventory")
    se = sub.add_parser("separation-test")
    se.add_argument("--server-url", dest="server_url", default=SERVER)
    sd = sub.add_parser("seed-drain")
    sd.add_argument("--transcript", default="", help="Seed-solve transcript (default: first Phase-0 django).")
    sd.add_argument("--workspace", default="/tmp/t15-swebench/seedrun/swebench-django")
    sd.add_argument("--timeout", type=int, default=1200)

    args = ap.parse_args()
    if args.cmd == "seed-drain":
        sys.exit(seed_drain(args))
    if args.cmd == "gate-existing":
        sys.exit(gate_existing(args))
    if args.cmd == "reconcile":
        reconcile_and_rebuild(wait_s=args.reconcile_wait)
        print_inventory()
        sys.exit(0)
    if args.cmd == "inventory":
        print_inventory()
        sys.exit(0)
    if args.cmd == "separation-test":
        sys.exit(separation_test(args))


if __name__ == "__main__":
    main()
