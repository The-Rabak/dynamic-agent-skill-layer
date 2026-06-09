#!/usr/bin/env python3
"""Capture top-N find_skill results for every held-out query from the REAL running
mcp-server, for whichever arm is currently booted. Run once per arm, then diff.

Usage:
  python3 scripts/compare_arms_top10.py capture <arm-label> <out.json>   # query live server
  python3 scripts/compare_arms_top10.py diff <dense.json> <hybrid.json>  # compare two captures
"""
import json
import sys
import urllib.request
from pathlib import Path

MCP_URL = "http://127.0.0.1:3001/mcp"
FIXTURE = "tests/fixtures/retrieval_quality_234_corpus_labeled.json"
LIMIT = 10


def find_skill(prompt: str, limit: int) -> list[str]:
    body = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "find_skill", "arguments": {"prompt": prompt, "limit": limit}},
    }).encode()
    req = urllib.request.Request(MCP_URL, data=body, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        r = json.loads(resp.read())
    if "error" in r:
        raise RuntimeError(f"find_skill RPC error for {prompt!r}: {r['error']}")
    matches = r["result"]["matches"]
    return [(m["name"], m.get("score")) for m in matches]


def capture(arm: str, out: str):
    fx = json.loads(Path(FIXTURE).read_text())
    queries = [q for q in fx["queries"] if q.get("split") == "held_out" and q["kind"] != "negative"]
    rows = []
    for q in queries:
        results = find_skill(q["text"], LIMIT)
        rows.append({
            "id": q["id"], "kind": q["kind"], "text": q["text"],
            "relevant": q["relevant"],
            "top10": [name for name, _ in results],
            "top10_scored": [[name, score] for name, score in results],
        })
    Path(out).write_text(json.dumps({"arm": arm, "limit": LIMIT, "queries": rows}, indent=2))
    print(f"[capture] arm={arm}  queries={len(rows)}  -> {out}")


def diff(a_path: str, b_path: str):
    a = json.loads(Path(a_path).read_text())
    b = json.loads(Path(b_path).read_text())
    arm_a, arm_b = a["arm"], b["arm"]
    ba = {q["id"]: q for q in b["queries"]}
    print(f"\n=== TOP-{a['limit']} DIFF: {arm_a}  vs  {arm_b}  (held_out, {len(a['queries'])} queries) ===\n")
    n_diff_set = n_diff_order = n_diff_hit = 0
    for qa in a["queries"]:
        qb = ba[qa["id"]]
        ta, tb = qa["top10"], qb["top10"]
        set_a, set_b = set(ta), set(tb)
        rel = set(qa["relevant"])
        # hit@10 membership for gold
        hit_a = bool(rel & set_a)
        hit_b = bool(rel & set_b)
        if set_a != set_b:
            n_diff_set += 1
        elif ta != tb:
            n_diff_order += 1
        if hit_a != hit_b:
            n_diff_hit += 1
        if ta != tb:
            only_a = [s for s in ta if s not in set_b]
            only_b = [s for s in tb if s not in set_a]
            tag = "SET" if set_a != set_b else "ORDER"
            print(f"[{tag}] {qa['id']} ({qa['kind']})  gold={qa['relevant']}")
            print(f"    {arm_a:>16}: {ta}")
            print(f"    {arm_b:>16}: {tb}")
            if only_a or only_b:
                print(f"    only in {arm_a}: {only_a}")
                print(f"    only in {arm_b}: {only_b}")
            print()
    total = len(a["queries"])
    same = total - n_diff_set - n_diff_order
    print("=== SUMMARY ===")
    print(f"  identical top-{a['limit']} (set+order): {same}/{total}")
    print(f"  differ in SET membership:            {n_diff_set}/{total}")
    print(f"  same set, different ORDER only:       {n_diff_order}/{total}")
    print(f"  gold hit@{a['limit']} flips between arms:    {n_diff_hit}/{total}")


if __name__ == "__main__":
    if len(sys.argv) >= 4 and sys.argv[1] == "capture":
        capture(sys.argv[2], sys.argv[3])
    elif len(sys.argv) >= 4 and sys.argv[1] == "diff":
        diff(sys.argv[2], sys.argv[3])
    else:
        print(__doc__)
        sys.exit(2)
