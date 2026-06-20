#!/usr/bin/env python3
"""T15 — self-seeding same-set compounding loop (no fakes, fail loud).

THE EXPERIMENT (owner design, 2026-06-20)
-----------------------------------------
Does the layer compound on its OWN recurring work? Run a bench of N instances with
the layer EMPTY (Round 1, OFF) → score X. EXTRACT skills from those N solve sessions
into a FRESH, initially-empty scope. Run the SAME N instances with the layer ON
(Round 2, TREAT) → score Y. Compounding = Y > X, on BOTH:
  * resolved-rate (the swebench oracle; paired McNemar Round2-vs-Round1), and
  * turns / output-tokens / cost-to-resolve (the efficiency instrument).

This is a SAME-SET self-improvement loop: it measures improvement on the system's
own task distribution, NOT generalization to unseen tasks. The skill mined from
instance i's Round-1 session is ALLOWED to help instance i in Round 2 — that is the
"learns from its own attempt and reapplies it" mechanism. Per-instance attribution
records whether each TREAT injection pulled the instance's OWN skill (source ==
this instance) or ANOTHER instance's (cross-task transfer within the bench).

Round-1 OFF and Round-2 TREAT are INDEPENDENT fresh solves of each instance that
differ ONLY in the layer injection, so Y−X is not a naive re-roll: the layer is the
sole difference (re-run variance is handled by the paired McNemar + bootstrap CI).

NO FAKES / FAIL LOUD: resolved bits come ONLY from the swebench oracle; transcripts
are the real claude session .jsonl; extraction runs the real host frontier worker
against the ISOLATED swebench_t15 DB; the gate is the real .pending→SKILL.md rename;
the dogfood corpus (skill_layer_test) is never read or mutated. A stuck drain fails
loud (no arbitrary cap — drain until the queue is empty or progress stalls).
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import t15_swebench_runner as runner   # noqa: E402  — solve_one_arm + scope/problem helpers
import t15_swebench_seed as seedmod    # noqa: E402  — psql, reconcile_and_rebuild, WORKER
import efficacy_ab as eff              # noqa: E402  — McNemar + efficiency aggregator

SERVER = "http://127.0.0.1:3002"
PROJECT_ROOT = runner.T15_PROJECT_ROOT                 # /tmp/t15-swebench/project
REPORT_DIR = runner.REPORT_DIR
CLAUDE_PROJECTS = Path.home() / ".claude/projects"


# ── transcript capture (claude writes one .jsonl per solve cwd) ────────────────

def encode_cwd(ws: Path) -> str:
    """Claude Code's per-project transcript dir name = the cwd with every
    non-alphanumeric char replaced by '-' (verified: /tmp/.../django__django-15789__off
    → -tmp-...-django--django-15789--off)."""
    return re.sub(r"[^a-zA-Z0-9]", "-", str(ws))


def locate_transcript(ws: Path) -> Path | None:
    """Newest claude session .jsonl for a solve cwd (None if claude wrote none)."""
    d = CLAUDE_PROJECTS / encode_cwd(ws)
    if not d.is_dir():
        return None
    cands = sorted(d.glob("*.jsonl"), key=lambda p: p.stat().st_mtime)
    return cands[-1] if cands else None


# ── seed: ingest OFF transcripts → drain via host worker → gate → reconcile ────

def ingest_transcript(session_id: str, content: str, repo_path: Path) -> str:
    body = json.dumps({"session_id": session_id, "source": "session_end",
                       "content": content, "repo_path": str(repo_path)}).encode()
    req = urllib.request.Request(SERVER + "/ingest/transcript", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read()).get("status", "?")


def _queue_pending() -> int:
    return int(seedmod.psql("SELECT count(*) FROM transcript_ingest_queue WHERE status='pending'") or 0)


def drain_until_empty(scope_dir: Path, log_dir: Path, timeout_s: int, max_passes: int = 25) -> dict[str, Any]:
    """Run the host frontier worker repeatedly until the ingest queue is EMPTY.

    Drain-until-done (no arbitrary cap): keep running MAINTENANCE_RUN_ONCE passes
    while the queue shrinks. FAIL LOUD if a pass makes zero progress with rows still
    pending (a real stuck state), rather than silently leaving the queue undrained.
    `max_passes` is only a runaway backstop (each pass drains many rows).
    """
    env = dict(os.environ)
    env.update({
        "DATABASE_URL": "postgres://skill_layer:skill_layer@127.0.0.1:15432/swebench_t15",
        "REDIS_URL": "redis://127.0.0.1:16379",
        "QDRANT_URL": "http://127.0.0.1:16333",
        "QDRANT_COLLECTION": "skills__t15_swebench",
        "OLLAMA_URL": "http://127.0.0.1:11444",
        "OLLAMA_EMBED_MODEL": "qwen3-embedding:4b",
        "EXTRACT_SESSION_PROVIDER": "claude-code",
        "EXTRACT_SESSION_MODEL": "claude-sonnet-4-6",
        "EXTRACT_SESSION_ROUTING": "frontier",
        "CLAUDE_TRANSCRIPT_ROOT": str(scope_dir),
        "SKILL_GLOBAL_PATHS": str(scope_dir / "global"),
        "SKILL_GLOBAL_ALLOWED_ROOTS": f"{scope_dir},{scope_dir}/global",
        "GRAPH_BUILDER_PROJECT_ROOT": str(scope_dir),
        "GRAPH_BUILDER_GLOBAL_ROOT": str(scope_dir / "global"),
        "SKILL_PROJECT_MARKER": ".skills",
        "MAINTENANCE_RUN_ONCE": "true",
        "MAINTENANCE_TRANSCRIPT_DRAIN": "on",
        "RUST_LOG": "info",
    })
    passes = 0
    pending = _queue_pending()
    print(f"[drain] queue pending at start = {pending}", flush=True)
    while pending > 0 and passes < max_passes:
        passes += 1
        log = log_dir / f"drain-pass-{passes}.log"
        with open(log, "w") as lf:
            proc = subprocess.run([seedmod.WORKER], env=env, stdout=lf,
                                  stderr=subprocess.STDOUT, timeout=timeout_s)
        after = _queue_pending()
        degraded = "merge_verifier_malformed_json_degraded" in log.read_text(errors="replace")
        print(f"[drain] pass {passes}: rc={proc.returncode} pending {pending}→{after} "
              f"e3_degrade_seen={degraded}", flush=True)
        if after >= pending:  # zero progress with rows still pending → real stuck state
            raise RuntimeError(
                f"drain STUCK: pass {passes} drained 0 rows (pending stayed {after}); "
                f"inspect {log} — refusing to proceed with an undrained queue")
        pending = after
    if pending > 0:
        raise RuntimeError(f"drain did not empty the queue after {passes} passes ({pending} pending)")
    drafts = sorted((scope_dir / ".skills").rglob("*.pending"))
    return {"passes": passes, "drafts_pending": [str(d) for d in drafts]}


def _parse_frontmatter(skill_md: Path) -> dict[str, str]:
    """Tiny frontmatter scan (name + source_session_id) — no yaml dependency."""
    out: dict[str, str] = {}
    text = skill_md.read_text(errors="replace")
    if not text.startswith("---"):
        return out
    body = text.split("---", 2)
    fm = body[1] if len(body) >= 3 else ""
    for line in fm.splitlines():
        for key in ("name", "source_session_id"):
            prefix = f"{key}:"
            if line.strip().startswith(prefix):
                out[key] = line.strip()[len(prefix):].strip().strip("'\"")
    return out


def gate_drafts(scope_dir: Path) -> list[dict[str, str]]:
    """Rename every <name>/SKILL.md.pending → SKILL.md (the REAL gate path), then
    read back source_session_id for own-vs-cross attribution. Returns gated metas."""
    gated: list[dict[str, str]] = []
    skills_dir = scope_dir / ".skills"
    for pending in sorted(skills_dir.rglob("*.pending")):
        final = pending.with_suffix("")            # SKILL.md.pending → SKILL.md
        pending.rename(final)                       # the gate
        meta = _parse_frontmatter(final)
        # Key by the CANONICAL frontmatter name (matches PG + compile_context
        # injection names); the slug dir name does not (self-seed smoke 2026-06-20).
        gated.append({"name": meta.get("name") or final.parent.name,
                      "dir": final.parent.name,
                      "source_session_id": meta.get("source_session_id", "")})
    return gated


def _iid_from_session(session_id: str, run_id: str) -> str | None:
    """Recover the source instance id from a `{run_id}-off-{iid}` ingest session id."""
    marker = f"{run_id}-off-"
    if session_id and session_id.startswith(marker):
        return session_id[len(marker):]
    return None


# ── auth-guard + per-instance checkpoint (robustness for a long N=40 run) ──────

class SolveInvalid(Exception):
    """A solve that did NOT really run (claude auth 401 / crash → empty `turns=1`
    output). Counting it as resolved=False would be a FAKE measurement, so the run
    FAILS LOUD and checkpoints — re-authenticate `claude` and re-run to resume."""


# claude marks `is_error=True` for BOTH a legit `--max-turns` cap-hit AND an auth
# death — so is_error alone is NOT the signal. The swebench ORACLE judges the patch
# deterministically, so the resolved bit is trustworthy for ANY real solve, including
# one that hit the turn cap and left an EMPTY patch (a genuine not-resolved — and
# exactly the OFF-failure / Round-2 flip candidate we must KEEP, verified live on
# django-15738: 40 turns, $1.31, terminal_reason=max_turns, 0-byte patch, no auth).
# Only a solve that NEVER REALLY RAN is invalid: no JSON summary at all, or an
# explicit claude AUTH failure (specific phrases, or a 401 in the dedicated
# api_error_status field — a bare "401" there is unambiguous, unlike "401" in prose).
_AUTH_PHRASES = ("failed to authenticate", "invalid authentication credentials")


def _solve_is_invalid(out: dict[str, Any]) -> bool:
    """True only when the solve produced no trustworthy attempt — a true crash (no
    JSON) or an explicit claude AUTH failure. A max-turns cap-hit (even empty patch)
    is VALID: the oracle scores the patch and an empty one is a real not-resolved."""
    e = out.get("efficiency")
    if e is None:
        return True  # no JSON summary → crashed / killed before emitting anything
    if "401" in str(e.get("api_error_status") or ""):
        return True  # dedicated API-error field carries the auth status → unambiguous
    txt = (out.get("result_text") or "").lower()
    return any(p in txt for p in _AUTH_PHRASES)  # explicit auth-failure text → retry


def solve_guarded(iid: str, arm: str, problem: str, args: argparse.Namespace,
                  run_id_prefix: str, scope_override: Path | None = None) -> dict[str, Any]:
    """runner.solve_one_arm + retry-on-invalid; FAIL LOUD if still invalid.

    Retries a transient auth blip a few times; a persistent 401/crash raises
    SolveInvalid so the caller checkpoints and exits rather than banking a fake
    not-resolved (the lesson of the 2026-06-20 mid-run auth-expiry that silently
    produced 9 empty solves)."""
    last = None
    for attempt in range(1, args.solve_retries + 1):
        out = runner.solve_one_arm(
            iid, arm, problem, SERVER, args.model, args.max_turns,
            args.solve_timeout, args.verify_timeout,
            run_id_prefix=run_id_prefix, treat_scope_override=scope_override)
        if not _solve_is_invalid(out):
            return out
        last = out
        e = out.get("efficiency") or {}
        print(f"  [guard] {iid} {arm}: INVALID solve (is_error/None, turns={e.get('num_turns')}, "
              f"out_tok={e.get('output_tokens')}) attempt {attempt}/{args.solve_retries} — "
              f"likely claude auth/crash; retrying after {args.retry_backoff_s}s", flush=True)
        time.sleep(args.retry_backoff_s)
    raise SolveInvalid(
        f"{iid} {arm}: solve still INVALID after {args.solve_retries} attempts "
        f"(turns={(last.get('efficiency') or {}).get('num_turns') if last else '?'}). "
        f"Re-authenticate the `claude` CLI (run a quick `claude --print` to confirm), then re-run "
        f"the SAME command — it resumes from the checkpoint and re-solves only what's missing.")


def _ckpt_load(path: Path) -> dict[str, Any]:
    if path.exists():
        return json.loads(path.read_text())
    return {"round1": {}, "seeding": None, "round2": {}}


def _ckpt_save(path: Path, ckpt: dict[str, Any]) -> None:
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(ckpt, indent=2) + "\n")
    tmp.replace(path)


_HF_ROWS_URL = ("https://datasets-server.huggingface.co/rows"
                "?dataset=princeton-nlp%2FSWE-bench_Lite&config=default&split=test"
                "&offset={offset}&limit=100")
PROBLEM_CACHE = Path("/tmp/t15-swebench/_cache/swebench_lite_problems.json")


def build_problem_map(retries: int = 8, base_backoff_s: float = 5.0) -> dict[str, str]:
    """Fetch the WHOLE SWE-bench Lite test split ONCE → {instance_id: problem_statement},
    cached to disk. The per-instance fetcher re-scans from offset 0 every call (≈4 page
    requests × 40 instances ≈ 160 bursty HF requests → 429). This pulls all ~300 rows in
    3–4 page requests, caches them, and serves every instance (and any re-run) locally —
    so the dataset's rate limit can't kill an overnight run. Page errors retry with
    growing backoff; a persistent page failure fails loud (no fabricated statements)."""
    if PROBLEM_CACHE.exists():
        cached = json.loads(PROBLEM_CACHE.read_text())
        if cached:
            print(f"[fetch] using cached problem map ({len(cached)} instances) {PROBLEM_CACHE}", flush=True)
            return cached
    m: dict[str, str] = {}
    for offset in range(0, 301, 100):
        data = None
        for attempt in range(1, retries + 1):
            try:
                req = urllib.request.Request(_HF_ROWS_URL.format(offset=offset),
                                             headers={"User-Agent": "dast-t15/bulk-problem-map"})
                with urllib.request.urlopen(req, timeout=30) as resp:
                    data = json.loads(resp.read())
                break
            except Exception as exc:  # noqa: BLE001 — retry transient (429/network); fail loud if persistent
                wait = base_backoff_s * attempt
                print(f"[fetch] dataset page offset={offset} attempt {attempt}/{retries} failed "
                      f"({str(exc)[:70]}); retrying in {wait:.0f}s", flush=True)
                time.sleep(wait)
        if data is None:
            raise RuntimeError(f"failed to fetch SWE-bench Lite page offset={offset} after {retries} attempts")
        rows = data.get("rows", [])
        if not rows:
            break
        for row in rows:
            r = row.get("row", {})
            iid = r.get("instance_id")
            ps = (r.get("problem_statement") or "").strip()
            if iid and ps:
                m[iid] = ps
    PROBLEM_CACHE.parent.mkdir(parents=True, exist_ok=True)
    PROBLEM_CACHE.write_text(json.dumps(m))
    print(f"[fetch] built + cached problem map ({len(m)} instances) → {PROBLEM_CACHE}", flush=True)
    return m


def fetch_problems_resilient(instances: list[str]) -> dict[str, str]:
    """All N problem statements from the one-shot cached dataset map. A missing instance
    is a HARD error (real id typo / dataset drift), never a fabricated/empty statement."""
    m = build_problem_map()
    problems: dict[str, str] = {}
    for iid in instances:
        if iid not in m or not m[iid].strip():
            raise RuntimeError(f"instance {iid} not in SWE-bench Lite problem map ({len(m)} rows) — "
                               f"check the id; refusing to fabricate a problem statement")
        problems[iid] = m[iid]
    return problems


# ── the loop ──────────────────────────────────────────────────────────────────

def run_loop(args: argparse.Namespace) -> int:
    log_dir = runner.REPO_ROOT / "logs/t15-selfseed" / args.run_id
    log_dir.mkdir(parents=True, exist_ok=True)
    ckpt_path = log_dir / "checkpoint.json"
    ckpt = _ckpt_load(ckpt_path)

    # ── Establish instances + scope (from checkpoint meta on resume, else args) ─
    if args.resume_round2:
        # Explicit "Round 1 + seeding already in a PRIOR REPORT; redo only Round 2"
        # entry — seed the checkpoint from that report so the rest is uniform.
        prior = json.loads(Path(args.resume_round2).read_text())
        ckpt.setdefault("meta", {"instances": prior["instances"], "scope": prior["scope"]})
        ckpt["round1"] = ckpt["round1"] or {r["instance_id"]: r for r in prior["round1_off"]}
        ckpt["seeding"] = ckpt["seeding"] or prior["seeding"]
        _ckpt_save(ckpt_path, ckpt)
    if "meta" in ckpt:
        instances = ckpt["meta"]["instances"]
        scope_dir = Path(ckpt["meta"]["scope"])
        print(f"=== T15 self-seed loop ({args.run_id}) — RESUMING from checkpoint "
              f"(R1 {len(ckpt['round1'])}/{len(instances)}, seeding={'done' if ckpt['seeding'] else 'pending'}, "
              f"R2 {len(ckpt['round2'])}/{len(instances)}) ===", flush=True)
    else:
        instances = [i.strip() for i in args.instances.split(",") if i.strip()]
        if not instances:
            print("ERROR: --instances is required (comma-separated swebench ids)", file=sys.stderr)
            return 2
        scope_dir = PROJECT_ROOT / args.scope_name
        existing = list((scope_dir / ".skills").rglob("SKILL.md")) if (scope_dir / ".skills").exists() else []
        if existing and not args.reuse_scope:
            print(f"ERROR: scope {scope_dir}/.skills is NOT empty ({len(existing)} SKILL.md). The same-set "
                  f"loop requires a fresh, empty scope so Round 2 injects ONLY skills learned from "
                  f"THIS bench's Round-1 runs. Pick a new --scope-name (or --reuse-scope to override).",
                  file=sys.stderr)
            return 2
        ckpt["meta"] = {"instances": instances, "scope": str(scope_dir)}
        _ckpt_save(ckpt_path, ckpt)
        print(f"=== T15 self-seeding same-set loop ({args.run_id}) ===", flush=True)
    (scope_dir / ".skills").mkdir(parents=True, exist_ok=True)
    (scope_dir / "global").mkdir(parents=True, exist_ok=True)
    print(f"instances (N={len(instances)}): scope={scope_dir}  model={args.model}  max_turns={args.max_turns}",
          flush=True)
    print("fetching problem statements (deterministic, no LLM; retry-on-429)...", flush=True)
    problems = fetch_problems_resilient(instances)

    # ── ROUND 1 — solve all N with the layer OFF; capture each transcript ─────
    print("\n── ROUND 1 (OFF — layer empty) ──", flush=True)
    for iid in instances:
        if iid in ckpt["round1"]:
            print(f"  [R1 OFF] {iid} (checkpointed: resolved={ckpt['round1'][iid]['resolved']})", flush=True)
            continue
        out = solve_guarded(iid, "off", problems[iid], args,
                            run_id_prefix=f"{args.run_id}-r1-{iid}".replace("__", "-"))
        transcript = locate_transcript(Path(f"/tmp/t15-swebench/solve/{iid}__off"))
        e = out["efficiency"] or {}
        ckpt["round1"][iid] = {"instance_id": iid, "resolved": out["resolved"],
                               "efficiency": out["efficiency"],
                               "transcript": str(transcript) if transcript else None,
                               "empty_patch": out["empty_patch"]}
        _ckpt_save(ckpt_path, ckpt)
        print(f"  [R1 OFF] {iid} resolved={out['resolved']} turns={e.get('num_turns')} "
              f"out_tok={e.get('output_tokens')} cost=${e.get('total_cost_usd')} "
              f"transcript={'yes' if transcript else 'MISSING'}", flush=True)

    # ── SEED — ingest OFF transcripts → drain → gate → reconcile ─────────────
    if ckpt["seeding"] is None:
        print("\n── SEED (extract Round-1 sessions → fresh scope) ──", flush=True)
        ingested = 0
        for iid in instances:
            r = ckpt["round1"][iid]
            if not r["transcript"]:
                print(f"  [seed] {iid}: NO transcript — skipped (recorded, not faked)", flush=True)
                continue
            status = ingest_transcript(f"{args.run_id}-off-{iid}",
                                       Path(r["transcript"]).read_text(errors="replace"), scope_dir)
            ingested += 1
            print(f"  [seed] ingested {iid} → {status}", flush=True)
        baseline_pg = int(seedmod.psql("SELECT count(*) FROM skills WHERE status != 'retired'") or 0)
        drain = drain_until_empty(scope_dir, log_dir, args.drain_timeout)
        gated = gate_drafts(scope_dir)
        print(f"  [seed] drained in {drain['passes']} pass(es); gated {len(gated)} skill(s):", flush=True)
        skill_source = {}
        for g in gated:
            src_iid = _iid_from_session(g["source_session_id"], args.run_id)
            skill_source[g["name"]] = src_iid
            print(f"        + {g['name']}  (from {src_iid or g['source_session_id'] or '?'})", flush=True)
        seedmod.reconcile_and_rebuild(wait_s=args.reconcile_wait, expected_total=baseline_pg + len(gated))
        ckpt["seeding"] = {"ingested": ingested, "gated_skills": gated,
                           "skill_source": skill_source, "drain_passes": drain["passes"]}
        _ckpt_save(ckpt_path, ckpt)
    else:
        print(f"\n── SEED (checkpointed: {len(ckpt['seeding']['gated_skills'])} skills gated) ──", flush=True)
    skill_source = ckpt["seeding"]["skill_source"]
    on_disk = list((scope_dir / ".skills").rglob("SKILL.md"))
    if not on_disk:
        print(f"ERROR: seeded scope {scope_dir}/.skills has NO gated skills on disk — cannot run "
              f"Round 2 against an empty corpus.", file=sys.stderr)
        return 2

    # ── ROUND 2 — solve the SAME N with the layer ON (self-seeded scope) ───────
    print("\n── ROUND 2 (TREAT — self-seeded layer) ──", flush=True)
    for iid in instances:
        if iid in ckpt["round2"]:
            print(f"  [R2 TREAT] {iid} (checkpointed: resolved={ckpt['round2'][iid]['resolved']})", flush=True)
            continue
        out = solve_guarded(iid, "treat", problems[iid], args,
                            run_id_prefix=f"{args.run_id}-r2-{iid}".replace("__", "-"),
                            scope_override=scope_dir)
        seed_hits = out["attribution"]["seed_hits"]
        own = [n for n in seed_hits if skill_source.get(n) == iid]
        cross = [n for n in seed_hits if skill_source.get(n) not in (iid, None)]
        e = out["efficiency"] or {}
        ckpt["round2"][iid] = {"instance_id": iid, "resolved": out["resolved"],
                               "efficiency": out["efficiency"], "empty_patch": out["empty_patch"],
                               "seed_hits": seed_hits, "own_skill_hits": own, "cross_skill_hits": cross,
                               "status": out["attribution"]["status"]}
        _ckpt_save(ckpt_path, ckpt)
        print(f"  [R2 TREAT] {iid} resolved={out['resolved']} turns={e.get('num_turns')} "
              f"out_tok={e.get('output_tokens')} cost=${e.get('total_cost_usd')} "
              f"seed_hits={seed_hits} (own={own} cross={cross})", flush=True)

    # ── AGGREGATE — X vs Y on resolved-rate (McNemar) + efficiency ────────────
    round1 = [ckpt["round1"][iid] for iid in instances]
    round2 = [ckpt["round2"][iid] for iid in instances]
    ingested = ckpt["seeding"]["ingested"]
    gated = ckpt["seeding"]["gated_skills"]
    drain = {"passes": ckpt["seeding"]["drain_passes"]}
    by_iid_r1 = {r["instance_id"]: r for r in round1}
    by_iid_r2 = {r["instance_id"]: r for r in round2}
    per_instance = [{"instance_id": iid,
                     "off": 1 if by_iid_r1[iid]["resolved"] else 0,
                     "treat": 1 if by_iid_r2[iid]["resolved"] else 0} for iid in instances]
    efficiency_rows = [{"instance_id": iid,
                        "off": {**(by_iid_r1[iid]["efficiency"] or {}),
                                "resolved": bool(by_iid_r1[iid]["resolved"])},
                        "treat": {**(by_iid_r2[iid]["efficiency"] or {}),
                                  "resolved": bool(by_iid_r2[iid]["resolved"])}} for iid in instances]

    x_rate = sum(p["off"] for p in per_instance) / len(per_instance)
    y_rate = sum(p["treat"] for p in per_instance) / len(per_instance)
    mcnemar = eff.mcnemar_treat_vs_off(per_instance)
    efficiency = eff.aggregate_efficiency(efficiency_rows, resolved_only=True,
                                          iterations=args.bootstrap_iterations, seed=args.bootstrap_seed)
    treat_seed_injected = sum(1 for r in round2 if r["seed_hits"])

    report = {
        "run_id": args.run_id, "design": "self-seed-same-set", "server_url": SERVER,
        "resumed_from": args.resume_round2,
        "instances": instances, "n": len(instances), "model": args.model,
        "max_turns": args.max_turns, "scope": str(scope_dir),
        "seeding": {"ingested": ingested, "gated_skills": gated,
                    "skill_source": skill_source, "drain_passes": drain["passes"]},
        "round1_off": round1, "round2_treat": round2,
        "per_instance_resolved": per_instance,
        "scores": {"X_off_resolved_rate": x_rate, "Y_treat_resolved_rate": y_rate,
                   "delta_Y_minus_X": y_rate - x_rate},
        "mcnemar_treat_vs_off": mcnemar,
        "efficiency_aggregate": efficiency,
        "treat_seed_injected_count": treat_seed_injected,
    }
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    out_path = REPORT_DIR / f"selfseed_{args.run_id}.json"
    out_path.write_text(json.dumps(report, indent=2) + "\n")

    print(f"\n[report] {out_path}", flush=True)
    print(f"SCORES: X(OFF round1)={x_rate:.3f}  Y(TREAT round2)={y_rate:.3f}  "
          f"Δ(Y−X)={y_rate - x_rate:+.3f}", flush=True)
    print(f"McNemar TREAT-vs-OFF: gained(off-fail→treat-pass)={mcnemar['treat_resolved_off_not']} "
          f"lost={mcnemar['off_resolved_treat_not']} p={mcnemar['p_value']:.3f}", flush=True)
    print(f"TREAT injected a seed on {treat_seed_injected}/{len(instances)} instances", flush=True)
    print("EFFICIENCY (TREAT−OFF, resolved-by-both; negative ⇒ cheaper on the re-run):", flush=True)
    for m, d in efficiency["metrics"].items():
        ci = d["bootstrap_ci_mean_delta"]
        print(f"  {m}: n={d['n_pairs']} mean_delta={d['mean_delta_treat_minus_off']} "
              f"(TREAT-cheaper {d['treat_cheaper_count']}/{d['n_pairs']}, sign p={d['sign_test_p']:.3f}, "
              f"CI=[{ci['ci_lo']}, {ci['ci_hi']}])", flush=True)
    return 0


# ── self-test (pure logic — no docker, no solves, no model calls) ─────────────

def _self_test() -> int:
    print("=== t15_selfseed_loop self-test ===")
    failures = 0

    def _assert(cond: bool, label: str, detail: str = "") -> bool:
        print(f"  {'PASS' if cond else 'FAIL'}  {label}{f'  [{detail}]' if detail else ''}")
        return cond

    enc = encode_cwd(Path("/tmp/t15-swebench/solve/django__django-15789__off"))
    failures += 0 if _assert(enc == "-tmp-t15-swebench-solve-django--django-15789--off",
                             "encode_cwd matches claude's dir rule", enc) else 1

    failures += 0 if _assert(
        _iid_from_session("rid-off-django__django-1", "rid") == "django__django-1"
        and _iid_from_session("rid-off-x", "other") is None,
        "iid recovered from ingest session id") else 1

    import tempfile
    with tempfile.TemporaryDirectory() as td:
        # dir name (slug) deliberately differs from the frontmatter name (the bug
        # the self-seed smoke caught): gate must key by the canonical frontmatter name.
        sk = Path(td) / ".skills" / "i-want-json-of-preference"
        sk.mkdir(parents=True)
        (sk / "SKILL.md.pending").write_text(
            "---\nname: I want JSON of (preference)\nsource_session_id: rid-off-django__django-1\n---\n# body\n")
        gated = gate_drafts(Path(td))
        renamed_ok = (sk / "SKILL.md").exists() and not (sk / "SKILL.md.pending").exists()
        failures += 0 if _assert(
            renamed_ok and gated == [{"name": "I want JSON of (preference)",
                                      "dir": "i-want-json-of-preference",
                                      "source_session_id": "rid-off-django__django-1"}],
            "gate keys by canonical frontmatter name, not slug dir", str(gated)) else 1
        # and seed_skill_names (runner) must return the SAME canonical name so seed_hits match
        names = runner.seed_skill_names(Path(td))
        failures += 0 if _assert(names == {"I want JSON of (preference)"},
                                 "seed_skill_names returns frontmatter name (seed_hit parity)",
                                 str(names)) else 1

    # invalid-solve detector — ONLY a true crash / explicit auth failure is invalid;
    # a max-turns cap-hit (even an empty patch) is a VALID legit not-resolved.
    inv_none = _solve_is_invalid({"efficiency": None})                                    # crash/killed
    inv_auth_status = _solve_is_invalid({"efficiency": {"is_error": True, "api_error_status": "401"},
                                         "empty_patch": True})                            # 401 instant
    inv_auth_text = _solve_is_invalid({"efficiency": {"is_error": True}, "empty_patch": False,
                                       "result_text": "Failed to authenticate. API Error: 401 Invalid "
                                       "authentication credentials"})                     # auth death mid-solve
    # the django-15738 case: 40 turns, terminal_reason max_turns, EMPTY patch, no auth → VALID (not-resolved)
    valid_maxturns_empty = _solve_is_invalid({
        "efficiency": {"is_error": True, "num_turns": 41, "terminal_reason": "max_turns",
                       "api_error_status": None},
        "empty_patch": True, "result_text": "I've been investigating the field rendering..."})
    valid_clean_fail = _solve_is_invalid({"efficiency": {"is_error": False, "num_turns": 46},
                                          "empty_patch": False})                          # legit not-resolved
    # guard against false-positive: "401" appearing in PROSE (problem about HTTP 401) is NOT an auth failure
    valid_401_in_prose = _solve_is_invalid({"efficiency": {"is_error": False, "api_error_status": None},
                                            "result_text": "fix the view returning a 401 response", "empty_patch": False})
    failures += 0 if _assert(
        inv_none and inv_auth_status and inv_auth_text
        and not valid_maxturns_empty and not valid_clean_fail and not valid_401_in_prose,
        "invalid detector: crash/auth invalid; max-turns-empty-patch + clean-fail + 401-in-prose VALID",
        f"none={inv_none} auth_status={inv_auth_status} auth_text={inv_auth_text} "
        f"maxturns_empty_valid={not valid_maxturns_empty} cleanfail_valid={not valid_clean_fail} "
        f"prose401_valid={not valid_401_in_prose}") else 1

    # checkpoint save/load round-trip + resume skips completed instances
    with tempfile.TemporaryDirectory() as td:
        cp = Path(td) / "checkpoint.json"
        empty = _ckpt_load(cp)
        empty["round1"]["django__django-1"] = {"resolved": True}
        _ckpt_save(cp, empty)
        reloaded = _ckpt_load(cp)
        failures += 0 if _assert(
            reloaded["round1"]["django__django-1"]["resolved"] is True and reloaded["seeding"] is None,
            "checkpoint save/load round-trip preserves progress") else 1

    print(f"\n{'=' * 40}")
    print("ALL TESTS PASSED" if failures == 0 else f"{failures} TEST(S) FAILED")
    return 0 if failures == 0 else 1


def main() -> None:
    ap = argparse.ArgumentParser(description="T15 self-seeding same-set compounding loop")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--run-id", default="t15-selfseed")
    ap.add_argument("--instances", default="")
    ap.add_argument("--scope-name", default="swebench-django-selfseed")
    ap.add_argument("--reuse-scope", action="store_true")
    ap.add_argument("--resume-round2", default=None,
                    help="path to a prior selfseed report — skip Round 1 + seeding and re-run ONLY "
                         "Round 2 against the already-seeded scope (recovery after a tainted Round 2).")
    ap.add_argument("--model", default="sonnet")
    ap.add_argument("--max-turns", dest="max_turns", type=int, default=40)
    ap.add_argument("--solve-timeout", dest="solve_timeout", type=int, default=2400)
    ap.add_argument("--verify-timeout", dest="verify_timeout", type=int, default=1800)
    ap.add_argument("--drain-timeout", dest="drain_timeout", type=int, default=3600)
    ap.add_argument("--reconcile-wait", dest="reconcile_wait", type=int, default=120)
    ap.add_argument("--solve-retries", dest="solve_retries", type=int, default=3,
                    help="retries for an INVALID solve (claude auth 401/crash → empty turns=1) before "
                         "failing loud + checkpointing (never banks a fake not-resolved).")
    ap.add_argument("--retry-backoff-s", dest="retry_backoff_s", type=int, default=20,
                    help="seconds to wait between invalid-solve retries (lets a transient auth blip clear).")
    ap.add_argument("--bootstrap-iterations", dest="bootstrap_iterations", type=int,
                    default=eff.T15_BOOTSTRAP_ITERATIONS)
    ap.add_argument("--bootstrap-seed", dest="bootstrap_seed", type=int, default=eff.T15_BOOTSTRAP_SEED)
    args = ap.parse_args()
    if args.self_test:
        sys.exit(_self_test())
    try:
        sys.exit(run_loop(args))
    except SolveInvalid as exc:
        # Progress IS checkpointed (logs/t15-selfseed/<run-id>/checkpoint.json); re-running
        # the same command resumes and re-solves only what's missing. Distinct exit code 3.
        print(f"\n*** RUN PAUSED (invalid solve — likely claude auth) ***\n{exc}", file=sys.stderr)
        sys.exit(3)


if __name__ == "__main__":
    main()
