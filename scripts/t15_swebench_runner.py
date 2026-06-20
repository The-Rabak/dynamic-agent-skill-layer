#!/usr/bin/env python3
"""T15 Phase 2 — the measured SWE-bench compounding runner (no fakes, fail loud).

WHAT this is
------------
Turns the SWE-bench *spike* into the instrumented DiD runner described in todo 284
(pieces A/B/D/E/F). For each held-out TEST instance it drives THREE byte-identical
claude solves that differ only in the skill-context wiring:
  OFF   — no layer, no injection
  CTRL  — harness-mediated compile_context against the FOREIGN-repo seed scope
  TREAT — harness-mediated compile_context against the SAME-repo seed scope
captures the model's /testbed patch, runs the **official SWE-bench oracle**
(`swebench.harness.run_evaluation` → deterministic FAIL_TO_PASS/PASS_TO_PASS),
records per-instance retrieval attribution, and aggregates the difference-of-
differences via `efficacy_ab.aggregate_did_run`.

NO FAKES / FAIL LOUD:
  * resolved bit comes ONLY from the swebench harness report — never inferred.
  * a non-applying / empty patch is recorded as not-resolved, never as resolved.
  * a missing harness report on a non-empty patch is a HARD error (verifier broke).
  * the injection drives the REAL isolated mcp-server over HTTP (default :3002).

ISOLATION: every measured call targets the clean isolated server (`--server-url`,
default http://127.0.0.1:3002) backed by `swebench_t15` — the 277-skill dogfood
corpus on :3001 is never in any arm's pool (plan HARD INVARIANT).

Subcommands:
  verify     — run the oracle on a single (instance, patch-file) pair (testable)
  solve-arm  — solve + extract + verify ONE arm of ONE instance
  run        — full DiD over N TEST instances × 3 arms from the split fixture
  --self-test — pure-logic checks (no docker, no solves, no model calls)
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import efficacy_ab as eff  # noqa: E402  — reuse injection + DiD aggregator (do not rebuild)

REPO_ROOT = _SCRIPTS_DIR.parent
VENV_PY = "/tmp/t15-venv-swebench/bin/python"
DATASET = "princeton-nlp/SWE-bench_Lite"
DEFAULT_SERVER = "http://127.0.0.1:3002"
SPLIT_FIXTURE = REPO_ROOT / "tests/fixtures/t15_swebench_split.json"
T15_PROJECT_ROOT = Path("/tmp/t15-swebench/project")
REPORT_DIR = REPO_ROOT / "tests/e2e/reports/swebench"

# Arm → seed scope directory (the `.skills`-marked project root on the clean
# server). TREAT = same-repo seeds; CTRL = foreign-repo seeds; OFF = no scope.
ARM_SCOPE = {
    "treat": T15_PROJECT_ROOT / "swebench-django",
    "ctrl": T15_PROJECT_ROOT / "swebench-sympy",
    "off": None,
}

# ── instance id / image helpers ───────────────────────────────────────────────

def parse_instance_id(instance_id: str) -> tuple[str, str, str]:
    """`django__django-14999` → (org='django', repo='django', issue='14999')."""
    if "__" not in instance_id or "-" not in instance_id.rsplit("__", 1)[1]:
        raise ValueError(f"malformed instance_id: {instance_id!r} (want <org>__<repo>-<issue>)")
    org, rest = instance_id.split("__", 1)
    repo, issue = rest.rsplit("-", 1)
    return org, repo, issue


def instance_image(instance_id: str) -> str:
    """The SWE-bench instance image tag for an id."""
    org, repo, issue = parse_instance_id(instance_id)
    return f"swebench/sweb.eval.x86_64.{org}_1776_{repo}-{issue}:latest"


def container_name(instance_id: str, arm: str) -> str:
    return f"t15-{arm}-{instance_id}".replace("__", "-")


# ── SWE-bench deterministic verifier (piece A) ────────────────────────────────

def build_prediction_row(instance_id: str, model_name: str, model_patch: str) -> dict[str, str]:
    """One predictions.jsonl row (swebench KEY_* contract)."""
    return {
        "instance_id": instance_id,
        "model_name_or_path": model_name,
        "model_patch": model_patch,
    }


def per_instance_report_path(run_id: str, model_name: str, instance_id: str) -> Path:
    """Location the harness writes its per-instance report.json to (cwd-relative).

    RUN_EVALUATION_LOG_DIR = logs/run_evaluation (relative to the harness CWD,
    which we pin to REPO_ROOT). model dir uses `/`→`__`.
    """
    return (
        REPO_ROOT
        / "logs/run_evaluation"
        / run_id
        / model_name.replace("/", "__")
        / instance_id
        / "report.json"
    )


def parse_resolved_from_report(report_obj: dict[str, Any], instance_id: str) -> dict[str, Any]:
    """Extract the resolved bit + F2P/P2P status from a swebench report.json object.

    Fails loud if the instance key or the `resolved` field is absent — a malformed
    report must never be scored as resolved (no-fakes mandate).
    """
    if instance_id not in report_obj:
        raise ValueError(f"report has no entry for {instance_id!r}: keys={list(report_obj)}")
    inst = report_obj[instance_id]
    if "resolved" not in inst:
        raise ValueError(f"report entry for {instance_id!r} missing 'resolved': {inst}")
    tests = inst.get("tests_status", {})
    f2p = tests.get("FAIL_TO_PASS", {})
    p2p = tests.get("PASS_TO_PASS", {})
    return {
        "resolved": bool(inst["resolved"]),
        "patch_applied": bool(inst.get("patch_successfully_applied", False)),
        "f2p_success": len(f2p.get("success", [])),
        "f2p_failure": len(f2p.get("failure", [])),
        "p2p_success": len(p2p.get("success", [])),
        "p2p_failure": len(p2p.get("failure", [])),
    }


def run_swebench_verifier(
    instance_id: str,
    model_patch: str,
    run_id: str,
    model_name: str,
    timeout_s: int = 1800,
) -> dict[str, Any]:
    """Run the official swebench oracle on one (instance, patch). Deterministic.

    Empty/whitespace patch ⇒ not-resolved, recorded explicitly (no harness run).
    Otherwise writes predictions.jsonl, runs run_evaluation (single worker, instance
    cache), and reads the per-instance report. A missing report on a non-empty patch
    is a HARD error — we never silently score it resolved.
    """
    if not model_patch.strip():
        return {
            "resolved": False, "empty_patch": True, "ran_harness": False,
            "detail": "empty model_patch — recorded as not-resolved",
        }

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    preds_path = REPORT_DIR / f"preds_{run_id}.jsonl"
    preds_path.write_text(json.dumps(build_prediction_row(instance_id, model_name, model_patch)) + "\n")

    cmd = [
        VENV_PY, "-m", "swebench.harness.run_evaluation",
        "--dataset_name", DATASET,
        "--predictions_path", str(preds_path),
        "--run_id", run_id,
        "--instance_ids", instance_id,
        "--max_workers", "1",
        "--cache_level", "instance",
        "--report_dir", str(REPORT_DIR),
    ]
    print(f"   [verify] swebench oracle: {instance_id} run_id={run_id}", flush=True)
    proc = subprocess.run(cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=timeout_s)

    report_path = per_instance_report_path(run_id, model_name, instance_id)
    if not report_path.exists():
        raise RuntimeError(
            f"swebench harness produced NO report for {instance_id} (run_id={run_id}); "
            f"verifier error — refusing to score it resolved.\n"
            f"--- harness stdout tail ---\n{proc.stdout[-1500:]}\n"
            f"--- harness stderr tail ---\n{proc.stderr[-1500:]}"
        )
    parsed = parse_resolved_from_report(json.loads(report_path.read_text()), instance_id)
    parsed.update({
        "empty_patch": False, "ran_harness": True,
        "report_path": str(report_path), "harness_rc": proc.returncode,
    })
    return parsed


# ── instance container + patch extraction (piece B) ───────────────────────────

def _docker(*args: str, check: bool = True, capture: bool = True, timeout: int | None = None):
    return subprocess.run(["docker", *args], check=check, capture_output=capture, text=True, timeout=timeout)


def start_instance_container(instance_id: str, arm: str) -> str:
    """Start a fresh instance container at its base commit; return the name."""
    name = container_name(instance_id, arm)
    img = instance_image(instance_id)
    _docker("rm", "-f", name, check=False)
    _docker("run", "-d", "--name", name, img, "sleep", "7200")
    # Defensive reset to the base commit so each arm starts byte-identical.
    _docker("exec", name, "bash", "-lc", "cd /testbed && git checkout -- . && git clean -fdq", check=False)
    return name


def extract_model_patch(name: str) -> str:
    """Capture ALL working-tree changes under /testbed as a base-commit diff.

    `git add -A` stages new files too; `git diff --cached` emits a single unified
    diff the swebench harness can `git apply`. An empty result is a real signal
    (the solve produced no edit) — returned verbatim, never faked.
    """
    _docker("exec", name, "bash", "-lc", "cd /testbed && git add -A", check=False)
    r = _docker("exec", name, "bash", "-lc", "cd /testbed && git diff --cached")
    return r.stdout


def stop_instance_container(name: str) -> None:
    _docker("rm", "-f", name, check=False)


# ── problem statement + prompt (piece D) ──────────────────────────────────────

def fetch_problem_statement(instance_id: str) -> str:
    """Fetch the instance problem statement via the spike fetcher (fail loud)."""
    script = REPO_ROOT / "scripts/swebench/fetch-problem-statement.py"
    proc = subprocess.run(["python3", str(script), instance_id], capture_output=True, text=True, timeout=120)
    if proc.returncode != 0 or not proc.stdout.strip():
        raise RuntimeError(f"failed to fetch problem statement for {instance_id}: {proc.stderr.strip()}")
    return proc.stdout.strip()


def compose_solve_prompt(instance_id: str, arm: str, problem_statement: str, injected_text: str) -> str:
    """The arm prompt: optional injected skill context + identical solve instructions.

    The OFF arm gets NO injected context; CTRL/TREAT get the harness-mediated
    compile_context block prepended (functionally identical to the SessionStart
    hook, but with exact attribution). Everything else is byte-identical.
    """
    org, repo, _ = parse_instance_id(instance_id)
    name = container_name(instance_id, arm)
    base = (
        f"Fix the following GitHub issue in the {repo} codebase located at /testbed.\n"
        f"Run commands inside the target container, e.g.: docker exec {name} bash -lc '<cmd>'\n"
        "Investigate the code, edit the source under /testbed, and run the relevant tests "
        "to verify your fix.\n\n"
        f"{problem_statement}"
    )
    if injected_text.strip():
        return eff._compose_arm_prompt(base, injected_text)
    return base


def skill_frontmatter_name(skill_md: Path) -> str:
    """A skill's CANONICAL name = its frontmatter `name:` (what graph-builder stores
    in PG and what compile_context returns on injection), falling back to the dir
    name. The slugified directory name (e.g. `i-want-...`) does NOT match the
    human-readable `name:` (e.g. `I want to customize... (preference)`); using the
    dir name for seed-hit attribution silently misses every injected preference-type
    skill (caught by the self-seed smoke 2026-06-20)."""
    try:
        text = skill_md.read_text(errors="replace")
    except OSError:
        return skill_md.parent.name
    if text.startswith("---"):
        parts = text.split("---", 2)
        if len(parts) >= 3:
            for line in parts[1].splitlines():
                st = line.strip()
                if st.startswith("name:"):
                    return st[len("name:"):].strip().strip("'\"")
    return skill_md.parent.name


def seed_skill_names(scope_dir: Path) -> set[str]:
    """Seed skill names gated in a scope — the canonical frontmatter `name:` values
    (these match PG + compile_context injection names, NOT the slugified dir name)."""
    skills_dir = scope_dir / ".skills"
    if not skills_dir.is_dir():
        return set()
    return {skill_frontmatter_name(p) for p in skills_dir.rglob("SKILL.md")}


def build_arm_injection(arm: str, problem_statement: str, server_url: str, instance_id: str,
                        scope_override: Path | None = None) -> dict[str, Any]:
    """Harness-mediated compile_context injection + attribution for one arm.

    OFF → empty. CTRL/TREAT → compile_context(problem_statement, repo_path=scope)
    against the REAL isolated server; records the injected skill names/ids and
    which were SEED skills (piece E). `scope_override` repoints a non-OFF arm at a
    different `.skills` scope (the self-seeding same-set loop points TREAT at the
    fresh scope grown from THIS bench's own OFF-run transcripts).

    Uses `trigger="session_start"` (RetrievalIntent::Priming): this IS the
    production SessionStart priming path the SWE-bench solve hook fires, and its
    lower floor + query-side multi-view is what retrieves against a verbose SWE-
    bench problem statement (the Task-intent floor no_matches verbose prompts on a
    small seed corpus). A fresh uuid session_id avoids priming dedup-suppression.
    """
    import uuid

    scope = scope_override if (arm != "off" and scope_override is not None) else ARM_SCOPE[arm]
    if arm == "off" or scope is None:
        return {"injected_text": "", "skill_names": [], "skill_ids": [],
                "seed_hits": [], "status": None, "repo_path": None}
    cc = eff.compile_context_http(
        server_url=server_url,
        prompt=problem_statement,
        session_id=f"t15-{arm}-{instance_id}-{uuid.uuid4()}",
        repo_path=str(scope),
        trigger="session_start",
    )
    names = cc["skill_names"]
    seeds = seed_skill_names(scope)
    seed_hits = [n for n in names if n in seeds]
    return {
        "injected_text": cc["additional_context"],
        "skill_names": names,
        "skill_ids": cc["skill_ids"],
        "seed_hits": seed_hits,
        "status": cc["raw"].get("status"),
        "repo_path": str(scope),
    }


def parse_claude_solve_output(stdout: str) -> tuple[dict[str, Any] | None, str]:
    """Parse `claude --output-format json` into (efficiency, result_text).

    The EFFICIENCY instrument (turns/tokens-to-resolve, the elevated secondary
    metric after the de-risk probe showed resolved-rate is base-model-dominated).
    Captures num_turns + token usage + total_cost_usd + duration_ms + is_error.

    NO FAKES: returns (None, raw) when the solve emitted no parseable JSON summary
    (timeout / crash) — an HONEST missing measurement, never a fabricated zero.
    """
    s = (stdout or "").strip()
    if not s:
        return None, ""
    try:
        o = json.loads(s)
    except (json.JSONDecodeError, ValueError):
        return None, s[-2000:]
    if not isinstance(o, dict):
        return None, s[-2000:]
    u = o.get("usage") or {}
    eff_metrics = {
        "num_turns": o.get("num_turns"),
        "input_tokens": u.get("input_tokens"),
        "output_tokens": u.get("output_tokens"),
        "cache_read_input_tokens": u.get("cache_read_input_tokens"),
        "cache_creation_input_tokens": u.get("cache_creation_input_tokens"),
        "total_cost_usd": o.get("total_cost_usd"),
        "duration_ms": o.get("duration_ms"),
        "is_error": o.get("is_error"),
        "subtype": o.get("subtype"),
        # error context for the loop's auth-vs-max-turns guard: api_error_status is a
        # dedicated field (a bare "401" here is unambiguous, unlike "401" in prose);
        # terminal_reason="max_turns" + errors=["Reached maximum…"] mark a legit cap-hit.
        "api_error_status": o.get("api_error_status"),
        "terminal_reason": o.get("terminal_reason"),
        "stop_reason": o.get("stop_reason"),
    }
    return eff_metrics, (o.get("result") or "")[-2000:]


def run_claude_solve_in_container(prompt: str, ws: Path, model: str, max_turns: int, timeout_s: int) -> dict[str, Any]:
    """Run one host claude solve (cwd=ws) that edits the instance container's /testbed.

    Prompt on STDIN (--add-dir is greedy and would swallow a positional prompt).
    `--output-format json` yields a single summary object (num_turns / usage /
    total_cost_usd) — the efficiency instrument; the solve work is unchanged.
    max_turns/timeout are stuck-detectors, recorded — not work caps.
    """
    ws.mkdir(parents=True, exist_ok=True)
    cmd = [
        "claude", "--print", "--output-format", "json", "--dangerously-skip-permissions",
        "--model", model, "--max-turns", str(max_turns), "--add-dir", str(ws),
    ]
    try:
        proc = subprocess.run(cmd, cwd=str(ws), capture_output=True, text=True, timeout=timeout_s, input=prompt)
    except subprocess.TimeoutExpired:
        return {"exit_code": -2, "result_text": "(solve timed out)", "timed_out": True, "efficiency": None}
    efficiency, result_text = parse_claude_solve_output(proc.stdout)
    return {
        "exit_code": proc.returncode,
        "result_text": result_text or (proc.stdout or "")[-2000:],
        "timed_out": False,
        "efficiency": efficiency,
    }


def solve_one_arm(
    instance_id: str, arm: str, problem_statement: str, server_url: str,
    model: str, max_turns: int, solve_timeout: int, verify_timeout: int, run_id_prefix: str,
    treat_scope_override: Path | None = None,
) -> dict[str, Any]:
    """Solve → extract patch → verify → attribution for ONE arm of ONE instance."""
    injection = build_arm_injection(arm, problem_statement, server_url, instance_id,
                                    scope_override=treat_scope_override)
    prompt = compose_solve_prompt(instance_id, arm, problem_statement, injection["injected_text"])
    ws = Path(f"/tmp/t15-swebench/solve/{instance_id}__{arm}")
    name = start_instance_container(instance_id, arm)
    try:
        solve = run_claude_solve_in_container(prompt, ws, model, max_turns, solve_timeout)
        model_patch = extract_model_patch(name)
    finally:
        stop_instance_container(name)

    (ws / "solve.log").write_text(solve["result_text"])
    (ws / "model.patch").write_text(model_patch)

    run_id = f"{run_id_prefix}-{arm}".replace("__", "-")
    model_name = f"t15-{arm}"
    verdict = run_swebench_verifier(instance_id, model_patch, run_id, model_name, timeout_s=verify_timeout)
    return {
        "instance_id": instance_id, "arm": arm,
        "solve_exit": solve["exit_code"], "solve_timed_out": solve["timed_out"],
        "result_text": solve["result_text"],
        "patch_bytes": len(model_patch), "empty_patch": not model_patch.strip(),
        "resolved": verdict["resolved"], "verify": verdict,
        "efficiency": solve["efficiency"],
        "attribution": {
            "skill_names": injection["skill_names"], "skill_ids": injection["skill_ids"],
            "seed_hits": injection["seed_hits"], "status": injection["status"],
            "repo_path": injection["repo_path"], "injected": bool(injection["seed_hits"]),
        },
    }


# ── full DiD run (pieces D+E+F) ───────────────────────────────────────────────

def load_split(fixture: Path) -> dict[str, Any]:
    if not fixture.exists():
        raise FileNotFoundError(f"split fixture missing: {fixture} (run t15_build_split_fixture.py)")
    return json.loads(fixture.read_text())


def run_did(args: argparse.Namespace) -> int:
    split = load_split(Path(args.split))
    # Deterministic fixture consumption: TEST = first n_test of the pre-registered
    # test block. `--instances` overrides with explicit ids (used by the Phase-2
    # PLUMBING dry-run to target a cached, pool-excluded instance so no held-out
    # TEST instance is burned before the Phase-3 N-lock).
    if args.instances:
        test_ids = [i.strip() for i in args.instances.split(",") if i.strip()]
    else:
        test_ids = split["test_block_ordered"][: args.n_test]

    if args.list:
        print("=== Phase-1 split fixture (deterministic consumption) ===")
        print(f"prereg_salt: {split.get('prereg_salt')}")
        print(f"SEED[:{args.n_seed}]: {split['seed_block'][: args.n_seed]}")
        print(f"TEST[:{args.n_test}]: {split['test_block_ordered'][: args.n_test]}")
        if args.instances:
            print(f"(--instances override active → would solve: {test_ids})")
        return 0

    arms = [a.strip() for a in args.arms.split(",")]
    print(f"=== T15 SWE-bench DiD runner ({args.run_id}) ===", flush=True)
    print(f"server={args.server_url}  arms={arms}  N_test={len(test_ids)}  model={args.model}", flush=True)
    print(f"TREAT scope={ARM_SCOPE['treat']}  CTRL scope={ARM_SCOPE['ctrl']}", flush=True)
    if args.n_test < 5:
        print(f"NOTE: dry/smoke run — {len(test_ids)} TEST instance(s); proves the chain, "
              "NOT the pre-registered DiD verdict (N locked at Phase 3).", flush=True)

    per_instance: list[dict[str, Any]] = []
    attribution_rows: list[dict[str, Any]] = []
    efficiency_rows: list[dict[str, Any]] = []
    treat_seed_injected = 0
    for iid in test_ids:
        print(f"\n── TEST instance {iid} ──", flush=True)
        problem = fetch_problem_statement(iid)
        rec: dict[str, Any] = {"instance_id": iid}
        eff_rec: dict[str, Any] = {"instance_id": iid}
        for arm in arms:
            out = solve_one_arm(
                iid, arm, problem, args.server_url, args.model,
                args.max_turns, args.solve_timeout, args.verify_timeout,
                run_id_prefix=f"{args.run_id}-{iid}".replace("__", "-"),
            )
            rec[arm] = 1 if out["resolved"] else 0
            attribution_rows.append({"instance_id": iid, "arm": arm, **out["attribution"],
                                     "resolved": out["resolved"], "empty_patch": out["empty_patch"]})
            # Per-arm efficiency for the turns/tokens-to-resolve analysis; carry the
            # resolved bit so aggregate_efficiency can restrict to resolved-by-both.
            eff_rec[arm] = {**(out["efficiency"] or {}), "resolved": bool(out["resolved"])}
            if arm == "treat" and out["attribution"]["injected"]:
                treat_seed_injected += 1
            e = out["efficiency"] or {}
            print(f"  [{arm.upper():5s}] resolved={out['resolved']} patch_bytes={out['patch_bytes']} "
                  f"turns={e.get('num_turns')} out_tok={e.get('output_tokens')} "
                  f"cost=${e.get('total_cost_usd')} seed_hits={out['attribution']['seed_hits']}", flush=True)
        per_instance.append(rec)
        efficiency_rows.append(eff_rec)

    # Resolved-rate DiD only when all 3 arms ran (DiD requires off/ctrl/treat).
    have_all = all(k in per_instance[0] for k in ("off", "ctrl", "treat")) if per_instance else False
    aggregate = None
    if have_all:
        aggregate = eff.aggregate_did_run(
            per_instance, treat_seed_injected_count=treat_seed_injected,
            iterations=args.bootstrap_iterations, seed=args.bootstrap_seed, mde=args.mde,
        )

    # Efficiency (turns/tokens-to-resolve) needs only OFF + TREAT — computed whenever
    # both arms ran, independent of the 3-arm resolved-rate DiD.
    efficiency_aggregate = None
    if "off" in arms and "treat" in arms:
        efficiency_aggregate = eff.aggregate_efficiency(
            efficiency_rows, resolved_only=True,
            iterations=args.bootstrap_iterations, seed=args.bootstrap_seed,
        )

    report = {
        "run_id": args.run_id, "server_url": args.server_url, "arms": arms,
        "n_test": len(test_ids), "model": args.model, "max_turns": args.max_turns,
        "split_fixture": str(args.split), "prereg_salt": split.get("prereg_salt"),
        "treat_scope": str(ARM_SCOPE["treat"]), "ctrl_scope": str(ARM_SCOPE["ctrl"]),
        "treat_seed_injected_count": treat_seed_injected,
        "corpus_inventory": {
            "treat_seed_skills": sorted(seed_skill_names(ARM_SCOPE["treat"])),
            "ctrl_seed_skills": sorted(seed_skill_names(ARM_SCOPE["ctrl"])),
        },
        "per_instance_resolved": per_instance,
        "attribution": attribution_rows,
        "efficiency_rows": efficiency_rows,
        "aggregate": aggregate,
        "efficiency_aggregate": efficiency_aggregate,
    }
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    out_path = REPORT_DIR / f"did_{args.run_id}.json"
    out_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"\n[report] {out_path}", flush=True)
    if aggregate:
        v = aggregate["verdict_summary"]
        print(f"VERDICT (resolved-rate DiD): {v['verdict']}\n  {v['detail']}", flush=True)
    else:
        print("VERDICT (resolved-rate DiD): (not computed — need all 3 arms off/ctrl/treat)", flush=True)
    if efficiency_aggregate:
        print("EFFICIENCY (TREAT−OFF, resolved-by-both; negative ⇒ TREAT cheaper):", flush=True)
        for m, d in efficiency_aggregate["metrics"].items():
            ci = d["bootstrap_ci_mean_delta"]
            print(f"  {m}: n={d['n_pairs']} mean_delta={d['mean_delta_treat_minus_off']} "
                  f"(TREAT-cheaper {d['treat_cheaper_count']}/{d['n_pairs']}, "
                  f"sign p={d['sign_test_p']:.3f}, CI=[{ci['ci_lo']}, {ci['ci_hi']}])", flush=True)
    return 0


# ── subcommand: verify a single patch file (testable standalone) ──────────────

def run_verify_cli(args: argparse.Namespace) -> int:
    patch = Path(args.patch_file).read_text() if args.patch_file else ""
    verdict = run_swebench_verifier(args.instance_id, patch, args.run_id,
                                    args.model_name, timeout_s=args.verify_timeout)
    print(json.dumps(verdict, indent=2))
    return 0


# ── self-test (no docker, no solves, no model calls) ──────────────────────────

def _self_test() -> int:
    print("=== t15_swebench_runner self-test ===")
    failures = 0

    def _assert(cond: bool, label: str, detail: str = "") -> bool:
        status = "PASS" if cond else "FAIL"
        print(f"  {status}  {label}{f'  [{detail}]' if detail else ''}")
        return cond

    # -- instance id / image parsing --
    org, repo, issue = parse_instance_id("django__django-14999")
    failures += 0 if _assert(org == "django" and repo == "django" and issue == "14999",
                             "parse django__django-14999") else 1
    org, repo, issue = parse_instance_id("sympy__sympy-20590")
    failures += 0 if _assert(repo == "sympy" and issue == "20590", "parse sympy id") else 1
    img = instance_image("django__django-14999")
    failures += 0 if _assert(img == "swebench/sweb.eval.x86_64.django_1776_django-14999:latest",
                             "image tag", img) else 1
    try:
        parse_instance_id("not-an-id")
        ok = _assert(False, "malformed id raises")
    except ValueError:
        ok = _assert(True, "malformed id raises")
    failures += 0 if ok else 1

    # -- prediction row contract --
    row = build_prediction_row("django__django-1", "t15-treat", "diff --git a b")
    failures += 0 if _assert(
        row == {"instance_id": "django__django-1", "model_name_or_path": "t15-treat",
                "model_patch": "diff --git a b"}, "prediction row shape") else 1

    # -- report path layout (model `/`→`__`) --
    p = per_instance_report_path("rid", "org/m", "django__django-1")
    failures += 0 if _assert(p.as_posix().endswith(
        "logs/run_evaluation/rid/org__m/django__django-1/report.json"), "report path", p.name) else 1

    # -- resolved parsing: resolved true --
    rep = {"django__django-1": {"resolved": True, "patch_successfully_applied": True,
                                "tests_status": {"FAIL_TO_PASS": {"success": ["t"], "failure": []},
                                                 "PASS_TO_PASS": {"success": ["a", "b"], "failure": []}}}}
    parsed = parse_resolved_from_report(rep, "django__django-1")
    failures += 0 if _assert(parsed["resolved"] is True and parsed["f2p_success"] == 1
                             and parsed["p2p_success"] == 2, "parse resolved=true + counts") else 1

    # -- resolved parsing: resolved false (F2P failure) --
    rep2 = {"x": {"resolved": False, "tests_status": {"FAIL_TO_PASS": {"success": [], "failure": ["t"]},
                                                      "PASS_TO_PASS": {"success": [], "failure": []}}}}
    parsed = parse_resolved_from_report(rep2, "x")
    failures += 0 if _assert(parsed["resolved"] is False and parsed["f2p_failure"] == 1,
                             "parse resolved=false") else 1

    # -- malformed report fails loud (no fabricated resolved) --
    try:
        parse_resolved_from_report({"x": {"tests_status": {}}}, "x")
        ok = _assert(False, "missing 'resolved' raises")
    except ValueError:
        ok = _assert(True, "missing 'resolved' raises (no fabrication)")
    failures += 0 if ok else 1
    try:
        parse_resolved_from_report({}, "x")
        ok = _assert(False, "missing instance entry raises")
    except ValueError:
        ok = _assert(True, "missing instance entry raises")
    failures += 0 if ok else 1

    # -- empty patch → not resolved, harness NOT run (no fake) --
    verdict = run_swebench_verifier("django__django-1", "   \n  ", "rid-empty", "t15-off")
    failures += 0 if _assert(verdict["resolved"] is False and verdict["ran_harness"] is False
                             and verdict["empty_patch"] is True, "empty patch → not resolved, no harness") else 1

    # -- arm scope mapping (treat=django, ctrl=sympy, off=None) --
    failures += 0 if _assert(ARM_SCOPE["treat"].name == "swebench-django"
                             and ARM_SCOPE["ctrl"].name == "swebench-sympy"
                             and ARM_SCOPE["off"] is None, "arm→scope mapping") else 1

    # -- container name sanitation --
    failures += 0 if _assert(container_name("django__django-1", "treat") == "t15-treat-django-django-1",
                             "container name", container_name("django__django-1", "treat")) else 1

    # -- efficiency JSON parse: valid claude summary → metrics extracted --
    valid = json.dumps({"num_turns": 14, "total_cost_usd": 0.42, "duration_ms": 90000,
                        "is_error": False, "subtype": "success",
                        "usage": {"input_tokens": 5, "output_tokens": 3100,
                                  "cache_read_input_tokens": 120000}})
    em, rt = parse_claude_solve_output(valid + "\n")
    failures += 0 if _assert(em is not None and em["num_turns"] == 14
                             and em["output_tokens"] == 3100 and em["total_cost_usd"] == 0.42,
                             "efficiency parse: valid summary → turns/tokens/cost") else 1
    # -- non-JSON / empty solve output → None efficiency (honest missing, no fake 0) --
    em2, _ = parse_claude_solve_output("claude crashed: some traceback text")
    em3, _ = parse_claude_solve_output("")
    failures += 0 if _assert(em2 is None and em3 is None,
                             "efficiency parse: non-JSON / empty → None (no fabrication)") else 1

    print(f"\n{'=' * 40}")
    if failures == 0:
        print("ALL TESTS PASSED")
    else:
        print(f"{failures} TEST(S) FAILED", file=sys.stderr)
    return 0 if failures == 0 else 1


# ── CLI ───────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", action="store_true", help="Run pure-logic self-tests and exit.")
    sub = ap.add_subparsers(dest="cmd")

    r = sub.add_parser("run", help="Full DiD over N TEST instances × arms from the split fixture.")
    r.add_argument("--run-id", dest="run_id", default=f"t15-did-{int(time.time())}")
    r.add_argument("--split", default=str(SPLIT_FIXTURE))
    r.add_argument("--n-test", dest="n_test", type=int, default=1)
    r.add_argument("--n-seed", dest="n_seed", type=int, default=12)
    r.add_argument("--instances", default=None,
                   help="Comma-separated instance ids overriding the fixture test block "
                        "(Phase-2 plumbing dry-run targets a cached, pool-excluded instance).")
    r.add_argument("--list", action="store_true",
                   help="Print the deterministic SEED/TEST selection from the fixture and exit "
                        "(no solves) — proves deterministic fixture consumption.")
    r.add_argument("--arms", default="off,ctrl,treat")
    r.add_argument("--server-url", dest="server_url", default=DEFAULT_SERVER)
    r.add_argument("--model", default="sonnet")
    r.add_argument("--max-turns", dest="max_turns", type=int, default=40)
    r.add_argument("--solve-timeout", dest="solve_timeout", type=int, default=2400)
    r.add_argument("--verify-timeout", dest="verify_timeout", type=int, default=1800)
    r.add_argument("--bootstrap-iterations", dest="bootstrap_iterations", type=int,
                   default=eff.T15_BOOTSTRAP_ITERATIONS)
    r.add_argument("--bootstrap-seed", dest="bootstrap_seed", type=int, default=eff.T15_BOOTSTRAP_SEED)
    r.add_argument("--mde", type=float, default=0.0)

    v = sub.add_parser("verify", help="Run the oracle on one (instance, patch-file).")
    v.add_argument("--instance-id", dest="instance_id", required=True)
    v.add_argument("--patch-file", dest="patch_file", default=None)
    v.add_argument("--run-id", dest="run_id", default=f"t15-verify-{int(time.time())}")
    v.add_argument("--model-name", dest="model_name", default="t15-verify")
    v.add_argument("--verify-timeout", dest="verify_timeout", type=int, default=1800)

    s = sub.add_parser("solve-arm", help="Solve + extract + verify one arm of one instance.")
    s.add_argument("--instance-id", dest="instance_id", required=True)
    s.add_argument("--arm", required=True, choices=["off", "ctrl", "treat"])
    s.add_argument("--run-id", dest="run_id", default=f"t15-arm-{int(time.time())}")
    s.add_argument("--server-url", dest="server_url", default=DEFAULT_SERVER)
    s.add_argument("--model", default="sonnet")
    s.add_argument("--max-turns", dest="max_turns", type=int, default=40)
    s.add_argument("--solve-timeout", dest="solve_timeout", type=int, default=2400)
    s.add_argument("--verify-timeout", dest="verify_timeout", type=int, default=1800)

    args = ap.parse_args()
    if args.self_test:
        sys.exit(_self_test())
    if args.cmd == "run":
        sys.exit(run_did(args))
    if args.cmd == "verify":
        sys.exit(run_verify_cli(args))
    if args.cmd == "solve-arm":
        problem = fetch_problem_statement(args.instance_id)
        out = solve_one_arm(args.instance_id, args.arm, problem, args.server_url, args.model,
                            args.max_turns, args.solve_timeout, args.verify_timeout,
                            run_id_prefix=args.run_id)
        print(json.dumps(out, indent=2))
        sys.exit(0)
    ap.print_help()
    sys.exit(1)


if __name__ == "__main__":
    main()
