#!/usr/bin/env python3
"""Retrieval-quality tuning sweep over the REAL running mcp-server (#210).

For each config, this reboots ONLY the mcp-server container with the config's
RETRIEVAL_* env overrides (RetrievalConfig::from_env), waits until it has
re-embedded the real 234-corpus and is serving, then measures quality by
driving the real server over HTTP (scripts/retrieval_quality_live.py). NO
in-process reconstruction; every config is a fully-booted real server.

Method (binding decisions):
  - Tune on the TUNING split only; the winner is validated on the disjoint
    HELD-OUT split. Target is FROZEN before the sweep: judge-augmented
    MRR >= 0.80, nDCG@3 >= 0.80, no_match precision >= 0.90.
  - Winner selected by judge-augmented tuning MRR (tie-break nDCG@3). No weight
    chosen to pass a single query.
  - The LLM-judge verdict cache is shared across configs (judging drains the
    union pool; cached pairs are never re-judged). No caps.
"""
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

COMPOSE = ["docker", "compose", "-f", "docker-compose.test.yml"]
MCP_URL = "http://127.0.0.1:3001/mcp"
REPORT_DIR = Path("tests/e2e/reports/sweep")
WARMUP_PROMPT = "conventional commits with co-authored-by trailer"  # a known corpus topic
ENV_KEYS = [
    "RETRIEVAL_ALPHA", "RETRIEVAL_BETA", "RETRIEVAL_GAMMA", "RETRIEVAL_LAMBDA",
    "RETRIEVAL_MMR_LAMBDA", "RETRIEVAL_CANDIDATE_LIMIT", "RETRIEVAL_MAX_RESULTS",
    "RETRIEVAL_MAX_SUBUNITS_PER_SKILL", "RETRIEVAL_RESCUE_THRESHOLD",
    "RETRIEVAL_RELEVANCE_THRESHOLD", "RETRIEVAL_PROJECT_SCOPE_WEIGHT",
    "RETRIEVAL_GLOBAL_SCOPE_WEIGHT", "RETRIEVAL_RRF_K",
]

# Default α=0.45 β=0.35 γ=0.20 λ=0.25, mmr=0.65, cand=50, subunits=3, floor=0.450.
# Each config overrides one or a few levers from default; "default" overrides nothing
# (also the faithfulness check — must reproduce the pre-sweep baseline).
CONFIGS = [
    ("default",            {}),
    ("lambda0",            {"RETRIEVAL_LAMBDA": "0.0"}),                                   # #208: graph off
    ("beta_heavy",         {"RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_GAMMA": "0.15"}),
    ("alpha_heavy",        {"RETRIEVAL_ALPHA": "0.60", "RETRIEVAL_BETA": "0.30", "RETRIEVAL_GAMMA": "0.10"}),
    ("lambda0_beta_heavy", {"RETRIEVAL_LAMBDA": "0.0", "RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_GAMMA": "0.15"}),
    ("subunit_deep",       {"RETRIEVAL_MAX_SUBUNITS_PER_SKILL": "5", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_GAMMA": "0.15"}),
    ("mmr_relevance",      {"RETRIEVAL_MMR_LAMBDA": "0.85"}),
    ("candidate_wide",     {"RETRIEVAL_CANDIDATE_LIMIT": "100"}),
]
LIMIT = 5  # find_skill depth for MRR (top-k injection is K=3)


def set_env(overrides: dict):
    for k in ENV_KEYS:
        os.environ.pop(k, None)
    for k, v in overrides.items():
        os.environ[k] = v


def reboot_mcp():
    subprocess.run(COMPOSE + ["up", "-d", "--no-deps", "--force-recreate", "mcp-server"],
                   check=True, capture_output=True, text=True)


def warmup_query():
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": "find_skill",
                                  "arguments": {"prompt": WARMUP_PROMPT, "limit": 3}}}).encode()
    req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        r = json.loads(resp.read())
    return r.get("result", {}).get("matches", [])


def wait_ready(deadline_s: int = 600):
    """Poll until the rebooted server has re-embedded the corpus and serves a
    known query. Fail loud only on a real stuck state (deadline), per the
    no-arbitrary-caps rule (the deadline is a stuck-detector, not a work cap)."""
    start = time.time()
    while time.time() - start < deadline_s:
        try:
            if warmup_query():
                return
        except Exception:
            pass
        time.sleep(3)
    raise RuntimeError(f"mcp-server did not serve the warmup query within {deadline_s}s after reboot")


def measure(label: str, split: str, gate: bool = False) -> dict:
    out = REPORT_DIR / f"{label}__{split}.json"
    cmd = [sys.executable, "scripts/retrieval_quality_live.py",
           "--split", split, "--config-label", label, "--limit", str(LIMIT),
           "--out", str(out),
           "--verdict-cache", "tests/e2e/reports/retrieval_234_live_verdicts.json"]
    if gate:
        cmd.append("--gate")
    res = subprocess.run(cmd, text=True)
    report = json.loads(out.read_text())
    report["_gate_exit"] = res.returncode
    return report


def main():
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    results = []
    for label, overrides in CONFIGS:
        print(f"\n########## CONFIG: {label}  {overrides or '(default)'} ##########", flush=True)
        set_env(overrides)
        reboot_mcp()
        wait_ready()
        rep = measure(label, "tuning")
        ja = rep["judge_augmented"]
        results.append((label, overrides, ja["mrr"], ja["ndcg_at_3"], ja["p_at_1"], ja["hit_at_3"]))
        print(f"  tuning judge-aug: MRR={ja['mrr']:.3f} nDCG@3={ja['ndcg_at_3']:.3f} "
              f"P@1={ja['p_at_1']:.3f} hit@3={ja['hit_at_3']:.3f}", flush=True)

    print("\n=== TUNING SWEEP (judge-augmented) ===")
    print(f"{'config':22s} {'MRR':>7s} {'nDCG@3':>7s} {'P@1':>7s} {'hit@3':>7s}")
    for label, _, mrr, ndcg, p1, hit in results:
        print(f"{label:22s} {mrr:>7.3f} {ndcg:>7.3f} {p1:>7.3f} {hit:>7.3f}")

    winner = max(results, key=lambda r: (r[2], r[3]))  # MRR, tie-break nDCG@3
    w_label, w_overrides = winner[0], winner[1]
    print(f"\nWINNER (tuning): {w_label}  {w_overrides or '(default)'}  MRR={winner[2]:.3f}")

    print(f"\n=== VALIDATING WINNER ON HELD-OUT: {w_label} ===")
    set_env(w_overrides)
    reboot_mcp()
    wait_ready()
    held = measure(f"{w_label}-WINNER", "held_out", gate=True)
    hja = held["judge_augmented"]
    print(f"held-out judge-aug: MRR={hja['mrr']:.3f} nDCG@3={hja['ndcg_at_3']:.3f} "
          f"P@1={hja['p_at_1']:.3f} hit@3={hja['hit_at_3']:.3f} "
          f"no_match_prec={held['no_match_precision']}")

    summary = dict(configs=[dict(label=l, overrides=o, tuning_mrr=m, tuning_ndcg=n,
                                 tuning_p1=p, tuning_hit3=h) for l, o, m, n, p, h in results],
                   winner=dict(label=w_label, overrides=w_overrides),
                   winner_held_out=held, target=held["target"])
    Path("tests/e2e/reports/retrieval_234_sweep_summary.json").write_text(json.dumps(summary, indent=1))
    print("\nsweep summary: tests/e2e/reports/retrieval_234_sweep_summary.json")
    print(f"gate exit for winner held-out: {held['_gate_exit']} (0=meets target, 1=below)")


if __name__ == "__main__":
    main()
