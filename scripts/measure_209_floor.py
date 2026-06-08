#!/usr/bin/env python3
"""#209 no_match relevance-floor calibration on the REAL 234-corpus.

Recalibrates RETRIEVAL_RELEVANCE_THRESHOLD by sweeping it on the REAL running
mcp-server (reboot per floor) and measuring, at each floor, the no_match
precision (negatives correctly rejected) vs positive recall (positives still
surfaced) — the operational ROC of the floor decision, measured end-to-end via
find_skill over HTTP. NO eq3 reconstruction, NO in-process scoring.

Calibrates on the TUNING split, then validates the chosen floor on the disjoint
HELD-OUT split (the #209 disjointness invariant). The floor is derived from the
measured precision/recall tradeoff, never to make a single test pass.
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
FLOORS = [0.40, 0.42, 0.44, 0.45, 0.46, 0.48, 0.50, 0.52]
OUT = Path("tests/e2e/reports/209")
CACHE = "tests/e2e/reports/retrieval_234_live_verdicts.json"


def reboot(floor: float):
    env = dict(os.environ)
    env["RETRIEVAL_RELEVANCE_THRESHOLD"] = str(floor)
    subprocess.run(COMPOSE + ["up", "-d", "--no-deps", "--force-recreate", "mcp-server"],
                   check=True, capture_output=True, text=True, env=env)


def wait_ready(deadline_s=600):
    start = time.time()
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": "find_skill",
                                  "arguments": {"prompt": "conventional commits", "limit": 3}}}).encode()
    while time.time() - start < deadline_s:
        try:
            req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=15) as resp:
                if json.loads(resp.read()).get("result", {}).get("matches", []):
                    return
        except Exception:
            pass
        time.sleep(3)
    raise RuntimeError(f"mcp-server not serving within {deadline_s}s after reboot at floor")


def measure(label, split):
    out = OUT / f"{label}__{split}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([sys.executable, "scripts/retrieval_quality_live.py",
                    "--split", split, "--config-label", label, "--limit", "5",
                    "--out", str(out), "--verdict-cache", CACHE], check=True)
    return json.loads(out.read_text())


def main():
    rows = []
    for floor in FLOORS:
        print(f"\n########## FLOOR {floor} ##########", flush=True)
        reboot(floor)
        wait_ready()
        rep = measure(f"floor_{floor}", "tuning")
        ja = rep["judge_augmented"]
        rows.append(dict(floor=floor, no_match_precision=rep["no_match_precision"],
                         pos_mrr=ja["mrr"], pos_hit3=ja["hit_at_3"], pos_recall3=ja["recall_at_3"]))
        print(f"  tuning: no_match_prec={rep['no_match_precision']:.3f} "
              f"pos_MRR={ja['mrr']:.3f} pos_hit@3={ja['hit_at_3']:.3f}", flush=True)

    print("\n=== #209 FLOOR CALIBRATION (tuning split) ===")
    print(f"{'floor':>6s} {'no_match_prec':>13s} {'pos_MRR':>8s} {'pos_hit@3':>10s} {'pos_recall@3':>13s}")
    for r in rows:
        print(f"{r['floor']:>6.2f} {r['no_match_precision']:>13.3f} {r['pos_mrr']:>8.3f} "
              f"{r['pos_hit3']:>10.3f} {r['pos_recall3']:>13.3f}")

    # Choose: highest floor that keeps positive hit@3 within 0.02 of its max
    # (no meaningful recall loss) AND maximizes no_match precision. Tie-break:
    # lower floor (more recall headroom).
    max_hit = max(r["pos_hit3"] for r in rows)
    eligible = [r for r in rows if r["pos_hit3"] >= max_hit - 0.02]
    best = max(eligible, key=lambda r: (round(r["no_match_precision"], 3), -r["floor"]))
    chosen = best["floor"]
    print(f"\nCHOSEN FLOOR: {chosen}  "
          f"(no_match_prec={best['no_match_precision']:.3f}, pos_hit@3={best['pos_hit3']:.3f}; "
          f"max pos_hit@3 over sweep={max_hit:.3f})")

    print(f"\n=== VALIDATING chosen floor {chosen} on HELD-OUT (disjoint) ===")
    reboot(chosen)
    wait_ready()
    held = measure(f"chosen_{chosen}", "held_out")
    hja = held["judge_augmented"]
    print(f"held-out: no_match_prec={held['no_match_precision']:.3f} "
          f"pos_MRR={hja['mrr']:.3f} pos_hit@3={hja['hit_at_3']:.3f} pos_recall@3={hja['recall_at_3']:.3f}")

    Path("tests/e2e/reports/retrieval_234_209_summary.json").write_text(json.dumps(
        dict(sweep=rows, chosen_floor=chosen, chosen_tuning=best,
             held_out_validation=dict(no_match_precision=held["no_match_precision"],
                                       pos_mrr=hja["mrr"], pos_hit3=hja["hit_at_3"],
                                       pos_recall3=hja["recall_at_3"])), indent=1))
    print("\nsummary: tests/e2e/reports/retrieval_234_209_summary.json")


if __name__ == "__main__":
    main()
