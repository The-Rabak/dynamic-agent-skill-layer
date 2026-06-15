#!/usr/bin/env python3
"""Focused Task-retrieval quality probe over the live mcp-server `find_skill` path.

WHY (T12 embedding-model experiment, arm A2 = qwen3-embedding:0.6b on Ollama):
The full `retrieval_sweep.py --gate` wrapper guards on a non-empty model-keyed
Qdrant collection (for arm reproducibility), which a brand-new embedding model has
not had populated by graph-builder yet. But `snapshot_dense` SERVES queries from
the in-memory snapshot (already re-embedded in the active model), so Task quality
IS directly measurable via `find_skill`. This probe drives the REAL server over
the 162 task-strata queries (the same set the 4B T11 numbers came from) and
computes MRR@3 / nDCG@3 / candidate-recall@limit / no_match-precision using the
VALIDATED `scripts/retrieval_metrics.py` functions — so the A2 numbers are
directly comparable to the T11 4B reference. It is NOT the full gate (no α=0
crater reboot — the fixture's discrimination was already validated on 4B); it is
an honest find_skill measurement on the live 0.6b snapshot.

Usage: python3 scripts/t12_task_quality_probe.py --label a2_0p6b --out <path> [--limit 50]
"""
import argparse
import json
import sys
import urllib.request
from pathlib import Path
from time import perf_counter

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retrieval_metrics as rm  # noqa: E402

MCP_URL = "http://127.0.0.1:3001/mcp"
FIXTURE = Path("tests/fixtures/retrieval_quality_262_corpus_labeled.json")
T11_4B_REFERENCE = {"mrr_at3": 0.743, "ndcg_at3": 0.755, "candidate_recall_at_limit": 0.796, "no_match_precision": 0.92}


def find_skill(prompt: str, limit: int):
    body = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "find_skill", "arguments": {"prompt": prompt, "limit": limit}},
    }).encode()
    req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
    started = perf_counter()
    with urllib.request.urlopen(req, timeout=60) as resp:
        r = json.loads(resp.read())
    latency_ms = round((perf_counter() - started) * 1000)
    if "error" in r:
        raise RuntimeError(f"find_skill RPC error for {prompt!r}: {r['error']}")
    return [m["name"] for m in r.get("result", {}).get("matches", [])], latency_ms


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--limit", type=int, default=50, help="candidate pool depth for cand-recall")
    args = ap.parse_args()

    data = json.loads(FIXTURE.read_text())
    queries = data["queries"] if isinstance(data, dict) and "queries" in data else data
    task = [q for q in queries if q.get("kind") != "session_start"]
    positives = [q for q in task if q.get("kind") != "negative"]
    negatives = [q for q in task if q.get("kind") == "negative"]

    pos_rows, mrr3_list, ndcg3_list, hit3_list, candrec_list, latencies = [], [], [], [], [], []
    for q in positives:
        rel = set(q.get("relevant", []))
        if q.get("anchor"):
            rel.add(q["anchor"])
        ranked, lat = find_skill(q["text"], args.limit)
        latencies.append(lat)
        mrr3 = rm.reciprocal_rank(ranked[:3], rel)
        ndcg3 = rm.ndcg_at_k(ranked, rel, 3)
        hit3 = rm.hit_at_k(ranked, rel, 3)
        candrec = rm.candidate_recall_at_limit(ranked, rel)
        mrr3_list.append(mrr3); ndcg3_list.append(ndcg3); hit3_list.append(hit3); candrec_list.append(candrec)
        pos_rows.append({"id": q["id"], "kind": q["kind"], "rel": sorted(rel),
                         "top5": ranked[:5], "mrr_at3": mrr3, "cand_recall": candrec, "latency_ms": lat})

    # no_match precision: a negative is correctly rejected when no relevant skill is in the top-3
    # (the floor should drop off-topic queries; matches empty OR no rel in top-3).
    neg_rows, correct_reject = [], 0
    for q in negatives:
        rel = set(q.get("relevant", []))
        if q.get("anchor"):
            rel.add(q["anchor"])
        ranked, lat = find_skill(q["text"], 3)
        latencies.append(lat)
        rejected = (len(ranked) == 0) or (len(set(ranked[:3]) & rel) == 0)
        correct_reject += 1 if rejected else 0
        neg_rows.append({"id": q["id"], "top3": ranked[:3], "rejected": rejected, "latency_ms": lat})

    def mean(xs):
        return sum(xs) / len(xs) if xs else 0.0

    metrics = {
        "mrr_at3": round(mean(mrr3_list), 4),
        "ndcg_at3": round(mean(ndcg3_list), 4),
        "candidate_recall_at_limit": round(mean(candrec_list), 4),
        "hit_at3": round(mean(hit3_list), 4),
        "no_match_precision": round(correct_reject / len(negatives), 4) if negatives else None,
        "p95_latency_ms": rm.percentile_nearest_rank(sorted(latencies), 95),
        "n_positives": len(positives), "n_negatives": len(negatives), "limit": args.limit,
    }
    # T11 gate floors (verbatim) for the verdict.
    floors = rm.GATE_THRESHOLDS
    gate = {
        k: {"got": metrics[k], "floor": floors[k], "passes": metrics[k] >= floors[k]}
        for k in ("mrr_at3", "ndcg_at3", "candidate_recall_at_limit", "no_match_precision")
    }
    report = {
        "unit": "T12 A2 Task-quality probe (find_skill, snapshot_dense, live)",
        "config_label": args.label,
        "metrics": metrics,
        "t11_4b_reference": T11_4B_REFERENCE,
        "gate_vs_t11_floors": gate,
        "all_floors_pass": all(g["passes"] for g in gate.values()),
        "positives": pos_rows, "negatives": neg_rows,
    }
    out = Path(args.out); out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2))
    print(f"[task] {args.label}: MRR@3={metrics['mrr_at3']} (4b ref {T11_4B_REFERENCE['mrr_at3']}, floor {floors['mrr_at3']}) "
          f"nDCG@3={metrics['ndcg_at3']} cand-recall@{args.limit}={metrics['candidate_recall_at_limit']} "
          f"(4b ref {T11_4B_REFERENCE['candidate_recall_at_limit']}, floor {floors['candidate_recall_at_limit']}) "
          f"no_match_prec={metrics['no_match_precision']} (floor {floors['no_match_precision']}) "
          f"p95={metrics['p95_latency_ms']}ms", file=sys.stderr)
    print(f"[task] ALL T11 FLOORS PASS: {report['all_floors_pass']}", file=sys.stderr)
    print(f"[task] wrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
