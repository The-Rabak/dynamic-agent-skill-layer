#!/usr/bin/env python3
"""#208 keep-or-cut measurement for the SkillRAE community boost.

Measures THREE arms of the eq.3 community boost on the REAL running mcp-server
(held-out split, judge-augmented), by rebooting the server per arm with
RETRIEVAL_COMMUNITY_BOOST_MODE and driving find_skill over HTTP:

  (a) binary            — historical 0.2-for-any-community (uniform → inert)
  (b) centroid_affinity — cosine(query, the skill's community centroid)
  (c) off               — no community boost (λ=0 equivalent)

Decision gate (from the ticket): if (b) does NOT beat (c) by a meaningful
measured margin, HDBSCAN community detection is cut/demoted from the read path
and λ is baked to 0. NO in-process reconstruction — every arm is a real reboot.
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
ARMS = [("a_binary", "binary"), ("b_centroid_affinity", "centroid_affinity"), ("c_off", "off")]


def reboot(mode: str):
    env = dict(os.environ)
    env["RETRIEVAL_COMMUNITY_BOOST_MODE"] = mode
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
    raise RuntimeError(f"mcp-server not serving within {deadline_s}s after reboot")


def measure(label):
    out = Path(f"tests/e2e/reports/208/{label}__held_out.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([sys.executable, "scripts/retrieval_quality_live.py",
                    "--split", "held_out", "--config-label", label, "--limit", "5",
                    "--out", str(out),
                    "--verdict-cache", "tests/e2e/reports/retrieval_234_live_verdicts.json"], check=True)
    return json.loads(out.read_text())


def main():
    results = []
    for label, mode in ARMS:
        print(f"\n########## ARM {label} (RETRIEVAL_COMMUNITY_BOOST_MODE={mode}) ##########", flush=True)
        reboot(mode)
        wait_ready()
        rep = measure(label)
        ja = rep["judge_augmented"]
        results.append((label, mode, ja, rep["no_match_precision"]))

    print("\n=== #208 COMMUNITY BOOST — keep-or-cut (held-out, judge-augmented) ===")
    print(f"{'arm':22s} {'MRR':>7s} {'nDCG@3':>7s} {'P@1':>7s} {'hit@3':>7s} {'no_match':>9s}")
    for label, mode, ja, nmp in results:
        print(f"{label:22s} {ja['mrr']:>7.3f} {ja['ndcg_at_3']:>7.3f} {ja['p_at_1']:>7.3f} "
              f"{ja['hit_at_3']:>7.3f} {nmp:>9.3f}")

    by = {label: ja for label, _, ja, _ in results}
    b, c = by["b_centroid_affinity"]["mrr"], by["c_off"]["mrr"]
    margin = b - c
    print(f"\n(b) centroid_affinity MRR {b:.3f} vs (c) off MRR {c:.3f} → margin {margin:+.3f}")
    verdict = "KEEP graph (b beats off)" if margin >= 0.02 else "CUT/demote graph (b does NOT beat off)"
    print(f"DECISION (>=+0.02 to keep): {verdict}")

    Path("tests/e2e/reports/retrieval_234_208_summary.json").write_text(json.dumps(
        dict(arms=[dict(label=l, mode=m, judge_augmented=ja, no_match_precision=nmp)
                   for l, m, ja, nmp in results],
             centroid_vs_off_margin=margin, decision=verdict), indent=1))
    print("summary: tests/e2e/reports/retrieval_234_208_summary.json")


if __name__ == "__main__":
    main()
