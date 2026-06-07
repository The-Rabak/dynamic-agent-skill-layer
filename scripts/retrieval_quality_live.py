#!/usr/bin/env python3
"""Retrieval-quality measurement that drives the REAL running mcp-server end-to-end.

NO in-process reconstruction. Retrieval logic runs 100% inside the live
mcp-server (queried over HTTP via the real `find_skill` tool); relevance
judging runs 100% inside the real `claude` CLI (the same provider the
production extraction seam uses: `claude --print --output-format json
--model <model>`). This script only sends queries to the real app + the real
model and tallies pure IR metrics. See the memory rule
"measurement-drives-real-app-no-in-process-reconstruction".

Ground truth (decision #2, anchor + LLM-judge pooling):
  relevant_set(q) = { anchor(q) } ∪ { skill s : the judge marked (q, s) relevant }
  MRR / recall credit the anchor OR any judge-relevant sibling;
  precision / nDCG credit the judged-relevant set (so valid alternates in a
  synonym-dense corpus are not counted as misses).

Committed target (FROZEN, decision #3): judge-augmented held-out
  MRR >= 0.80, nDCG@3 >= 0.80, no_match precision >= 0.90.
Do NOT lower the target to force green; document the gap instead.
"""
import argparse
import json
import math
import subprocess
import sys
import urllib.request
from pathlib import Path

MCP_URL = "http://127.0.0.1:3001/mcp"
JUDGE_MODEL = "claude-sonnet-4-6"  # production default (DEFAULT_CLAUDE_MODEL)
K = 3  # the product injects/uses the top-k; metrics reported @K


# ─── real mcp-server over HTTP ────────────────────────────────────────────────
def find_skill(prompt: str, limit: int) -> list[str]:
    """Call the live server's find_skill tool; return ranked skill names."""
    body = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "find_skill", "arguments": {"prompt": prompt, "limit": limit}},
    }).encode()
    req = urllib.request.Request(MCP_URL, data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        r = json.loads(resp.read())
    if "error" in r and r["error"]:
        raise RuntimeError(f"find_skill RPC error for prompt {prompt!r}: {r['error']}")
    return [m["name"] for m in r.get("result", {}).get("matches", [])]


# ─── real claude CLI judge (same invocation as the production seam) ───────────
def judge_query(query_text: str, candidates: list[dict]) -> dict[str, bool]:
    """Batch-judge a query's candidate skills with the REAL claude CLI.

    candidates: list of {name, description}. Returns {skill_name: relevant_bool}.
    Fails loud on any error (no silent skip) per the no-fakes mandate.
    """
    if not candidates:
        return {}
    listing = "\n".join(
        f"{i+1}. {c['name']}: {c['description']}" for i, c in enumerate(candidates)
    )
    prompt = f"""You are a retrieval-quality judge. Given a user query and a numbered list of skills, decide for EACH skill whether it is genuinely relevant to the query.

A skill is relevant if it would meaningfully help a developer who asked the query. Be strict: tangential or merely topical overlap is NOT relevant. Only mark relevant if the skill directly addresses the query's core intent.

Respond with EXACTLY a JSON array, one object per skill in the same order, and nothing else:
[{{"skill": "<name>", "relevant": true}}, ...]

Query: {query_text}

Skills:
{listing}

JSON:"""
    proc = subprocess.run(
        ["claude", "--print", "--output-format", "json", "--model", JUDGE_MODEL],
        input=prompt, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"claude judge failed (rc={proc.returncode}): {proc.stderr[:500]}")
    # `--output-format json` wraps the model text in an envelope; extract `.result`.
    try:
        env = json.loads(proc.stdout)
        raw = env["result"] if isinstance(env, dict) and "result" in env else proc.stdout
    except json.JSONDecodeError:
        raw = proc.stdout
    raw = raw.strip()
    start, end = raw.find("["), raw.rfind("]")
    if start < 0 or end < 0:
        raise RuntimeError(f"judge response has no JSON array:\n{raw[:500]}")
    verdicts = json.loads(raw[start:end + 1])
    out = {}
    for v in verdicts:
        out[v["skill"]] = bool(v["relevant"])
    return out


# ─── pure IR metrics ─────────────────────────────────────────────────────────
def reciprocal_rank(ranked, rel):
    for i, x in enumerate(ranked):
        if x in rel:
            return 1.0 / (i + 1)
    return 0.0


def ndcg_at_k(ranked, rel, k):
    if not rel:
        return 0.0
    dcg = sum((1.0 if x in rel else 0.0) / math.log2(i + 2) for i, x in enumerate(ranked[:k]))
    ideal = min(len(rel), k)
    idcg = sum(1.0 / math.log2(i + 2) for i in range(ideal))
    return dcg / idcg if idcg else 0.0


def precision_at_1(ranked, rel):
    return 1.0 if ranked and ranked[0] in rel else 0.0


def recall_at_k(ranked, rel, k):
    if not rel:
        return 0.0
    return len(set(ranked[:k]) & rel) / len(rel)


def hit_at_k(ranked, rel, k):
    return 1.0 if set(ranked[:k]) & rel else 0.0


def mean(xs):
    return sum(xs) / len(xs) if xs else 0.0


# ─── main ────────────────────────────────────────────────────────────────────
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixture", default="tests/fixtures/retrieval_quality_234_corpus_labeled.json")
    ap.add_argument("--limit", type=int, default=10, help="find_skill depth for MRR")
    ap.add_argument("--split", choices=["tuning", "held_out", "all"], default="all")
    ap.add_argument("--verdict-cache", default="tests/e2e/reports/retrieval_234_live_verdicts.json")
    ap.add_argument("--out", default="tests/e2e/reports/retrieval_234_live_report.json")
    ap.add_argument("--no-judge", action="store_true", help="anchor-only (skip the LLM judge)")
    ap.add_argument("--gate", action="store_true", help="exit nonzero if held-out target unmet")
    ap.add_argument("--config-label", default="default")
    args = ap.parse_args()

    fixture = json.loads(Path(args.fixture).read_text())
    queries = fixture["queries"]
    if args.split != "all":
        queries = [q for q in queries if q.get("split") == args.split]

    # Descriptions for judging come from the live server's own returns.
    cache_path = Path(args.verdict_cache)
    cache = {}
    if cache_path.exists():
        for rec in json.loads(cache_path.read_text()):
            cache[(rec["query_id"], rec["skill_name"])] = rec["relevant"]

    pos = [q for q in queries if q.get("kind") != "negative"]
    neg = [q for q in queries if q.get("kind") == "negative"]

    # 1. Drive the REAL server for every query; collect ranked results + descriptions.
    per_query = []   # (q, ranked_names, name->desc)
    for q in pos:
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "find_skill",
                                      "arguments": {"prompt": q["text"], "limit": args.limit}}}).encode()
        req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=60) as resp:
            r = json.loads(resp.read())
        matches = r.get("result", {}).get("matches", [])
        ranked = [m["name"] for m in matches]
        descs = {m["name"]: m.get("description", "") for m in matches}
        per_query.append((q, ranked, descs))

    # 2. Judge the pooled candidates (real claude CLI), draining uncached pairs. No cap.
    judged_new = 0
    if not args.no_judge:
        for q, ranked, descs in per_query:
            to_judge = [{"name": n, "description": descs.get(n, "")}
                        for n in ranked if (q["id"], n) not in cache]
            if not to_judge:
                continue
            verdicts = judge_query(q["text"], to_judge)
            for n in [c["name"] for c in to_judge]:
                # A candidate the judge omitted defaults to NOT relevant (strict).
                cache[(q["id"], n)] = bool(verdicts.get(n, False))
                judged_new += 1
        # Persist the accumulated verdict cache.
        cache_path.parent.mkdir(parents=True, exist_ok=True)
        cache_path.write_text(json.dumps(
            [{"query_id": k[0], "skill_name": k[1], "relevant": v} for k, v in sorted(cache.items())],
            indent=1))

    # 3. Compute anchor-only AND judge-augmented metrics.
    def metrics_over(relevant_fn):
        rr, nd, p1, rc, ht = [], [], [], [], []
        rows = []
        for q, ranked, _ in per_query:
            rel = relevant_fn(q, ranked)
            m = dict(id=q["id"], kind=q["kind"], anchor=q["anchor"],
                     rr=reciprocal_rank(ranked, rel),
                     ndcg=ndcg_at_k(ranked, rel, K),
                     p1=precision_at_1(ranked, rel),
                     recall=recall_at_k(ranked, rel, K),
                     hit=hit_at_k(ranked, rel, K),
                     top3=ranked[:3], relevant=sorted(rel))
            rows.append(m)
            rr.append(m["rr"]); nd.append(m["ndcg"]); p1.append(m["p1"])
            rc.append(m["recall"]); ht.append(m["hit"])
        return dict(mrr=mean(rr), ndcg_at_3=mean(nd), p_at_1=mean(p1),
                    recall_at_3=mean(rc), hit_at_3=mean(ht), n=len(per_query)), rows

    anchor_only = lambda q, ranked: {q["anchor"]}
    def judge_aug(q, ranked):
        rel = {q["anchor"]}
        for n in ranked:
            if cache.get((q["id"], n), False):
                rel.add(n)
        return rel

    agg_anchor, _ = metrics_over(anchor_only)
    agg_judge, rows_judge = metrics_over(judge_aug)

    # 4. Negatives: a non-empty find_skill result for an off-topic query = fabricated match.
    false_matches = 0
    neg_rows = []
    for q in neg:
        ranked = find_skill(q["text"], args.limit)
        fabricated = len(ranked) > 0
        false_matches += 1 if fabricated else 0
        neg_rows.append(dict(id=q["id"], served=ranked[:3], fabricated=fabricated))
    no_match_precision = 1.0 - (false_matches / len(neg)) if neg else None

    report = dict(
        config_label=args.config_label, split=args.split, k=K,
        positives=len(per_query), negatives=len(neg), judged_new=judged_new,
        judged_total=len(cache), judged_relevant=sum(1 for v in cache.values() if v),
        anchor_only=agg_anchor, judge_augmented=agg_judge,
        no_match_precision=no_match_precision,
        target=dict(mrr=0.80, ndcg_at_3=0.80, no_match_precision=0.90),
        per_query=rows_judge, negatives_detail=neg_rows,
    )
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(report, indent=1))

    print(f"\n=== RETRIEVAL QUALITY (LIVE mcp-server / find_skill) — {args.config_label} / {args.split} ===")
    print(f"positives={len(per_query)} negatives={len(neg)} "
          f"judged={len(cache)} (relevant={report['judged_relevant']})")
    print(f"{'metric':12s} {'anchor-only':>12s} {'judge-augmented':>16s}")
    for key in ("mrr", "ndcg_at_3", "p_at_1", "recall_at_3", "hit_at_3"):
        print(f"{key:12s} {agg_anchor[key]:>12.3f} {agg_judge[key]:>16.3f}")
    if no_match_precision is not None:
        print(f"{'no_match_prec':12s} {'':>12s} {no_match_precision:>16.3f}")
    print(f"report: {args.out}")

    if args.gate:
        t = report["target"]
        ok = (agg_judge["mrr"] >= t["mrr"] and agg_judge["ndcg_at_3"] >= t["ndcg_at_3"]
              and (no_match_precision is None or no_match_precision >= t["no_match_precision"]))
        if not ok:
            print(f"\n=== RETRIEVAL QUALITY BELOW COMMITTED TARGET (judge-augmented) ===\n"
                  f"MRR={agg_judge['mrr']:.3f} (min {t['mrr']}), "
                  f"nDCG@3={agg_judge['ndcg_at_3']:.3f} (min {t['ndcg_at_3']}), "
                  f"no_match_precision={no_match_precision} (min {t['no_match_precision']})\n"
                  f"Do NOT lower the target; document the gap in docs/assessments/.", file=sys.stderr)
            sys.exit(1)


if __name__ == "__main__":
    main()
