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

V1.7 arm metadata (T01):
  Each report now carries an `arm` block that identifies which backend,
  embedder model, and dense/sparse/rerank flags produced it.  The metadata
  is read from environment variables that mirror the real server's
  RetrievalConfig::from_env surface:

    OLLAMA_EMBED_MODEL   — embedder model name (default: nomic-embed-text)
    RETRIEVAL_BACKEND    — candidate generation backend (default: snapshot_dense)
    RETRIEVAL_SPARSE     — BM25/sparse enabled (default: false; arrives in T04)
    RETRIEVAL_RERANK     — local reranker enabled (default: false; arrives in T07)

  Where a variable is absent, the arm defaults to the current production value
  and is clearly labelled.  Experimental arms (qwen/hybrid/rerank) that do not
  yet exist in the server default to false/production-name; they will be wired
  in T02/T04/T07 using these same env names.
"""
import argparse
import json
import math
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

MCP_URL = "http://127.0.0.1:3001/mcp"
JUDGE_MODEL = "claude-sonnet-4-6"  # production default (DEFAULT_CLAUDE_MODEL)
K = 3  # the product injects/uses the top-k; metrics reported @K

# ─── V1.7 arm metadata ────────────────────────────────────────────────────────

# Production defaults for the current V1.7 baseline arm.
# These are the values the real server uses today (2026-06-09) and must match
# what RetrievalConfig::default() + build_embedding_service() produce.
ARM_METADATA_DEFAULTS = {
    "backend": "snapshot_dense",      # in-memory dense cosine over RetrievalSnapshot
    "embedder_model": "nomic-embed-text",  # hardcoded in mcp-server build_embedding_service()
    "dense": True,                    # always on; dense cosine is the only candidate path
    "sparse": False,                  # BM25/sparse not yet implemented (arrives in T04)
    "rerank": False,                  # local reranker not yet implemented (arrives in T07)
}


def _parse_bool_env(value: str) -> bool:
    """Parse a boolean env var value; true/1/yes → True, anything else → False."""
    return value.strip().lower() in ("true", "1", "yes")


def build_arm_metadata(env_overrides: dict | None = None) -> dict:
    """Read V1.7 retrieval arm metadata from the environment (or env_overrides).

    Returns a dict with keys: backend, embedder_model, dense, sparse, rerank.

    Fields are read from environment variables that mirror the real server's
    configuration surface (RetrievalConfig::from_env + OLLAMA_EMBED_MODEL).
    When a variable is absent the current production default is used and is
    honestly labelled — no invented values.

    env_overrides: optional dict of {name: value} to treat as additional env
    vars (used by the sweep to test each config arm without actually setting
    process-level env; callers pass the overrides dict from CONFIGS).
    """
    merged_env = dict(os.environ)
    if env_overrides:
        merged_env.update(env_overrides)

    embedder_model = merged_env.get("OLLAMA_EMBED_MODEL",
                                    ARM_METADATA_DEFAULTS["embedder_model"])
    backend = merged_env.get("RETRIEVAL_BACKEND",
                             ARM_METADATA_DEFAULTS["backend"])
    sparse_raw = merged_env.get("RETRIEVAL_SPARSE", "")
    rerank_raw = merged_env.get("RETRIEVAL_RERANK", "")

    return {
        "backend": backend,
        "embedder_model": embedder_model,
        "dense": True,  # always on; sparse is additive, not a replacement
        "sparse": _parse_bool_env(sparse_raw) if sparse_raw.strip() else False,
        "rerank": _parse_bool_env(rerank_raw) if rerank_raw.strip() else False,
    }


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


def _latency_stats(latencies_ms: list[float]) -> dict:
    """Compute mean, p50, p95 latency summary over a list of per-query latencies.

    Returns a dict with keys: mean, p50, p95, n (all in milliseconds).
    Returns zeros for all stats when the input list is empty (e.g. no positive
    queries in the split) so the report key is always present and well-formed.
    """
    if not latencies_ms:
        return {"mean": 0.0, "p50": 0.0, "p95": 0.0, "n": 0}
    sorted_lat = sorted(latencies_ms)
    n = len(sorted_lat)

    def percentile(p):
        # Nearest-rank method: index = ceil(p/100 * n) - 1, clamped to [0, n-1].
        idx = max(0, min(n - 1, int(math.ceil(p / 100.0 * n)) - 1))
        return round(sorted_lat[idx], 1)

    return {
        "mean": round(sum(sorted_lat) / n, 1),
        "p50": percentile(50),
        "p95": percentile(95),
        "n": n,
    }


# ─── main ────────────────────────────────────────────────────────────────────
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixture", default="tests/fixtures/retrieval_quality_234_corpus_labeled.json")
    ap.add_argument("--limit", type=int, default=10, help="find_skill depth for MRR")
    ap.add_argument("--split", choices=["tuning", "held_out", "all"], default="all")
    ap.add_argument("--verdict-cache", default="tests/e2e/reports/retrieval_234_live_verdicts.json")
    ap.add_argument("--out", default="tests/e2e/reports/retrieval_234_live_report.json")
    ap.add_argument("--no-judge", action="store_true", help="anchor-only (skip the LLM judge)")
    ap.add_argument("--gate", action="store_true",
                    help="exit nonzero if quality regresses below the regression floor")
    ap.add_argument("--regression-floor", type=float, default=0.60,
                    help="judge-augmented MRR hard floor for --gate (regression guard); "
                         "the 0.80 target stays the documented aspiration")
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

    # Read arm metadata from the environment.  Fields default to the current
    # production values when the env var is absent (honest, no invented values).
    arm = build_arm_metadata()

    # 1. Drive the REAL server for every query; collect ranked results + descriptions
    #    and capture per-query end-to-end latency of the find_skill HTTP call.
    per_query = []   # (q, ranked_names, name->desc, latency_ms)
    for q in pos:
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "find_skill",
                                      "arguments": {"prompt": q["text"], "limit": args.limit}}}).encode()
        req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
        t_start = time.monotonic()
        with urllib.request.urlopen(req, timeout=60) as resp:
            r = json.loads(resp.read())
        query_latency_ms = (time.monotonic() - t_start) * 1000.0
        matches = r.get("result", {}).get("matches", [])
        ranked = [m["name"] for m in matches]
        descs = {m["name"]: m.get("description", "") for m in matches}
        per_query.append((q, ranked, descs, query_latency_ms))

    # 2. Judge the pooled candidates (real claude CLI), draining uncached pairs. No cap.
    judged_new = 0
    if not args.no_judge:
        for q, ranked, descs, _latency in per_query:
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
        for q, ranked, _, latency_ms in per_query:
            rel = relevant_fn(q, ranked)
            m = dict(id=q["id"], kind=q["kind"], anchor=q["anchor"],
                     rr=reciprocal_rank(ranked, rel),
                     ndcg=ndcg_at_k(ranked, rel, K),
                     p1=precision_at_1(ranked, rel),
                     recall=recall_at_k(ranked, rel, K),
                     hit=hit_at_k(ranked, rel, K),
                     top3=ranked[:3], relevant=sorted(rel),
                     latency_ms=round(latency_ms, 1))
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

    # 5. Compute aggregate latency stats over the positive query set.
    latencies = [lat for _, _, _, lat in per_query]
    latency_summary = _latency_stats(latencies)

    report = dict(
        config_label=args.config_label, split=args.split, k=K,
        # V1.7 arm: backend/embedder/flags that produced this report.  Read from
        # environment; defaults to current production values when vars are absent.
        arm=arm,
        positives=len(per_query), negatives=len(neg), judged_new=judged_new,
        judged_total=len(cache), judged_relevant=sum(1 for v in cache.values() if v),
        anchor_only=agg_anchor, judge_augmented=agg_judge,
        no_match_precision=no_match_precision,
        # Per-arm latency summary over the positive query find_skill calls.
        latency_ms=latency_summary,
        target=dict(mrr=0.80, ndcg_at_3=0.80, no_match_precision=0.90),
        regression_floor=dict(mrr=args.regression_floor, no_match_precision=0.90),
        per_query=rows_judge, negatives_detail=neg_rows,
    )
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(report, indent=1))

    print(f"\n=== RETRIEVAL QUALITY (LIVE mcp-server / find_skill) — {args.config_label} / {args.split} ===")
    print(f"arm: backend={arm['backend']}  embedder={arm['embedder_model']}  "
          f"dense={arm['dense']}  sparse={arm['sparse']}  rerank={arm['rerank']}")
    print(f"positives={len(per_query)} negatives={len(neg)} "
          f"judged={len(cache)} (relevant={report['judged_relevant']})")
    lat = report["latency_ms"]
    print(f"latency (find_skill): mean={lat['mean']:.1f}ms  p50={lat['p50']:.1f}ms  p95={lat['p95']:.1f}ms  n={lat['n']}")
    print(f"{'metric':12s} {'anchor-only':>12s} {'judge-augmented':>16s}")
    for key in ("mrr", "ndcg_at_3", "p_at_1", "recall_at_3", "hit_at_3"):
        print(f"{key:12s} {agg_anchor[key]:>12.3f} {agg_judge[key]:>16.3f}")
    if no_match_precision is not None:
        print(f"{'no_match_prec':12s} {'':>12s} {no_match_precision:>16.3f}")
    print(f"report: {args.out}")

    if args.gate:
        t, rf = report["target"], report["regression_floor"]
        # Aspiration (0.80) is reported for visibility; the hard gate is the
        # regression floor (guards against backslide below today's measured
        # level without faking the unmet aspiration green).
        asp_met = (agg_judge["mrr"] >= t["mrr"] and agg_judge["ndcg_at_3"] >= t["ndcg_at_3"])
        print(f"\n[gate] judge-aug MRR={agg_judge['mrr']:.3f} nDCG@3={agg_judge['ndcg_at_3']:.3f} "
              f"no_match={no_match_precision} | aspiration 0.80/0.80/0.90 "
              f"{'MET' if asp_met else 'UNMET (tracked in docs/assessments/)'}")
        regressed = (agg_judge["mrr"] < rf["mrr"]
                     or (no_match_precision is not None and no_match_precision < rf["no_match_precision"]))
        if regressed:
            print(f"\n=== RETRIEVAL QUALITY REGRESSED BELOW FLOOR ===\n"
                  f"judge-aug MRR={agg_judge['mrr']:.3f} (floor {rf['mrr']}), "
                  f"no_match_precision={no_match_precision} (floor {rf['no_match_precision']}).\n"
                  f"This is a real backslide below the last measured level — investigate; "
                  f"do NOT lower the floor to force green.", file=sys.stderr)
            sys.exit(1)


if __name__ == "__main__":
    main()
