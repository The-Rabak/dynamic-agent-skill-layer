#!/usr/bin/env python3
"""T23 — CL acquisition BAND orchestrator (Steps 0–5 per context, unattended, resumable).

Drives the full 8-context T14 acquisition band end-to-end over the REAL live stack, fully automated
under the LOCKED auto-gate amendment (gate_mode=auto-accept-all, clband-* scopes ONLY). Every heavy
action (claude solves, extraction, rebuilds) is serialized here — this is the singleton heavy driver.

TWO PASSES (so cross-scope PLACEBO is trivial and context #1 is the live canary for the full
teach->extract->gate->retrievable path):

  PASS 1 — build each context's isolated scope:
    Step 0  OFF pre-gate     : each measured sibling solved bare (no layer); must FAIL to qualify
                               (discrimination). A sibling OFF passes -> dropped. A context losing ALL
                               siblings -> substitute the next alternate (the ONLY substitution path).
    Step 1  Session A teach   : genuine claude-code solve on the teach workspace -> transcript.
    Step 2  Pipeline (extract): clband_extract.py (claude-code provider, teach-doc delivery) -> .pending.
    Step 3  Fidelity gate     : operative sentinels must appear across the drafts. RED ->
                               INSTRUMENT-FAILURE(extraction): no efficacy point, CONTINUE the band.
            Auto-gate         : (green only) scope-guarded rename .pending->SKILL.md in the volume +
                               scope rebuild + poll until the live mcp-server retrieves the skills.

  PASS 2 — measure (per surviving sibling):
    Step 4  Session B paired  : ON (compile_context from the context's OWN clband scope, focused
                               inject-query) / OFF (reuse the Step-0 pre-gate bare solve) / PLACEBO
                               (compile_context from a DIFFERENT context's scope, matched mass).
    Step 5  Verifier          : deterministic core decides pass/fail per arm. ON loss with the
                               rule-bearing skill injected -> INSTRUMENT-FAILURE(injection/obedience).

Unattended policy (LOCKED): harness-level breakage (crash, /health failure, dataset drift, scope-guard
trip) -> STOP, preserve checkpoint, write a stop report. Per-context INSTRUMENT-FAILURE -> record +
CONTINUE. NEVER delete outputs; drain-until-done; no arbitrary time/token caps (stuck-detectors only).

Usage:
  run_band.py --plan                # print the resolved roster + per-context instruments; no runs
  run_band.py [--contexts a,b,...]  # run the band (default: all 8 full contexts), resumable
  run_band.py --closeout            # remove all clband scopes + rebuild + re-probe the 262 dogfood
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
import traceback
import urllib.request
from pathlib import Path

CLBAND = Path(__file__).resolve().parent
ROOT = CLBAND.parents[3]
SCRIPTS = ROOT / "scripts"
for p in (str(CLBAND), str(SCRIPTS)):
    if p not in sys.path:
        sys.path.insert(0, p)

import efficacy_ab            # noqa: E402  validated solve + verifier + verdict primitives
import scope_rebuild as sr    # noqa: E402  volume + auto-gate + retrieval-readiness (canary-proven)

BAND_DIR = ROOT / "tests/e2e/reports/efficacy/clband-band"
CHECKPOINT = BAND_DIR / "checkpoint.json"
MANIFEST = json.loads((CLBAND / "manifest.json").read_text())
MCP_URL = "http://127.0.0.1:3001"
SOLVER_MODEL = "sonnet"
SOLVER_CHECKPOINT = "claude-code 2.1.175, --model sonnet"
DATASET_SHA = MANIFEST["_dataset"]["pinned_sha"]
GATE_MODE = "auto-accept-all (clband-* scopes only)"

FULL_CONTEXTS = [c["name"] for c in MANIFEST["contexts"] if c["role"] == "full"]
ALTERNATES = [c["name"] for c in MANIFEST["contexts"] if c["role"] == "alternate"]
SHORT_BY_NAME = {c["name"]: c["short"] for c in MANIFEST["contexts"]}


# ── checkpoint I/O ─────────────────────────────────────────────────────────────

def load_checkpoint() -> dict:
    if CHECKPOINT.exists():
        return json.loads(CHECKPOINT.read_text())
    return {"roster": [], "contexts": {}, "stop": None,
            "solver_checkpoint": SOLVER_CHECKPOINT, "dataset_sha": DATASET_SHA, "gate_mode": GATE_MODE}


def save_checkpoint(ck: dict) -> None:
    BAND_DIR.mkdir(parents=True, exist_ok=True)
    CHECKPOINT.write_text(json.dumps(ck, indent=2))


class HarnessStop(Exception):
    """A harness-level failure that must STOP the band and preserve state for a morning resume."""


# ── live-stack guards ──────────────────────────────────────────────────────────

def health_ok() -> bool:
    try:
        with urllib.request.urlopen(f"{MCP_URL}/health", timeout=10) as r:
            return json.loads(r.read()).get("healthy") is True
    except Exception:
        return False


def require_health(where: str) -> None:
    if not health_ok():
        raise HarnessStop(f"/health not healthy before {where}")


def require_dataset_pin() -> None:
    """Fail loud if the live dataset drifted from the pinned sha (fetch script re-verifies)."""
    fetch = SCRIPTS / "fetch_clband_contexts.py"
    if not fetch.exists():
        return
    # The contexts are already materialized + sentinel-verified; re-assert the pin is unchanged.
    if MANIFEST["_dataset"]["pinned_sha"] != DATASET_SHA:
        raise HarnessStop("dataset sha drift detected in manifest")


# ── instruments ────────────────────────────────────────────────────────────────

def load_instruments(name: str) -> dict:
    meta = CLBAND / "instruments" / f"{name}.json"
    if not meta.exists():
        raise HarnessStop(f"missing instruments/{name}.json (Unit A must author it before the run)")
    return json.loads(meta.read_text())


def task_spec(slug: str) -> dict:
    p = CLBAND / "tasks" / f"{slug}.json"
    return json.loads(p.read_text())


# ── solve + persist (no output is ever deleted) ────────────────────────────────

def _persist_workspace(ws: Path, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    for fn in ("solution.md", "prompt.used.txt"):
        src = ws / fn
        if src.exists():
            shutil.copy2(src, dest / fn)


def run_solve(prompt: str, dest: Path, max_turns: int = 50, timeout_s: int = 1200) -> dict:
    """One bare claude-code solve in a fresh scratch workspace; persist solution.md; run nothing else.

    Returns {exit_code, timed_out, elapsed_s, workspace}. The workspace is kept under dest so the
    actual answer is never deleted (standing rule)."""
    import tempfile
    dest.mkdir(parents=True, exist_ok=True)
    ws = Path(tempfile.mkdtemp(prefix="clband-solve-"))
    (ws / "prompt.used.txt").write_text(prompt)
    t0 = time.time()
    res = efficacy_ab.run_claude_solve(prompt=prompt, workspace_dir=ws, model=SOLVER_MODEL,
                                       max_turns=max_turns, timeout_s=timeout_s)
    res["elapsed_s"] = round(time.time() - t0, 1)
    _persist_workspace(ws, dest)
    res["workspace"] = str(dest)
    shutil.rmtree(ws, ignore_errors=True)
    return res


def verify(slug: str, workspace_dir: Path) -> dict:
    spec = task_spec(slug)
    return efficacy_ab.run_verifier(str(ROOT / spec["verifier"]["command"]), workspace_dir)


# ── PASS 1: Step 0 OFF pre-gate ────────────────────────────────────────────────

def off_pregate(name: str, ctx_dir: Path) -> dict:
    """Solve each measured sibling bare (OFF). A sibling OFF WINS -> non-discriminating -> dropped.
    Returns {sibling_results: {slug: {off_outcome, elapsed_s, ...}}, surviving: [slug,...]}."""
    instr = load_instruments(name)
    out = {"sibling_results": {}, "surviving": []}
    for sib in instr["measured_siblings"]:
        slug = sib["slug"]
        spec = task_spec(slug)
        dest = ctx_dir / "offpregate" / slug
        print(f"  [Step0 OFF] {slug} ...", flush=True)
        solve = run_solve(spec["prompt"], dest)
        v = verify(slug, Path(dest))
        off_win = v["outcome"] == "win"
        out["sibling_results"][slug] = {
            "off_outcome": v["outcome"], "verifier_reason": v["verifier_reason"],
            "solve_exit": solve["exit_code"], "elapsed_s": solve["elapsed_s"],
            "discriminating": not off_win,
        }
        (dest / "off_result.json").write_text(json.dumps(out["sibling_results"][slug], indent=2))
        print(f"    OFF={v['outcome']} discriminating={not off_win}  {v['verifier_reason'][:80]}", flush=True)
        if not off_win:
            out["surviving"].append(slug)
    return out


# ── PASS 1: Step 1 teach -> Step 2 extract -> Step 3 fidelity + auto-gate ───────

def teach_session(name: str, ctx_dir: Path) -> Path:
    """Run the genuine Session A teach solve in the authored teach workspace; return the transcript."""
    teach_ws = CLBAND / "teach" / name
    if not (teach_ws / "prompt.txt").exists():
        raise HarnessStop(f"missing teach/{name}/prompt.txt (Unit A)")
    prompt = (teach_ws / "prompt.txt").read_text()
    proj = _munged_project_dir(teach_ws)
    before = set(proj.glob("*.jsonl")) if proj.exists() else set()
    print(f"  [Step1 teach] {name} (cwd={teach_ws}) ...", flush=True)
    res = efficacy_ab.run_claude_solve(prompt=prompt, workspace_dir=teach_ws, model=SOLVER_MODEL,
                                       max_turns=60, timeout_s=1800)
    print(f"    teach solve rc={res['exit_code']} timed_out={res['timed_out']}", flush=True)
    after = set(proj.glob("*.jsonl")) if proj.exists() else set()
    new = sorted(after - before, key=lambda p: p.stat().st_mtime)
    if not new and proj.exists():
        new = sorted(proj.glob("*.jsonl"), key=lambda p: p.stat().st_mtime)[-1:]
    if not new:
        raise HarnessStop(f"teach session for {name} produced no transcript under {proj}")
    tr = new[-1]
    dest = ctx_dir / "transcript.jsonl"
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(tr, dest)
    sol = teach_ws / "solution.md"
    if sol.exists():
        shutil.copy2(sol, ctx_dir / "teach_solution.md")
    print(f"    transcript -> {dest} ({dest.stat().st_size} bytes)", flush=True)
    return dest


def _munged_project_dir(ws: Path) -> Path:
    enc = str(ws.resolve()).replace("/", "-").replace(".", "-")
    return Path.home() / ".claude" / "projects" / enc


def extract_and_gate(name: str, transcript: Path, ctx_dir: Path) -> dict:
    """Step 2 extraction (host) -> Step 3 fidelity gate -> auto-gate (green only). Returns a dict
    with fidelity outcome + accepted count + retrievable flag."""
    short = SHORT_BY_NAME[name]
    host_scope = ctx_dir / "scope"
    if host_scope.exists():
        shutil.rmtree(host_scope)
    host_scope.mkdir(parents=True)
    (host_scope / ".git").mkdir()
    (host_scope / ".git" / "HEAD").write_text("ref: refs/heads/main\n")

    print(f"  [Step2 extract] {name} ...", flush=True)
    rc = subprocess.run([sys.executable, str(CLBAND / "clband_extract.py"), name,
                         str(transcript), str(host_scope)],
                        capture_output=True, text=True)
    (ctx_dir / "extract.log").write_text(rc.stdout + "\n--- STDERR ---\n" + rc.stderr)
    if rc.returncode != 0:
        raise HarnessStop(f"extraction failed for {name} (rc={rc.returncode}); see extract.log")
    n_drafts = len(list((host_scope / ".skills").rglob("*.pending"))) if (host_scope / ".skills").exists() else 0
    print(f"    extracted {n_drafts} .pending draft(s)", flush=True)

    # Step 3 — fidelity gate (operative sentinels across the drafts).
    print(f"  [Step3 fidelity] {name} ...", flush=True)
    fg = subprocess.run(["bash", str(CLBAND / "fidelity_gate.sh"), short, str(host_scope)],
                        capture_output=True, text=True)
    (ctx_dir / "fidelity_gate.txt").write_text(fg.stdout + "\n--- STDERR ---\n" + fg.stderr)
    fidelity_pass = fg.returncode == 0
    print(f"    fidelity {'PASS' if fidelity_pass else 'FAIL (INSTRUMENT-FAILURE extraction)'} (exit {fg.returncode})",
          flush=True)
    result = {"n_drafts": n_drafts, "fidelity_pass": fidelity_pass,
              "fidelity_exit": fg.returncode, "accepted": 0, "retrievable": False}
    if not fidelity_pass:
        return result  # INSTRUMENT-FAILURE(extraction): no efficacy point; band CONTINUES.

    # Auto-gate: place into the volume, scope-guarded rename, rebuild, poll until retrievable.
    placed = sr.place_pending(name, host_scope / ".skills")
    accepted = sr.accept_all(name)   # the AUTO-GATE — scope guard fails loud on any non-clband path
    result["accepted"] = len(accepted)
    print(f"    auto-gate: placed {placed}, accepted {len(accepted)} (scope clband-{name})", flush=True)
    (ctx_dir / "auto_gate.json").write_text(json.dumps(
        {"gate_mode": GATE_MODE, "scope": sr.scope_path(name), "placed": placed,
         "accepted_paths": accepted}, indent=2))
    require_health("retrieval readiness poll")
    instr = load_instruments(name)
    query = instr["measured_siblings"][0]["summary"]
    try:
        res = sr.wait_retrievable(name, query, timeout_s=420)
    except TimeoutError as e:
        raise HarnessStop(f"{name}: accepted skills never became retrievable — {e}")
    result["retrievable"] = bool(res.get("skill_names"))
    result["retrieval_probe"] = {"skill_names": res.get("skill_names"), "status": res.get("raw", {}).get("status")}
    print(f"    retrievable: {result['retrievable']} {res.get('skill_names')}", flush=True)
    return result


# ── PASS 2: Step 4 ON + PLACEBO (OFF reused from pre-gate) ──────────────────────

def _inject(repo_scope_name: str, query: str, tag: str) -> dict:
    cc = sr.probe(sr.scope_path(repo_scope_name), query, session_tag=tag)
    return {"injected_text": cc.get("additional_context", "") or "",
            "skill_names": cc.get("skill_names", []) or [],
            "skill_ids": cc.get("skill_ids", []) or [],
            "status": cc.get("raw", {}).get("status")}


def session_b(name: str, slug: str, placebo_donor: str, ctx_dir: Path, off_outcome: str) -> dict:
    """Paired ON/PLACEBO solve for one surviving sibling (OFF reused from the Step-0 pre-gate)."""
    spec = task_spec(slug)
    summary = spec["invented_rule"]["summary"]
    base_prompt = spec["prompt"]
    row = {"slug": slug, "off_outcome": off_outcome, "placebo_donor": placebo_donor}

    # ON — inject from the context's OWN clband scope (focused inject-query = the rule summary).
    on_inj = _inject(name, summary, f"on-{slug}")
    on_prompt = efficacy_ab._compose_arm_prompt(base_prompt, on_inj["injected_text"])
    on_dest = ctx_dir / "sessionB" / slug / "on"
    on_solve = run_solve(on_prompt, on_dest)
    on_v = verify(slug, Path(on_dest))
    row["on_outcome"] = on_v["outcome"]
    row["on_attribution"] = {"skill_names": on_inj["skill_names"], "skill_ids": on_inj["skill_ids"],
                             "injected_mass_chars": len(on_inj["injected_text"]), "status": on_inj["status"]}
    row["on_elapsed_s"] = on_solve["elapsed_s"]
    # In an isolated clband scope, ANY returned skill IS the taught rule -> clean obedience attribution.
    rule_injected = len(on_inj["skill_names"]) > 0
    row["rule_injected"] = rule_injected
    row["instrument_failure_injection"] = (on_v["outcome"] == "loss") and rule_injected

    # PLACEBO — inject from a DIFFERENT context's scope at matched mass (cross-scope control).
    pl_inj = _inject(placebo_donor, load_instruments(placebo_donor)["measured_siblings"][0]["summary"],
                     f"placebo-{slug}")
    pl_prompt = efficacy_ab._compose_arm_prompt(base_prompt, pl_inj["injected_text"])
    pl_dest = ctx_dir / "sessionB" / slug / "placebo"
    pl_solve = run_solve(pl_prompt, pl_dest)
    pl_v = verify(slug, Path(pl_dest))
    row["placebo_outcome"] = pl_v["outcome"]
    row["placebo_attribution"] = {"donor": placebo_donor, "skill_names": pl_inj["skill_names"],
                                  "injected_mass_chars": len(pl_inj["injected_text"])}
    row["placebo_elapsed_s"] = pl_solve["elapsed_s"]
    (ctx_dir / "sessionB" / slug / "result.json").write_text(json.dumps(row, indent=2))
    print(f"  [Step4 {slug}] ON={row['on_outcome']} OFF={off_outcome} PLACEBO={row['placebo_outcome']} "
          f"rule_injected={rule_injected} IF(inj)={row['instrument_failure_injection']}", flush=True)
    return row


# ── band driver ────────────────────────────────────────────────────────────────

def resolve_roster(requested: list[str] | None) -> list[str]:
    return list(requested) if requested else list(FULL_CONTEXTS)


def _has_instruments(name: str) -> bool:
    return (CLBAND / "instruments" / f"{name}.json").exists()


def run_pass1_context(name: str, ck: dict) -> str:
    """Build one context's scope through Steps 0–3. Returns its status; updates checkpoint.

    Statuses: 'built' | 'instrument_failure_extraction' | 'off_pregate_failed'."""
    ctx_dir = BAND_DIR / name
    ctx_dir.mkdir(parents=True, exist_ok=True)
    cstate = ck["contexts"].setdefault(name, {})
    require_health(f"pass1 {name}")

    # Step 0
    if "off_pregate" not in cstate:
        cstate["off_pregate"] = off_pregate(name, ctx_dir)
        save_checkpoint(ck)
    surviving = cstate["off_pregate"]["surviving"]
    if not surviving:
        cstate["status"] = "off_pregate_failed"
        save_checkpoint(ck)
        return "off_pregate_failed"

    # Step 1
    if "transcript" not in cstate:
        cstate["transcript"] = str(teach_session(name, ctx_dir))
        save_checkpoint(ck)

    # Step 2 + 3 + auto-gate
    if "pipeline" not in cstate:
        cstate["pipeline"] = extract_and_gate(name, Path(cstate["transcript"]), ctx_dir)
        save_checkpoint(ck)
    if not cstate["pipeline"]["fidelity_pass"]:
        cstate["status"] = "instrument_failure_extraction"
        save_checkpoint(ck)
        return "instrument_failure_extraction"
    if not cstate["pipeline"].get("retrievable"):
        # Built + accepted but the server never surfaced the skills -> harness issue, STOP.
        raise HarnessStop(f"{name}: accepted skills not retrievable (rebuild/reload did not surface them)")
    cstate["status"] = "built"
    cstate["surviving"] = surviving
    save_checkpoint(ck)
    return "built"


def run_band(requested: list[str] | None) -> int:
    BAND_DIR.mkdir(parents=True, exist_ok=True)
    ck = load_checkpoint()
    require_dataset_pin()
    roster = resolve_roster(requested)
    ck["roster"] = roster
    save_checkpoint(ck)
    alt_idx = 0

    # ── PASS 1 — build scopes (context #1 is the live canary) ──
    built: list[str] = []
    i = 0
    while i < len(roster):
        name = roster[i]
        cstate = ck["contexts"].get(name, {})
        if cstate.get("status") == "built":
            built.append(name); i += 1; continue
        if cstate.get("status") in ("instrument_failure_extraction",):
            i += 1; continue
        try:
            status = run_pass1_context(name, ck)
        except HarnessStop as e:
            ck["stop"] = {"reason": str(e), "where": f"pass1:{name}", "ts": time.time()}
            save_checkpoint(ck)
            _write_stop_report(ck)
            print(f"\n*** HARNESS STOP: {e} — checkpoint preserved for morning resume ***", flush=True)
            return 2
        if status == "off_pregate_failed":
            # Substitution path: swap in the next alternate that HAS committed instruments.
            # (Unit A instrumented the 8 full contexts only; an un-instrumented alternate cannot
            # be measured, so the band continues with fewer contexts rather than STOPPING — N is
            # still well-powered at up to 12 measured siblings.)
            sub = None
            while alt_idx < len(ALTERNATES):
                cand = ALTERNATES[alt_idx]; alt_idx += 1
                if _has_instruments(cand):
                    sub = cand; break
                print(f"  [substitute] alternate {cand} has no committed instruments — skipping", flush=True)
            if sub:
                print(f"  [substitute] {name} lost all siblings to OFF pre-gate -> alternate {sub}", flush=True)
                roster[i] = sub
                ck["roster"] = roster
                ck["contexts"][name]["substituted_by"] = sub
                save_checkpoint(ck)
                continue  # retry the same slot with the alternate
            print(f"  [substitute] {name} failed OFF pre-gate; no instrumented alternate -> "
                  f"band continues with fewer contexts", flush=True)
            i += 1; continue
        if status == "built":
            built.append(name)
        i += 1

    # ── PASS 2 — measure surviving siblings (cross-scope placebo) ──
    rows: list[dict] = []
    for idx, name in enumerate(built):
        cstate = ck["contexts"][name]
        donor = built[(idx + 1) % len(built)] if len(built) > 1 else name
        ctx_dir = BAND_DIR / name
        cstate.setdefault("sessionB", {})
        for slug in cstate["surviving"]:
            if slug in cstate["sessionB"]:
                rows.append(cstate["sessionB"][slug]); continue
            off_outcome = cstate["off_pregate"]["sibling_results"][slug]["off_outcome"]
            try:
                row = session_b(name, slug, donor, ctx_dir, off_outcome)
            except HarnessStop as e:
                ck["stop"] = {"reason": str(e), "where": f"pass2:{name}:{slug}", "ts": time.time()}
                save_checkpoint(ck); _write_stop_report(ck)
                print(f"\n*** HARNESS STOP: {e} ***", flush=True)
                return 2
            cstate["sessionB"][slug] = row
            rows.append(row)
            save_checkpoint(ck)

    _write_band_results(ck, built, rows)
    print(f"\n=== BAND COMPLETE: {len(built)} contexts built, {len(rows)} measured siblings ===", flush=True)
    return 0


def _write_band_results(ck: dict, built: list[str], rows: list[dict]) -> None:
    """Aggregate the paired rows into the verdict (reusing efficacy_ab) + persist band_results.json."""
    per_task = [{
        "task_id": r["slug"], "on_outcome": r["on_outcome"], "off_outcome": r["off_outcome"],
        "placebo_outcome": r["placebo_outcome"], "catastrophic_regression": False,
        "attribution": [r.get("on_attribution", {})],
        "instrument_failure": r.get("instrument_failure_injection", False),
    } for r in rows]
    verdict = efficacy_ab.classify_efficacy_verdict(per_task) if per_task else {"verdict": "NO-DATA"}
    extraction_failures = [n for n, c in ck["contexts"].items()
                           if c.get("status") == "instrument_failure_extraction"]
    out = {
        "gate_mode": GATE_MODE, "solver_checkpoint": SOLVER_CHECKPOINT, "dataset_sha": DATASET_SHA,
        "pre_registered_criterion": efficacy_ab.PRE_REGISTERED_CRITERION,
        "built_contexts": built, "n_measured_siblings": len(rows),
        "instrument_failure_extraction": extraction_failures,
        "verdict": verdict, "rows": rows,
    }
    (BAND_DIR / "band_results.json").write_text(json.dumps(out, indent=2))
    print(f"  band_results.json -> verdict={verdict.get('verdict')} "
          f"(ON {verdict.get('on_wins')} / OFF {verdict.get('off_wins')} / ties {verdict.get('n_ties')})",
          flush=True)


def _write_stop_report(ck: dict) -> None:
    lines = ["# clband band — HARNESS STOP report", "",
             f"Stop: {ck.get('stop')}", "",
             "Per-context status at stop:"]
    for n, c in ck["contexts"].items():
        lines.append(f"- {n}: {c.get('status', 'pending')}")
    lines += ["", "Resume: re-run `run_band.py` — completed (context, step) work is skipped via checkpoint.json."]
    (BAND_DIR / "STOP_REPORT.md").write_text("\n".join(lines))


# ── closeout: restore the pristine 262 dogfood corpus ──────────────────────────

def closeout() -> int:
    print("=== clband closeout: removing all clband scopes + restoring 262 ===", flush=True)
    scopes = sr.list_clband_scopes()
    print(f"clband scopes present: {scopes}", flush=True)
    for dirname in scopes:
        name = dirname[len(sr.CLBAND_PREFIX):]
        sr.remove_scope(name)
        print(f"  removed {dirname}", flush=True)
    # Poll one removed scope to absence as the rebuild signal, then assert the 262.
    if scopes:
        any_name = scopes[0][len(sr.CLBAND_PREFIX):]
        try:
            sr.wait_absent(any_name, "any query", timeout_s=240)
        except TimeoutError as e:
            print(f"WARNING: {e}", flush=True)
    total = sr.project_total()
    dogfood = sr.dogfood_total()
    print(f"project_total={total} dogfood_total={dogfood} (expect 262 / 262)", flush=True)
    ok = (total == 262 and dogfood == 262 and not sr.list_clband_scopes())
    (BAND_DIR / "closeout.json").write_text(json.dumps(
        {"project_total": total, "dogfood_total": dogfood, "clband_scopes_remaining": sr.list_clband_scopes(),
         "restored_262": ok}, indent=2))
    print(f"=== closeout {'OK — 262 restored' if ok else 'NEEDS ATTENTION'} ===", flush=True)
    return 0 if ok else 1


# ── plan (no runs) ─────────────────────────────────────────────────────────────

def plan() -> int:
    print("=== clband band plan ===")
    print(f"gate_mode: {GATE_MODE}")
    print(f"solver: {SOLVER_CHECKPOINT}  dataset_sha: {DATASET_SHA}")
    print(f"full roster ({len(FULL_CONTEXTS)}): {FULL_CONTEXTS}")
    print(f"alternates: {ALTERNATES}")
    missing = []
    for name in FULL_CONTEXTS:
        meta = CLBAND / "instruments" / f"{name}.json"
        if not meta.exists():
            missing.append(name); print(f"  {name}: MISSING instruments/{name}.json"); continue
        m = json.loads(meta.read_text())
        sibs = [s["slug"] for s in m.get("measured_siblings", [])]
        print(f"  {name}: doc={m.get('doc_file')} teach={m.get('teach_sibling_id','?')[:8]} "
              f"siblings={sibs} operative_sentinels={len(m.get('sentinels_operative', []))} "
              f"self_test={m.get('self_test')}")
    if missing:
        print(f"\nMISSING instruments for: {missing}")
        return 1
    return 0


def main() -> None:
    ap = argparse.ArgumentParser(description="T23 CL acquisition band orchestrator (unattended, resumable)")
    ap.add_argument("--plan", action="store_true", help="print resolved roster + instruments; no runs")
    ap.add_argument("--contexts", default=None, help="comma-separated context names (default: all 8 full)")
    ap.add_argument("--closeout", action="store_true", help="remove all clband scopes + restore 262")
    args = ap.parse_args()
    if args.plan:
        sys.exit(plan())
    if args.closeout:
        sys.exit(closeout())
    requested = [c.strip() for c in args.contexts.split(",")] if args.contexts else None
    try:
        sys.exit(run_band(requested))
    except HarnessStop as e:
        print(f"\n*** HARNESS STOP (top-level): {e} ***", flush=True)
        sys.exit(2)
    except Exception:
        traceback.print_exc()
        print("\n*** UNEXPECTED ERROR — checkpoint preserved; resume after diagnosis ***", flush=True)
        sys.exit(3)


if __name__ == "__main__":
    main()
