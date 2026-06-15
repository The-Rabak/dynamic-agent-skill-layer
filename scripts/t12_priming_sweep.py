#!/usr/bin/env python3
"""T12 priming sweep — drives the REAL mcp-server `compile_context` over the T18
session_start stratum and measures the priming ranker against the T18 pre-registered
instrument (set-coverage@3 headline, freshness hit-rate, permutation negative control,
paired sign-test).

WHY this lives here (T12, measurement-drives-the-real-app rule)
--------------------------------------------------------------
T18 (LOCKED pre-registration) graded T12 on `compile_context` (the production
SessionStart surface) over the 22-query `session_start` stratum, NOT find_skill /
in-process reconstruction. This script restores that coverage on the validated ruler:
for each stratum query it calls the live server twice with UNIQUE session_ids — once
WITHOUT a trigger (the `Task`/baseline path = the T18 before-number) and once WITH
`trigger:"session_start"` (the `Priming` ranker under test) — parses the injected
skills from the real `## Skill:` markdown headers, and computes the pre-registered
metrics via `scripts/retrieval_metrics.py`.

Per-config invocation (one server config per run; pass --label):
  python3 scripts/t12_priming_sweep.py --label default --out tests/e2e/reports/retrieval/t12_priming_default.json

Negative-control gate (runs FIRST in the report): permutation derangement of the gold
sets over the PRIMED results; the primed prime is VALID-on-this-stratum iff permuted
coverage craters (<= 0.5x true), reusing crater_check() — the T18 §5 gate.
"""
import argparse
import json
import re
import sys
import urllib.request
import uuid
from pathlib import Path
from time import perf_counter

# retrieval_metrics is the T20 shared lib (same dir).
sys.path.insert(0, str(Path(__file__).resolve().parent))
import retrieval_metrics as rm  # noqa: E402

MCP_URL = "http://127.0.0.1:3001/mcp"
FIXTURE = Path("tests/fixtures/retrieval_quality_262_corpus_labeled.json")
SKILL_HEADER = re.compile(r"^## Skill:\s*(.+?)\s*$", re.MULTILINE)
HEADLINE_N = 3  # production compile_context max_results default (T18 LOCK)


def load_session_start_queries():
    data = json.loads(FIXTURE.read_text())
    queries = data["queries"] if isinstance(data, dict) and "queries" in data else data
    return [q for q in queries if q.get("kind") == "session_start"]


def call_compile_context(prompt: str, with_trigger: bool, timeout: float = 60.0):
    """One live compile_context call with a UNIQUE session_id (dodge dedup/suppression).

    Returns (status, injected_names, latency_ms). Raises on transport error (fail loud).
    """
    args = {
        "prompt": prompt,
        "session_id": f"t12-{uuid.uuid4()}",
        "repo_path": "project",
    }
    if with_trigger:
        args["trigger"] = "session_start"
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "compile_context", "arguments": args},
        }
    ).encode()
    req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
    started = perf_counter()
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        payload = json.loads(resp.read().decode())
    latency_ms = round((perf_counter() - started) * 1000.0)

    # MCP tools/call wraps the tool JSON in result.content[0].text (a JSON string)
    # OR returns it structured — handle both, fail loud if neither.
    result = payload.get("result")
    if result is None:
        raise RuntimeError(f"compile_context returned no result: {json.dumps(payload)[:400]}")
    tool = _extract_tool_payload(result)
    status = tool.get("status", "unknown")
    additional = tool.get("additional_context") or ""
    injected = SKILL_HEADER.findall(additional)
    return status, injected, latency_ms


def _extract_tool_payload(result):
    """Unwrap the compile_context response object from the MCP result envelope."""
    if isinstance(result, dict) and "status" in result:
        return result
    content = result.get("content") if isinstance(result, dict) else None
    if content and isinstance(content, list):
        text = content[0].get("text", "")
        return json.loads(text)
    if isinstance(result, dict) and "structuredContent" in result:
        return result["structuredContent"]
    raise RuntimeError(f"cannot extract tool payload from result: {json.dumps(result)[:400]}")


def measure_arm(queries, with_trigger):
    """Run one arm (baseline=no-trigger, or primed=trigger) over all queries."""
    rows = []
    for q in queries:
        status, injected, latency_ms = call_compile_context(q["text"], with_trigger)
        relevant = set(q.get("relevant", []))
        fresh = q.get("fresh_golds", []) or []
        cov3 = rm.set_coverage_at_n(injected, relevant, HEADLINE_N)
        fhr = rm.freshness_hit_rate(injected, fresh)  # None when no fresh golds
        rows.append(
            {
                "query_id": q["id"],
                "substratum": q.get("substratum"),
                "split": q.get("split"),
                "text": q["text"][:160],
                "relevant": sorted(relevant),
                "fresh_golds": fresh,
                "status": status,
                "latency_ms": latency_ms,
                "injected_names": injected,
                "coverage_at_3": cov3,
                "freshness_hit": fhr,
                "no_match": status in ("no_match", "degraded"),
            }
        )
    return rows


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else 0.0


def _arm_summary(rows):
    by_sub = {}
    for sub in ("thin", "verbose"):
        srows = [r for r in rows if r["substratum"] == sub]
        if not srows:
            continue
        by_sub[sub] = {
            "n": len(srows),
            "mean_coverage_at_3": round(_mean([r["coverage_at_3"] for r in srows]), 6),
            "no_match_rate": round(_mean([1.0 if r["no_match"] else 0.0 for r in srows]), 6),
            "freshness_hit_rate": round(_mean([r["freshness_hit"] for r in srows]), 6),
            "p95_latency_ms": rm.percentile_nearest_rank(sorted(r["latency_ms"] for r in srows), 95),
        }
    return {
        "n": len(rows),
        "mean_coverage_at_3": round(_mean([r["coverage_at_3"] for r in rows]), 6),
        "no_match_rate": round(_mean([1.0 if r["no_match"] else 0.0 for r in rows]), 6),
        "freshness_hit_rate": round(_mean([r["freshness_hit"] for r in rows]), 6),
        "p95_latency_ms": rm.percentile_nearest_rank(sorted(r["latency_ms"] for r in rows), 95),
        "by_substratum": by_sub,
    }


def permutation_negative_control(primed_rows):
    """T18 §5 gate: derange (cyclic shift by 1) each query's gold set onto a
    DIFFERENT query's primed injected set, recompute coverage@3, and crater_check.
    Proceed iff permuted mean <= 0.5x true mean (instrument-valid for this arm)."""
    n = len(primed_rows)
    true_cov = [r["coverage_at_3"] for r in primed_rows]
    permuted_cov = []
    for i, r in enumerate(primed_rows):
        other_relevant = set(primed_rows[(i + 1) % n]["relevant"])
        permuted_cov.append(rm.set_coverage_at_n(r["injected_names"], other_relevant, HEADLINE_N))
    true_mean = _mean(true_cov)
    perm_mean = _mean(permuted_cov)
    crater = rm.crater_check(true_mean, perm_mean)
    return {
        "control": "permutation (cyclic-shift-1 derangement of gold sets)",
        "true_mean_coverage_at_3": round(true_mean, 6),
        "permuted_mean_coverage_at_3": round(perm_mean, 6),
        "separation_S": round(true_mean - perm_mean, 6),
        "rel_drop": crater["rel_drop"],
        "craters": crater["craters"],
        "gate": "PASS (instrument-valid for this arm)" if crater["craters"] else "FAIL (vacuous metric)",
    }


def paired_baseline_vs_primed(baseline_rows, primed_rows):
    """Per-query coverage@3 paired comparison + exact binomial sign test."""
    by_id = {r["query_id"]: r for r in baseline_rows}
    n_primed_better = n_baseline_better = n_tie = 0
    deltas = []
    per_query = []
    for pr in primed_rows:
        br = by_id[pr["query_id"]]
        d = pr["coverage_at_3"] - br["coverage_at_3"]
        deltas.append(d)
        if d > 1e-9:
            n_primed_better += 1
        elif d < -1e-9:
            n_baseline_better += 1
        else:
            n_tie += 1
        per_query.append(
            {
                "query_id": pr["query_id"],
                "substratum": pr["substratum"],
                "baseline_cov3": round(br["coverage_at_3"], 6),
                "primed_cov3": round(pr["coverage_at_3"], 6),
                "delta": round(d, 6),
            }
        )
    p = rm.sign_test(n_primed_better, n_baseline_better)
    return {
        "n": len(primed_rows),
        "n_primed_better": n_primed_better,
        "n_baseline_better": n_baseline_better,
        "n_tie": n_tie,
        "mean_paired_delta": round(_mean(deltas), 6),
        "sign_test_p": round(p, 6),
        "per_query": per_query,
    }


def main():
    ap = argparse.ArgumentParser(description="T12 priming sweep over the T18 session_start stratum.")
    ap.add_argument("--label", required=True, help="server-config label (e.g. default, norec_nofresh)")
    ap.add_argument("--out", required=True, help="output JSON path")
    ap.add_argument("--no-baseline", action="store_true", help="skip the no-trigger baseline arm")
    args = ap.parse_args()

    queries = load_session_start_queries()
    print(f"[t12] {len(queries)} session_start queries; config-label={args.label}", file=sys.stderr)

    primed_rows = measure_arm(queries, with_trigger=True)
    baseline_rows = None if args.no_baseline else measure_arm(queries, with_trigger=False)

    report = {
        "unit": "T12 Unit 4",
        "config_label": args.label,
        "stratum": "session_start (22 queries: 11 thin + 11 verbose)",
        "headline_N": HEADLINE_N,
        "t18_before_number_coverage_at_3": 0.068513,
        "t18_thresholds_verbatim": {
            "recurrence_baseline": "+0.10 absolute set-coverage (→ >=0.17) by paired sign test p<0.05",
            "freshness_slot": "+0.15 hit-rate with <=0.02 coverage cannibalization",
            "centrality_recent_use": "+0.043 (default DROP — T11 ranking-inert @262)",
            "negative_control_gate": "permuted mean coverage <= 0.5x true mean (crater)",
        },
        "negative_control": permutation_negative_control(primed_rows),
        "primed_arm": _arm_summary(primed_rows),
    }
    if baseline_rows is not None:
        report["baseline_arm"] = _arm_summary(baseline_rows)
        report["paired_baseline_vs_primed"] = paired_baseline_vs_primed(baseline_rows, primed_rows)
    report["primed_per_query"] = primed_rows
    if baseline_rows is not None:
        report["baseline_per_query"] = baseline_rows

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2))

    # Console summary.
    nc = report["negative_control"]
    print(f"[t12] NEG-CONTROL: true={nc['true_mean_coverage_at_3']} permuted={nc['permuted_mean_coverage_at_3']} "
          f"rel_drop={nc['rel_drop']} craters={nc['craters']} → {nc['gate']}", file=sys.stderr)
    print(f"[t12] PRIMED coverage@3={report['primed_arm']['mean_coverage_at_3']} "
          f"(thin={report['primed_arm']['by_substratum'].get('thin',{}).get('mean_coverage_at_3')} "
          f"verbose={report['primed_arm']['by_substratum'].get('verbose',{}).get('mean_coverage_at_3')}) "
          f"p95={report['primed_arm']['p95_latency_ms']}ms", file=sys.stderr)
    if baseline_rows is not None:
        pb = report["paired_baseline_vs_primed"]
        print(f"[t12] BASELINE coverage@3={report['baseline_arm']['mean_coverage_at_3']} | "
              f"PAIRED delta={pb['mean_paired_delta']} primed_better={pb['n_primed_better']} "
              f"baseline_better={pb['n_baseline_better']} tie={pb['n_tie']} sign_p={pb['sign_test_p']}", file=sys.stderr)
    print(f"[t12] wrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
